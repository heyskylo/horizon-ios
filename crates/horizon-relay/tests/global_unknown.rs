//! The relay-wide budget for unknown codes refuses unknown codes only: once a few sites have spent
//! it, a guest with a real code still gets through to its host.
use horizon_friends::wire::{self, ErrorCode, FromRelay, ToRelay};
use horizon_relay::{Config, Relay};
use std::io::BufReader;
use std::net::{SocketAddr, TcpStream};
use std::time::Duration;

const PROTOCOL: i32 = 777;

fn connect(address: SocketAddr) -> TcpStream {
    let s = TcpStream::connect(address).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    s
}

fn join(address: SocketAddr, nameplate: &str) -> FromRelay {
    let mut s = connect(address);
    let hello = ToRelay::Join {
        v: wire::VERSION,
        nameplate: nameplate.into(),
        protocol: PROTOCOL,
        game: "26.3".into(),
    };
    wire::send(&mut s, &hello).unwrap();
    wire::recv(&mut s).unwrap()
}

#[test]
fn a_spent_global_budget_refuses_unknown_codes_but_not_real_ones() {
    let (_relay, address, _) = Relay::spawn_local(Config {
        per_address: false,
        global_unknown_per_min: 0.01,
        global_unknown_burst: 1.0,
        attach_wait: Duration::from_millis(300),
        ..Config::default()
    })
    .unwrap();
    let mut host = connect(address);
    let hello = ToRelay::Host {
        v: wire::VERSION,
        protocol: PROTOCOL,
        game: "26.3".into(),
    };
    wire::send(&mut host, &hello).unwrap();
    let mut reader = BufReader::new(host.try_clone().unwrap());
    let FromRelay::Registered { nameplate, .. } =
        wire::recv_buffered::<_, FromRelay>(&mut reader).unwrap()
    else {
        panic!("not registered");
    };
    // Two codes no host holds (whichever differs from the host's): the first spends the budget.
    let unknown: Vec<String> = (1..4)
        .map(horizon_friends::code::nameplate)
        .filter(|n| *n != nameplate)
        .take(2)
        .collect();
    let code = |answer: FromRelay| match answer {
        FromRelay::Error { code, .. } => Some(code),
        _ => None,
    };
    assert_eq!(
        code(join(address, &unknown[0])),
        Some(ErrorCode::UnknownCode)
    );
    assert_eq!(
        code(join(address, &unknown[1])),
        Some(ErrorCode::RateLimited)
    );
    // The real code is matched with its host, not refused.
    assert_ne!(
        code(join(address, &nameplate)),
        Some(ErrorCode::RateLimited)
    );
}
