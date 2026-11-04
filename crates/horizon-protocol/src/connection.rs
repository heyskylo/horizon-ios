//! A blocking connection with vanilla's pipeline: optional AES/CFB8
//! encryption, VarInt21 framing and threshold compression. After login it
//! splits into a reader (for a receive thread) and a shareable writer, or
//! gives its stream up ([`Connection::detach`]) to be polled without blocking,
//! the halves then keeping only the framing, compression and encryption. It
//! runs over TCP, or over an in-memory pipe between two threads of one process
//! (vanilla's `LocalChannel`, which an integrated server's player uses).
use crate::{Error, Result, buf::Reader, crypto::Cfb8, frame};
use std::{
    io::{Read, Write},
    net::{Shutdown, SocketAddr, TcpStream, ToSocketAddrs},
    sync::{
        Arc, Condvar, Mutex,
        atomic::{AtomicI64, Ordering},
    },
    time::{Duration, Instant},
};

/// Called when a [`Pipe`] end has bytes to read or was closed ([`Pipe::on_ready`]).
type Notify = Arc<dyn Fn() + Send + Sync>;

/// One direction of a [`Pipe`]: the bytes written and not read yet, and
/// whether either end closed it.
#[derive(Default)]
struct Channel {
    state: Mutex<(std::collections::VecDeque<u8>, bool)>,
    ready: Condvar,
    /// The reading end's [`Pipe::on_ready`].
    notify: Mutex<Option<Notify>>,
}
impl Channel {
    fn close(&self) {
        self.state.lock().unwrap().1 = true;
        self.ready.notify_all();
        self.notify();
    }
    /// Tell a reader that doesn't block, outside the state's lock.
    fn notify(&self) {
        let notify = self.notify.lock().unwrap().clone();
        if let Some(notify) = notify {
            notify();
        }
    }
}
/// The channels of one end; dropping the end's last clone closes them, as
/// a socket closes with its process.
struct End {
    incoming: Arc<Channel>,
    outgoing: Arc<Channel>,
}
impl Drop for End {
    fn drop(&mut self) {
        self.incoming.close();
        self.outgoing.close();
    }
}
/// One end of an in-memory byte pipe: what one end writes the other reads.
pub struct Pipe {
    end: Arc<End>,
    /// The read timeout (`set_read_timeout`).
    timeout: Mutex<Option<Duration>>,
    /// Reads with nothing to read fail with `WouldBlock` instead of waiting.
    nonblocking: bool,
}
impl Pipe {
    /// Two connected ends.
    pub fn pair() -> (Self, Self) {
        let (a, b) = (Arc::new(Channel::default()), Arc::new(Channel::default()));
        let end = |incoming, outgoing| Self {
            end: Arc::new(End { incoming, outgoing }),
            timeout: Mutex::new(None),
            nonblocking: false,
        };
        (end(a.clone(), b.clone()), end(b, a))
    }
    fn try_clone(&self) -> Self {
        Self {
            end: self.end.clone(),
            timeout: Mutex::new(*self.timeout.lock().unwrap()),
            nonblocking: self.nonblocking,
        }
    }
    /// Reads with nothing to read fail with `WouldBlock`, as a non-blocking socket's do.
    pub fn set_nonblocking(&mut self, nonblocking: bool) {
        self.nonblocking = nonblocking;
    }
    /// Call `notify` whenever bytes arrive at this end or the pipe closes (also
    /// now, if either already happened), as a poller's readiness would: a
    /// non-blocking reader then reads until `WouldBlock`.
    pub fn on_ready(&self, notify: impl Fn() + Send + Sync + 'static) {
        let incoming = &self.end.incoming;
        *incoming.notify.lock().unwrap() = Some(Arc::new(notify));
        let state = incoming.state.lock().unwrap();
        let ready = !state.0.is_empty() || state.1;
        drop(state);
        if ready {
            incoming.notify();
        }
    }
    /// Close both directions: reads at either end then see the end of the stream.
    pub fn shutdown(&self) {
        self.end.incoming.close();
        self.end.outgoing.close();
    }
}
impl Read for Pipe {
    fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
        let deadline = self.timeout.lock().unwrap().map(|t| Instant::now() + t);
        let mut state = self.end.incoming.state.lock().unwrap();
        if self.nonblocking && state.0.is_empty() && !state.1 {
            return Err(std::io::ErrorKind::WouldBlock.into());
        }
        while state.0.is_empty() && !state.1 {
            state = match deadline {
                None => self.end.incoming.ready.wait(state).unwrap(),
                Some(deadline) => {
                    let left = deadline.saturating_duration_since(Instant::now());
                    if left.is_zero() {
                        return Err(std::io::ErrorKind::TimedOut.into());
                    }
                    self.end.incoming.ready.wait_timeout(state, left).unwrap().0
                }
            };
        }
        state.0.read(out)
    }
}
impl Write for Pipe {
    fn write(&mut self, data: &[u8]) -> std::io::Result<usize> {
        let mut state = self.end.outgoing.state.lock().unwrap();
        if state.1 {
            return Err(std::io::ErrorKind::BrokenPipe.into());
        }
        state.0.extend(data);
        self.end.outgoing.ready.notify_all();
        drop(state);
        self.end.outgoing.notify();
        Ok(data.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// What a connection runs over.
enum Stream {
    Tcp(TcpStream),
    Memory(Pipe),
    /// Given up by [`Connection::detach`]: reads and writes fail.
    Detached,
}
fn detached() -> std::io::Error {
    std::io::ErrorKind::NotConnected.into()
}
impl Stream {
    fn try_clone(&self) -> std::io::Result<Self> {
        Ok(match self {
            Self::Tcp(s) => Self::Tcp(s.try_clone()?),
            Self::Memory(p) => Self::Memory(p.try_clone()),
            Self::Detached => Self::Detached,
        })
    }
    fn set_read_timeout(&self, timeout: Option<Duration>) -> std::io::Result<()> {
        match self {
            Self::Tcp(s) => s.set_read_timeout(timeout),
            Self::Memory(p) => {
                *p.timeout.lock().unwrap() = timeout;
                Ok(())
            }
            Self::Detached => Ok(()),
        }
    }
    fn shutdown(&self) {
        match self {
            Self::Tcp(s) => {
                let _ = s.shutdown(Shutdown::Both);
            }
            Self::Memory(p) => p.shutdown(),
            Self::Detached => {}
        }
    }
}
impl Read for Stream {
    fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
        match self {
            Self::Tcp(s) => s.read(out),
            Self::Memory(p) => p.read(out),
            Self::Detached => Err(detached()),
        }
    }
}
impl Write for Stream {
    fn write(&mut self, data: &[u8]) -> std::io::Result<usize> {
        match self {
            Self::Tcp(s) => s.write(data),
            Self::Memory(p) => p.write(data),
            Self::Detached => Err(detached()),
        }
    }
    fn flush(&mut self) -> std::io::Result<()> {
        match self {
            Self::Tcp(s) => s.flush(),
            Self::Memory(p) => p.flush(),
            Self::Detached => Err(detached()),
        }
    }
}

/// A connection's stream, given up by [`Connection::detach`].
pub enum Socket {
    Tcp(TcpStream),
    Memory(Pipe),
}

/// Packets received/sent and bytes in/out, for the debug screen.
#[derive(Debug, Default)]
pub struct Counters {
    pub received: AtomicI64,
    pub sent: AtomicI64,
    pub bytes_in: AtomicI64,
    pub bytes_out: AtomicI64,
}

pub struct ConnReader {
    stream: Stream,
    /// Bytes read: `buffer[start..end]` are not consumed yet, and reads go
    /// straight into `buffer[end..]`.
    buffer: Vec<u8>,
    start: usize,
    end: usize,
    inflater: frame::Inflater,
    decrypt: Option<Cfb8>,
    pub threshold: Option<usize>,
    pub counters: Arc<Counters>,
    /// Set for a server of an older release: what arrives is renumbered.
    adapter: Option<Arc<crate::adapter::Session>>,
}
/// Closes a connection from another thread: a read or a write blocked on it fails.
pub struct Closer(Stream);
impl Closer {
    pub fn close(&self) {
        self.0.shutdown();
    }
}

pub struct ConnWriter {
    stream: Stream,
    encrypt: Option<Cfb8>,
    pub threshold: Option<usize>,
    counters: Arc<Counters>,
    /// Set for a server of an older release: what is sent is renumbered.
    adapter: Option<Arc<crate::adapter::Session>>,
}
/// `InetAddress.getByName` under Java's default `java.net.preferIPv6Addresses=false`: IPv4
/// addresses first, whatever order the system resolver gives (macOS lists IPv6 first). Vanilla
/// connects to that first address only; the rest stay here as fallbacks.
fn ipv4_first(mut addrs: Vec<SocketAddr>) -> Vec<SocketAddr> {
    addrs.sort_by_key(SocketAddr::is_ipv6);
    addrs
}
/// RFC 8305's connection attempt delay: how long an address has alone before the next
/// one is tried beside it.
const ATTEMPT_DELAY: Duration = Duration::from_millis(250);
/// Both halves before the split.
pub struct Connection {
    pub reader: ConnReader,
    pub writer: ConnWriter,
}
impl Connection {
    pub fn connect(host: &str, port: u16, timeout: Duration) -> Result<Self> {
        let addrs: Vec<_> = (host, port)
            .to_socket_addrs()
            .map_err(|e| Error::new(format!("Unknown host {host}: {e}")))?
            .collect();
        Self::connect_addresses(host, addrs, timeout)
    }
    /// Connect through Minecraft's SRV redirect. Status requests still use the
    /// original address in their handshake (`ServerStatusPinger.pingServer`).
    pub fn connect_server(host: &str, port: u16, timeout: Duration) -> Result<Self> {
        Self::connect_server_with_address(host, port, timeout).map(|(connection, _, _)| connection)
    }
    /// `ConnectScreen.startConnecting`: login handshakes use the resolved host
    /// and port, including an SRV redirect, rather than the status request's address.
    pub fn connect_server_with_address(
        host: &str,
        port: u16,
        timeout: Duration,
    ) -> Result<(Self, String, u16)> {
        let resolved = crate::resolver::resolve(host, port)?;
        let connection = Self::connect_addresses(host, resolved.addresses, timeout)?;
        Ok((connection, resolved.host, resolved.port))
    }
    /// Each address starts once the earlier ones fail or [`ATTEMPT_DELAY`] after the last
    /// start, alongside those still trying, and the first to connect wins: a dead address
    /// in a round-robin set costs 250 ms instead of the whole timeout.
    fn connect_addresses(host: &str, addrs: Vec<SocketAddr>, timeout: Duration) -> Result<Self> {
        let addrs = ipv4_first(addrs);
        let (send, recv) = std::sync::mpsc::channel();
        let mut errors: Vec<Option<std::io::Error>> = addrs.iter().map(|_| None).collect();
        let (mut started, mut pending) = (0, 0);
        loop {
            if let Some(&addr) = addrs.get(started) {
                let (send, index) = (send.clone(), started);
                let spawned = std::thread::Builder::new()
                    .name("horizon-connect".into())
                    .spawn(move || {
                        let _ = send.send((index, TcpStream::connect_timeout(&addr, timeout)));
                    });
                match spawned {
                    Ok(_) => pending += 1,
                    Err(e) => errors[index] = Some(e),
                }
                started += 1;
            }
            if pending == 0 {
                if started < addrs.len() {
                    continue;
                }
                break;
            }
            let reply = if started < addrs.len() {
                match recv.recv_timeout(ATTEMPT_DELAY) {
                    Ok(reply) => reply,
                    Err(_) => continue,
                }
            } else {
                recv.recv().expect("a sender is held here")
            };
            pending -= 1;
            match reply {
                (_, Ok(stream)) => return Self::from_stream(stream),
                (index, Err(e)) => errors[index] = Some(e),
            }
        }
        // Report the first address's failure: it is the one vanilla tries, and the IPv6
        // fallbacks after it often fail only because the client has no IPv6 route.
        match errors.into_iter().flatten().next() {
            Some(e) => Err(Error::new(format!("Connection refused: {e}"))),
            None => Err(Error::new(format!("Unknown host {host}"))),
        }
    }
    pub fn from_stream(stream: TcpStream) -> Result<Self> {
        stream.set_nodelay(true)?;
        Self::over(Stream::Tcp(stream))
    }
    /// A connection over one end of an in-memory [`Pipe`].
    pub fn local(pipe: Pipe) -> Self {
        Self::over(Stream::Memory(pipe)).expect("a pipe end clones")
    }
    fn over(stream: Stream) -> Result<Self> {
        let counters = Arc::new(Counters::default());
        let write = stream.try_clone()?;
        Ok(Self {
            reader: ConnReader {
                stream,
                buffer: Vec::new(),
                start: 0,
                end: 0,
                inflater: frame::Inflater::default(),
                decrypt: None,
                threshold: None,
                counters: counters.clone(),
                adapter: None,
            },
            writer: ConnWriter {
                stream: write,
                encrypt: None,
                threshold: None,
                counters,
                adapter: None,
            },
        })
    }
    /// Speak to a server of `version` from now on (before the handshake): packets are renumbered both
    /// ways (crate::adapter). The pinned version needs nothing.
    pub fn adapt(&mut self, version: crate::adapter::ServerVersion) {
        let session = (version != crate::adapter::ServerVersion::Current)
            .then(|| Arc::new(crate::adapter::Session::new(version)));
        self.reader.adapter = session.clone();
        self.writer.adapter = session;
    }
    /// `Connection.isMemoryConnection`: through a [`Pipe`] rather than a socket.
    pub fn is_memory(&self) -> bool {
        matches!(self.reader.stream, Stream::Memory(_))
    }
    /// ClientboundLoginCompressionPacket: negative thresholds disable compression.
    pub fn set_compression(&mut self, threshold: i32) {
        let t = usize::try_from(threshold).ok();
        self.reader.threshold = t;
        self.writer.threshold = t;
    }
    /// Connection.setEncryptionKey: both directions keyed with the shared secret.
    pub fn enable_encryption(&mut self, secret: &[u8; 16]) {
        let mut decrypt = Cfb8::new(secret, secret);
        // Bytes already read past the last packet were sent encrypted.
        let r = &mut self.reader;
        decrypt.decrypt(&mut r.buffer[r.start..r.end]);
        self.reader.decrypt = Some(decrypt);
        self.writer.encrypt = Some(Cfb8::new(secret, secret));
    }
    pub fn send(&mut self, body: &[u8]) -> Result<()> {
        self.writer.send(body)
    }
    pub fn recv(&mut self) -> Result<(i32, Vec<u8>)> {
        self.reader.recv()
    }
    pub fn split(self) -> (ConnReader, ConnWriter) {
        (self.reader, self.writer)
    }
    /// Take the stream out, to read and write it elsewhere (a poller's thread,
    /// without blocking): the halves keep the framing, compression and
    /// encryption, and bytes already read past the last packet. What arrives
    /// goes in through [`ConnReader::read_from`]; [`ConnWriter::frame_into`]
    /// makes what is to be sent. Their own reads and writes fail from now on.
    pub fn detach(mut self) -> (Socket, ConnReader, ConnWriter) {
        self.writer.stream = Stream::Detached;
        let socket = match std::mem::replace(&mut self.reader.stream, Stream::Detached) {
            Stream::Tcp(s) => Socket::Tcp(s),
            Stream::Memory(p) => Socket::Memory(p),
            Stream::Detached => unreachable!("a connection is detached once, by value"),
        };
        (socket, self.reader, self.writer)
    }
}

/// The next frame in a reader's buffer.
enum Next {
    Packet((i32, Vec<u8>)),
    /// A frame with no packet in it.
    Empty,
    /// No whole frame yet: the bytes it needs, once its length is known (0 before).
    Partial(usize),
}
impl ConnReader {
    /// Read exactly `len` bytes as they are, with no framing.
    pub fn read_raw(&mut self, len: usize) -> Result<Vec<u8>> {
        let mut out = vec![0u8; len];
        self.stream.read_exact(&mut out)?;
        Ok(out)
    }
    /// Block until a whole packet arrives: (id, payload).
    pub fn recv(&mut self) -> Result<(i32, Vec<u8>)> {
        if let Some(packet) = self.adapter.as_ref().and_then(|a| a.next_incoming()) {
            return Ok(packet);
        }
        loop {
            match self.next()? {
                Next::Packet(packet) => match &self.adapter {
                    None => return Ok(packet),
                    Some(a) => {
                        if let Some(packet) = a.clientbound(packet.0, packet.1)? {
                            return Ok(packet);
                        }
                        if let Some(packet) = a.next_incoming() {
                            return Ok(packet);
                        }
                    }
                },
                Next::Empty => {}
                Next::Partial(wanted) => self.fill(wanted)?,
            }
        }
    }
    /// The next whole packet among the bytes already read, without reading
    /// more: for a detached reader, after [`Self::read_from`]. No adapter
    /// renumbers it (a server's connection has none).
    pub fn buffered(&mut self) -> Result<Option<(i32, Vec<u8>)>> {
        loop {
            match self.next()? {
                Next::Packet(packet) => return Ok(Some(packet)),
                Next::Empty => {}
                Next::Partial(_) => return Ok(None),
            }
        }
    }
    /// Read once from `source` (a detached stream, without blocking) into the
    /// buffer: the bytes read, 0 at the end of the stream, or its error
    /// (`WouldBlock` when nothing waits).
    pub fn read_from(&mut self, source: &mut impl Read) -> std::io::Result<usize> {
        let wanted = match self.next_header() {
            Ok(Some(wanted)) => wanted,
            _ => 0,
        };
        self.fill_from(source, wanted)
    }
    /// The bytes the next frame needs, once its length prefix is whole.
    fn next_header(&self) -> Result<Option<usize>> {
        Ok(frame::header(&self.buffer[self.start..self.end])?.map(|(prefix, len)| prefix + len))
    }
    /// Take the next frame off the buffer, if it is whole.
    fn next(&mut self) -> Result<Next> {
        let Some(wanted) = self.next_header()? else {
            return Ok(Next::Partial(0));
        };
        if self.end - self.start < wanted {
            return Ok(Next::Partial(wanted));
        }
        let (prefix, _) = frame::header(&self.buffer[self.start..self.end])?.expect("whole");
        let frame = self.start + prefix..self.start + wanted;
        self.start = frame.end;
        Ok(match self.packet(frame)? {
            Some(packet) => {
                self.counters.received.fetch_add(1, Ordering::Relaxed);
                Next::Packet(packet)
            }
            None => Next::Empty,
        })
    }
    /// The packet in `buffer[frame]`, or `None` for a frame with no packet in it:
    /// vanilla's PacketDecoder only decodes a packet from a readable buffer, so an
    /// empty compressed frame is skipped and the connection stays up.
    fn packet(&mut self, frame: std::ops::Range<usize>) -> Result<Option<(i32, Vec<u8>)>> {
        let frame = &self.buffer[frame];
        match frame::body(frame, self.threshold, &mut self.inflater)? {
            frame::Body::Plain(at) => {
                let body = &frame[at..];
                if body.is_empty() {
                    return Ok(None);
                }
                let mut r = Reader::new(body);
                let id = r.varint()?;
                Ok(Some((id, body[r.position()..].to_vec())))
            }
            frame::Body::Inflated(mut body) => {
                let mut r = Reader::new(&body);
                let id = r.varint()?;
                let at = r.position();
                body.drain(..at);
                Ok(Some((id, body)))
            }
        }
    }
    /// Read once from the stream into the buffer, with room for at least
    /// `frame` bytes of unconsumed data (a whole frame, once its length is known).
    fn fill(&mut self, frame: usize) -> Result<()> {
        let mut stream = std::mem::replace(&mut self.stream, Stream::Detached);
        let read = self.fill_from(&mut stream, frame);
        self.stream = stream;
        match read {
            Ok(0) => Err(Error::end_of_stream()),
            Ok(_) => Ok(()),
            // A signal (a profiler attaching, SIGCHLD...) cut the read short
            // before any byte came: nothing is wrong with the connection, the
            // caller reads again.
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => Ok(()),
            // The read timeout passed: Windows reports TimedOut, Unix
            // WouldBlock.
            Err(e)
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock
                ) =>
            {
                Err(Error::timed_out())
            }
            Err(e) => Err(e.into()),
        }
    }
    /// [`Self::fill`] from `source`: the bytes read, decrypted into the buffer.
    fn fill_from(&mut self, source: &mut impl Read, frame: usize) -> std::io::Result<usize> {
        const READ: usize = 1 << 16;
        let pending = self.end - self.start;
        let room = READ.max(frame.saturating_sub(pending));
        if self.buffer.len() - self.end < room {
            self.buffer.copy_within(self.start..self.end, 0);
            (self.start, self.end) = (0, pending);
            if self.buffer.len() < pending + room {
                self.buffer.resize(pending + room, 0);
            }
        }
        let n = source.read(&mut self.buffer[self.end..])?;
        self.counters
            .bytes_in
            .fetch_add(n as i64, Ordering::Relaxed);
        let data = &mut self.buffer[self.end..self.end + n];
        if let Some(c) = &mut self.decrypt {
            c.decrypt(data);
        }
        self.end += n;
        Ok(n)
    }
    /// A [`Closer`] for this connection.
    pub fn closer(&self) -> Result<Closer> {
        Ok(Closer(self.stream.try_clone()?))
    }
    pub fn set_read_timeout(&self, timeout: Option<Duration>) -> Result<()> {
        Ok(self.stream.set_read_timeout(timeout)?)
    }
}
impl ConnWriter {
    /// The server release selected for this connection (local play uses the pinned release).
    pub fn server_version(&self) -> crate::adapter::ServerVersion {
        self.adapter
            .as_ref()
            .map_or(crate::adapter::ServerVersion::Current, |a| a.version)
    }

    /// The old ServerData flag, once it arrived on a pre-component connection.
    pub fn legacy_secure_chat_enforcement(&self) -> Option<bool> {
        self.adapter
            .as_ref()
            .and_then(|a| a.legacy_secure_chat_enforcement())
    }

    /// Write bytes as they are, with no framing: the legacy server list ping
    /// (`LegacyServerPinger`) is not a framed protocol.
    pub fn write_raw(&mut self, data: &[u8]) -> Result<()> {
        self.stream.write_all(data)?;
        self.stream.flush()?;
        Ok(())
    }
    /// Send one packet body (VarInt id + payload).
    pub fn send(&mut self, body: &[u8]) -> Result<()> {
        if let Some(a) = self.adapter.clone() {
            if let Some(body) = a.serverbound(body)? {
                self.send_contents(&frame::contents(&body, self.threshold)?)?;
            }
            for body in a.take_outgoing() {
                self.send_contents(&frame::contents(&body, self.threshold)?)?;
            }
            return Ok(());
        }
        self.send_contents(&frame::contents(body, self.threshold)?)
    }
    /// Send a frame whose contents [`frame::contents`] made with this
    /// writer's threshold.
    pub fn send_contents(&mut self, contents: &[u8]) -> Result<()> {
        let mut framed = Vec::with_capacity(contents.len() + 3);
        self.frame_into(contents, &mut framed);
        self.stream.write_all(&framed)?;
        Ok(())
    }
    /// The bytes [`Self::send_contents`] writes, appended to `out` instead: a
    /// detached writer's, written later, in the order framed (the cipher
    /// runs on).
    pub fn frame_into(&mut self, contents: &[u8], out: &mut Vec<u8>) {
        let at = out.len();
        crate::buf::write_varint(out, contents.len() as i32);
        out.extend_from_slice(contents);
        if let Some(c) = &mut self.encrypt {
            c.encrypt(&mut out[at..]);
        }
        self.counters.sent.fetch_add(1, Ordering::Relaxed);
        self.counters
            .bytes_out
            .fetch_add((out.len() - at) as i64, Ordering::Relaxed);
    }
    pub fn shutdown(&self) {
        self.stream.shutdown();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;

    #[test]
    fn ipv4_first_keeps_each_familys_order() {
        let a = |s: &str| s.parse::<SocketAddr>().unwrap();
        let resolved = vec![
            a("[2001:db8::1]:25565"),
            a("192.0.2.1:25565"),
            a("[2001:db8::2]:25565"),
            a("10.0.0.1:25565"),
        ];
        assert_eq!(
            ipv4_first(resolved),
            [
                a("192.0.2.1:25565"),
                a("10.0.0.1:25565"),
                a("[2001:db8::1]:25565"),
                a("[2001:db8::2]:25565"),
            ]
        );
    }

    fn peer(c: &Connection) -> SocketAddr {
        match &c.reader.stream {
            Stream::Tcp(s) => s.peer_addr().unwrap(),
            Stream::Memory(_) | Stream::Detached => panic!("not a socket"),
        }
    }

    /// Binds `[::1]`, or `None` where the machine has no IPv6 loopback.
    fn ipv6_loopback() -> Option<TcpListener> {
        let listener = TcpListener::bind("[::1]:0");
        if listener.is_err() {
            eprintln!("skipped: no IPv6 loopback");
        }
        listener.ok()
    }

    #[test]
    fn connects_to_ipv4_although_the_resolver_lists_ipv6_first() {
        // Both listen, as with a server whose IPv6 address accepts and then resets.
        let Some(v6_listener) = ipv6_loopback() else {
            return;
        };
        let v4_listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let v6 = v6_listener.local_addr().unwrap();
        let v4 = v4_listener.local_addr().unwrap();
        let c =
            Connection::connect_addresses("test", vec![v6, v4], Duration::from_secs(5)).unwrap();
        assert_eq!(peer(&c), v4);
    }

    #[test]
    fn falls_back_to_ipv6_when_ipv4_refuses() {
        let Some(v6_listener) = ipv6_loopback() else {
            return;
        };
        let v6 = v6_listener.local_addr().unwrap();
        let closed = TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap();
        let c = Connection::connect_addresses("test", vec![v6, closed], Duration::from_secs(5))
            .unwrap();
        assert_eq!(peer(&c), v6);
    }

    #[test]
    fn a_dead_address_does_not_hold_up_the_next() {
        // TEST-NET-1 never answers (or is unreachable at once).
        let dead: SocketAddr = "192.0.2.1:25565".parse().unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let live = listener.local_addr().unwrap();
        let start = Instant::now();
        let c = Connection::connect_addresses("test", vec![dead, live], Duration::from_secs(10))
            .unwrap();
        assert_eq!(peer(&c), live);
        assert!(
            start.elapsed() < Duration::from_secs(5),
            "{:?}",
            start.elapsed()
        );
    }

    #[test]
    fn every_failure_reports_the_first_address() {
        let closed = |_| {
            TcpListener::bind("127.0.0.1:0")
                .unwrap()
                .local_addr()
                .unwrap()
        };
        let error = |host, addrs| {
            Connection::connect_addresses(host, addrs, Duration::from_secs(5))
                .err()
                .unwrap()
                .to_string()
        };
        let refused = error("test", (0..3).map(closed).collect());
        assert!(refused.starts_with("Connection refused"), "{refused}");
        assert_eq!(error("nowhere", Vec::new()), "Unknown host nowhere");
    }

    #[test]
    fn a_silent_peer_times_out_and_a_closed_one_ends_the_stream() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let mut c = Connection::connect("127.0.0.1", port, Duration::from_secs(5)).unwrap();
        let (server, _) = listener.accept().unwrap();
        c.reader
            .set_read_timeout(Some(Duration::from_millis(100)))
            .unwrap();
        let silent = c.recv().unwrap_err();
        assert!(silent.is_timed_out(), "{silent}");
        assert_eq!(silent.to_string(), "Timed out");
        drop(server);
        let closed = c.recv().unwrap_err();
        assert!(closed.is_end_of_stream(), "{closed}");
    }

    #[test]
    fn loopback_with_compression_and_encryption() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let mut c = Connection::from_stream(stream).unwrap();
            let (id, payload) = c.recv().unwrap();
            assert_eq!((id, payload.as_slice()), (0, &b"plain"[..]));
            c.enable_encryption(&[7; 16]);
            c.set_compression(64);
            let (id, payload) = c.recv().unwrap();
            assert_eq!((id, payload.len()), (5, 1000));
            let mut body = vec![9];
            body.extend_from_slice(b"reply");
            c.send(&body).unwrap();
        });
        let mut c = Connection::connect("127.0.0.1", port, Duration::from_secs(5)).unwrap();
        let mut body = vec![0];
        body.extend_from_slice(b"plain");
        c.send(&body).unwrap();
        c.enable_encryption(&[7; 16]);
        c.set_compression(64);
        let mut big = vec![5];
        big.extend(std::iter::repeat_n(3u8, 1000));
        c.send(&big).unwrap();
        let (id, payload) = c.recv().unwrap();
        assert_eq!((id, payload.as_slice()), (9, &b"reply"[..]));
        server.join().unwrap();
    }

    #[test]
    fn local_pipe_carries_packets_and_closes() {
        let (a, b) = Pipe::pair();
        let server = std::thread::spawn(move || {
            let mut c = Connection::local(b);
            c.set_compression(64);
            let (id, payload) = c.recv().unwrap();
            assert_eq!((id, payload.len()), (5, 1000));
            c.send(&[9, 1, 2]).unwrap();
            let (_, writer) = c.split();
            writer.shutdown();
        });
        let mut c = Connection::local(a);
        c.set_compression(64);
        let mut big = vec![5];
        big.extend(std::iter::repeat_n(3u8, 1000));
        c.send(&big).unwrap();
        assert_eq!(c.recv().unwrap(), (9, vec![1, 2]));
        assert_eq!(c.reader.counters.sent.load(Ordering::Relaxed), 1);
        assert_eq!(c.reader.counters.received.load(Ordering::Relaxed), 1);
        server.join().unwrap();
        assert!(c.recv().is_err());
    }

    /// Packets from one byte to past the read size, compressed and
    /// encrypted, arrive whole and in order however the reads cut them.
    #[test]
    fn packets_of_every_size_arrive_whole() {
        let (a, b) = Pipe::pair();
        let bodies: Vec<Vec<u8>> = (0..400u32)
            .map(|i| {
                let len = [1, 2, 63, 64, 65, 300, 4000, 70_000, 200_000][i as usize % 9];
                (0..len).map(|j| (j as u32 * 31 + i) as u8 & 0x7f).collect()
            })
            .collect();
        let sent = bodies.clone();
        let server = std::thread::spawn(move || {
            let mut c = Connection::local(b);
            c.set_compression(64);
            c.enable_encryption(&[3; 16]);
            for body in &sent {
                c.send(body).unwrap();
            }
            c
        });
        let mut c = Connection::local(a);
        c.set_compression(64);
        c.enable_encryption(&[3; 16]);
        for body in &bodies {
            let (id, payload) = c.recv().unwrap();
            assert_eq!(id, i32::from(body[0]));
            assert_eq!(payload, body[1..]);
        }
        drop(server.join().unwrap());
    }

    #[test]
    fn dropping_a_pipe_end_closes_it() {
        let (a, b) = Pipe::pair();
        let mut c = Connection::local(a);
        drop(b);
        assert!(c.recv().is_err());
        assert!(c.send(&[1]).is_err());
    }

    #[test]
    fn local_pipe_read_times_out() {
        let (a, _b) = Pipe::pair();
        let mut c = Connection::local(a);
        c.reader
            .set_read_timeout(Some(Duration::from_millis(20)))
            .unwrap();
        assert!(c.recv().is_err());
    }

    /// Encrypted bytes that arrive in the same read as the last plain packet
    /// are decrypted once encryption is enabled.
    #[test]
    fn encrypted_bytes_read_before_enabling_encryption() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let (sent, both_sent) = std::sync::mpsc::channel();
        let client = std::thread::spawn(move || {
            let mut c = Connection::connect("127.0.0.1", port, Duration::from_secs(5)).unwrap();
            c.send(&[0, 1, 2]).unwrap();
            c.enable_encryption(&[7; 16]);
            c.send(&[5, 4, 3]).unwrap();
            sent.send(c).unwrap();
        });
        let (stream, _) = listener.accept().unwrap();
        let mut c = Connection::from_stream(stream).unwrap();
        let _keep_open = both_sent.recv().unwrap();
        assert_eq!(c.recv().unwrap(), (0, vec![1, 2]));
        c.enable_encryption(&[7; 16]);
        assert_eq!(c.recv().unwrap(), (5, vec![4, 3]));
        client.join().unwrap();
    }

    /// A server may send a compressed frame that carries no packet: the
    /// uncompressed length is 0 and the frame holds nothing else. Vanilla
    /// skips it (its PacketDecoder only reads a packet from a readable
    /// buffer), so the client stays connected.
    #[test]
    fn an_empty_compressed_frame_is_skipped() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let mut c = Connection::from_stream(stream).unwrap();
            c.set_compression(64);
            // The empty frame: its length prefix, then the uncompressed
            // length 0 and nothing else.
            c.writer.send_contents(&[0]).unwrap();
            let mut body = vec![3];
            body.extend_from_slice(b"after");
            c.writer.send(&body).unwrap();
        });
        let mut c = Connection::connect("127.0.0.1", port, Duration::from_secs(5)).unwrap();
        c.set_compression(64);
        let (id, payload) = c.recv().unwrap();
        assert_eq!((id, payload.as_slice()), (3, &b"after"[..]));
        server.join().unwrap();
    }
}
