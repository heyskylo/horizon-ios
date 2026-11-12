//! The guest's end: reach the host from the code (directly, or through a
//! relay), agree on the key, then offer the session on a loopback port the
//! game's client connects to like to any server.
use crate::{
    Code, PathMode,
    crypto::{self, Role},
    direct,
    invite::{Invite, Parsed},
    resolve_relay,
    session::{self, Secure},
    wire::{self, ErrorCode, FromRelay, ToRelay},
};
use std::{
    fmt, io,
    net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream},
    sync::mpsc::channel,
    thread::JoinHandle,
    time::{Duration, Instant},
};

pub struct GuestConfig {
    /// The code as pasted: a full invite, or a relay's short code.
    pub code: String,
    /// The relay to use for short codes (the player's setting); a full
    /// invite names its own.
    pub relay: Option<String>,
    /// The game's protocol number and version name (a relay refuses a
    /// guest whose protocol differs from the host's).
    pub protocol: i32,
    pub game: String,
    pub path: PathMode,
}

/// Why joining failed, in words for the player.
#[derive(Debug)]
pub enum JoinError {
    BadCode(crate::invite::CodeError),
    /// A short code without a relay to resolve it.
    NeedsRelay,
    /// None of the code's endpoints answered, and no relay to fall back to.
    NotReachable,
    Unreachable(String),
    UnknownCode,
    Ended,
    VersionMismatch {
        host: Option<String>,
        mine: String,
    },
    HostUnreachable,
    Full,
    RateLimited,
    WrongCode,
    Failed(String),
}
impl fmt::Display for JoinError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BadCode(e) => write!(f, "{e}"),
            Self::NeedsRelay => write!(
                f,
                "This short code works through your friend's relay: add the relay's address under Join a friend, or ask for the full code."
            ),
            Self::NotReachable => write!(
                f,
                "Couldn't reach your friend's game. Ask them to read the note under their code: they may need UPnP on their router, a forwarded port, a relay or a VPN."
            ),
            Self::Unreachable(e) => write!(f, "Couldn't reach the relay ({e})."),
            Self::UnknownCode => write!(
                f,
                "No world uses this code. Check it, or ask your friend for a new one."
            ),
            Self::Ended => write!(
                f,
                "This invite has ended: your friend stopped sharing or left the world."
            ),
            Self::VersionMismatch {
                host: Some(v),
                mine,
            } => {
                write!(f, "Your friend plays Minecraft {v}; you have {mine}.")
            }
            Self::VersionMismatch { host: None, mine } => {
                write!(
                    f,
                    "Your friend plays another version of Minecraft than {mine}."
                )
            }
            Self::HostUnreachable => write!(f, "Your friend's game didn't answer."),
            Self::Full => write!(
                f,
                "Your friend's world is full, or the invite service is busy. Try again in a moment."
            ),
            Self::RateLimited => {
                write!(f, "Too many attempts. Wait a few minutes, then try again.")
            }
            Self::WrongCode => write!(f, "The code doesn't match. Check the last four characters."),
            Self::Failed(e) => write!(f, "Couldn't connect to your friend ({e})."),
        }
    }
}
impl std::error::Error for JoinError {}

/// A settled session, ready to carry the game's connection.
pub struct Tunnel {
    secure: Secure,
    /// Whether the stream goes straight to the host (else through the relay).
    pub direct: bool,
}

/// Join with what the player pasted: a full invite is dialled directly
/// (every endpoint at once), then through the relay it names, if any; a
/// short code goes through the configured relay. Blocks for a few seconds.
pub fn join(config: &GuestConfig) -> Result<Tunnel, JoinError> {
    match crate::invite::parse_detailed(&config.code).map_err(JoinError::BadCode)? {
        Parsed::Short(code) => {
            let relay = config.relay.as_deref().ok_or(JoinError::NeedsRelay)?;
            join_relay(config, relay, &code)
        }
        Parsed::Long(invite) => {
            if config.path != PathMode::RelayOnly
                && !invite.endpoints.is_empty()
                && let Some(t) = join_direct(
                    &invite,
                    invite.relay.is_some() && config.path != PathMode::DirectOnly,
                )?
            {
                return Ok(t);
            }
            match &invite.relay {
                Some(r) if config.path != PathMode::DirectOnly => {
                    join_relay(config, &r.address, &r.code)
                }
                _ => Err(JoinError::NotReachable),
            }
        }
    }
}

/// Dial every endpoint of the code; each that answers as a Horizon host gets the handshake in
/// turn until one takes the code. Another host can answer first (a housemate's game at the same
/// private address on the default port), so a failed handshake moves on rather than ending the
/// join. None when no endpoint took it and `fallback` (the relay) is still to be tried; else the
/// last failure.
fn join_direct(invite: &Invite, fallback: bool) -> Result<Option<Tunnel>, JoinError> {
    let (tx, rx) = channel();
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut session = [0u8; 8];
    crate::fill_random(&mut session).map_err(|e| JoinError::Failed(e.to_string()))?;
    let session = u64::from_le_bytes(session);
    for &endpoint in &invite.endpoints {
        let tx = tx.clone();
        let _ = std::thread::Builder::new()
            .name("horizon-friends-dial".into())
            .spawn(move || {
                if let Ok(mut s) = TcpStream::connect_timeout(&endpoint, Duration::from_secs(4))
                    && direct::code_hello(&mut s, session).is_ok()
                {
                    let _ = tx.send(s);
                }
            });
    }
    drop(tx);
    let mut failure = None;
    while let Ok(mut stream) = rx.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
        let key = match crypto::exchange(&mut stream, &invite.secret_text(), "direct", session) {
            Ok(key) => key,
            Err(e) => {
                failure.get_or_insert(JoinError::Failed(e.to_string()));
                continue;
            }
        };
        match session::secure(stream, &key, Role::Guest) {
            Ok(secure) => {
                return Ok(Some(Tunnel {
                    secure,
                    direct: true,
                }));
            }
            // A wrong code says more than any other failure: it is the one kept.
            Err(e) if crypto::is_wrong_code(&e) => failure = Some(JoinError::WrongCode),
            Err(e) => {
                failure.get_or_insert(JoinError::Failed(e.to_string()));
            }
        }
    }
    match failure {
        Some(failure) if !fallback => Err(failure),
        _ => Ok(None),
    }
}

/// Through a relay with its short code (its own direct race, then the
/// relay's byte stream).
fn join_relay(config: &GuestConfig, relay_url: &str, code: &Code) -> Result<Tunnel, JoinError> {
    let relays = resolve_relay(relay_url).map_err(|e| JoinError::Unreachable(e.to_string()))?;
    let direct_allowed = config.path != PathMode::RelayOnly;
    let (listeners, port) = if direct_allowed {
        match direct::listen(0) {
            Ok((l, p)) => (l, Some(p)),
            Err(_) => (Vec::new(), None),
        }
    } else {
        (Vec::new(), None)
    };
    let mut last = io::Error::other("no address");
    let mut stream = None;
    for relay in &relays {
        match direct::connect_from(port, *relay, Duration::from_secs(5)) {
            Ok(s) => {
                stream = Some((s, *relay));
                break;
            }
            Err(e) => last = e,
        }
    }
    let (mut stream, relay) = stream.ok_or_else(|| JoinError::Unreachable(last.to_string()))?;
    let failed = |e: io::Error| JoinError::Failed(e.to_string());
    stream
        .set_read_timeout(Some(Duration::from_secs(15)))
        .map_err(failed)?;
    let _ = stream.set_nodelay(true);
    wire::send(
        &mut stream,
        &ToRelay::Join {
            v: wire::VERSION,
            nameplate: code.nameplate.clone(),
            protocol: config.protocol,
            game: config.game.clone(),
        },
    )
    .map_err(failed)?;
    let (session, observed) = match wire::recv::<_, FromRelay>(&mut stream).map_err(failed)? {
        FromRelay::Bridged { session, observed } => (session, observed),
        FromRelay::Error { code, game } => {
            return Err(match code {
                ErrorCode::UnknownCode => JoinError::UnknownCode,
                ErrorCode::Ended => JoinError::Ended,
                ErrorCode::VersionMismatch => JoinError::VersionMismatch {
                    host: game,
                    mine: config.game.clone(),
                },
                ErrorCode::HostUnreachable => JoinError::HostUnreachable,
                ErrorCode::Full => JoinError::Full,
                ErrorCode::RateLimited => JoinError::RateLimited,
                ErrorCode::BadRequest => JoinError::Failed("the relay refused the request".into()),
            });
        }
        other => return Err(JoinError::Failed(format!("unexpected answer {other:?}"))),
    };
    let key = crypto::exchange(&mut stream, &code.secret, &code.nameplate, session)
        .map_err(|_| JoinError::HostUnreachable)?;
    let relayed = session::secure(stream, &key, Role::Guest).map_err(|e| {
        if crypto::is_wrong_code(&e) {
            JoinError::WrongCode
        } else {
            JoinError::HostUnreachable
        }
    })?;
    let observed = observed.and_then(|o| o.parse::<SocketAddr>().ok());
    let candidates = port.map_or_else(Vec::new, |p| direct::candidates(p, Some(relay), observed));
    let settled = session::settle(
        relayed,
        &key,
        Role::Guest,
        config.path,
        session,
        port,
        candidates,
        Box::new(move |tx, deadline, stop| direct::accept(listeners, session, deadline, stop, tx)),
    )
    .map_err(|e| JoinError::Failed(e.to_string()))?;
    Ok(Tunnel {
        secure: settled.secure,
        direct: settled.direct,
    })
}

/// The bytes a client sends first on the loopback port, before its handshake: what
/// [`Tunnel::serve_loopback`] handed it.
pub const TOKEN_LEN: usize = 16;

/// The session offered on a loopback port ([`Tunnel::serve_loopback`]).
pub struct Loopback {
    pub port: u16,
    /// What the connection must send first: any other local process or a web page that finds
    /// the port gets nothing.
    pub token: [u8; TOKEN_LEN],
    /// Ends when the connection does, or when none came in time.
    pub thread: JoinHandle<()>,
}

impl Tunnel {
    /// Offer the session on `127.0.0.1:port` for one connection (the game's client), the first
    /// that sends `token` within a second of connecting. Another process on the computer could
    /// otherwise reach the port first and take the authenticated session to the host's server.
    pub fn serve_loopback(self) -> io::Result<Loopback> {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))?;
        let port = listener.local_addr()?.port();
        listener.set_nonblocking(true)?;
        let mut token = [0u8; TOKEN_LEN];
        crate::fill_random(&mut token)?;
        let thread = std::thread::Builder::new()
            .name("horizon-friends-guest".into())
            .spawn(move || {
                let deadline = std::time::Instant::now() + Duration::from_secs(30);
                let plain = loop {
                    match listener.accept() {
                        Ok((s, from)) if from.ip().is_loopback() => {
                            if sends_token(&s, &token) {
                                break Some(s);
                            }
                            // Anything else is dropped, and the port waits on.
                        }
                        Ok(_) => {}
                        Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                            if std::time::Instant::now() > deadline {
                                break None;
                            }
                            std::thread::sleep(Duration::from_millis(10));
                        }
                        Err(_) => break None,
                    }
                };
                drop(listener);
                let (reader, writer) = self.secure;
                let Some(plain) = plain else {
                    let _ = reader.get_ref().shutdown(std::net::Shutdown::Both);
                    return;
                };
                if let Ok(t) = crate::pump(plain, reader, writer) {
                    let _ = t.join();
                }
            })?;
        Ok(Loopback {
            port,
            token,
            thread,
        })
    }
}

/// Whether `s` sends `token` within a second, compared in constant time. Leaves it blocking.
fn sends_token(mut s: &TcpStream, token: &[u8; TOKEN_LEN]) -> bool {
    use std::io::Read;
    let mut got = [0u8; TOKEN_LEN];
    let read = s.set_nonblocking(false).is_ok()
        && s.set_read_timeout(Some(Duration::from_secs(1))).is_ok()
        && s.read_exact(&mut got).is_ok()
        && s.set_read_timeout(None).is_ok();
    read && got.iter().zip(token).fold(0u8, |acc, (a, b)| acc | (a ^ b)) == 0
}
