//! Which other players play an emote, from the cosmetics service: the game names the Horizon
//! players in view each tick (`Feed::ask`, at most `BATCH`), a thread of its own asks
//! `POST /v1/emotes` about them at most once a `POLL`, and the frame reads the answer
//! (`Feed::playing`). Nobody in view, nothing is asked. A failure waits 15 s, doubling to 10
//! minutes, and ends the emotes shown.
//!
//! `POST /v1/emotes {"players": ["<uuid hex>", …]}` answers
//! `{"playing": {"<uuid hex>": {"id", "hash", "ms"}}}`: the emote each plays and how long ago it
//! started. The item files come from `GET /v1/items/<id>?hash=`, kept in memory.

use crate::{BATCH, Item, Slot, Transport};
use std::collections::HashMap;
use std::sync::{Arc, Condvar, Mutex, PoisonError};
use std::time::{Duration, Instant};

/// The shortest time between two requests.
pub const POLL: Duration = Duration::from_secs(1);
/// A list of players in view counts this long after the game last named it.
const FRESH: Duration = Duration::from_secs(2);
const BACKOFF: (Duration, Duration) = (Duration::from_secs(15), Duration::from_secs(600));

/// An emote another player plays: the item, and when it started on this machine's clock.
#[derive(Clone, Debug)]
pub struct Seen {
    pub item: Arc<Item>,
    pub started: Instant,
}

#[derive(Default)]
struct State {
    want: Vec<u128>,
    asked_at: Option<Instant>,
    playing: HashMap<u128, Seen>,
    /// By `id@hash`; `None`: unreadable or not an emote.
    items: HashMap<String, Option<Arc<Item>>>,
    next: Option<Instant>,
    wait: Option<Duration>,
    stop: bool,
}

struct Shared {
    state: Mutex<State>,
    wake: Condvar,
}

/// The game's handle; dropping it stops the thread.
pub struct Feed {
    shared: Arc<Shared>,
}

impl Feed {
    pub fn new(transport: impl Transport) -> Self {
        let shared = Arc::new(Shared {
            state: Mutex::default(),
            wake: Condvar::new(),
        });
        let inner = shared.clone();
        let _ = std::thread::Builder::new()
            .name("Cosmetics emotes".into())
            .spawn(move || {
                loop {
                    let wait = {
                        let s = inner.state.lock().unwrap_or_else(PoisonError::into_inner);
                        if s.stop {
                            return;
                        }
                        due(&s, Instant::now())
                    };
                    match wait {
                        Some(Duration::ZERO) => round(&inner, &transport, Instant::now()),
                        // Waiting for the next poll, or for the game to name anyone.
                        Some(wait) => {
                            let s = inner.state.lock().unwrap_or_else(PoisonError::into_inner);
                            let (s, _) = inner
                                .wake
                                .wait_timeout_while(s, wait, |s| !s.stop)
                                .unwrap_or_else(PoisonError::into_inner);
                            drop(s);
                        }
                        None => {
                            let s = inner.state.lock().unwrap_or_else(PoisonError::into_inner);
                            let s = inner
                                .wake
                                .wait_while(s, |s| !s.stop && due(s, Instant::now()).is_none())
                                .unwrap_or_else(PoisonError::into_inner);
                            drop(s);
                        }
                    }
                }
            });
        Self { shared }
    }

    /// The Horizon players in view now; only the first `BATCH` are asked about.
    pub fn ask(&self, players: impl IntoIterator<Item = u128>) {
        let mut want: Vec<u128> = players.into_iter().take(BATCH).collect();
        want.sort_unstable();
        want.dedup();
        let mut s = self
            .shared
            .state
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let was_idle = s.want.is_empty() || s.asked_at.is_none_or(|a| a.elapsed() > FRESH);
        s.asked_at = (!want.is_empty()).then(Instant::now);
        s.want = want;
        if was_idle {
            self.shared.wake.notify_all();
        }
    }

    /// The emote `uuid` plays, as last answered.
    pub fn playing(&self, uuid: u128) -> Option<Seen> {
        let s = self
            .shared
            .state
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        s.playing.get(&uuid).cloned()
    }
}

impl Drop for Feed {
    fn drop(&mut self) {
        self.shared
            .state
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .stop = true;
        self.shared.wake.notify_all();
    }
}

/// How long until the next request: `ZERO` now, `None` while nobody is in view.
fn due(s: &State, now: Instant) -> Option<Duration> {
    let fresh = s.asked_at.is_some_and(|a| now.duration_since(a) <= FRESH);
    if s.want.is_empty() || !fresh {
        return None;
    }
    Some(
        s.next
            .map_or(Duration::ZERO, |n| n.saturating_duration_since(now)),
    )
}

fn hex(uuid: u128) -> String {
    format!("{uuid:032x}")
}

/// One request about the players in view, then the item files it names.
fn round(shared: &Shared, transport: &dyn Transport, now: Instant) {
    let want = {
        let mut s = shared.state.lock().unwrap_or_else(PoisonError::into_inner);
        s.next = Some(now + POLL);
        s.want.clone()
    };
    let body = serde_json::json!({ "players": want.iter().map(|u| hex(*u)).collect::<Vec<_>>() });
    let answer = transport
        .post("/v1/emotes", body.to_string().as_bytes())
        .ok()
        .and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok());
    let Some(answer) = answer else {
        let mut s = shared.state.lock().unwrap_or_else(PoisonError::into_inner);
        let wait = s.wait.map_or(BACKOFF.0, |w| (w * 2).min(BACKOFF.1));
        s.wait = Some(wait);
        s.next = Some(now + wait);
        s.playing.clear();
        return;
    };
    let mut playing = HashMap::new();
    for uuid in &want {
        let Some(e) = answer["playing"].get(hex(*uuid)) else {
            continue;
        };
        let (Some(id), Some(hash), Some(ms)) =
            (e["id"].as_str(), e["hash"].as_str(), e["ms"].as_u64())
        else {
            continue;
        };
        let Some(item) = emote_item(shared, transport, id, hash) else {
            continue;
        };
        // When it started here, keeping a start it already had (the answer's `ms` jitters with
        // the request's own time).
        let started = now.checked_sub(Duration::from_millis(ms)).unwrap_or(now);
        playing.insert(*uuid, (item, started));
    }
    let mut s = shared.state.lock().unwrap_or_else(PoisonError::into_inner);
    s.wait = None;
    let old = std::mem::take(&mut s.playing);
    for (uuid, (item, started)) in playing {
        let started = match old.get(&uuid) {
            Some(seen) if Arc::ptr_eq(&seen.item, &item) => seen.started,
            _ => started,
        };
        s.playing.insert(uuid, Seen { item, started });
    }
}

fn emote_item(
    shared: &Shared,
    transport: &dyn Transport,
    id: &str,
    hash: &str,
) -> Option<Arc<Item>> {
    let safe = |t: &str| {
        !t.is_empty() && t.len() <= 64 && t.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
    };
    if !safe(id) || !safe(hash) {
        return None;
    }
    let key = format!("{id}@{hash}");
    if let Some(known) = shared
        .state
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .items
        .get(&key)
    {
        return known.clone();
    }
    // A network failure is tried again at the next poll; a bad file never.
    let bytes = transport.get(&format!("/v1/items/{id}?hash={hash}")).ok()?;
    let item = Item::parse(&bytes)
        .ok()
        .filter(|i| i.id == id && i.slot == Slot::Emote)
        .map(Arc::new);
    shared
        .state
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .items
        .insert(key, item.clone());
    item
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOD: &str = r#"{"format": 1, "id": "nod", "slot": "emote", "parts": [],
        "emote": {"length": 20, "tracks": [{"part": "head", "keys": [{"t": 10, "rotation": [20, 0, 0]}]}]}}"#;

    /// The service: player 1 plays `nod` started 500 ms ago; logs each request.
    #[derive(Clone, Default)]
    struct Service {
        log: Arc<Mutex<Vec<String>>>,
        down: Arc<std::sync::atomic::AtomicBool>,
    }

    impl Transport for Service {
        fn post(&self, path: &str, body: &[u8]) -> Result<Vec<u8>, String> {
            self.log
                .lock()
                .unwrap()
                .push(format!("POST {path} {}", String::from_utf8_lossy(body)));
            if self.down.load(std::sync::atomic::Ordering::SeqCst) {
                return Err("HTTP 503".into());
            }
            Ok(br#"{"playing": {"00000000000000000000000000000001": {"id": "nod", "hash": "h1", "ms": 500}}}"#.to_vec())
        }
        fn get(&self, path: &str) -> Result<Vec<u8>, String> {
            self.log.lock().unwrap().push(format!("GET {path}"));
            Ok(NOD.as_bytes().to_vec())
        }
    }

    fn shared() -> Shared {
        Shared {
            state: Mutex::default(),
            wake: Condvar::new(),
        }
    }

    #[test]
    fn asks_only_about_the_players_in_view_and_only_once_a_poll() {
        let (service, shared) = (Service::default(), shared());
        let t0 = Instant::now();
        // Nobody named: nothing due.
        assert_eq!(due(&shared.state.lock().unwrap(), t0), None);
        {
            let mut s = shared.state.lock().unwrap();
            s.want = vec![1, 2];
            s.asked_at = Some(t0);
        }
        assert_eq!(due(&shared.state.lock().unwrap(), t0), Some(Duration::ZERO));
        round(&shared, &service, t0);
        let log = service.log.lock().unwrap().clone();
        assert_eq!(
            log,
            [
                r#"POST /v1/emotes {"players":["00000000000000000000000000000001","00000000000000000000000000000002"]}"#,
                "GET /v1/items/nod?hash=h1"
            ]
        );
        let s = shared.state.lock().unwrap();
        let seen = s.playing.get(&1).unwrap();
        assert_eq!(seen.item.id, "nod");
        assert_eq!(t0.duration_since(seen.started), Duration::from_millis(500));
        assert!(!s.playing.contains_key(&2));
        // The next request waits a poll; a list the game stopped naming is not asked about.
        assert_eq!(due(&s, t0 + POLL / 2), Some(POLL / 2));
        assert_eq!(due(&s, t0 + FRESH + POLL), None);
    }

    #[test]
    fn the_same_emote_keeps_its_start_the_file_is_fetched_once_and_failures_back_off() {
        let (service, shared) = (Service::default(), shared());
        let t0 = Instant::now();
        {
            let mut s = shared.state.lock().unwrap();
            s.want = vec![1];
            s.asked_at = Some(t0);
        }
        round(&shared, &service, t0);
        round(&shared, &service, t0 + POLL);
        let started = shared.state.lock().unwrap().playing[&1].started;
        assert_eq!(t0.duration_since(started), Duration::from_millis(500));
        let gets = service
            .log
            .lock()
            .unwrap()
            .iter()
            .filter(|l| l.starts_with("GET"))
            .count();
        assert_eq!(gets, 1);
        // Down: nothing shown, and the next try waits 15 s, then 30 s.
        service
            .down
            .store(true, std::sync::atomic::Ordering::SeqCst);
        round(&shared, &service, t0 + POLL * 2);
        let s = shared.state.lock().unwrap();
        assert!(s.playing.is_empty());
        assert_eq!(s.next, Some(t0 + POLL * 2 + BACKOFF.0));
        drop(s);
        round(&shared, &service, t0 + POLL * 3);
        assert_eq!(
            shared.state.lock().unwrap().next,
            Some(t0 + POLL * 3 + BACKOFF.0 * 2)
        );
    }

    #[test]
    fn a_file_that_is_not_an_emote_is_never_played() {
        struct Hat;
        impl Transport for Hat {
            fn post(&self, _: &str, _: &[u8]) -> Result<Vec<u8>, String> {
                Ok(br#"{"playing": {"00000000000000000000000000000001": {"id": "nod", "hash": "h1", "ms": 0}}}"#.to_vec())
            }
            fn get(&self, _: &str) -> Result<Vec<u8>, String> {
                Ok(NOD
                    .replace(r#""slot": "emote""#, r#""slot": "head""#)
                    .into_bytes())
            }
        }
        let shared = shared();
        {
            let mut s = shared.state.lock().unwrap();
            s.want = vec![1];
            s.asked_at = Some(Instant::now());
        }
        round(&shared, &Hat, Instant::now());
        assert!(shared.state.lock().unwrap().playing.is_empty());
    }

    #[test]
    fn the_thread_answers_the_game() {
        let feed = Feed::new(Service::default());
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            feed.ask([1, 2]);
            if feed.playing(1).is_some() || Instant::now() > deadline {
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        assert_eq!(
            feed.playing(1).map(|s| s.item.id.clone()).as_deref(),
            Some("nod")
        );
        assert!(feed.playing(2).is_none());
    }
}
