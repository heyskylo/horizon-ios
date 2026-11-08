//! `max_bridges` counts from the moment a guest is matched with its host, and every way out of a
//! join gives the slot back, spoken to the relay over raw sockets.
use horizon_friends::wire::{self, ErrorCode, FromRelay, ToRelay};
use horizon_relay::{Config, Relay};
use std::io::BufReader;
use std::net::{Shutdown, SocketAddr, TcpStream};
use std::time::{Duration, Instant};

const PROTOCOL: i32 = 777;

fn relay(max_bridges: usize) -> (Relay, SocketAddr) {
    let (relay, address, _) = Relay::spawn_local(Config {
        max_bridges,
        per_address: false,
        attach_wait: Duration::from_millis(400),
        ..Config::default()
    })
    .unwrap();
    (relay, address)
}

fn connect(address: SocketAddr) -> TcpStream {
    let s = TcpStream::connect(address).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    s
}

/// A registered host's control connection (reader, writer), its nameplate and token.
fn host(address: SocketAddr) -> (BufReader<TcpStream>, TcpStream, String, String) {
    let mut s = connect(address);
    wire::send(
        &mut s,
        &ToRelay::Host {
            v: wire::VERSION,
            protocol: PROTOCOL,
            game: "26.3".into(),
        },
    )
    .unwrap();
    let mut reader = BufReader::new(s.try_clone().unwrap());
    match wire::recv_buffered::<_, FromRelay>(&mut reader).unwrap() {
        FromRelay::Registered {
            nameplate, token, ..
        } => (reader, s, nameplate, token),
        other => panic!("{other:?}"),
    }
}

/// A guest's join, sent; the stream to read the relay's answer from.
fn join(address: SocketAddr, nameplate: &str) -> TcpStream {
    let mut s = connect(address);
    wire::send(
        &mut s,
        &ToRelay::Join {
            v: wire::VERSION,
            nameplate: nameplate.into(),
            protocol: PROTOCOL,
            game: "26.3".into(),
        },
    )
    .unwrap();
    s
}

/// The next message that isn't a ping.
fn next(reader: &mut BufReader<TcpStream>) -> FromRelay {
    loop {
        match wire::recv_buffered::<_, FromRelay>(reader).unwrap() {
            FromRelay::Ping => continue,
            m => return m,
        }
    }
}

fn answer(s: &mut TcpStream) -> FromRelay {
    wire::recv(s).unwrap()
}

fn settles_to_zero(relay: &Relay) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while relay.counts().bridges != 0 {
        assert!(
            Instant::now() < deadline,
            "bridges stuck at {}",
            relay.counts().bridges
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn a_waiting_join_holds_its_slot_and_a_timeout_gives_it_back() {
    let (relay, address) = relay(1);
    let (mut control, _keep, nameplate, _) = host(address);
    // The first guest is matched: the host is asked to attach, and doesn't.
    let mut first = join(address, &nameplate);
    assert!(matches!(next(&mut control), FromRelay::Join { .. }));
    assert_eq!(relay.counts().bridges, 1, "reserved at the match");
    // A second guest meanwhile finds the relay full.
    let mut second = join(address, &nameplate);
    assert!(matches!(
        answer(&mut second),
        FromRelay::Error {
            code: ErrorCode::Full,
            ..
        }
    ));
    // The first one's wait ends: refused, and the slot is back.
    assert!(matches!(
        answer(&mut first),
        FromRelay::Error {
            code: ErrorCode::HostUnreachable,
            ..
        }
    ));
    settles_to_zero(&relay);
    // So the next guest is matched again.
    let mut third = join(address, &nameplate);
    assert!(matches!(next(&mut control), FromRelay::Join { .. }));
    assert!(matches!(
        answer(&mut third),
        FromRelay::Error {
            code: ErrorCode::HostUnreachable,
            ..
        }
    ));
    settles_to_zero(&relay);
}

#[test]
fn a_bridge_gives_its_slot_back_when_either_end_closes() {
    let (relay, address) = relay(1);
    let (mut control, _keep, nameplate, token) = host(address);
    let mut guest = join(address, &nameplate);
    let FromRelay::Join { session } = next(&mut control) else {
        panic!()
    };
    let mut attach = connect(address);
    wire::send(
        &mut attach,
        &ToRelay::Attach {
            v: wire::VERSION,
            session,
            token,
        },
    )
    .unwrap();
    assert!(matches!(answer(&mut guest), FromRelay::Bridged { .. }));
    assert!(matches!(answer(&mut attach), FromRelay::Bridged { .. }));
    assert_eq!(relay.counts().bridges, 1);
    guest.shutdown(Shutdown::Both).unwrap();
    settles_to_zero(&relay);
}

#[test]
fn a_wrong_attach_or_a_refusal_holds_no_slot() {
    let (relay, address) = relay(1);
    let (mut control, _keep, nameplate, _) = host(address);
    // Refused before any match: an unknown code, another protocol.
    let mut unknown = join(address, "ZZZZ");
    assert!(matches!(
        answer(&mut unknown),
        FromRelay::Error {
            code: ErrorCode::UnknownCode,
            ..
        }
    ));
    let mut other = connect(address);
    wire::send(
        &mut other,
        &ToRelay::Join {
            v: wire::VERSION,
            nameplate: nameplate.clone(),
            protocol: PROTOCOL + 1,
            game: "x".into(),
        },
    )
    .unwrap();
    assert!(matches!(
        answer(&mut other),
        FromRelay::Error {
            code: ErrorCode::VersionMismatch,
            ..
        }
    ));
    assert_eq!(relay.counts().bridges, 0);
    // Matched, then an attach with the wrong token: refused, and the join's wait runs out.
    let mut guest = join(address, &nameplate);
    let FromRelay::Join { session } = next(&mut control) else {
        panic!()
    };
    let mut forged = connect(address);
    wire::send(
        &mut forged,
        &ToRelay::Attach {
            v: wire::VERSION,
            session,
            token: "0".repeat(32),
        },
    )
    .unwrap();
    assert!(matches!(
        answer(&mut forged),
        FromRelay::Error {
            code: ErrorCode::BadRequest,
            ..
        }
    ));
    assert!(matches!(
        answer(&mut guest),
        FromRelay::Error {
            code: ErrorCode::HostUnreachable,
            ..
        }
    ));
    settles_to_zero(&relay);
}
