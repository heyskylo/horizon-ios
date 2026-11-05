//! Close-only handles for managed HTTPS requests, including a stalled
//! TLS handshake. Cancellation never changes the service's native deadlines.
// TLS configuration defaults adapted from ureq 2.12.1, src/rtls.rs (MIT).
// Copyright (c) 2019 Martin Algesten. See THIRD_PARTY_NOTICES.md.
use std::{
    collections::HashMap,
    fmt,
    io::{self, Read, Write},
    net::{Shutdown, TcpStream},
    sync::{
        Arc, Mutex, OnceLock,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
};
use ureq::{ReadWrite, TlsConnector, rustls};

#[derive(Default)]
pub struct Cancellation {
    stopped: AtomicBool,
    next: AtomicU64,
    sockets: Mutex<HashMap<u64, TcpStream>>,
}
impl Cancellation {
    pub fn cancelled(&self) -> bool {
        self.stopped.load(Ordering::Acquire)
    }

    /// Number of live close-only leases (diagnostics; no socket access).
    pub fn attached_sockets(&self) -> usize {
        self.sockets.lock().unwrap().len()
    }

    pub fn cancel(&self) {
        self.stopped.store(true, Ordering::Release);
        // Registration never holds this lock over network IO. If it overlaps
        // cancellation, registration observes stopped; the next gate poll also
        // retries closing any older sockets. Never wait for a lock on a frame.
        let sockets = match self.sockets.try_lock() {
            Ok(sockets) => sockets,
            Err(std::sync::TryLockError::Poisoned(e)) => e.into_inner(),
            Err(std::sync::TryLockError::WouldBlock) => return,
        };
        for socket in sockets.values() {
            let _ = socket.shutdown(Shutdown::Both);
        }
    }

    fn attach(self: &Arc<Self>, socket: &TcpStream) -> io::Result<Lease> {
        let close = socket.try_clone()?;
        let mut sockets = self.sockets.lock().unwrap_or_else(|e| e.into_inner());
        if self.cancelled() {
            let _ = close.shutdown(Shutdown::Both);
            return Err(cancelled());
        }
        let id = self.next.fetch_add(1, Ordering::Relaxed);
        sockets.insert(id, close);
        drop(sockets);
        if self.cancelled() {
            self.cancel();
        }
        Ok(Lease {
            cancellation: self.clone(),
            id,
        })
    }
}

struct Lease {
    cancellation: Arc<Cancellation>,
    id: u64,
}
impl Drop for Lease {
    fn drop(&mut self) {
        self.cancellation
            .sockets
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&self.id);
    }
}

fn cancelled() -> io::Error {
    io::Error::new(io::ErrorKind::ConnectionAborted, "request cancelled")
}

pub struct Connector {
    cancellation: Arc<Cancellation>,
}
impl Connector {
    pub fn new(cancellation: Arc<Cancellation>) -> Self {
        Self { cancellation }
    }
}

fn configuration() -> Arc<rustls::ClientConfig> {
    static CONFIG: OnceLock<Arc<rustls::ClientConfig>> = OnceLock::new();
    CONFIG
        .get_or_init(|| {
            // Match ureq's WebPKI/ring TLS 1.2 and 1.3 defaults. Only the socket
            // ownership wrapper changes; certificate verification stays enabled.
            let roots = rustls::RootCertStore {
                roots: webpki_roots::TLS_SERVER_ROOTS.to_vec(),
            };
            Arc::new(
                rustls::ClientConfig::builder_with_provider(
                    rustls::crypto::ring::default_provider().into(),
                )
                .with_protocol_versions(&[&rustls::version::TLS12, &rustls::version::TLS13])
                .expect("ring supports TLS 1.2 and 1.3")
                .with_root_certificates(roots)
                .with_no_client_auth(),
            )
        })
        .clone()
}

impl TlsConnector for Connector {
    fn connect(
        &self,
        name: &str,
        io: Box<dyn ReadWrite>,
    ) -> Result<Box<dyn ReadWrite>, ureq::Error> {
        let socket = io.socket().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::Unsupported,
                "HTTPS transport has no closeable socket",
            )
        })?;
        let lease = self.cancellation.attach(socket)?;
        // Keep the close handle during the handshake, not just after TLS is
        // ready: this is where a peer sending no ServerHello stalls.
        let stream = configuration().connect(name, io)?;
        if self.cancellation.cancelled() {
            return Err(cancelled().into());
        }
        Ok(Box::new(Stream { stream, lease }))
    }
}

struct Stream {
    stream: Box<dyn ReadWrite>,
    lease: Lease,
}
impl fmt::Debug for Stream {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ManagedTlsStream")
    }
}
impl Read for Stream {
    fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
        if self.lease.cancellation.cancelled() {
            return Err(cancelled());
        }
        self.stream.read(bytes)
    }
}
impl Write for Stream {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if self.lease.cancellation.cancelled() {
            return Err(cancelled());
        }
        self.stream.write(bytes)
    }
    fn flush(&mut self) -> io::Result<()> {
        if self.lease.cancellation.cancelled() {
            return Err(cancelled());
        }
        self.stream.flush()
    }
}
impl ReadWrite for Stream {
    fn socket(&self) -> Option<&TcpStream> {
        self.stream.socket()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{net::TcpListener, time::Duration};

    #[test]
    fn cancellation_before_a_late_connect_closes_without_starting_tls() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let peer = std::thread::spawn(move || {
            let (mut socket, _) = listener.accept().unwrap();
            socket
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            let n = socket.read(&mut [0; 1]).unwrap();
            assert_eq!(
                n, 0,
                "a cancelled late connection must send no handshake or credentials"
            );
        });
        let cancellation = Arc::new(Cancellation::default());
        cancellation.cancel();
        // Equivalent to a DNS/TCP completion arriving after shutdown began.
        let socket = TcpStream::connect(address).unwrap();
        assert!(
            Connector::new(cancellation.clone())
                .connect("localhost", Box::new(socket))
                .is_err()
        );
        assert_eq!(cancellation.attached_sockets(), 0);
        peer.join().unwrap();
    }

    #[test]
    fn a_failed_tls_handshake_releases_its_close_only_socket() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let peer = std::thread::spawn(move || {
            let (mut socket, _) = listener.accept().unwrap();
            socket.write_all(b"not a TLS response").unwrap();
        });
        let cancellation = Arc::new(Cancellation::default());
        let socket = TcpStream::connect(address).unwrap();
        socket
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        assert!(
            Connector::new(cancellation.clone())
                .connect("localhost", Box::new(socket))
                .is_err()
        );
        assert_eq!(cancellation.attached_sockets(), 0);
        assert!(!cancellation.cancelled());
        peer.join().unwrap();
    }
}
