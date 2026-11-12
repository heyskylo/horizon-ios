use std::io::{self, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use url::Url;

const MAX_BYTES: u64 = 64 * 1024 * 1024;
const TIMEOUT: Duration = Duration::from_secs(120);
const REQUEST_HEAD: usize = 8 * 1024;

/// A capture waiting on 127.0.0.1 for the viewer to fetch it once.
pub struct Offer {
    /// The viewer page with the capture's local address in its fragment.
    pub url: String,
    stop: Arc<AtomicBool>,
    thread: JoinHandle<bool>,
}

impl Offer {
    /// Blocks until the capture was fetched (`true`) or the offer ran out.
    pub fn wait(self) -> bool {
        self.thread.join().unwrap_or(false)
    }

    pub fn cancel(&self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

/// Serves `file` once on 127.0.0.1, for `viewer` only, and gives the viewer link that fetches it.
pub fn offer(file: &Path, viewer: &str) -> Result<Offer, String> {
    offer_for(file, viewer, TIMEOUT)
}

pub(crate) fn offer_for(file: &Path, viewer: &str, timeout: Duration) -> Result<Offer, String> {
    let origin = Url::parse(viewer)
        .map_err(|e| format!("the viewer address is not a URL: {e}"))?
        .origin()
        .ascii_serialization();
    let mut bytes = Vec::new();
    std::fs::File::open(file)
        .map_err(|e| e.to_string())?
        .take(MAX_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() as u64 > MAX_BYTES {
        return Err("the capture exceeds 64 MiB".into());
    }
    let listener = TcpListener::bind(("127.0.0.1", 0)).map_err(|e| e.to_string())?;
    listener.set_nonblocking(true).map_err(|e| e.to_string())?;
    let port = listener.local_addr().map_err(|e| e.to_string())?.port();
    let token = token()?;
    let url = format!("{viewer}#local=http://127.0.0.1:{port}/{token}");
    let stop = Arc::new(AtomicBool::new(false));
    let flag = stop.clone();
    let thread = std::thread::Builder::new()
        .name("perf-open".into())
        .spawn(move || {
            serve(
                || listener.accept().map(|(stream, _)| stream),
                &token,
                &origin,
                &bytes,
                timeout,
                &flag,
            )
        })
        .map_err(|e| e.to_string())?;
    Ok(Offer { url, stop, thread })
}

fn token() -> Result<String, String> {
    let mut raw = [0u8; 16];
    getrandom::fill(&mut raw).map_err(|e| e.to_string())?;
    Ok(raw.iter().map(|b| format!("{b:02x}")).collect())
}

/// Whether the capture was handed over before the timeout or a stop; a failed accept (an aborted connection, a full file table) is retried until then.
fn serve(
    mut accept: impl FnMut() -> io::Result<TcpStream>,
    token: &str,
    origin: &str,
    bytes: &[u8],
    timeout: Duration,
    stop: &AtomicBool,
) -> bool {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline && !stop.load(Ordering::Relaxed) {
        match accept() {
            Ok(stream) => {
                if answer(stream, token, origin, bytes).unwrap_or(false) {
                    return true;
                }
            }
            Err(_) => std::thread::sleep(Duration::from_millis(10)),
        }
    }
    false
}

struct Head {
    method: String,
    path: String,
    origin: Option<String>,
}

fn read_head(stream: &mut TcpStream) -> io::Result<Option<Head>> {
    stream.set_nonblocking(false)?;
    stream.set_read_timeout(Some(Duration::from_secs(2)))?;
    let mut buf = Vec::new();
    let mut chunk = [0u8; 1024];
    while !buf.windows(4).any(|w| w == b"\r\n\r\n") {
        if buf.len() > REQUEST_HEAD {
            return Ok(None);
        }
        let n = stream.read(&mut chunk)?;
        if n == 0 {
            return Ok(None);
        }
        buf.extend_from_slice(&chunk[..n]);
    }
    let text = String::from_utf8_lossy(&buf);
    let mut lines = text.split("\r\n");
    let mut first = lines.next().unwrap_or("").split(' ');
    let (Some(method), Some(path)) = (first.next(), first.next()) else {
        return Ok(None);
    };
    let origin = lines.find_map(|line| {
        let (name, value) = line.split_once(':')?;
        name.eq_ignore_ascii_case("origin")
            .then(|| value.trim().to_owned())
    });
    Ok(Some(Head {
        method: method.to_owned(),
        path: path.to_owned(),
        origin,
    }))
}

/// Answers one connection; `true` when it carried the capture away.
fn answer(mut stream: TcpStream, token: &str, origin: &str, bytes: &[u8]) -> io::Result<bool> {
    let Some(head) = read_head(&mut stream)? else {
        return Ok(false);
    };
    let right_path = head.path.strip_prefix('/') == Some(token);
    let right_origin = head.origin.as_deref().is_none_or(|o| o == origin);
    let cors = format!(
        "Access-Control-Allow-Origin: {origin}\r\nAccess-Control-Allow-Private-Network: true\r\nVary: Origin\r\n"
    );
    if !right_path || !right_origin {
        stream.write_all(
            b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
        )?;
        return Ok(false);
    }
    match head.method.as_str() {
        "OPTIONS" => {
            write!(
                stream,
                "HTTP/1.1 204 No Content\r\n{cors}Access-Control-Allow-Methods: GET\r\nAccess-Control-Max-Age: 60\r\nConnection: close\r\n\r\n"
            )?;
            Ok(false)
        }
        "GET" => {
            write!(
                stream,
                "HTTP/1.1 200 OK\r\n{cors}Content-Type: application/octet-stream\r\nCache-Control: no-store\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                bytes.len()
            )?;
            stream.write_all(bytes)?;
            stream.flush()?;
            Ok(true)
        }
        _ => {
            stream.write_all(
                b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
            )?;
            Ok(false)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const VIEWER: &str = "https://viewer.example/perf";
    const ORIGIN: &str = "https://viewer.example";

    fn offer_of(bytes: &[u8], timeout: Duration) -> (Offer, u16, String, tempdir::Dir) {
        let dir = tempdir::Dir::new();
        let file = dir.0.join("capture.hzperf");
        std::fs::write(&file, bytes).unwrap();
        let offer = offer_for(&file, VIEWER, timeout).unwrap();
        let local = offer.url.split_once("#local=http://127.0.0.1:").unwrap().1;
        let (port, token) = local.split_once('/').unwrap();
        let (port, token) = (port.parse().unwrap(), token.to_owned());
        (offer, port, token, dir)
    }

    mod tempdir {
        pub struct Dir(pub std::path::PathBuf);
        impl Dir {
            pub fn new() -> Self {
                static NEXT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
                let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                let path = std::env::temp_dir()
                    .join(format!("horizon-perf-share-{}-{n}", std::process::id()));
                std::fs::create_dir_all(&path).unwrap();
                Self(path)
            }
        }
        impl Drop for Dir {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.0);
            }
        }
    }

    fn send(port: u16, request: &str) -> Vec<u8> {
        let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
        stream.write_all(request.as_bytes()).unwrap();
        let mut reply = Vec::new();
        stream.read_to_end(&mut reply).unwrap();
        reply
    }

    fn get(token: &str, extra: &str) -> String {
        format!("GET /{token} HTTP/1.1\r\nHost: 127.0.0.1\r\n{extra}\r\n")
    }

    fn status(reply: &[u8]) -> String {
        String::from_utf8_lossy(reply)
            .lines()
            .next()
            .unwrap()
            .to_owned()
    }

    #[test]
    fn the_right_token_is_served_once_and_then_refused() {
        let (offer, port, token, _dir) = offer_of(b"\x1f\x8bcapture", Duration::from_secs(30));
        let reply = send(port, &get(&token, &format!("Origin: {ORIGIN}\r\n")));
        let text = String::from_utf8_lossy(&reply);
        assert_eq!(status(&reply), "HTTP/1.1 200 OK");
        assert!(text.contains("Content-Type: application/octet-stream"));
        assert!(text.contains(&format!("Access-Control-Allow-Origin: {ORIGIN}")));
        assert!(reply.ends_with(b"\x1f\x8bcapture"));
        assert!(offer.wait());
        assert!(TcpStream::connect(("127.0.0.1", port)).is_err());
    }

    #[test]
    fn a_wrong_token_origin_or_method_gets_404_and_keeps_the_token() {
        let (offer, port, token, _dir) = offer_of(b"bytes", Duration::from_secs(30));
        let wrong_token = get(&"0".repeat(32), "");
        let wrong_origin = get(&token, "Origin: https://evil.example\r\n");
        let wrong_method = format!("POST /{token} HTTP/1.1\r\nHost: x\r\n\r\n");
        let other_path = get(&format!("{token}/x"), "");
        for request in [wrong_token, wrong_origin, wrong_method, other_path] {
            assert_eq!(status(&send(port, &request)), "HTTP/1.1 404 Not Found");
        }
        let reply = send(port, &get(&token, ""));
        assert_eq!(status(&reply), "HTTP/1.1 200 OK");
        assert!(offer.wait());
    }

    #[test]
    fn the_preflight_allows_the_viewer_and_private_networks_without_spending_the_token() {
        let (offer, port, token, _dir) = offer_of(b"bytes", Duration::from_secs(30));
        let preflight = format!(
            "OPTIONS /{token} HTTP/1.1\r\nHost: x\r\nOrigin: {ORIGIN}\r\nAccess-Control-Request-Private-Network: true\r\n\r\n"
        );
        let reply = String::from_utf8(send(port, &preflight)).unwrap();
        assert!(reply.starts_with("HTTP/1.1 204"), "{reply}");
        assert!(reply.contains(&format!("Access-Control-Allow-Origin: {ORIGIN}")));
        assert!(reply.contains("Access-Control-Allow-Private-Network: true"));
        assert_eq!(
            status(&send(port, &get(&token, &format!("Origin: {ORIGIN}\r\n")))),
            "HTTP/1.1 200 OK"
        );
        assert!(offer.wait());
    }

    #[test]
    fn an_aborted_connection_does_not_end_the_offer() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        listener.set_nonblocking(true).unwrap();
        let port = listener.local_addr().unwrap().port();
        let token = "t".repeat(32);
        let server = std::thread::spawn(move || {
            let mut failures =
                [io::ErrorKind::ConnectionAborted, io::ErrorKind::Interrupted].into_iter();
            let accept = || match failures.next() {
                Some(kind) => Err(kind.into()),
                None => listener.accept().map(|(stream, _)| stream),
            };
            serve(
                accept,
                &"t".repeat(32),
                ORIGIN,
                b"bytes",
                Duration::from_secs(30),
                &AtomicBool::new(false),
            )
        });
        std::thread::sleep(Duration::from_millis(50));
        assert_eq!(status(&send(port, &get(&token, ""))), "HTTP/1.1 200 OK");
        assert!(server.join().unwrap());
    }

    #[test]
    fn the_thread_is_gone_after_the_timeout() {
        let (offer, port, _token, _dir) = offer_of(b"bytes", Duration::from_millis(200));
        assert!(!offer.wait());
        assert!(TcpStream::connect(("127.0.0.1", port)).is_err());
    }

    #[test]
    fn the_link_is_the_viewer_with_a_random_token() {
        let (offer, _, token, _dir) = offer_of(b"bytes", Duration::from_millis(50));
        assert!(
            offer
                .url
                .starts_with(&format!("{VIEWER}#local=http://127.0.0.1:"))
        );
        assert_eq!(token.len(), 32);
        assert!(token.bytes().all(|b| b.is_ascii_hexdigit()));
        let (other, _, other_token, _dir2) = offer_of(b"bytes", Duration::from_millis(50));
        assert_ne!(token, other_token);
        other.cancel();
    }
}
