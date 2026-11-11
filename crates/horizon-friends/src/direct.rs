//! Direct connections between a host and a guest: the addresses each end
//! offers, and the race that dials all of the other's at once from the port
//! it also listens on (so two NATs that map a port the same way for every
//! destination let the two SYNs cross: TCP simultaneous open).
use socket2::{Domain, Protocol, SockAddr, Socket, Type};
use std::{
    io::{self, Read, Write},
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, TcpListener, TcpStream},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::Sender,
    },
    time::{Duration, Instant},
};

/// How long the ends try to reach each other directly.
pub const RACE: Duration = Duration::from_millis(3000);
const RETRY: Duration = Duration::from_millis(250);
const MAGIC: &[u8; 4] = b"HZF1";
/// A guest dialling an endpoint from an invite code (no relay involved).
pub const CODE_MAGIC: &[u8; 4] = b"HZF2";

fn socket_for(addr: &SocketAddr) -> io::Result<Socket> {
    let s = Socket::new(
        Domain::for_address(*addr),
        Type::STREAM,
        Some(Protocol::TCP),
    )?;
    s.set_reuse_address(true)?;
    #[cfg(all(unix, not(any(target_os = "solaris", target_os = "illumos"))))]
    s.set_reuse_port(true)?;
    if addr.is_ipv6() {
        s.set_only_v6(true)?;
    }
    Ok(s)
}

fn unspecified(v6: bool, port: u16) -> SocketAddr {
    if v6 {
        SocketAddr::new(IpAddr::V6(Ipv6Addr::UNSPECIFIED), port)
    } else {
        SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), port)
    }
}

/// The direct socket's listeners: IPv4 on `port` (0 picks one), and IPv6 on
/// the same port when the system has it.
pub fn listen(port: u16) -> io::Result<(Vec<TcpListener>, u16)> {
    let bind = |addr: SocketAddr| -> io::Result<TcpListener> {
        let s = socket_for(&addr)?;
        s.bind(&SockAddr::from(addr))?;
        s.listen(16)?;
        let l: TcpListener = s.into();
        l.set_nonblocking(true)?;
        Ok(l)
    };
    let v4 = bind(unspecified(false, port))?;
    let port = v4.local_addr()?.port();
    let mut out = vec![v4];
    if let Ok(v6) = bind(unspecified(true, port)) {
        out.push(v6);
    }
    Ok((out, port))
}

/// Connect to `to` from local `port` (shared with the listener), or from any
/// port when that one can't be used (the same pair already connected).
pub fn connect_from(port: Option<u16>, to: SocketAddr, timeout: Duration) -> io::Result<TcpStream> {
    if let Some(port) = port {
        let s = socket_for(&to)?;
        if s.bind(&SockAddr::from(unspecified(to.is_ipv6(), port)))
            .is_ok()
        {
            match s.connect_timeout(&SockAddr::from(to), timeout) {
                Ok(()) => return Ok(s.into()),
                Err(e)
                    if e.kind() != io::ErrorKind::AddrInUse
                        && e.kind() != io::ErrorKind::AddrNotAvailable =>
                {
                    return Err(e);
                }
                Err(_) => {}
            }
        }
    }
    TcpStream::connect_timeout(&to, timeout)
}

/// This end's candidates for a relayed session: its interface addresses
/// with the direct port (loopback when the relay is local), then the address
/// the relay saw (when it is not one of them).
pub fn candidates(
    port: u16,
    relay: Option<SocketAddr>,
    observed: Option<SocketAddr>,
) -> Vec<SocketAddr> {
    let loopback = relay.is_some_and(|r| r.ip().is_loopback());
    let mut out: Vec<SocketAddr> = crate::reach::interface_addresses(loopback)
        .into_iter()
        .map(|ip| SocketAddr::new(ip, port))
        .collect();
    if let Some(o) = observed
        && !o.ip().is_unspecified()
        && !out.contains(&o)
    {
        out.push(o);
    }
    out.truncate(crate::invite::MAX_ENDPOINTS);
    out
}

/// Both ends open every direct stream the same way, whoever dialled: the
/// magic and the session, then the other's must match.
pub fn introduce(stream: &mut TcpStream, session: u64) -> io::Result<()> {
    let mut hello = [0u8; 12];
    hello[..4].copy_from_slice(MAGIC);
    hello[4..].copy_from_slice(&session.to_be_bytes());
    stream.set_read_timeout(Some(Duration::from_secs(2)))?;
    stream.write_all(&hello)?;
    let mut theirs = [0u8; 12];
    stream.read_exact(&mut theirs)?;
    if theirs != hello {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "not this session",
        ));
    }
    Ok(())
}

/// The session an inbound direct stream belongs to (the host's listener
/// serves every session): reads the other end's introduction.
pub fn read_session(stream: &mut TcpStream) -> io::Result<u64> {
    stream.set_read_timeout(Some(Duration::from_secs(2)))?;
    let mut theirs = [0u8; 12];
    stream.read_exact(&mut theirs)?;
    if &theirs[..4] != MAGIC {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "not a direct stream",
        ));
    }
    Ok(u64::from_be_bytes(theirs[4..].try_into().unwrap()))
}
/// What an inbound stream on the host's direct port is.
pub enum Hello {
    /// A relayed session's direct race.
    Relayed(u64),
    /// A guest with the invite code.
    Code(u64),
}
pub fn read_hello(stream: &mut TcpStream) -> io::Result<Hello> {
    stream.set_read_timeout(Some(Duration::from_secs(5)))?;
    let mut theirs = [0u8; 12];
    stream.read_exact(&mut theirs)?;
    let session = u64::from_be_bytes(theirs[4..].try_into().unwrap());
    match &theirs[..4] {
        m if m == MAGIC => Ok(Hello::Relayed(session)),
        m if m == CODE_MAGIC => Ok(Hello::Code(session)),
        _ => Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "not a Horizon stream",
        )),
    }
}
/// The guest's side of a code session's opening: send the hello, wait for
/// its echo (proof a Horizon host listens there).
pub fn code_hello(stream: &mut TcpStream, session: u64) -> io::Result<()> {
    let mut hello = [0u8; 12];
    hello[..4].copy_from_slice(CODE_MAGIC);
    hello[4..].copy_from_slice(&session.to_be_bytes());
    stream.set_read_timeout(Some(Duration::from_secs(4)))?;
    stream.write_all(&hello)?;
    let mut theirs = [0u8; 12];
    stream.read_exact(&mut theirs)?;
    if theirs != hello {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "not a Horizon host",
        ));
    }
    Ok(())
}
/// The host echoes a code session's hello.
pub fn answer_code(stream: &mut TcpStream, session: u64) -> io::Result<()> {
    let mut hello = [0u8; 12];
    hello[..4].copy_from_slice(CODE_MAGIC);
    hello[4..].copy_from_slice(&session.to_be_bytes());
    stream.write_all(&hello)
}
/// Answer an inbound stream's introduction for `session`.
pub fn answer(stream: &mut TcpStream, session: u64) -> io::Result<()> {
    let mut hello = [0u8; 12];
    hello[..4].copy_from_slice(MAGIC);
    hello[4..].copy_from_slice(&session.to_be_bytes());
    stream.write_all(&hello)
}

/// Dial each target from `port` until one connects, the deadline passes or
/// `stop` is set; streams that connect are introduced and sent on.
pub fn dial(
    port: Option<u16>,
    targets: Vec<SocketAddr>,
    session: u64,
    deadline: Instant,
    stop: Arc<AtomicBool>,
    found: Sender<TcpStream>,
) {
    for target in targets {
        let (stop, found) = (stop.clone(), found.clone());
        let _ = std::thread::Builder::new()
            .name("horizon-friends-dial".into())
            .spawn(move || {
                while !stop.load(Ordering::Relaxed) {
                    let now = Instant::now();
                    if now >= deadline {
                        return;
                    }
                    let tried = Instant::now();
                    let wait = (deadline - now).min(Duration::from_millis(600));
                    if let Ok(mut s) = connect_from(port, target, wait)
                        && introduce(&mut s, session).is_ok()
                    {
                        let _ = found.send(s);
                        return;
                    }
                    let spent = tried.elapsed();
                    if spent < RETRY {
                        std::thread::sleep(RETRY - spent);
                    }
                }
            });
    }
}

/// Accept on the listeners until the deadline or `stop`; inbound streams for
/// `session` are answered and sent on.
pub fn accept(
    listeners: Vec<TcpListener>,
    session: u64,
    deadline: Instant,
    stop: Arc<AtomicBool>,
    found: Sender<TcpStream>,
) {
    let _ = std::thread::Builder::new()
        .name("horizon-friends-accept".into())
        .spawn(move || {
            while !stop.load(Ordering::Relaxed) && Instant::now() < deadline {
                let mut idle = true;
                for l in &listeners {
                    if let Ok((mut s, _)) = l.accept() {
                        idle = false;
                        // Each on its own thread, within one deadline: a stranger dripping a byte
                        // at a time must not hold the only accept thread past the race.
                        let found = found.clone();
                        let _ = std::thread::Builder::new()
                            .name("horizon-friends-accept-one".into())
                            .spawn(move || {
                                let _ = s.set_nonblocking(false);
                                let Ok(limit) = crate::deadline(&s, Duration::from_secs(2)) else {
                                    return;
                                };
                                if read_session(&mut s).ok() == Some(session)
                                    && answer(&mut s, session).is_ok()
                                {
                                    drop(limit);
                                    let _ = found.send(s);
                                }
                            });
                    }
                }
                if idle {
                    std::thread::sleep(Duration::from_millis(20));
                }
            }
        });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc::channel;

    #[test]
    fn candidates_have_the_port_and_the_observed_address() {
        let relay = SocketAddr::from(([127, 0, 0, 1], 1));
        let observed = SocketAddr::from(([203, 0, 113, 5], 40000));
        let c = candidates(5555, Some(relay), Some(observed));
        assert!(c.contains(&SocketAddr::from(([127, 0, 0, 1], 5555))));
        assert_eq!(c.last(), Some(&observed));
        let far = candidates(5555, None, None);
        assert!(
            far.iter()
                .all(|a| !a.ip().is_loopback() && a.port() == 5555)
        );
    }

    /// Two direct sockets on loopback: each listens on its port and dials
    /// the other's from it; the session's streams meet and wrong sessions
    /// don't.
    #[test]
    fn the_race_connects_and_checks_the_session() {
        let (la, pa) = listen(0).unwrap();
        let (lb, pb) = listen(0).unwrap();
        assert_ne!(pa, pb);
        let stop = Arc::new(AtomicBool::new(false));
        let deadline = Instant::now() + Duration::from_secs(3);
        let (ta, ra) = channel();
        let (tb, rb) = channel();
        accept(la, 42, deadline, stop.clone(), ta.clone());
        accept(lb, 42, deadline, stop.clone(), tb.clone());
        dial(
            Some(pa),
            vec![SocketAddr::from(([127, 0, 0, 1], pb))],
            42,
            deadline,
            stop.clone(),
            ta,
        );
        let mut a = ra.recv_timeout(Duration::from_secs(3)).unwrap();
        let mut b = rb.recv_timeout(Duration::from_secs(3)).unwrap();
        a.write_all(b"ping").unwrap();
        let mut got = [0u8; 4];
        b.read_exact(&mut got).unwrap();
        assert_eq!(&got, b"ping");
        stop.store(true, Ordering::Relaxed);

        let (lc, pc) = listen(0).unwrap();
        let (tc, rc) = channel();
        let deadline = Instant::now() + Duration::from_millis(800);
        let stop = Arc::new(AtomicBool::new(false));
        accept(lc, 7, deadline, stop.clone(), tc.clone());
        let (td, rd) = channel();
        dial(
            None,
            vec![SocketAddr::from(([127, 0, 0, 1], pc))],
            8,
            deadline,
            stop,
            td,
        );
        assert!(rc.recv_timeout(Duration::from_secs(2)).is_err());
        assert!(rd.recv_timeout(Duration::from_millis(100)).is_err());
    }
}
