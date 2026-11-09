//! Owned Realms HTTP and HTTPS sockets. Original plain-HTTP framing uses ureq's
//! public URL parser; HTTPS keeps the shared verified TLS connector.
use std::{
    collections::HashMap,
    io::{self, BufRead, BufReader, Read, Write},
    net::{Shutdown, TcpStream, ToSocketAddrs},
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

#[derive(Default)]
pub(crate) struct Cancellation {
    tls: Arc<horizon_cosmetics::transport::Cancellation>,
    next: AtomicU64,
    http: Mutex<HashMap<u64, TcpStream>>,
}
impl Cancellation {
    pub fn cancelled(&self) -> bool {
        self.tls.cancelled()
    }
    #[cfg(test)]
    pub fn attached_sockets(&self) -> usize {
        self.tls.attached_sockets() + self.http.lock().unwrap_or_else(|e| e.into_inner()).len()
    }
    pub fn cancel(&self) {
        self.tls.cancel();
        let sockets = match self.http.try_lock() {
            Ok(s) => s,
            Err(std::sync::TryLockError::Poisoned(e)) => e.into_inner(),
            Err(std::sync::TryLockError::WouldBlock) => return,
        };
        for socket in sockets.values() {
            let _ = socket.shutdown(Shutdown::Both);
        }
    }
    fn attach(self: &Arc<Self>, socket: &TcpStream) -> io::Result<Lease> {
        let close = socket.try_clone()?;
        let mut sockets = self.http.lock().unwrap_or_else(|e| e.into_inner());
        if self.cancelled() {
            return Err(cancelled());
        }
        let id = self.next.fetch_add(1, Ordering::Relaxed);
        sockets.insert(id, close);
        drop(sockets);
        let lease = Lease {
            owner: self.clone(),
            id,
        };
        if self.cancelled() {
            self.cancel();
            return Err(cancelled());
        }
        Ok(lease)
    }
}

pub(crate) fn connector(
    cancellation: Arc<Cancellation>,
) -> horizon_cosmetics::transport::Connector {
    horizon_cosmetics::transport::Connector::new(cancellation.tls.clone())
}
struct Lease {
    owner: Arc<Cancellation>,
    id: u64,
}
impl Drop for Lease {
    fn drop(&mut self) {
        self.owner
            .http
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&self.id);
    }
}
fn cancelled() -> io::Error {
    io::Error::new(io::ErrorKind::ConnectionAborted, "Realms request cancelled")
}
fn invalid() -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, "invalid Realms HTTP response")
}

struct LeasedStream {
    stream: TcpStream,
    lease: Lease,
    deadline: Instant,
}
impl LeasedStream {
    fn remaining(&self) -> io::Result<Duration> {
        if self.lease.owner.cancelled() {
            return Err(cancelled());
        }
        let left = self.deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "Realms request timed out",
            ));
        }
        Ok(left)
    }
}
impl Read for LeasedStream {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        self.stream.set_read_timeout(Some(self.remaining()?))?;
        self.stream.read(out)
    }
}
impl Write for LeasedStream {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.stream.set_write_timeout(Some(self.remaining()?))?;
        self.stream.write(bytes)
    }
    fn flush(&mut self) -> io::Result<()> {
        self.stream.flush()
    }
}

pub(crate) struct Response {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Box<dyn Read + Send + Sync>,
}
impl Response {
    pub fn header(&self, key: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(key))
            .map(|(_, v)| v.as_str())
    }
}

/// A single plain HTTP exchange. Never sends cookies across a redirect, never
/// accepts HTTPS (that scheme stays on the certificate-verified TLS path).
#[allow(clippy::too_many_arguments)]
pub(crate) fn http(
    method: &str,
    link: &str,
    headers: &[(&str, &str)],
    body: Option<(&mut dyn Read, u64)>,
    cancel: Arc<Cancellation>,
    connect_timeout: Duration,
    timeout: Duration,
) -> io::Result<Response> {
    if !matches!(method, "GET" | "HEAD" | "POST") || cancel.cancelled() {
        return Err(cancelled());
    }
    let parsed = ureq::get(link).request_url().map_err(|_| invalid())?;
    let url = parsed.as_url();
    if url.scheme() != "http" || !url.username().is_empty() || url.password().is_some() {
        return Err(invalid());
    }
    let host = url.host_str().ok_or_else(invalid)?.trim_matches(['[', ']']);
    let port = url.port_or_known_default().ok_or_else(invalid)?;
    let start = Instant::now();
    let deadline = start + timeout;
    let connect_deadline = start + connect_timeout;
    let mut stream = None;
    let mut last = io::Error::new(io::ErrorKind::NotConnected, "Realms endpoint unavailable");
    for address in (host, port).to_socket_addrs()? {
        loop {
            if cancel.cancelled() {
                return Err(cancelled());
            }
            let left = connect_deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                break;
            }
            match TcpStream::connect_timeout(&address, left.min(Duration::from_millis(250))) {
                Ok(s) => {
                    stream = Some(s);
                    break;
                }
                Err(e)
                    if matches!(
                        e.kind(),
                        io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock
                    ) =>
                {
                    last = e;
                }
                Err(e) => {
                    last = e;
                    break;
                }
            }
        }
        if stream.is_some() {
            break;
        }
    }
    let stream = stream.ok_or(last)?;
    let lease = cancel.attach(&stream)?;
    let mut stream = LeasedStream {
        stream,
        lease,
        deadline,
    };
    let authority = match url.port() {
        Some(port) => format!(
            "{}:{port}",
            if host.contains(':') {
                format!("[{host}]")
            } else {
                host.to_owned()
            }
        ),
        None => {
            if host.contains(':') {
                format!("[{host}]")
            } else {
                host.to_owned()
            }
        }
    };
    let mut target = url.path().to_owned();
    if let Some(query) = url.query() {
        target.push('?');
        target.push_str(query);
    }
    write!(
        stream,
        "{method} {target} HTTP/1.1\r\nHost: {authority}\r\nConnection: close\r\n"
    )?;
    for (key, value) in headers {
        if key.bytes().any(|b| !b.is_ascii_alphanumeric() && b != b'-')
            || value.contains(['\r', '\n'])
        {
            return Err(invalid());
        }
        write!(stream, "{key}: {value}\r\n")?;
    }
    if let Some((body, bytes)) = body {
        write!(stream, "Content-Length: {bytes}\r\n\r\n")?;
        let copied = io::copy(&mut body.take(bytes), &mut stream)?;
        if copied != bytes {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "short upload archive",
            ));
        }
    } else {
        stream.write_all(b"\r\n")?;
    }
    stream.flush()?;
    let mut reader = BufReader::new(stream);
    let (status, headers) = loop {
        let mut used = 0usize;
        let status_line = line(&mut reader, &mut used)?;
        let mut words = status_line.split_whitespace();
        if !matches!(words.next(), Some("HTTP/1.1" | "HTTP/1.0")) {
            return Err(invalid());
        }
        let status = words
            .next()
            .and_then(|s| s.parse::<u16>().ok())
            .filter(|s| (100..=599).contains(s))
            .ok_or_else(invalid)?;
        let mut headers = Vec::new();
        loop {
            let value = line(&mut reader, &mut used)?;
            if value.is_empty() {
                break;
            }
            let (key, value) = value.split_once(':').ok_or_else(invalid)?;
            headers.push((key.to_owned(), value.trim().to_owned()));
        }
        if status >= 200 {
            break (status, headers);
        }
    };
    let header = |name: &str| {
        headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    };
    let chunked = header("Transfer-Encoding").is_some_and(|v| v.eq_ignore_ascii_case("chunked"));
    let length = header("Content-Length")
        .map(|s| s.parse::<u64>().map_err(|_| invalid()))
        .transpose()?;
    let mode = if method == "HEAD" || matches!(status, 204 | 304) {
        Mode::Length(0)
    } else if chunked {
        Mode::Chunked {
            remaining: 0,
            first: true,
            done: false,
        }
    } else if let Some(bytes) = length {
        Mode::Length(bytes)
    } else {
        Mode::UntilEof
    };
    Ok(Response {
        status,
        headers,
        body: Box::new(Body { reader, mode }),
    })
}
fn line(reader: &mut impl BufRead, used: &mut usize) -> io::Result<String> {
    let mut bytes = Vec::new();
    let n = reader.take(8193).read_until(b'\n', &mut bytes)?;
    *used += n;
    if n == 0 || n > 8192 || *used > 65536 || !bytes.ends_with(b"\r\n") {
        return Err(invalid());
    }
    bytes.truncate(n - 2);
    String::from_utf8(bytes).map_err(|_| invalid())
}
enum Mode {
    UntilEof,
    Length(u64),
    Chunked {
        remaining: u64,
        first: bool,
        done: bool,
    },
}
struct Body {
    reader: BufReader<LeasedStream>,
    mode: Mode,
}
impl Read for Body {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        if out.is_empty() {
            return Ok(0);
        }
        match &mut self.mode {
            Mode::UntilEof => self.reader.read(out),
            Mode::Length(left) => {
                if *left == 0 {
                    return Ok(0);
                }
                let count = usize::try_from(*left).unwrap_or(usize::MAX).min(out.len());
                let n = self.reader.read(&mut out[..count])?;
                if n == 0 {
                    return Err(io::Error::new(
                        io::ErrorKind::UnexpectedEof,
                        "short Realms HTTP body",
                    ));
                }
                *left -= n as u64;
                Ok(n)
            }
            Mode::Chunked {
                remaining,
                first,
                done,
            } => {
                if *done {
                    return Ok(0);
                }
                if *remaining == 0 {
                    let mut used = 0;
                    if !*first && !line(&mut self.reader, &mut used)?.is_empty() {
                        return Err(invalid());
                    }
                    *first = false;
                    let size = line(&mut self.reader, &mut used)?;
                    *remaining =
                        u64::from_str_radix(size.split(';').next().unwrap_or("").trim(), 16)
                            .map_err(|_| invalid())?;
                    if *remaining == 0 {
                        while !line(&mut self.reader, &mut used)?.is_empty() {}
                        *done = true;
                        return Ok(0);
                    }
                }
                let count = usize::try_from(*remaining)
                    .unwrap_or(usize::MAX)
                    .min(out.len());
                let n = self.reader.read(&mut out[..count])?;
                if n == 0 {
                    return Err(io::Error::new(
                        io::ErrorKind::UnexpectedEof,
                        "short Realms HTTP chunk",
                    ));
                }
                *remaining -= n as u64;
                Ok(n)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{net::TcpListener, sync::mpsc, thread};

    #[test]
    fn chunked_bodies_and_trailers_stream_without_content_length() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let link = format!("http://{}/world.tar.gz", listener.local_addr().unwrap());
        let server = thread::spawn(move || {
            let (mut socket, _) = listener.accept().unwrap();
            socket
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            let mut header = Vec::new();
            while !header.ends_with(b"\r\n\r\n") {
                let mut b = [0];
                socket.read_exact(&mut b).unwrap();
                header.push(b[0]);
            }
            socket.write_all(b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n3;ok=yes\r\nabc\r\n2\r\nde\r\n0\r\nX-Trailer: done\r\n\r\n").unwrap();
        });
        let mut response = http(
            "GET",
            &link,
            &[],
            None,
            Arc::default(),
            Duration::from_secs(2),
            Duration::from_secs(2),
        )
        .unwrap();
        let mut text = String::new();
        response.body.read_to_string(&mut text).unwrap();
        assert_eq!(response.status, 200);
        assert_eq!(text, "abcde");
        server.join().unwrap();
    }

    fn stalled_request_is_interrupted(upload: bool) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let link = format!("http://{}/transfer", listener.local_addr().unwrap());
        let (ready_tx, ready) = mpsc::channel();
        let (release, released) = mpsc::channel();
        let server = thread::spawn(move || {
            let (mut socket, _) = listener.accept().unwrap();
            socket
                .set_read_timeout(Some(Duration::from_secs(3)))
                .unwrap();
            let mut header = Vec::new();
            while !header.ends_with(b"\r\n\r\n") {
                let mut b = [0];
                socket.read_exact(&mut b).unwrap();
                header.push(b[0]);
            }
            if !upload {
                socket
                    .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 1000000\r\n\r\n")
                    .unwrap();
            }
            ready_tx.send(()).unwrap();
            let _ = released.recv_timeout(Duration::from_secs(4));
        });
        let cancel = Arc::new(Cancellation::default());
        let owned = cancel.clone();
        let (done_tx, done) = mpsc::channel();
        let worker = thread::spawn(move || {
            let mut bytes = io::repeat(0).take(64 * 1024 * 1024);
            let body = upload.then_some((&mut bytes as &mut dyn Read, 64 * 1024 * 1024));
            let result = http(
                if upload { "POST" } else { "GET" },
                &link,
                &[],
                body,
                owned,
                Duration::from_secs(3),
                Duration::from_secs(600),
            )
            .and_then(|mut r| io::copy(&mut r.body, &mut io::sink()).map(|_| ()));
            done_tx.send(result.is_err()).unwrap();
        });
        ready.recv_timeout(Duration::from_secs(3)).unwrap();
        assert_eq!(cancel.attached_sockets(), 1);
        thread::sleep(Duration::from_millis(30));
        assert!(
            matches!(done.try_recv(), Err(mpsc::TryRecvError::Empty)),
            "request must still be stalled before cancellation"
        );
        cancel.cancel();
        let timely = done.recv_timeout(Duration::from_secs(1));
        release.send(()).unwrap();
        server.join().unwrap();
        worker.join().unwrap();
        assert!(
            timely.unwrap(),
            "native 600-second transfer deadline must not delay cancellation"
        );
        assert_eq!(cancel.attached_sockets(), 0);
    }
    #[test]
    fn cancellation_interrupts_stalled_http_download_body() {
        stalled_request_is_interrupted(false);
    }
    #[test]
    fn cancellation_interrupts_stalled_http_upload_body() {
        stalled_request_is_interrupted(true);
    }
}
