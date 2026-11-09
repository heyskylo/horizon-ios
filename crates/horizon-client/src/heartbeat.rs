//! The anonymous "I'm playing" ping (`horizon_telemetry::live`, docs/telemetry.md) behind the
//! launcher's "N playing now". While the game runs, a thread of its own sends a random session
//! ID made here and kept only in memory, the version, the platform and whether a LAN or
//! play-with-friends world is open, every minute and at once when that last one changes.
//! It sends nothing when sending is turned off (`HORIZON_TELEMETRY_URL=off`) or with
//! `HORIZON_NO_HEARTBEAT=1` (automated runs).
//!
//! It plugs into the game through its hooks ([`HOOKS`], `horizon_hooks`), as a Horizon crate
//! does: it leaves the engine for the Horizon layer when the official executable ships (layer
//! plan M5, docs/layers.md).
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::Thread;
use std::time::Duration;

use horizon_hooks::{Hooks, Started};
use horizon_telemetry::live::{self, Ping};

/// A world is open to LAN or to friends.
static LAN: AtomicBool = AtomicBool::new(false);
static PINGER: OnceLock<Thread> = OnceLock::new();

/// After a failure the next ping waits longer, doubling up to this.
const MAX_WAIT: Duration = Duration::from_secs(600);

/// The ping's hooks, for `horizon_hooks::register`.
pub static HOOKS: Heartbeat = Heartbeat;

pub struct Heartbeat;

impl Hooks for Heartbeat {
    /// Starts the pings, once: someone is playing (not a dedicated server, a benchmark or an
    /// automated run).
    fn started(&self, game: &Started<'_>) {
        start(game.version);
    }

    /// A change is sent at once.
    fn published(&self, open: bool) {
        LAN.store(open, Ordering::Relaxed);
        if let Some(pinger) = PINGER.get() {
            pinger.unpark();
        }
    }
}

fn start(version: &str) {
    if PINGER.get().is_some() || std::env::var_os("HORIZON_NO_HEARTBEAT").is_some() {
        return;
    }
    let Some(endpoint) = horizon_telemetry::endpoint() else {
        return;
    };
    let version = version.to_owned();
    let spawned = std::thread::Builder::new()
        .name("Heartbeat".into())
        .spawn(move || run(&endpoint, &version));
    match spawned {
        Ok(handle) => {
            let _ = PINGER.set(handle.thread().clone());
        }
        Err(e) => log::warn!("Horizon: no live count ping: {e}"),
    }
}

fn run(endpoint: &str, version: &str) {
    let session = live::session_id();
    let mut wait = live::INTERVAL;
    let mut failed = false;
    loop {
        let ping = Ping::new(&session, version, LAN.load(Ordering::Relaxed));
        match live::send(endpoint, &ping) {
            Ok(()) => {
                wait = live::INTERVAL;
                failed = false;
            }
            Err(e) => {
                // Said once, not every minute the server is away.
                if !failed {
                    log::info!("Horizon: the live count ping failed ({e}); trying again later");
                }
                failed = true;
                wait = (wait * 2).min(MAX_WAIT);
            }
        }
        // Woken early by a change of `published`; a spurious wake only sends one ping more.
        std::thread::park_timeout(wait);
    }
}
