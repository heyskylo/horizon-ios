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
