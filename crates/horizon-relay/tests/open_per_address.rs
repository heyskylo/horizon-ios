//! One client address can't fill the relay with silent connections: past its cap each new one is
//! closed at once, not held for the first-line wait with one of the relay's global slots.
use horizon_relay::{Config, Relay, limits::Rates};
use std::{
    io::Read,
    net::TcpStream,
    time::{Duration, Instant},
};

#[test]
fn silent_connections_past_an_address_cap_close_at_once() {
    let (_relay, address, _) = Relay::spawn_local(Config {
        rates: Rates {
            open_per_address: 4,
            ..Rates::default()
        },
        first_line: Duration::from_secs(10),
        ..Config::default()
    })
    .unwrap();
    // Four silent connections: held open (the relay waits for their first line).
    let held: Vec<TcpStream> = (0..4)
        .map(|_| TcpStream::connect(address).unwrap())
        .collect();
    std::thread::sleep(Duration::from_millis(300));
    for s in &held {
        s.set_read_timeout(Some(Duration::from_millis(200)))
            .unwrap();
        let mut b = [0u8; 1];
        assert!(
            matches!((&*s).read(&mut b), Err(e) if e.kind() == std::io::ErrorKind::WouldBlock
                || e.kind() == std::io::ErrorKind::TimedOut),
            "within the cap, still open"
        );
    }
    // The fifth is closed at once, long before the 10 s wait.
    let mut over = TcpStream::connect(address).unwrap();
    over.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    let started = Instant::now();
    assert_eq!(over.read(&mut [0u8; 1]).unwrap_or(0), 0);
    assert!(
        started.elapsed() < Duration::from_secs(3),
        "{:?}",
        started.elapsed()
    );
    // One leaves: its place comes back once the relay sees it go.
    drop(held);
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let mut s = TcpStream::connect(address).unwrap();
        s.set_read_timeout(Some(Duration::from_millis(500)))
            .unwrap();
        match s.read(&mut [0u8; 1]) {
            Err(_) => break, // still open after half a second: let in
            Ok(_) => assert!(Instant::now() < deadline, "the places never came back"),
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}
