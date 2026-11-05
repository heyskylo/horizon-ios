//! A PROXY protocol line counts only from the load balancers in `proxy_from`.
use horizon_relay::{Config, Net, Relay};
use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::Duration;

fn whoami(proxy_from: &str) -> String {
    let (_relay, address, _) = Relay::spawn_local(Config {
        proxy_protocol: true,
        proxy_from: Net::parse_list(proxy_from).unwrap(),
        ..Config::default()
    })
    .unwrap();
    let mut s = TcpStream::connect(address).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    s.write_all(b"PROXY TCP4 203.0.113.7 10.0.0.1 51000 7000\r\nGET /whoami HTTP/1.1\r\n\r\n")
        .unwrap();
    let mut answer = String::new();
    let _ = s.read_to_string(&mut answer);
    answer
}

#[test]
fn a_listed_load_balancer_names_the_client() {
    assert!(whoami("127.0.0.0/8").contains("203.0.113.7:51000"));
}

#[test]
fn anyone_else_is_closed_unanswered() {
    assert_eq!(whoami("10.0.0.0/8"), "");
}

#[test]
fn an_unlisted_peer_is_closed_before_it_sends_a_line() {
    let (relay, address, worker) = Relay::spawn_local(Config {
        proxy_protocol: true,
        proxy_from: Net::parse_list("10.0.0.0/8").unwrap(),
        first_line: Duration::from_secs(5),
        ..Config::default()
    })
    .unwrap();
    let mut stream = TcpStream::connect(address).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(1)))
        .unwrap();
    let read = stream.read(&mut [0u8; 1]);
    drop(stream);
    relay.stop();
    worker.join().unwrap();
    assert!(
        matches!(read, Ok(0)),
        "an untrusted peer must be rejected without consuming the first-line deadline: {read:?}"
    );
}

#[test]
fn a_mapped_load_balancer_address_matches_the_listener_peer() {
    assert!(whoami("::ffff:127.0.0.1").contains("203.0.113.7:51000"));
    let net = Net::parse("::ffff:192.0.2.0/120").unwrap();
    for inside in ["192.0.2.7", "::ffff:192.0.2.7"] {
        assert!(net.contains(inside.parse().unwrap()), "{inside}");
    }
    assert!(!net.contains("192.0.3.7".parse().unwrap()));
    assert!(!net.contains("2001:db8::7".parse().unwrap()));
}
