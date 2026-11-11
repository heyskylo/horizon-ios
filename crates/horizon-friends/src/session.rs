//! What both ends do once the relay has bridged them and the key is agreed:
//! protect the relayed stream, swap candidates over it, race direct streams
//! for [`direct::RACE`] and settle on one. The guest decides and says so;
//! the host follows.
use crate::{
    PathMode,
    crypto::{self, Frame, Key, Role, SecureReader, SecureWriter},
    direct,
};
use serde::{Deserialize, Serialize};
use std::{
    io,
    net::{SocketAddr, TcpStream},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{Sender, channel},
    },
    time::{Duration, Instant},
};

pub(crate) type Secure = (SecureReader<TcpStream>, SecureWriter<TcpStream>);

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "t", rename_all = "snake_case")]
enum Control {
    /// Where this end can be reached; `direct` false when it won't try.
    Hello {
        direct: bool,
        candidates: Vec<String>,
    },
    /// Sent by the guest as the first frame of the direct stream it keeps.
    Use,
    /// The guest's decision, on the relayed stream.
    Path { direct: bool },
}

/// Streams of the direct race a session authenticates, at most.
const MAX_RACE_STREAMS: usize = 16;

fn send(w: &mut SecureWriter<TcpStream>, c: &Control) -> io::Result<()> {
    w.write_control(&serde_json::to_vec(c).map_err(io::Error::other)?)
}
fn recv(r: &mut SecureReader<TcpStream>) -> io::Result<Control> {
    match r.read_frame()? {
        Some(Frame::Control(c)) => {
            serde_json::from_slice(&c).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
        }
        Some(Frame::Data(_)) => Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "data before the path",
        )),
        None => Err(io::ErrorKind::UnexpectedEof.into()),
    }
}

pub(crate) fn secure(stream: TcpStream, key: &Key, role: Role) -> io::Result<Secure> {
    crypto::secure(stream.try_clone()?, stream, key, role)
}

pub(crate) struct Established {
    pub secure: Secure,
    pub direct: bool,
}

/// Start accepting this session's inbound direct streams (already
/// introduced) into the sender until the deadline or the stop flag.
pub(crate) type Listen<'a> = Box<dyn FnOnce(Sender<TcpStream>, Instant, Arc<AtomicBool>) + 'a>;

/// Settle a session on the relayed stream `relay` (already protected) or a
/// direct one.
pub(crate) fn settle(
    relay: Secure,
    key: &Key,
    role: Role,
    mode: PathMode,
    session: u64,
    direct_port: Option<u16>,
    candidates: Vec<SocketAddr>,
    listen: Listen<'_>,
) -> io::Result<Established> {
    let (mut rr, mut rw) = relay;
    rr.get_ref()
        .set_read_timeout(Some(Duration::from_secs(15)))?;
    let wants = mode != PathMode::RelayOnly && direct_port.is_some();
    send(
        &mut rw,
        &Control::Hello {
            direct: wants,
            candidates: if wants {
                candidates.iter().map(SocketAddr::to_string).collect()
            } else {
                Vec::new()
            },
        },
    )?;
    let Control::Hello {
        direct: they_want,
        candidates: theirs,
    } = recv(&mut rr)?
    else {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "expected hello"));
    };
    let theirs: Vec<SocketAddr> = theirs
        .iter()
        .filter_map(|a| a.parse().ok())
        .take(16)
        .collect();
    let stop = Arc::new(AtomicBool::new(false));
    let (atx, arx) = channel::<Secure>();
    if wants && they_want {
        let deadline = Instant::now() + direct::RACE;
        let (raw_tx, raw_rx) = channel::<TcpStream>();
        listen(raw_tx.clone(), deadline, stop.clone());
        direct::dial(direct_port, theirs, session, deadline, stop.clone(), raw_tx);
        let key = key.clone();
        let _ = std::thread::Builder::new()
            .name("horizon-friends-auth".into())
            .spawn(move || {
                // A few streams a session, each with one deadline for all of its handshake: whoever
                // knows a live session id could otherwise open stream after stream to the direct
                // port and drip bytes, a thread for each.
                for raw in raw_rx.into_iter().take(MAX_RACE_STREAMS) {
                    let (key, atx) = (key.clone(), atx.clone());
                    let _ = std::thread::Builder::new()
                        .name("horizon-friends-auth".into())
                        .spawn(move || {
                            let Ok(_deadline) = crate::deadline(&raw, Duration::from_secs(6))
                            else {
                                return;
                            };
                            let Ok(mut s) = secure(raw, &key, role) else {
                                return;
                            };
                            // The host only keeps the stream the guest marks.
                            if role == Role::Host && !matches!(recv(&mut s.0), Ok(Control::Use)) {
                                return;
                            }
                            let _ = atx.send(s);
                        });
                }
            });
    } else {
        drop(atx);
    }
    let finish = |(r, w): Secure, direct: bool| -> io::Result<Established> {
        r.get_ref().set_read_timeout(None)?;
        Ok(Established {
            secure: (r, w),
            direct,
        })
    };
    let fallback = |rr: SecureReader<TcpStream>, rw: SecureWriter<TcpStream>| {
        if mode == PathMode::DirectOnly {
            Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "no direct connection",
            ))
        } else {
            finish((rr, rw), false)
        }
    };
    match role {
        Role::Guest => {
            let chosen = if wants && they_want {
                let left = direct::RACE + Duration::from_millis(500);
                arx.recv_timeout(left).ok()
            } else {
                None
            };
            stop.store(true, Ordering::Relaxed);
            if let Some((r, mut w)) = chosen
                && send(&mut w, &Control::Use).is_ok()
            {
                send(&mut rw, &Control::Path { direct: true })?;
                let _ = rr.get_ref().shutdown(std::net::Shutdown::Both);
                return finish((r, w), true);
            }
            send(&mut rw, &Control::Path { direct: false })?;
            fallback(rr, rw)
        }
        Role::Host => {
            let Control::Path { direct } = recv(&mut rr)? else {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "expected the path",
                ));
            };
            if direct {
                let chosen = arx.recv_timeout(Duration::from_secs(5));
                stop.store(true, Ordering::Relaxed);
                let _ = rr.get_ref().shutdown(std::net::Shutdown::Both);
                let chosen = chosen.map_err(|_| {
                    io::Error::new(io::ErrorKind::TimedOut, "the direct stream vanished")
                })?;
                return finish(chosen, true);
            }
            stop.store(true, Ordering::Relaxed);
            fallback(rr, rw)
        }
    }
}
