//! `horizon-relay`: listens on `$PORT` (7000 when unset; `RELAY_BIND` for one
//! address), limits from the `RELAY_*` variables (docs/play-with-friends.md).
//! Prints counts every five minutes, nothing else.
use horizon_relay::{Config, Relay};
use std::{net::TcpListener, time::Duration};

fn main() {
    let port: u16 = std::env::var("PORT")
        .ok()
        .and_then(|p| p.trim().parse().ok())
        .unwrap_or(7000);
    // RELAY_BIND picks one address (e.g. only a tailnet); else dual-stack
    // where the system allows it, else IPv4.
    let bind = std::env::var("RELAY_BIND")
        .ok()
        .filter(|b| !b.trim().is_empty());
    let listener = match bind {
        Some(address) => TcpListener::bind((address.trim(), port)),
        None => TcpListener::bind(("::", port)).or_else(|_| TcpListener::bind(("0.0.0.0", port))),
    }
    .unwrap_or_else(|e| {
        eprintln!("horizon-relay: can't listen on port {port}: {e}");
        std::process::exit(1);
    });
    let config = Config::from_env().unwrap_or_else(|e| {
        eprintln!("horizon-relay: {e}");
        std::process::exit(1);
    });
    println!(
        "horizon-relay {} listening on port {port} (hosts {}, bridges {}, {} KiB/s per stream, per-address limits {})",
        env!("CARGO_PKG_VERSION"),
        config.max_hosts,
        config.max_bridges,
        (config.session_rate / 1024.0) as u64,
        if config.per_address { "on" } else { "off" },
    );
    let relay = Relay::new(config);
    let r = relay.clone();
    std::thread::spawn(move || {
        loop {
            std::thread::sleep(Duration::from_secs(300));
            println!("{}", r.counts().json());
        }
    });
    relay.serve(listener);
}
