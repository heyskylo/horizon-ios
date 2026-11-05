//! One client address can't take every host slot by registering and
//! holding its control connections open.
use horizon_friends::wire::{self, ErrorCode, FromRelay, ToRelay};
use horizon_relay::{Config, Relay, limits::Rates};
use std::{
    net::{SocketAddr, TcpStream},
    time::{Duration, Instant},
};

/// Register as a host: the open control connection, or the refusal.
fn register(relay: SocketAddr) -> Result<TcpStream, ErrorCode> {
    let mut s = TcpStream::connect(relay).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    let hello = ToRelay::Host {
        v: wire::VERSION,
        protocol: 777,
        game: "26.3".into(),
    };
    wire::send(&mut s, &hello).unwrap();
    match wire::recv::<_, FromRelay>(&mut s).unwrap() {
        FromRelay::Registered { .. } => Ok(s),
        FromRelay::Error { code, .. } => Err(code),
        other => panic!("unexpected answer {other:?}"),
    }
}

#[test]
fn an_address_holds_only_so_many_hosts() {
    let per_address = Rates::default().hosts_per_address;
    let (relay, address, _) = Relay::spawn_local(Config {
        rates: Rates {
            register_per_min: 600.0,
            register_burst: 100.0,
            ..Rates::default()
        },
        ..Config::default()
    })
    .unwrap();
    let mut held: Vec<TcpStream> = (0..per_address)
        .map(|_| register(address).expect("a host within the cap"))
        .collect();
    assert_eq!(register(address).err(), Some(ErrorCode::Full));
    assert_eq!(relay.counts().hosts, per_address);
    // One host leaves: its slot is free once the relay sees it go.
    drop(held.pop());
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        match register(address) {
            Ok(s) => {
                held.push(s);
                break;
            }
            Err(ErrorCode::Full) => {
                assert!(Instant::now() < deadline, "the slot never came back");
                std::thread::sleep(Duration::from_millis(100));
            }
            Err(e) => panic!("refused: {e:?}"),
        }
    }
    assert_eq!(register(address).err(), Some(ErrorCode::Full));
    relay.stop();
}
