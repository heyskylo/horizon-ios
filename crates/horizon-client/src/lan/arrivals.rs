//! Players that are about to arrive from another server, and a connected
//! player read out of this one and sent away: the control endpoint's
//! `player-expect`, `player-drop`, `player-export`, `player-release` and
//! `player-confirm` (`game/dedicated/transfer.rs` runs them; `player-import` is
//! `import.rs`'s), and `transfer-status`, the counts a proxy reads before it sends
//! players here.
//!
//! A proxy that moves a player between two servers first tells the target
//! server who is coming (`player-expect <uuid>`); the player's next login is then
//! taken in as a *held* player: connected, receiving the world, but not
//! simulated, shown or announced until the hand-over finishes. It reads the
//! player out of the source (`player-export <uuid> [freeze]`). Nothing here may
//! last without bound, because a proxy that dies in the middle of a move leaves
//! its expectations, its held players and its freezes behind: every expectation
//! expires, the table has a size cap, a held player is dropped and a freeze is
//! let go by themselves.
//!
//! All of it is off unless `server.toml`'s `[transfer]` turns it on
//! (horizon-server passes it to the game as `--transfers`, [`Settings`]). While
//! it is off the hub has no [`Transfers`], every request is refused and nothing
//! is done on a tick. It has nothing to do with vanilla's `accepts-transfers`,
//! which is about the transfer packet a server sends a client.
use super::hub::Hub;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// `server.toml`'s `[transfer]`, as horizon-server passes it: `--transfers
/// expect_seconds=10,max_expect_seconds=60,max_expected=64,max_freeze_seconds=30,max_hold_seconds=60,stop_wait_seconds=10`.
/// A key left out keeps its default.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Settings {
    /// How long an expectation lives when the request names no time.
    pub expect: Duration,
    /// The longest an expectation may be asked to live.
    pub max_expect: Duration,
    /// The most expectations the table keeps at once.
    pub max_expected: usize,
    /// Server ticks a player may stay frozen for a hand-over: a proxy that dies
    /// in the middle of one must not leave the player stuck.
    pub max_freeze_ticks: u64,
    /// Server ticks a held player may wait for its hand-over before the server
    /// drops it.
    pub max_hold_ticks: u64,
    /// How long a stop waits for a proxy to move the players out ([`StopWait`]); zero stops at once.
    pub stop_wait: Duration,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            expect: Duration::from_secs(10),
            max_expect: Duration::from_secs(60),
            max_expected: 64,
            max_freeze_ticks: 20 * 30,
            max_hold_ticks: 20 * 60,
            stop_wait: Duration::from_secs(10),
        }
    }
}

impl Settings {
    /// Reads `--transfers`' value: `key=value` pairs separated by commas, each a
    /// whole number above zero, but `stop_wait_seconds`, which is zero to [`MAX_STOP_WAIT`].
    pub fn parse(text: &str) -> Result<Self, String> {
        let mut s = Self::default();
        for pair in text.split(',').map(str::trim).filter(|p| !p.is_empty()) {
            let (key, value) = pair
                .split_once('=')
                .ok_or_else(|| format!("{pair:?} is not key=value"))?;
            let key = key.trim();
            let n: u64 = value
                .trim()
                .parse()
                .ok()
                .filter(|n| *n > 0 || key == "stop_wait_seconds")
                .ok_or_else(|| format!("{key} must be a whole number above zero"))?;
            match key {
                "stop_wait_seconds" if n > MAX_STOP_WAIT.as_secs() => {
                    return Err(format!(
                        "stop_wait_seconds is above {}",
                        MAX_STOP_WAIT.as_secs()
                    ));
                }
                "stop_wait_seconds" => s.stop_wait = Duration::from_secs(n),
                "expect_seconds" => s.expect = Duration::from_secs(n),
                "max_expect_seconds" => s.max_expect = Duration::from_secs(n),
                "max_expected" => {
                    s.max_expected = usize::try_from(n).map_err(|_| "max_expected is too large")?
                }
                "max_hold_seconds" => {
                    s.max_hold_ticks = n.checked_mul(20).ok_or("max_hold_seconds is too large")?
                }
                "max_freeze_seconds" => {
                    s.max_freeze_ticks =
                        n.checked_mul(20).ok_or("max_freeze_seconds is too large")?
                }
                other => return Err(format!("unknown setting {other:?}")),
            }
        }
        if s.expect > s.max_expect {
            return Err("expect_seconds is above max_expect_seconds".into());
        }
        // A stop's wait is cut at STOP_WAIT_LIMIT: past it, no hand-over could start during the wait.
        if !s.stop_wait.is_zero()
            && s.stop_wait.as_secs() + s.max_freeze_ticks / 20 > STOP_WAIT_LIMIT.as_secs()
        {
            return Err(format!(
                "stop_wait_seconds and max_freeze_seconds add up to more than {}",
                STOP_WAIT_LIMIT.as_secs()
            ));
        }
        Ok(s)
    }
}

/// What a server with transfers on keeps: its settings and the players it was
/// told to expect. The table is shared with the network threads, which use an
/// expectation up when a login starts.
#[derive(Debug)]
pub struct Transfers {
    pub settings: Settings,
    pub expected: Arc<Mutex<Expectations>>,
    /// The server has begun to stop (`/stop` or a stop signal; the game sets it, and a stop is not
    /// taken back): no player is expected, imported or placed any more, but the players here can
    /// still be exported and released, which is their way out.
    pub stopping: bool,
    /// The wait of that stop, while it lasts.
    pub stop_wait: Option<StopWait>,
}

impl Transfers {
    pub fn new(settings: Settings) -> Self {
        let expected = Arc::new(Mutex::new(Expectations::new(
            settings.max_expected,
            settings.max_expect,
        )));
        Self {
            settings,
            expected,
            stopping: false,
            stop_wait: None,
        }
    }

    /// The table, locked. A thread that panicked while holding it left it
    /// whole: every change is one map operation.
    pub fn table(&self) -> std::sync::MutexGuard<'_, Expectations> {
        self.expected.lock().unwrap_or_else(|e| e.into_inner())
    }
}

/// The longest `stop_wait_seconds` may be.
pub const MAX_STOP_WAIT: Duration = Duration::from_secs(60);

/// The longest a stop waits, a hand-over's freeze included: horizon-server kills a game that has not
/// stopped within 120 s (its `STOP_TIMEOUT`), and the saves need the rest.
pub const STOP_WAIT_LIMIT: Duration = Duration::from_secs(90);

/// A stop with `[transfer]` on and players here waits, so a proxy told the server is stopping
/// (`server/stopping`) can move them out first: no login comes in, the server ticks on and answers the
/// proxy's requests. The wait ends as soon as no player is left, at `until` when no hand-over is frozen,
/// and at `limit` whatever is under way: `until` and a freeze's longest time, so a hand-over started in
/// time can finish, within [`STOP_WAIT_LIMIT`]. No freeze starts that the limit would cut: a player whose
/// source saved it after the target placed it would be on both servers.
///
/// A freeze lasts `max_freeze_ticks` ticks, not seconds: its length in time is reckoned at the tick
/// length the server has now (`period`, [`tick_period`]), which the game sets again at every tick, so a
/// slow server or `/tick rate 10` refuses a freeze sooner. A server that slows down after a freeze began
/// can still reach the limit with it: that is the warning of the stop.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StopWait {
    pub until: Instant,
    pub limit: Instant,
    freeze_ticks: u64,
    /// The length of a tick now.
    pub period: Duration,
}

impl StopWait {
    /// The wait of a stop at `now`, if the settings make one, with ticks of `period`.
    pub fn start(settings: &Settings, now: Instant, period: Duration) -> Option<Self> {
        let freeze = freeze_time(settings.max_freeze_ticks, period);
        (!settings.stop_wait.is_zero()).then(|| Self {
            until: now + settings.stop_wait,
            limit: now
                + settings
                    .stop_wait
                    .saturating_add(freeze)
                    .min(STOP_WAIT_LIMIT),
            freeze_ticks: settings.max_freeze_ticks,
            period,
        })
    }

    /// Whether the stop goes on now, with these players placed and frozen here.
    pub fn over(&self, now: Instant, placed: usize, frozen: usize) -> bool {
        placed == 0 || (now >= self.until && frozen == 0) || now >= self.limit
    }

    /// Whether a hand-over that starts now would end before the limit, at the tick length of now.
    pub fn may_freeze(&self, now: Instant) -> bool {
        now.checked_add(freeze_time(self.freeze_ticks, self.period))
            .is_some_and(|end| end <= self.limit)
    }
}

/// How long `ticks` ticks of `period` take.
fn freeze_time(ticks: u64, period: Duration) -> Duration {
    period.saturating_mul(u32::try_from(ticks).unwrap_or(u32::MAX))
}

/// The length of a tick: the rate's (`target`, which `/tick rate` sets), or longer when the server
/// is slower than that (`tps`, the ticks it made a second lately; 0 when unknown).
pub fn tick_period(target: Duration, tps: f64) -> Duration {
    let measured = if tps > 0.0 {
        Duration::try_from_secs_f64(1.0 / tps).unwrap_or(Duration::MAX)
    } else {
        Duration::ZERO
    };
    measured.max(target)
}

/// The players placed here (a player a `moved` release took out is not counted) and those of them
/// frozen for a hand-over.
pub fn placed_and_frozen(hub: &Hub) -> (usize, usize) {
    let here = || hub.guests.iter().filter(|g| g.placed && !g.released);
    (
        here().count(),
        here().filter(|g| g.handover.is_some()).count(),
    )
}

/// The log line of a stop that waits.
pub fn stop_wait_line(wait: Duration) -> String {
    format!(
        "Stopping in {} s: a proxy may move the players out first",
        wait.as_secs()
    )
}

/// Why an expectation could not be recorded.
#[derive(Debug, PartialEq, Eq)]
pub enum ExpectError {
    /// The table is full of live expectations.
    Full,
}

/// The players a server was told to expect, by unique id.
#[derive(Debug)]
pub struct Expectations {
    until: HashMap<u128, Instant>,
    max: usize,
    max_ttl: Duration,
}

impl Default for Expectations {
    fn default() -> Self {
        let s = Settings::default();
        Self::new(s.max_expected, s.max_expect)
    }
}

impl Expectations {
    pub fn new(max: usize, max_ttl: Duration) -> Self {
        Self {
            until: HashMap::new(),
            max,
            max_ttl,
        }
    }

    /// Records that `uuid` will log in within `ttl` (cut to the longest time
    /// allowed). A second request for the same player replaces the first one's
    /// deadline. Returns whether the player was already expected.
    pub fn expect(&mut self, uuid: u128, ttl: Duration, now: Instant) -> Result<bool, ExpectError> {
        self.expire(now);
        if !self.until.contains_key(&uuid) && self.until.len() >= self.max {
            return Err(ExpectError::Full);
        }
        let deadline = now + ttl.min(self.max_ttl);
        Ok(self.until.insert(uuid, deadline).is_some())
    }

    /// Uses up the expectation of `uuid`, and says whether it was still alive:
    /// the login that starts takes it, and a second login of the same player
    /// finds none.
    pub fn take(&mut self, uuid: u128, now: Instant) -> bool {
        self.take_until(uuid, now).is_some()
    }

    /// [`Self::take`], giving the deadline of the expectation used up, for
    /// [`Self::put_back`].
    pub fn take_until(&mut self, uuid: u128, now: Instant) -> Option<Instant> {
        self.until.remove(&uuid).filter(|deadline| now < *deadline)
    }

    /// Gives back an expectation a login used up and that did not reach play (it
    /// failed in configuration), with its own deadline: the proxy's next attempt
    /// within that time is still expected. Nothing is put back once its time has
    /// passed, over a newer expectation of the same player, or into a full table.
    pub fn put_back(&mut self, uuid: u128, deadline: Instant, now: Instant) {
        if now < deadline && !self.until.contains_key(&uuid) && self.until.len() < self.max {
            self.until.insert(uuid, deadline);
        }
    }

    /// Forgets the expectation of `uuid`. Returns whether there was one.
    pub fn forget(&mut self, uuid: u128) -> bool {
        self.until.remove(&uuid).is_some()
    }

    /// Drops the expectations whose time has passed and returns them, so the
    /// caller can log that the player never came.
    pub fn expire(&mut self, now: Instant) -> Vec<u128> {
        let gone: Vec<u128> = self
            .until
            .iter()
            .filter(|(_, deadline)| now >= **deadline)
            .map(|(uuid, _)| *uuid)
            .collect();
        for uuid in &gone {
            self.until.remove(uuid);
        }
        gone
    }

    /// The longest an expectation may live.
    pub fn max_ttl(&self) -> Duration {
        self.max_ttl
    }

    /// How many expectations are recorded (some may be past their time but not
    /// yet swept).
    pub fn len(&self) -> usize {
        self.until.len()
    }

    /// How many expectations are still alive at `now`, without sweeping the others.
    pub fn alive(&self, now: Instant) -> usize {
        self.holding(None, now)
    }

    /// The places the live expectations of players other than `except` hold at `now`.
    pub fn holding(&self, except: Option<u128>, now: Instant) -> usize {
        self.until
            .iter()
            .filter(|(uuid, deadline)| Some(**uuid) != except && now < **deadline)
            .count()
    }

    pub fn is_empty(&self) -> bool {
        self.until.is_empty()
    }
}

/// The version of the record `player-export` answers with.
pub const RECORD_VERSION: i64 = 1;

/// A hand-over in progress on the source: the player was exported with
/// `freeze`. Its id names it in the later requests, and its record is what
/// every later export answers with until it ends, the same bytes each time,
/// so a proxy that lost the reply can ask again.
#[derive(Clone, Debug, PartialEq)]
pub struct Handover {
    pub id: u128,
    /// The hub tick the freeze started at.
    pub since: u64,
    pub record: serde_json::Value,
}

/// A hand-over id as a reply writes it: 32 lowercase hexadecimal digits.
pub fn handover_text(id: u128) -> String {
    format!("{id:032x}")
}

/// A hand-over id from a request: 32 hexadecimal digits.
pub fn parse_handover(text: &str) -> Option<u128> {
    if text.len() != 32 || !text.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    u128::from_str_radix(text, 16).ok()
}

/// A new hand-over id, from the operating system's secure randomness.
fn new_handover_id() -> Result<u128, String> {
    let mut bytes = [0u8; 16];
    horizon_protocol::crypto::random_bytes(&mut bytes).map_err(|e| e.to_string())?;
    Ok(u128::from_le_bytes(bytes))
}

/// Whether a held player that joined at tick `since` has waited too long at tick
/// `now` for its hand-over, for a wait of at most `max` ticks.
pub fn held_too_long(since: u64, now: u64, max: u64) -> bool {
    now.saturating_sub(since) >= max
}

/// Whether a player frozen at tick `since` has been frozen too long at tick
/// `now`, for a freeze of at most `max` ticks.
pub fn frozen_too_long(since: u64, now: u64, max: u64) -> bool {
    now.saturating_sub(since) >= max
}

/// `player-export <uuid> freeze` during a stop's wait: refused once a hand-over that starts now could
/// not finish before the stop's limit ([`StopWait`]). A player already frozen is answered as before.
pub fn check_freeze(hub: &Hub, i: usize, now: Instant) -> Result<(), &'static str> {
    let wait = hub.transfers.as_ref().and_then(|t| t.stop_wait);
    if hub.guests[i].handover.is_none() && wait.is_some_and(|w| !w.may_freeze(now)) {
        return Err("the server is stopping: no hand-over starts this late");
    }
    Ok(())
}

/// Why the connected player `uuid` cannot be exported now, or the index of its
/// guest. A held player is not exportable: it has no state of its own yet.
pub fn check_exportable(hub: &Hub, uuid: u128) -> Result<usize, &'static str> {
    let Some(i) = hub.guests.iter().position(|g| g.uuid == uuid) else {
        return Err("that player is not connected to this server");
    };
    let g = &hub.guests[i];
    if g.held {
        Err("that player is held: it has not been handed over yet")
    } else if !g.placed {
        Err("that player has not finished joining")
    } else if !g.left_reason.is_empty() {
        Err("that player is leaving")
    } else if g.vehicle.is_some() {
        Err("that player is riding")
    } else if g.sleeping {
        Err("that player is asleep")
    } else if g.dead {
        Err("that player is dead")
    } else {
        Ok(i)
    }
}

/// What a record carries besides the player compound: the JSON the server keeps
/// for the player's advancements and statistics (null where it keeps none).
#[derive(Clone, Debug, Default)]
pub struct Records {
    pub advancements: serde_json::Value,
    pub stats: serde_json::Value,
}

/// The record of the player at guest index `i` (which [`check_exportable`] gave,
/// with its menus closed and its attachments in place): the player compound as
/// the server saves it, as the uncompressed NBT of the compound in base64, its
/// advancements and statistics, and the few values a proxy reads without opening
/// it. With `freeze`, a hand-over starts: the record gets an id, the player's
/// own inputs stop being acted on until the hand-over ends, and every later
/// export answers with this same record. A player already frozen is answered
/// with its hand-over's record, whatever `freeze` says.
pub fn export_record(
    hub: &mut Hub,
    i: usize,
    freeze: bool,
    records: Records,
) -> Result<serde_json::Value, String> {
    use horizon_world::nbt::{Limits, NamedTag, write_named};
    if let Some(h) = &hub.guests[i].handover {
        return Ok(h.record.clone());
    }
    let id = freeze.then(new_handover_id).transpose()?;
    let g = &hub.guests[i];
    let tag = super::playerdata::player_tag(g);
    let bytes = write_named(
        &NamedTag {
            name: "".into(),
            tag,
        },
        Limits::saved(),
    )
    .map_err(|e| e.to_string())?;
    let velocity = g
        .motion
        .as_ref()
        .map_or([0.0; 3], |m| [m.velocity.x, m.velocity.y, m.velocity.z]);
    let record = serde_json::json!({
        "version": RECORD_VERSION,
        "data_version": horizon_world::DATA_VERSION,
        "handover": id.map(handover_text),
        "uuid": horizon_protocol::crypto::uuid_string(g.uuid),
        "name": g.name,
        "dimension": g.dimension,
        "position": g.position,
        "rotation": [g.yaw, g.pitch],
        "velocity": velocity,
        "game_mode": g.game_mode,
        "player": super::base64(&bytes),
        "advancements": records.advancements,
        "stats": records.stats,
    });
    if let Some(id) = id {
        let since = hub.ticks;
        hub.guests[i].handover = Some(Handover {
            id,
            since,
            record: record.clone(),
        });
    }
    Ok(record)
}

/// The reply to an export: the record and, while the player is frozen, the ticks left
/// before its hand-over ends by itself (`freeze_ticks_left`), so that a proxy does not
/// commit on the target so late that its `moved` would come after the end. The count is
/// beside the record, which stays the same bytes from one export to the next.
pub fn export_reply(hub: &Hub, i: usize, record: serde_json::Value) -> serde_json::Value {
    let left = hub.guests[i]
        .handover
        .as_ref()
        .zip(hub.transfers.as_ref())
        .map(|(h, t)| {
            t.settings
                .max_freeze_ticks
                .saturating_sub(hub.ticks.saturating_sub(h.since))
        });
    let mut reply = serde_json::json!({ "ok": true, "record": record });
    if let Some(left) = left {
        reply["freeze_ticks_left"] = left.into();
    }
    reply
}

/// The log line for a proxy's `player-*` request and what became of it, with its level.
/// `name` is the player's name when the server knows it. Only the player's id and a few
/// words of the outcome are written: never the record, which is large and is the player's.
pub fn request_log(
    verb: &str,
    name: Option<&str>,
    args: &str,
    reply: &serde_json::Value,
) -> (&'static str, String) {
    let mut words = args.split_whitespace();
    let id = words.next().unwrap_or("");
    let who = match (parse_uuid(id), name) {
        (Some(u), Some(name)) => format!("{name} ({})", horizon_protocol::crypto::uuid_string(u)),
        (Some(u), None) => horizon_protocol::crypto::uuid_string(u),
        (None, _) => format!("{id:?}"),
    };
    if let Some(why) = reply.get("error").and_then(serde_json::Value::as_str) {
        return ("WARN", format!("{verb} {who}: refused: {why}"));
    }
    let flag = |key: &str| reply.get(key).and_then(serde_json::Value::as_bool);
    // The hand-over a request names (release, confirm, drop) or a reply gives (export,
    // import): it ties the lines of one hand-over together on both servers.
    let named = words.next();
    let handover = |given: Option<&serde_json::Value>| {
        given
            .and_then(serde_json::Value::as_str)
            .or(named.filter(|w| parse_handover(w).is_some()))
            .map_or(String::new(), |id| format!(", hand-over {id}"))
    };
    let details = match verb {
        "player-expect" => format!(
            "expected for {} s{}",
            reply["ttl_seconds"],
            if flag("renewed") == Some(true) {
                " (renewed)"
            } else {
                ""
            }
        ),
        "player-export" => format!(
            "record of {} bytes{}",
            reply["record"]["player"]
                .as_str()
                .map_or(0, |p| p.len() / 4 * 3),
            match reply["record"]["handover"].as_str() {
                Some(id) => format!(", frozen, hand-over {id}"),
                None => ", a read".to_owned(),
            }
        ),
        "player-import" => format!(
            "parts {}, placed {}, announced {}, recentered {}{}",
            reply["parts"]
                .as_array()
                .map(|p| p
                    .iter()
                    .filter_map(|v| v.as_str())
                    .collect::<Vec<_>>()
                    .join(","))
                .unwrap_or_default(),
            flag("placed").unwrap_or(false),
            flag("announced").unwrap_or(false),
            flag("recentered").unwrap_or(false),
            handover(reply.get("handover")),
        ),
        "player-confirm" => format!(
            "placed, announced {}{}",
            flag("announced").unwrap_or(false),
            handover(None)
        ),
        "player-release" => match reply.get("released").and_then(serde_json::Value::as_str) {
            Some("moved") => format!(
                "moved, announced {}{}",
                flag("announced").unwrap_or(false),
                handover(None)
            ),
            other => format!("{}{}", other.unwrap_or("released"), handover(None)),
        },
        "player-drop" => format!(
            "held {}, expected {}{}",
            flag("held").unwrap_or(false),
            flag("expected").unwrap_or(false),
            handover(None)
        ),
        _ => "done".to_owned(),
    };
    // A retry after the commit, whose reply was lost: nothing was done again.
    let retried = if flag("retried") == Some(true) {
        " (a retry: nothing done again)"
    } else {
        ""
    };
    ("INFO", format!("{verb} {who}: ok, {details}{retried}"))
}

/// The line written when a player is held: it logged in as the player a proxy expected.
pub fn held_line(name: &str) -> String {
    format!("{name} is held: it arrives from another server")
}

/// The line written when a held player is let into the world.
pub fn placed_line(name: &str) -> String {
    format!("{name} placed: it has arrived from another server")
}

/// The line written when a held player's connection ends before it was placed: it was
/// never announced, so it has no "left the game".
pub fn dropped_line(name: &str, reason: &str) -> String {
    format!("{name} was dropped while held: {reason}")
}

/// A unique id as text, with or without dashes.
pub fn parse_uuid(text: &str) -> Option<u128> {
    let plain: String = text.chars().filter(|c| *c != '-').collect();
    if plain.len() != 32 || !plain.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    u128::from_str_radix(&plain, 16).ok()
}

fn error(message: &str) -> serde_json::Value {
    serde_json::json!({ "error": message })
}

/// The reply to every `player-*` request on a server with transfers off.
pub const OFF: &str = "transfers are off on this server";

/// The reply to `player-expect` when the players here and those expected take every place.
pub const FULL: &str = "the server is full";

/// The reply to `player-expect`, `player-import` and `player-confirm` once the server has begun
/// to stop: a player must not arrive on a server that is about to close.
pub const STOPPING: &str = "the server is stopping";

/// `player-expect <uuid> [seconds]`: the player will log in within that time
/// (`[transfer]`'s `expect_seconds` by default, `max_expect_seconds` at most).
/// The reply says whether an earlier expectation was renewed. The login uses
/// the expectation up when it starts; one that no login used is reported when
/// it runs out. Each expectation holds a place: once the places the player limit
/// counts and those of the other expected players reach `max-players`, the
/// request is refused as full, unless the player is an op who bypasses the limit
/// (`PlayerList.canPlayerLogin`). It is refused too once the server has begun to stop.
pub fn expect_request(hub: &mut Hub, args: &str, now: Instant) -> serde_json::Value {
    let Some(transfers) = &hub.transfers else {
        return error(OFF);
    };
    if transfers.stopping {
        return error(STOPPING);
    }
    let mut parts = args.split_whitespace();
    let Some(uuid) = parts.next().and_then(parse_uuid) else {
        return error("player-expect needs the player's unique id");
    };
    let ttl = match parts.next() {
        None => transfers.settings.expect,
        Some(seconds) => match seconds.parse::<u64>() {
            Ok(s) if s > 0 => Duration::from_secs(s),
            _ => return error("the time must be a number of seconds above zero"),
        },
    };
    if parts.next().is_some() {
        return error("player-expect takes a unique id and at most a time");
    }
    if hub.guests.iter().any(|g| g.uuid == uuid) {
        return error("that player is already connected to this server");
    }
    let shared = &hub.lan.shared;
    let (online, max_players) = (shared.online(), shared.max_players());
    let bypass = shared
        .config
        .bypasses_limit
        .as_ref()
        .is_some_and(|b| b(uuid));
    let mut table = transfers.table();
    // A renewal holds no second place.
    if !bypass && online + table.holding(Some(uuid), now) >= max_players {
        return error(FULL);
    }
    match table.expect(uuid, ttl, now) {
        Ok(renewed) => serde_json::json!({
            "ok": true,
            "renewed": renewed,
            "ttl_seconds": ttl.min(table.max_ttl()).as_secs(),
        }),
        Err(ExpectError::Full) => error("too many players are expected at once"),
    }
}

/// `transfer-status`: what a proxy reads before it sends players here. The players placed, held
/// and expected, the hand-overs under way, the places taken (what the player limit counts: the
/// placed, the held and the logins still joining) and the limit, and whether a login would be
/// refused as full. Counts only: no name, no unique id.
pub fn status_request(hub: &Hub, now: Instant) -> serde_json::Value {
    let Some(transfers) = &hub.transfers else {
        return error(OFF);
    };
    let (online, max_players) = (hub.lan.shared.online(), hub.lan.shared.max_players());
    let (placed, frozen) = placed_and_frozen(hub);
    serde_json::json!({
        "ok": true,
        "players": placed,
        "held": hub.guests.iter().filter(|g| g.held).count(),
        "expected": transfers.table().alive(now),
        "frozen": frozen,
        "online": online,
        "max_players": max_players,
        "full": online >= max_players,
        "stopping": transfers.stopping,
    })
}

/// `player-drop <uuid> [hand-over]`: forgets the expectation of that player, and
/// disconnects it if it is already held, without a save or a word to anyone. With a
/// hand-over id, the held player must be the one whose record of that hand-over was
/// imported: a late drop of an older hand-over leaves a newer one alone. Before any
/// import there is no id to name, and the drop goes by the unique id alone.
pub fn drop_request(hub: &mut Hub, args: &str) -> serde_json::Value {
    if hub.transfers.is_none() {
        return error(OFF);
    }
    let mut parts = args.split_whitespace();
    let Some(uuid) = parts.next().and_then(parse_uuid) else {
        return error("player-drop needs the player's unique id");
    };
    let id = match parts.next() {
        None => None,
        Some(text) => match parse_handover(text) {
            Some(id) => Some(id),
            None => return error("player-drop takes a unique id and at most a hand-over id"),
        },
    };
    if parts.next().is_some() {
        return error("player-drop takes a unique id and at most a hand-over id");
    }
    if let Some(id) = id
        && let Err(why) = hub.imported_arrival(uuid, id)
    {
        return error(why);
    }
    let held = hub.drop_held(uuid);
    let expected = hub
        .transfers
        .as_ref()
        .is_some_and(|t| t.table().forget(uuid));
    if !held && !expected {
        return error("no player with that unique id is held or expected");
    }
    serde_json::json!({ "ok": true, "held": held, "expected": expected })
}

/// `player-confirm <uuid> <hand-over>`: the held player whose record of that hand-over was
/// imported, ready to be placed (`Hub::imported_arrival`). The game writes its file, then places
/// it (`Hub::confirm_arrival`). Until then `player-drop` removes it without a save or a word to
/// anyone. Once the server has begun to stop it is refused, though a retry after the commit is
/// still answered.
pub fn confirm_args(hub: &Hub, args: &str) -> Result<(u128, u128), String> {
    let mut parts = args.split_whitespace();
    let Some(uuid) = parts.next().and_then(parse_uuid) else {
        return Err("player-confirm needs the player's unique id".into());
    };
    let Some(id) = parts.next().and_then(parse_handover) else {
        return Err("player-confirm needs the hand-over id of the record".into());
    };
    if parts.next().is_some() {
        return Err("player-confirm takes a unique id and a hand-over id".into());
    }
    if hub.placed_arrival(uuid, id).is_none() {
        if hub.transfers.as_ref().is_some_and(|t| t.stopping) {
            return Err(STOPPING.into());
        }
        hub.imported_arrival(uuid, id)?;
    }
    Ok((uuid, id))
}

/// `player-release <uuid> <hand-over> moved [announce]` or `player-release <uuid> <hand-over>
/// abort`: ends the hand-over the export with `freeze` started. `moved` takes the player out of
/// the world, saving nothing and, unless `announce`, without the "left the game" line; `abort`
/// lets it go on as it was. Both name the hand-over, so a late or repeated request for one that
/// is over touches nothing.
pub fn release_request(hub: &mut Hub, args: &str) -> serde_json::Value {
    let mut parts = args.split_whitespace();
    let Some(uuid) = parts.next().and_then(parse_uuid) else {
        return error("player-release needs the player's unique id");
    };
    let Some(id) = parts.next().and_then(parse_handover) else {
        return error("player-release needs the hand-over id the export gave");
    };
    let how = parts.next();
    let announce = match (how, parts.next()) {
        (Some("moved"), None) => false,
        (Some("moved"), Some("announce")) => true,
        (Some("abort"), None) => false,
        _ => {
            return error(
                "player-release <uuid> <hand-over> is followed by moved [announce] or abort",
            );
        }
    };
    if parts.next().is_some() {
        return error(
            "player-release takes a unique id, a hand-over id, moved or abort, and at most announce",
        );
    }
    if how == Some("moved") {
        return match hub.release_moved(uuid, id, announce) {
            Ok(()) => serde_json::json!({
                "ok": true,
                "released": "moved",
                "announced": announce,
            }),
            Err(why) => error(why),
        };
    }
    match hub.handover_of(uuid, id) {
        Ok(i) => {
            hub.guests[i].handover = None;
            serde_json::json!({ "ok": true, "released": "abort" })
        }
        Err(why) => error(why),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const A: u128 = 0xA;
    const B: u128 = 0xB;

    fn secs(n: u64) -> Duration {
        Duration::from_secs(n)
    }

    #[test]
    fn an_expected_player_is_taken_once_before_the_deadline() {
        let t0 = Instant::now();
        let mut e = Expectations::default();
        assert_eq!(e.expect(A, secs(10), t0), Ok(false));
        assert!(e.take(A, t0 + secs(9)));
        // The login consumed it: a second login of the same player is ordinary.
        assert!(!e.take(A, t0 + secs(9)));
        assert!(e.is_empty());
    }

    #[test]
    fn an_expectation_is_dead_at_its_deadline() {
        let t0 = Instant::now();
        let mut e = Expectations::default();
        e.expect(A, secs(10), t0).unwrap();
        // Exactly at the deadline is already too late.
        assert!(!e.take(A, t0 + secs(10)));
        assert!(e.is_empty());
    }

    #[test]
    fn a_player_nobody_expected_has_no_expectation() {
        let mut e = Expectations::default();
        assert!(!e.take(B, Instant::now()));
    }

    #[test]
    fn asking_again_moves_the_deadline_and_says_so() {
        let t0 = Instant::now();
        let mut e = Expectations::default();
        e.expect(A, secs(5), t0).unwrap();
        assert_eq!(e.expect(A, secs(30), t0 + secs(4)), Ok(true));
        assert!(e.take(A, t0 + secs(20)));
    }

    #[test]
    fn a_held_player_waits_one_minute_of_ticks() {
        let max = Settings::default().max_hold_ticks;
        assert_eq!(max, 1200);
        assert!(!held_too_long(100, 100 + 1199, max));
        assert!(held_too_long(100, 100 + 1200, max));
        // A clock that went backwards never drops anyone.
        assert!(!held_too_long(500, 100, max));
    }

    #[test]
    fn forgetting_removes_only_that_player() {
        let t0 = Instant::now();
        let mut e = Expectations::default();
        e.expect(A, secs(10), t0).unwrap();
        e.expect(B, secs(10), t0).unwrap();
        assert!(e.forget(A));
        assert!(!e.forget(A));
        assert!(e.take(B, t0));
    }

    #[test]
    fn a_long_wait_is_cut_to_the_maximum() {
        let t0 = Instant::now();
        let mut e = Expectations::default();
        e.expect(A, secs(3600), t0).unwrap();
        assert!(!e.take(A, t0 + Settings::default().max_expect));
    }

    #[test]
    fn expiry_returns_the_players_that_never_came_and_keeps_the_others() {
        let t0 = Instant::now();
        let mut e = Expectations::default();
        e.expect(A, secs(5), t0).unwrap();
        e.expect(B, secs(30), t0).unwrap();
        assert_eq!(e.expire(t0 + secs(6)), vec![A]);
        assert_eq!(e.len(), 1);
        assert!(e.take(B, t0 + secs(7)));
    }

    #[test]
    fn the_table_refuses_new_players_when_full_but_expired_ones_make_room() {
        let t0 = Instant::now();
        let mut e = Expectations::default();
        for n in 0..Settings::default().max_expected as u128 {
            e.expect(100 + n, secs(10), t0).unwrap();
        }
        assert_eq!(e.expect(A, secs(10), t0), Err(ExpectError::Full));
        // A player already in the table can still be renewed when it is full.
        assert_eq!(e.expect(100, secs(10), t0), Ok(true));
        // Once the old ones are past their time, there is room again.
        assert_eq!(e.expect(A, secs(10), t0 + secs(11)), Ok(false));
        assert_eq!(e.len(), 1);
    }

    #[test]
    fn the_settings_are_read_from_the_supervisors_line() {
        assert_eq!(Settings::parse(""), Ok(Settings::default()));
        let s = Settings::parse(
            "expect_seconds=5,max_expect_seconds=20,max_expected=3,max_freeze_seconds=12",
        )
        .unwrap();
        assert_eq!(s.expect, secs(5));
        assert_eq!(s.max_expect, secs(20));
        assert_eq!(s.max_expected, 3);
        assert_eq!(s.max_freeze_ticks, 240);
        // One key alone keeps the others' defaults.
        assert_eq!(
            Settings::parse("max_expected=8").unwrap(),
            Settings {
                max_expected: 8,
                ..Settings::default()
            }
        );
        for bad in [
            "expect_seconds=0",
            "expect_seconds=-1",
            "expect_seconds",
            "colour=blue",
            "expect_seconds=90",
            "stop_wait_seconds=61",
            "max_freeze_seconds=81",
            "stop_wait_seconds=60,max_freeze_seconds=31",
        ] {
            assert!(Settings::parse(bad).is_err(), "{bad:?} was taken");
        }
        // A stop that does not wait is zero seconds.
        assert_eq!(Settings::default().stop_wait, secs(10));
        assert_eq!(
            Settings::parse("stop_wait_seconds=0").unwrap().stop_wait,
            secs(0)
        );
        assert_eq!(
            Settings::parse("stop_wait_seconds=60").unwrap().stop_wait,
            secs(60)
        );
        // The two fit in the stop's limit, or the stop does not wait.
        assert!(Settings::parse("stop_wait_seconds=60,max_freeze_seconds=30").is_ok());
        assert!(Settings::parse("stop_wait_seconds=0,max_freeze_seconds=300").is_ok());
    }

    /// A stop's wait ends when nobody is left, at its time when no hand-over is frozen, and at its
    /// limit (its time and a freeze's) whatever is under way; no freeze starts that the limit would
    /// cut. Zero seconds is no wait.
    #[test]
    fn a_stop_waits_for_the_players_to_be_moved_out_but_not_for_ever() {
        let now = Instant::now();
        let settings = Settings::default();
        let tick = Duration::from_millis(50);
        let wait = StopWait::start(&settings, now, tick).unwrap();
        assert_eq!((wait.until, wait.limit), (now + secs(10), now + secs(40)));
        assert!(!wait.over(now, 2, 0));
        assert!(wait.over(now, 0, 0), "nobody is left");
        assert!(!wait.over(now + secs(9), 2, 0));
        assert!(
            wait.over(now + secs(10), 2, 0),
            "its time, and no hand-over"
        );
        assert!(!wait.over(now + secs(39), 2, 1), "a hand-over may finish");
        assert!(wait.over(now + secs(40), 2, 1), "the limit");
        assert!(wait.may_freeze(now + secs(10)));
        assert!(!wait.may_freeze(now + secs(11)));
        // However long the settings, the whole wait fits in the supervisor's time for a stop.
        let long = Settings {
            stop_wait: secs(60),
            max_freeze_ticks: 20 * 300,
            ..Settings::default()
        };
        let wait = StopWait::start(&long, now, tick).unwrap();
        assert_eq!(wait.limit, now + STOP_WAIT_LIMIT);
        assert!(!wait.may_freeze(now), "a freeze of 300 s would be cut");
        let none = Settings {
            stop_wait: secs(0),
            ..Settings::default()
        };
        assert_eq!(StopWait::start(&none, now, tick), None);
    }

    /// A freeze lasts ticks: at 10 ticks a second it lasts twice as long, the limit with it, and a
    /// server that slows down during the wait refuses a freeze it would have let start at 20.
    #[test]
    fn a_stop_reckons_a_freeze_at_the_tick_length_of_now() {
        let now = Instant::now();
        let slow = Duration::from_millis(100);
        let wait = StopWait::start(&Settings::default(), now, slow).unwrap();
        assert_eq!((wait.until, wait.limit), (now + secs(10), now + secs(70)));
        assert!(wait.may_freeze(now + secs(10)));
        assert!(!wait.may_freeze(now + secs(11)));
        let mut wait =
            StopWait::start(&Settings::default(), now, Duration::from_millis(50)).unwrap();
        assert!(wait.may_freeze(now));
        wait.period = slow;
        assert!(
            !wait.may_freeze(now),
            "60 s of freeze would pass the limit at 40 s"
        );
        // The tick length: the rate's, or longer when the server is slower than its rate.
        let rate = Duration::from_millis(50);
        assert_eq!(tick_period(rate, 20.0), rate);
        assert_eq!(
            tick_period(rate, 40.0),
            rate,
            "sprinting is not a shorter freeze"
        );
        assert_eq!(tick_period(rate, 10.0), Duration::from_millis(100));
        assert_eq!(tick_period(rate, 0.0), rate, "no measure yet");
        assert_eq!(
            tick_period(Duration::from_millis(100), 20.0),
            slow,
            "/tick rate 10"
        );
    }

    #[test]
    fn the_table_follows_the_settings() {
        let t0 = Instant::now();
        let mut e = Expectations::new(2, secs(5));
        e.expect(A, secs(60), t0).unwrap();
        assert!(!e.take(A, t0 + secs(5)), "cut to five seconds");
        e.expect(A, secs(1), t0).unwrap();
        e.expect(B, secs(1), t0).unwrap();
        assert_eq!(e.expect(3, secs(1), t0), Err(ExpectError::Full));
    }

    /// A login that does not reach play gives its expectation back, with its
    /// own deadline, so the proxy's next attempt is still expected.
    #[test]
    fn an_expectation_a_failed_login_used_is_put_back_with_its_deadline() {
        let t0 = Instant::now();
        let mut e = Expectations::default();
        e.expect(A, secs(10), t0).unwrap();
        let deadline = e.take_until(A, t0 + secs(2)).unwrap();
        assert!(e.is_empty());
        e.put_back(A, deadline, t0 + secs(4));
        assert!(e.take(A, t0 + secs(9)), "the same deadline, not a new one");
        // Not once its time has passed.
        e.put_back(A, deadline, t0 + secs(10));
        assert!(e.is_empty());
        // Not over a newer expectation of the same player.
        e.expect(A, secs(30), t0 + secs(5)).unwrap();
        e.put_back(A, deadline, t0 + secs(6));
        assert!(e.take(A, t0 + secs(20)), "the newer deadline stands");
    }

    #[test]
    fn a_hand_over_id_is_32_hexadecimal_digits() {
        let id = 0x0123_4567_89ab_cdef_0011_2233_4455_6677;
        assert_eq!(handover_text(id), "0123456789abcdef0011223344556677");
        assert_eq!(parse_handover(&handover_text(id)), Some(id));
        assert_eq!(parse_handover("0123456789ABCDEF0011223344556677"), Some(id));
        assert_eq!(parse_handover("0123456789abcdef001122334455667"), None);
        assert_eq!(parse_handover("01234567-89ab-cdef-0011-223344556677"), None);
        assert_ne!(new_handover_id().unwrap(), new_handover_id().unwrap());
    }
}

/// The hub's side: the requests, the record and the freeze.
#[cfg(test)]
mod hub_tests {
    use crate::lan::{
        Joined, Lan, NetConfig, Out, PlayerSender,
        chunks::ChunkEncoder,
        data,
        guest::Guest,
        hub::{Hub, HubConfig, TickCtx},
    };
    use horizon_protocol::ids::play::clientbound as id;
    use std::sync::mpsc::{Receiver, channel};
    use std::time::{Duration, Instant};

    fn hub() -> Hub {
        hub_with(NetConfig {
            announce: false,
            ..NetConfig::loopback(0, "ArrivalsTest".into(), 4)
        })
    }

    fn hub_with(config: NetConfig) -> Hub {
        let mut sync = data::SyncData::empty();
        // The entity types the tracker names: the player, as id 0.
        sync.builtin.registries.insert(
            "minecraft:entity_type".to_string(),
            horizon_protocol::registry::Ids::from_names(vec!["minecraft:player".to_string()]),
        );
        let sync = std::sync::Arc::new(sync);
        let lan = Lan::start_with(sync.clone(), config).unwrap();
        let encoder = ChunkEncoder::new(
            [] as [(std::sync::Arc<str>, bool); 0],
            &horizon_protocol::registry::Builtin::default(),
            &[],
            &[],
        );
        let config = HubConfig {
            default_game_mode: 0,
            guest_commands: false,
            force_game_mode: false,
            hardcore: false,
            view_distance: 4,
            simulation_distance: 4,
            dimension: "minecraft:overworld".into(),
            allow_flight: false,
            seed: 0,
            max_players: 4,
            integrated: false,
            levels: Default::default(),
        };
        let mut hub = Hub::new(lan, sync, encoder, config);
        hub.transfers = Some(super::Transfers::new(super::Settings::default()));
        hub
    }

    /// The table a server with transfers on keeps.
    fn table(hub: &Hub) -> std::sync::MutexGuard<'_, super::Expectations> {
        hub.transfers.as_ref().unwrap().table()
    }

    /// A guest in the overworld and the end of the channel its packets go to.
    fn guest(conn: u32, uuid: u128, name: &str) -> (Guest, Receiver<Out>) {
        let (tx, rx) = channel();
        let mut g = Guest::new(
            Joined {
                conn,
                uuid,
                name: name.into(),
                properties: Vec::new(),
                info: horizon_protocol::packets::configuration::ClientInformation::new(4),
                brand: "vanilla".into(),
                sender: PlayerSender::new(tx, std::sync::Arc::default()),
                latency: std::sync::Arc::new(std::sync::atomic::AtomicI32::new(0)),
                address: "127.0.0.1:25565".parse().unwrap(),
                expected: false,
                configured: Default::default(),
            },
            conn as i32,
            [0.5, 64.0, 0.5],
            4,
        );
        g.dimension = "minecraft:overworld".into();
        (g, rx)
    }

    /// The packets waiting in a channel, in order, each as its id and payload.
    fn packets(rx: &Receiver<Out>) -> Vec<(i32, Vec<u8>)> {
        let mut out = Vec::new();
        let mut one = |body: &[u8]| {
            let mut r = horizon_protocol::buf::Reader::new(body);
            let id = r.varint().unwrap();
            out.push((id, r.rest().to_vec()));
        };
        while let Ok(o) = rx.try_recv() {
            match o {
                Out::Packet(b) => one(&b),
                Out::Packets(bs) => bs.iter().for_each(|b| one(b)),
                _ => {}
            }
        }
        out
    }

    /// The ids of the packets waiting in a channel, in order.
    fn ids(rx: &Receiver<Out>) -> Vec<i32> {
        packets(rx).into_iter().map(|(id, _)| id).collect()
    }

    /// What the network thread does as a login starts (`lan/conn.rs`): the login
    /// uses up the player's expectation, if there is one.
    fn login_starts(hub: &mut Hub, uuid: u128) {
        let taken = table(hub).take(uuid, Instant::now());
        if let Some(g) = hub.guests.iter_mut().find(|g| g.uuid == uuid) {
            g.expected = taken;
        }
    }

    fn place(hub: &mut Hub, conn: u32) {
        let mut world = crate::world::World::new(horizon_world::registry::BlockStateId(0), 0);
        let border = crate::worldborder::WorldBorder::default();
        let mut next = 100;
        let mut ctx = TickCtx {
            world: &mut world,
            entities: &[],
            next_entity: &mut next,
            spawn: [0.5, 64.0, 0.5],
            spawn_angle: [0.0; 2],
            difficulty: 2,
            difficulty_locked: false,
            clock_rate: 1.0,
            tick_rate: 20.0,
            tick_frozen: false,
            tick_steps: Default::default(),
            collision: None,
            border: &border,
            streaming: None,
        };
        hub.place_new_player(conn, &mut ctx);
    }

    fn tick_context<'a>(
        world: &'a mut crate::world::World,
        border: &'a crate::worldborder::WorldBorder,
        next: &'a mut u32,
    ) -> TickCtx<'a> {
        TickCtx {
            world,
            entities: &[],
            next_entity: next,
            spawn: [0.5, 64.0, 0.5],
            spawn_angle: [0.0; 2],
            difficulty: 2,
            difficulty_locked: false,
            clock_rate: 1.0,
            tick_rate: 20.0,
            tick_frozen: false,
            tick_steps: Default::default(),
            collision: None,
            border,
            streaming: None,
        }
    }

    fn is_warn(event: &crate::lan::hub::HostEvent, needle: &str) -> bool {
        matches!(event, crate::lan::hub::HostEvent::Warn(w) if w.contains(needle))
    }

    /// An expectation nobody came for is reported once and forgotten.
    #[test]
    fn an_expectation_nobody_came_for_is_reported_and_swept() {
        let mut hub = hub();
        // Already past its time when the hub next ticks.
        table(&hub)
            .expect(7, Duration::ZERO, Instant::now())
            .unwrap();
        let mut world = crate::world::World::new(horizon_world::registry::BlockStateId(0), 0);
        let border = crate::worldborder::WorldBorder::default();
        let mut next = 100;
        let mut ctx = tick_context(&mut world, &border, &mut next);
        let events = hub.poll(&mut ctx);
        assert_eq!(
            events.iter().filter(|e| is_warn(e, "never came")).count(),
            1
        );
        assert!(table(&hub).is_empty());
    }

    /// The control: a player nobody expected is announced as always, to the
    /// others by a chat line and a player-list entry.
    #[test]
    fn an_ordinary_login_is_announced_to_the_others() {
        let mut hub = hub();
        let (mut watcher, seen) = guest(1, 1, "Watcher");
        watcher.placed = true;
        hub.guests.push(watcher);
        let (newcomer, mine) = guest(2, 2, "Newcomer");
        hub.guests.push(newcomer);
        place(&mut hub, 2);
        let told = ids(&seen);
        assert!(told.contains(&id::SYSTEM_CHAT), "{told:?}");
        assert!(told.contains(&id::PLAYER_INFO_UPDATE), "{told:?}");
        assert!(hub.guests[1].placed && !hub.guests[1].held);
        assert_eq!(ids(&mine)[0], id::LOGIN);
    }

    /// A player the proxy announced is held: it gets the whole join sequence,
    /// and the player already there is told nothing at all.
    #[test]
    fn an_expected_player_is_held_and_no_one_else_hears_of_it() {
        let mut hub = hub();
        let (mut watcher, seen) = guest(1, 1, "Watcher");
        watcher.placed = true;
        hub.guests.push(watcher);
        let (newcomer, mine) = guest(2, 2, "Newcomer");
        hub.guests.push(newcomer);
        table(&hub)
            .expect(2, Duration::from_secs(10), Instant::now())
            .unwrap();
        login_starts(&mut hub, 2);
        place(&mut hub, 2);
        assert_eq!(
            ids(&seen),
            Vec::<i32>::new(),
            "the watcher must hear nothing"
        );
        let g = &hub.guests[1];
        assert!(g.held && !g.placed);
        assert!(table(&hub).is_empty(), "the login used up the expectation");
        let sent = ids(&mine);
        assert_eq!(sent[0], id::LOGIN);
        for wanted in [id::PLAYER_INFO_UPDATE, id::SET_CHUNK_CACHE_CENTER] {
            assert!(
                sent.contains(&wanted),
                "packet {wanted} missing from {sent:?}"
            );
        }
        // The held player is in its own list but did not announce itself.
        assert!(!sent.contains(&id::SYSTEM_CHAT), "{sent:?}");
    }

    /// An expectation is for one player: another uuid logs in as usual.
    #[test]
    fn the_expectation_of_one_player_does_not_hold_another() {
        let mut hub = hub();
        let (mut watcher, seen) = guest(1, 1, "Watcher");
        watcher.placed = true;
        hub.guests.push(watcher);
        let (newcomer, _mine) = guest(2, 2, "Newcomer");
        hub.guests.push(newcomer);
        table(&hub)
            .expect(3, Duration::from_secs(10), Instant::now())
            .unwrap();
        login_starts(&mut hub, 2);
        place(&mut hub, 2);
        assert!(hub.guests[1].placed && !hub.guests[1].held);
        assert!(ids(&seen).contains(&id::SYSTEM_CHAT));
        assert_eq!(table(&hub).len(), 1, "the other player is still expected");
    }

    /// A held player whose hand-over never finishes is dropped after a minute of
    /// ticks, and the players in the world are told nothing about it.
    #[test]
    fn a_held_player_that_is_never_imported_is_dropped_without_a_word() {
        let mut hub = hub();
        let (mut watcher, seen) = guest(1, 1, "Watcher");
        watcher.placed = true;
        hub.guests.push(watcher);
        let (newcomer, mine) = guest(2, 2, "Newcomer");
        hub.guests.push(newcomer);
        table(&hub)
            .expect(2, Duration::from_secs(10), Instant::now())
            .unwrap();
        login_starts(&mut hub, 2);
        place(&mut hub, 2);
        let _ = (ids(&seen), ids(&mine));
        let mut world = crate::world::World::new(horizon_world::registry::BlockStateId(0), 0);
        let border = crate::worldborder::WorldBorder::default();
        let mut next = 100;
        // Two ticks short of the limit (poll counts one): still waiting.
        hub.ticks = hub.guests[1].held_since + super::Settings::default().max_hold_ticks - 2;
        let mut ctx = tick_context(&mut world, &border, &mut next);
        let events = hub.poll(&mut ctx);
        assert!(!events.iter().any(|e| is_warn(e, "dropped")));
        // At the limit it is kicked, with a warning in the log.
        let events = hub.poll(&mut ctx);
        assert!(
            events
                .iter()
                .any(|e| is_warn(e, "Newcomer") && is_warn(e, "dropped"))
        );
        assert!(matches!(mine.try_recv(), Ok(Out::Disconnect(_))));
        // The connection closes: the hub removes the guest.
        let gone = hub.remove_guest(1, "Timed out");
        assert!(gone.held && !gone.placed);
        assert_eq!(
            ids(&seen),
            Vec::<i32>::new(),
            "the watcher must hear nothing"
        );
    }

    /// The control for the one above: when a placed player leaves, the others
    /// are told.
    #[test]
    fn a_placed_player_leaving_is_announced() {
        let mut hub = hub();
        let (mut watcher, seen) = guest(1, 1, "Watcher");
        watcher.placed = true;
        hub.guests.push(watcher);
        let (mut leaver, _mine) = guest(2, 2, "Leaver");
        leaver.placed = true;
        hub.guests.push(leaver);
        hub.remove_guest(1, "bye");
        let told = ids(&seen);
        assert!(told.contains(&id::PLAYER_INFO_REMOVE), "{told:?}");
        assert!(told.contains(&id::SYSTEM_CHAT), "{told:?}");
    }

    /// A held player is sent the chunks and the entities around it, and takes
    /// nothing from the players in the world: they still get their chunks, and
    /// never learn the held player's entity.
    #[test]
    fn a_held_player_sees_the_world_without_the_world_seeing_it() {
        let mut hub = hub();
        let mut world = crate::world::World::new(horizon_world::registry::BlockStateId(0), 0);
        for x in -5..=5 {
            for z in -5..=5 {
                // Only a lit column is sent.
                let mut chunk = crate::chunk::Chunk::empty(x, z);
                chunk.lit = true;
                world.chunks.insert((x, z), std::sync::Arc::new(chunk));
            }
        }
        let border = crate::worldborder::WorldBorder::default();
        let (mut watcher, seen) = guest(1, 1, "Watcher");
        watcher.placed = true;
        hub.guests.push(watcher);
        let (newcomer, mine) = guest(2, 2, "Newcomer");
        hub.guests.push(newcomer);
        // A second player in the world, to show that players are tracked at all.
        let (mut other, _other_rx) = guest(3, 3, "Other");
        other.placed = true;
        hub.guests.push(other);
        table(&hub)
            .expect(2, Duration::from_secs(10), Instant::now())
            .unwrap();
        login_starts(&mut hub, 2);
        place(&mut hub, 2);
        // The server has run a few ticks since they joined.
        hub.ticks = 10;
        let mut next = 100;
        let mut ctx = tick_context(&mut world, &border, &mut next);
        let (mut held_chunks, mut watcher_chunks) = (0, 0);
        let (mut held_adds, mut watcher_adds) = (Vec::new(), Vec::new());
        for _ in 0..30 {
            hub.tick_level(&mut ctx);
            hub.end_tick();
            let mine_now = packets(&mine);
            let seen_now = packets(&seen);
            held_chunks += mine_now
                .iter()
                .filter(|p| p.0 == id::LEVEL_CHUNK_WITH_LIGHT)
                .count();
            watcher_chunks += seen_now
                .iter()
                .filter(|p| p.0 == id::LEVEL_CHUNK_WITH_LIGHT)
                .count();
            held_adds.extend(added_of(&mine_now));
            watcher_adds.extend(added_of(&seen_now));
        }
        assert!(
            held_chunks > 0,
            "the held player is sent chunks (held {held_chunks}, watcher {watcher_chunks})"
        );
        assert!(watcher_chunks > 0, "the placed player is still sent chunks");
        assert!(
            watcher_adds.contains(&3),
            "players are tracked at all: {watcher_adds:?}"
        );
        assert!(
            held_adds.contains(&1),
            "the held player sees the watcher: {held_adds:?}"
        );
        assert!(
            !watcher_adds.contains(&2),
            "the watcher must not see the held player: {watcher_adds:?}"
        );
    }

    /// The entity ids in the Add Entity packets among some packets.
    fn added_of(packets: &[(i32, Vec<u8>)]) -> Vec<i32> {
        packets
            .iter()
            .filter(|(i, _)| *i == id::ADD_ENTITY)
            .map(|(_, payload)| {
                horizon_protocol::buf::Reader::new(payload)
                    .varint()
                    .unwrap()
            })
            .collect()
    }

    /// A held player's acknowledgements are handled (they keep the chunks
    /// coming), and nothing else it sends reaches the world.
    #[test]
    fn a_held_player_is_heard_only_when_it_acknowledges() {
        use horizon_protocol::packets::play::{ChunkBatchReceived, MovePlayerPos, Serverbound};
        let mut hub = hub();
        let (newcomer, _mine) = guest(2, 2, "Newcomer");
        hub.guests.push(newcomer);
        table(&hub)
            .expect(2, Duration::from_secs(10), Instant::now())
            .unwrap();
        login_starts(&mut hub, 2);
        place(&mut hub, 2);
        let g = &mut hub.guests[0];
        g.unacknowledged = 3;
        let before = g.position;
        g.inbox.push_back((
            Serverbound::MovePlayerPos(MovePlayerPos {
                x: before[0] + 40.0,
                y: before[1],
                z: before[2],
                flags: 1,
            }),
            10,
        ));
        g.inbox.push_back((
            Serverbound::ChunkBatchReceived(ChunkBatchReceived {
                chunks_per_tick: 12.0,
            }),
            5,
        ));
        g.inbox_bytes = 15;
        let mut world = crate::world::World::new(horizon_world::registry::BlockStateId(0), 0);
        let border = crate::worldborder::WorldBorder::default();
        let mut next = 100;
        let mut ctx = tick_context(&mut world, &border, &mut next);
        hub.drain_packets(&mut ctx);
        let g = &hub.guests[0];
        assert_eq!(g.position, before, "a held player does not move");
        assert_eq!(g.unacknowledged, 2, "its acknowledgement was handled");
        assert_eq!(g.chunks_per_tick, 12.0);
        assert_eq!(g.inbox_bytes, 0);
    }

    const UUID: &str = "123e4567-e89b-12d3-a456-426614174000";
    const UUID_N: u128 = 0x123e4567_e89b_12d3_a456_426614174000;

    #[test]
    fn a_unique_id_is_read_with_or_without_dashes_and_nothing_else() {
        assert_eq!(super::parse_uuid(UUID), Some(UUID_N));
        assert_eq!(
            super::parse_uuid("123e4567e89b12d3a456426614174000"),
            Some(UUID_N)
        );
        assert_eq!(
            super::parse_uuid("123E4567-E89B-12D3-A456-426614174000"),
            Some(UUID_N)
        );
        assert_eq!(
            super::parse_uuid("123e4567-e89b-12d3-a456-42661417400"),
            None
        );
        assert_eq!(
            super::parse_uuid("123e4567-e89b-12d3-a456-42661417400g"),
            None
        );
        assert_eq!(super::parse_uuid(""), None);
    }

    /// `transfer-status` counts each kind of player once, takes the places from the player limit's
    /// own count, and says the server is full once every place is taken.
    #[test]
    fn transfer_status_counts_the_players_and_the_places() {
        let mut hub = hub();
        let now = Instant::now();
        assert_eq!(
            super::status_request(&hub, now),
            serde_json::json!({"ok": true, "players": 0, "held": 0, "expected": 0, "frozen": 0,
                "online": 0, "max_players": 4, "full": false, "stopping": false})
        );
        let (mut placed, _a) = guest(1, 1, "Placed");
        placed.placed = true;
        let (mut frozen, _b) = guest(2, 2, "Frozen");
        frozen.placed = true;
        frozen.handover = Some(super::Handover {
            id: 7,
            since: 0,
            record: serde_json::Value::Null,
        });
        let (mut held, _c) = guest(3, 3, "Held");
        held.held = true;
        // Moved to another server, and on its way out.
        let (mut gone, _d) = guest(4, 4, "Gone");
        gone.placed = true;
        gone.released = true;
        hub.guests.extend([placed, frozen, held, gone]);
        table(&hub).expect(5, Duration::from_secs(10), now).unwrap();
        // A login takes its place on the network thread (`try_admit`): three logins here.
        let address = "127.0.0.1:25565".parse().unwrap();
        for uuid in 1..=3 {
            assert!(hub.lan.shared.try_admit(address, uuid, "p").is_none());
        }
        assert_eq!(
            super::status_request(&hub, now),
            serde_json::json!({"ok": true, "players": 2, "held": 1, "expected": 1, "frozen": 1,
                "online": 3, "max_players": 4, "full": false, "stopping": false})
        );
        assert!(hub.lan.shared.try_admit(address, 6, "p").is_none());
        let status = super::status_request(&hub, now);
        assert_eq!(status["online"], 4);
        assert_eq!(status["full"], true);
        // An expectation past its time is not counted, though nothing swept it yet.
        let later = super::status_request(&hub, now + Duration::from_secs(11));
        assert_eq!(later["expected"], 0);
        assert_eq!(table(&hub).len(), 1);
    }

    /// With `[transfer]` off it is refused like every request of a proxy, and its answer never
    /// names a player.
    #[test]
    fn transfer_status_names_no_player_and_is_off_with_transfers() {
        let mut hub = hub();
        let (mut alex, _rx) = guest(1, UUID_N, "Alex");
        alex.placed = true;
        hub.guests.push(alex);
        let text = super::status_request(&hub, Instant::now()).to_string();
        let uuid = horizon_protocol::crypto::uuid_string(UUID_N);
        assert!(!text.contains("Alex") && !text.contains(&uuid), "{text}");
        hub.transfers = None;
        assert_eq!(
            super::status_request(&hub, Instant::now()),
            serde_json::json!({ "error": super::OFF })
        );
    }

    #[test]
    fn player_expect_records_the_player_and_says_for_how_long() {
        let mut hub = hub();
        let now = Instant::now();
        let reply = super::expect_request(&mut hub, UUID, now);
        assert_eq!(reply["ok"], true);
        assert_eq!(reply["renewed"], false);
        assert_eq!(reply["ttl_seconds"], 10);
        // Asking again renews, with the time asked.
        let reply = super::expect_request(&mut hub, &format!("{UUID} 30"), now);
        assert_eq!(
            (reply["renewed"].as_bool(), reply["ttl_seconds"].as_u64()),
            (Some(true), Some(30))
        );
        // A time past the maximum is cut, and the reply says what it became.
        let reply = super::expect_request(&mut hub, &format!("{UUID} 3600"), now);
        assert_eq!(reply["ttl_seconds"], 60);
        assert_eq!(table(&hub).len(), 1);
        assert!(table(&hub).take(UUID_N, now));
        // The default time is the server's own.
        hub.transfers = Some(super::Transfers::new(super::Settings {
            expect: Duration::from_secs(4),
            ..super::Settings::default()
        }));
        let reply = super::expect_request(&mut hub, UUID, now);
        assert_eq!(reply["ttl_seconds"], 4);
    }

    #[test]
    fn player_expect_refuses_what_it_cannot_do_and_changes_nothing() {
        let mut hub = hub();
        let now = Instant::now();
        for args in [
            "",
            "not-a-uuid",
            &format!("{UUID} 0"),
            &format!("{UUID} soon"),
            &format!("{UUID} 5 6"),
        ] {
            let reply = super::expect_request(&mut hub, args, now);
            assert!(reply["error"].is_string(), "{args:?} gave {reply}");
        }
        // A player who is on the server already cannot be expected.
        let (online, _rx) = guest(1, UUID_N, "Online");
        hub.guests.push(online);
        let reply = super::expect_request(&mut hub, UUID, now);
        assert!(
            reply["error"]
                .as_str()
                .unwrap()
                .contains("already connected"),
            "{reply}"
        );
        assert!(table(&hub).is_empty());
    }

    #[test]
    fn player_expect_refuses_when_the_table_is_full() {
        let mut hub = hub();
        // Places for every expectation: it is the table that is full, not the server.
        hub.lan.shared.set_max_players(1000);
        let now = Instant::now();
        for n in 0..super::Settings::default().max_expected as u128 {
            let reply = super::expect_request(&mut hub, &format!("{:032x}", 1000 + n), now);
            assert_eq!(reply["ok"], true);
        }
        let reply = super::expect_request(&mut hub, UUID, now);
        assert!(
            reply["error"].as_str().unwrap().contains("too many"),
            "{reply}"
        );
    }

    /// Each expectation holds a place: once the logins and the other expected players take every
    /// place, a new expectation is refused as a login would be, though not for an op who bypasses
    /// the limit. A renewal holds no second place, and one past its time frees its place.
    #[test]
    fn player_expect_refuses_a_full_server_and_the_expected_hold_their_places() {
        const OP: u128 = 0xF;
        let mut hub = hub_with(NetConfig {
            announce: false,
            bypasses_limit: Some(std::sync::Arc::new(|uuid| uuid == OP)),
            ..NetConfig::loopback(0, "ArrivalsTest".into(), 2)
        });
        let now = Instant::now();
        let expect = |hub: &mut Hub, uuid: u128, at: Instant| {
            super::expect_request(hub, &format!("{uuid:032x}"), at)
        };
        let address = "127.0.0.1:25565".parse().unwrap();
        assert!(hub.lan.shared.try_admit(address, 1, "p").is_none());
        assert_eq!(expect(&mut hub, 2, now)["ok"], true);
        assert_eq!(expect(&mut hub, 2, now)["renewed"], true);
        let full = serde_json::json!({ "error": super::FULL });
        assert_eq!(expect(&mut hub, 3, now), full);
        assert_eq!(expect(&mut hub, OP, now)["ok"], true);
        assert_eq!(table(&hub).len(), 2);
        // Player 2's expectation ran out: its place is free again.
        let later = now + Duration::from_secs(11);
        assert_eq!(expect(&mut hub, 3, later)["ok"], true);
        assert_eq!(expect(&mut hub, 4, later), full);
        // A login gives a place back when it ends (`Shared::release`).
        hub.lan.shared.release(1);
        assert_eq!(expect(&mut hub, 4, later)["ok"], true);
    }

    /// Once the server has begun to stop, no player is expected, imported or placed any more, and
    /// `transfer-status` says so; a player here can still be exported and released, which is its
    /// way out, and a held one dropped.
    #[test]
    fn a_server_that_is_stopping_takes_no_one_in_but_lets_its_players_go() {
        let stopping = serde_json::json!({ "error": super::STOPPING });
        let stop = |hub: &mut Hub| hub.transfers.as_mut().unwrap().stopping = true;
        let mut hub = hub();
        let now = Instant::now();
        stop(&mut hub);
        assert_eq!(super::expect_request(&mut hub, UUID, now), stopping);
        assert!(table(&hub).is_empty());
        assert_eq!(super::status_request(&hub, now)["stopping"], true);
        // A player here is exported and released as before.
        let (mut mover, _rx) = guest(1, UUID_N, "Mover");
        mover.placed = true;
        hub.guests.push(mover);
        assert!(super::export_record(&mut hub, 0, true, Default::default()).is_ok());
        let (mut hub, _seen, _mine, h) = frozen_with_a_watcher();
        stop(&mut hub);
        let reply = super::release_request(&mut hub, &format!("{UUID} {h} moved"));
        assert_eq!(reply["released"], "moved");
        // The record of a held player cannot be imported...
        let (record, mut hub, _seen, _mine) = held_in_a_second_hub();
        stop(&mut hub);
        let error =
            import_player(&mut hub, UUID_N, &serde_json::json!({ "record": record })).unwrap_err();
        assert_eq!(error, super::STOPPING);
        assert!(hub.guests[1].held && !hub.guests[1].imported);
        // ... nor one already imported confirmed, but it can be dropped.
        let (mut hub, seen, _mine) = import_without_placing(serde_json::json!({}));
        stop(&mut hub);
        assert_eq!(confirm(&mut hub, UUID), stopping);
        assert!(hub.guests[1].held && !hub.guests[1].placed);
        assert_eq!(super::drop_request(&mut hub, UUID)["held"], true);
        assert_eq!(ids(&seen), Vec::<i32>::new());
    }

    /// During a stop's wait a player is exported, and frozen while a hand-over can still finish in
    /// time; later only an export already frozen is answered.
    #[test]
    fn a_stop_lets_a_hand_over_start_only_while_it_can_finish() {
        let (mut hub, _seen, _mine, _h) = frozen_with_a_watcher();
        let now = Instant::now();
        let transfers = hub.transfers.as_mut().unwrap();
        transfers.stop_wait =
            super::StopWait::start(&transfers.settings, now, Duration::from_millis(50));
        assert_eq!(super::placed_and_frozen(&hub), (2, 1));
        assert_eq!(
            super::check_freeze(&hub, 0, now + Duration::from_secs(10)),
            Ok(())
        );
        let late = now + Duration::from_secs(11);
        assert!(super::check_freeze(&hub, 0, late).is_err());
        assert_eq!(super::check_freeze(&hub, 1, late), Ok(()), "already frozen");
    }

    /// After the commit, a retry finds the player placed from that hand-over whatever came since:
    /// a stop that began meanwhile does not turn the retry into a refusal.
    #[test]
    fn a_retry_after_the_commit_is_answered_on_a_server_that_is_stopping() {
        let (mut hub, _seen, _mine) = import_without_placing(serde_json::json!({}));
        let id = hub.guests[1].arrival_handover.unwrap();
        let args = format!("{UUID} {}", super::handover_text(id));
        assert_eq!(confirm(&mut hub, &args)["placed"], true);
        hub.transfers.as_mut().unwrap().stopping = true;
        assert_eq!(super::confirm_args(&hub, &args), Ok((UUID_N, id)));
    }

    /// Dropping a held player disconnects it and says nothing to the world.
    #[test]
    fn player_drop_removes_a_held_player_without_a_word_to_anyone() {
        let mut hub = hub();
        let (mut watcher, seen) = guest(1, 1, "Watcher");
        watcher.placed = true;
        hub.guests.push(watcher);
        // The proxy announces the player first; then the player logs in.
        assert_eq!(
            super::expect_request(&mut hub, UUID, Instant::now())["ok"],
            true
        );
        let (newcomer, mine) = guest(2, UUID_N, "Newcomer");
        hub.guests.push(newcomer);
        login_starts(&mut hub, UUID_N);
        place(&mut hub, 2);
        let _ = (ids(&seen), ids(&mine));
        let reply = super::drop_request(&mut hub, UUID);
        assert_eq!(
            (reply["held"].as_bool(), reply["expected"].as_bool()),
            (Some(true), Some(false))
        );
        assert!(matches!(mine.try_recv(), Ok(Out::Disconnect(_))));
        // The connection closes and the hub forgets the guest.
        let gone = hub.remove_guest(1, "dropped");
        assert!(gone.held && !gone.placed);
        assert_eq!(
            ids(&seen),
            Vec::<i32>::new(),
            "the watcher must hear nothing"
        );
    }

    #[test]
    fn player_drop_forgets_an_expectation_and_refuses_a_stranger() {
        let mut hub = hub();
        super::expect_request(&mut hub, UUID, Instant::now());
        let reply = super::drop_request(&mut hub, UUID);
        assert_eq!(
            (reply["held"].as_bool(), reply["expected"].as_bool()),
            (Some(false), Some(true))
        );
        assert!(table(&hub).is_empty());
        let reply = super::drop_request(&mut hub, UUID);
        assert!(reply["error"].is_string(), "{reply}");
        assert!(super::drop_request(&mut hub, "nonsense")["error"].is_string());
    }

    /// A placed player cannot be dropped this way: only a held one.
    #[test]
    fn player_drop_leaves_a_placed_player_alone() {
        let mut hub = hub();
        let (mut here, rx) = guest(1, UUID_N, "Here");
        here.placed = true;
        hub.guests.push(here);
        let reply = super::drop_request(&mut hub, UUID);
        assert!(reply["error"].is_string(), "{reply}");
        assert!(
            rx.try_recv().is_err(),
            "a placed player must not be disconnected"
        );
    }

    /// The keys a vanilla server writes in every player's file, read with `javap` from the
    /// 26.3 server jar: `Player.addAdditionalSaveData` (Inventory, SelectedItemSlot,
    /// EnderItems, XpLevel, XpP, XpTotal, XpSeed, abilities), `FoodData` (foodLevel,
    /// foodSaturationLevel, foodExhaustionLevel, foodTickTimer), `ServerPlayer` (Dimension),
    /// `Entity.saveWithoutId` (Pos, Motion, Rotation, OnGround, Air, Fire) and
    /// `LivingEntity` (Health, AbsorptionAmount).
    const VANILLA_PLAYER_KEYS: [&str; 21] = [
        "Inventory",
        "SelectedItemSlot",
        "EnderItems",
        "XpLevel",
        "XpP",
        "XpTotal",
        "XpSeed",
        "abilities",
        "foodLevel",
        "foodSaturationLevel",
        "foodExhaustionLevel",
        "foodTickTimer",
        "Dimension",
        "Pos",
        "Motion",
        "Rotation",
        "OnGround",
        "Air",
        "Fire",
        "Health",
        "AbsorptionAmount",
    ];

    /// A guest standing at a known place with known values.
    fn exported_guest() -> (Hub, usize) {
        use crate::items::ItemStack;
        let mut hub = hub();
        let (mut g, _rx) = guest(5, UUID_N, "Alex");
        g.placed = true;
        g.position = [10.5, 70.0, -3.25];
        g.yaw = 90.0;
        g.pitch = 15.0;
        g.health = 14.0;
        g.food = 12;
        g.xp_level = 7;
        g.game_mode = 1;
        g.selected = 3;
        g.inventory[0] = Some(ItemStack {
            item: std::sync::Arc::from("minecraft:stone"),
            count: 5,
            damage: 0,
            extra: None,
        });
        hub.guests.push(g);
        (hub, 0)
    }

    /// Decodes the compound of a record back into its tag.
    fn compound_of(record: &serde_json::Value) -> horizon_world::nbt::Tag {
        use horizon_world::nbt::{Limits, read_named};
        let bytes = crate::skins::base64_decode(record["player"].as_str().unwrap()).unwrap();
        read_named(&bytes, Limits::saved()).unwrap().0.tag
    }

    #[test]
    fn a_player_who_cannot_be_exported_is_refused_with_the_reason() {
        let mut hub = hub();
        let expect = |hub: &Hub| super::check_exportable(hub, UUID_N);
        assert_eq!(
            expect(&hub),
            Err("that player is not connected to this server")
        );
        let (g, _rx) = guest(5, UUID_N, "Alex");
        hub.guests.push(g);
        assert_eq!(expect(&hub), Err("that player has not finished joining"));
        hub.guests[0].placed = true;
        assert_eq!(expect(&hub), Ok(0));
        hub.guests[0].vehicle = Some(77);
        assert_eq!(expect(&hub), Err("that player is riding"));
        hub.guests[0].vehicle = None;
        hub.guests[0].sleeping = true;
        assert_eq!(expect(&hub), Err("that player is asleep"));
        hub.guests[0].sleeping = false;
        hub.guests[0].dead = true;
        assert_eq!(expect(&hub), Err("that player is dead"));
        hub.guests[0].dead = false;
        hub.guests[0].placed = false;
        hub.guests[0].held = true;
        assert_eq!(
            expect(&hub),
            Err("that player is held: it has not been handed over yet")
        );
    }

    #[test]
    fn the_record_carries_what_a_proxy_reads_and_the_vanilla_player_compound() {
        use crate::save::{float, number};
        let (mut hub, i) = exported_guest();
        let records = super::Records {
            advancements: serde_json::json!({"done": []}),
            stats: serde_json::json!({"stats": {"minecraft:custom": {"minecraft:jump": 3}}}),
        };
        let record = super::export_record(&mut hub, i, false, records).unwrap();
        assert_eq!(record["version"], 1);
        assert_eq!(record["data_version"], horizon_world::DATA_VERSION);
        assert_eq!(record["uuid"], UUID);
        assert_eq!(record["name"], "Alex");
        assert_eq!(record["dimension"], "minecraft:overworld");
        assert_eq!(record["position"], serde_json::json!([10.5, 70.0, -3.25]));
        assert_eq!(record["rotation"], serde_json::json!([90.0, 15.0]));
        assert_eq!(record["game_mode"], 1);
        assert_eq!(record["advancements"], serde_json::json!({"done": []}));
        assert_eq!(
            record["stats"]["stats"]["minecraft:custom"]["minecraft:jump"],
            3
        );
        // A read, not a hand-over: no id, and the player goes on.
        assert_eq!(record["handover"], serde_json::Value::Null);
        assert_eq!(hub.guests[i].handover, None, "no freeze was asked for");

        let tag = compound_of(&record);
        for key in VANILLA_PLAYER_KEYS {
            assert!(tag.get(key).is_some(), "the compound has no {key}");
        }
        assert_eq!(number(tag.get("XpLevel")), Some(7));
        assert_eq!(number(tag.get("foodLevel")), Some(12));
        assert_eq!(number(tag.get("SelectedItemSlot")), Some(3));
        assert_eq!(float(tag.get("Health")), Some(14.0));
        let horizon_world::nbt::Tag::String(dimension) = tag.get("Dimension").unwrap() else {
            panic!("Dimension is not a string");
        };
        assert_eq!(dimension.to_string_lossy(), "minecraft:overworld");
        // The stone in the first slot, as vanilla writes a stack: Slot, id, count.
        let horizon_world::nbt::Tag::List(items) = tag.get("Inventory").unwrap() else {
            panic!("Inventory is not a list");
        };
        let first = &items[0];
        assert_eq!(number(first.get("Slot")), Some(0));
        assert_eq!(number(first.get("count")), Some(5));
    }

    /// A frozen player's inputs are not acted on, its acknowledgements are, and a freeze
    /// nobody finishes is let go.
    #[test]
    fn a_frozen_player_is_inert_until_it_is_let_go() {
        use horizon_protocol::packets::play::{ChunkBatchReceived, MovePlayerPos, Serverbound};
        let (mut hub, i) = exported_guest();
        let record = super::export_record(&mut hub, i, true, Default::default()).unwrap();
        let handover = hub.guests[i].handover.clone().unwrap();
        assert_eq!(handover.since, hub.ticks);
        assert_eq!(
            record["handover"].as_str(),
            Some(super::handover_text(handover.id).as_str())
        );
        let before = hub.guests[i].position;
        let g = &mut hub.guests[i];
        g.unacknowledged = 2;
        g.inbox.push_back((
            Serverbound::MovePlayerPos(MovePlayerPos {
                x: before[0] + 30.0,
                y: before[1],
                z: before[2],
                flags: 1,
            }),
            10,
        ));
        g.inbox.push_back((
            Serverbound::ChunkBatchReceived(ChunkBatchReceived {
                chunks_per_tick: 9.0,
            }),
            5,
        ));
        g.inbox_bytes = 15;
        let mut world = crate::world::World::new(horizon_world::registry::BlockStateId(0), 0);
        let border = crate::worldborder::WorldBorder::default();
        let mut next = 100;
        let mut ctx = tick_context(&mut world, &border, &mut next);
        hub.drain_packets(&mut ctx);
        assert_eq!(
            hub.guests[i].position, before,
            "a frozen player does not move"
        );
        assert_eq!(
            hub.guests[i].unacknowledged, 1,
            "its acknowledgement was handled"
        );
        // Nobody finishes the hand-over: thirty seconds of ticks later it is let go.
        let max = super::Settings::default().max_freeze_ticks;
        hub.ticks = handover.since + max - 2;
        assert!(!hub.poll(&mut ctx).iter().any(|e| is_warn(e, "frozen")));
        assert!(hub.guests[i].handover.is_some());
        assert!(hub.poll(&mut ctx).iter().any(|e| is_warn(e, "frozen")));
        assert_eq!(hub.guests[i].handover, None, "the hand-over is over");
    }

    #[test]
    fn the_freeze_lasts_the_settings_ticks() {
        assert_eq!(super::Settings::default().max_freeze_ticks, 600);
        assert!(!super::frozen_too_long(10, 10 + 599, 600));
        assert!(super::frozen_too_long(10, 10 + 600, 600));
        assert!(!super::frozen_too_long(500, 100, 600));
        assert!(super::frozen_too_long(10, 50, 40));
    }

    /// A proxy that lost the reply asks again: the same hand-over, the same
    /// bytes, and the freeze is not restarted, however the player changed since.
    #[test]
    fn a_repeated_export_answers_with_the_first_record() {
        let (mut hub, i) = exported_guest();
        let first = super::export_record(&mut hub, i, true, Default::default()).unwrap();
        let since = hub.guests[i].handover.as_ref().unwrap().since;
        hub.ticks += 50;
        hub.guests[i].health = 3.0;
        hub.guests[i].position[0] += 4.0;
        for freeze in [true, false] {
            let again = super::export_record(&mut hub, i, freeze, Default::default()).unwrap();
            assert_eq!(
                serde_json::to_string(&again).unwrap(),
                serde_json::to_string(&first).unwrap()
            );
        }
        assert_eq!(hub.guests[i].handover.as_ref().unwrap().since, since);
        // The reply says how long the freeze still lasts: 50 ticks have gone.
        let reply = super::export_reply(&hub, i, first.clone());
        assert_eq!(reply["freeze_ticks_left"], 550);
        assert_eq!(reply["record"], first);
        // Once it is over, a new freeze is a new hand-over.
        hub.guests[i].handover = None;
        let next = super::export_record(&mut hub, i, true, Default::default()).unwrap();
        assert_ne!(next["handover"], first["handover"]);
        assert_eq!(next["position"][0], 14.5);
    }

    /// A player frozen in the rising part of a jump is not kicked for flying:
    /// its moves are dropped, so the last one's floating is stale.
    #[test]
    fn a_frozen_player_is_not_kicked_for_floating() {
        let (mut hub, i) = exported_guest();
        super::export_record(&mut hub, i, true, Default::default()).unwrap();
        hub.guests[i].client_is_floating = true;
        let mut world = crate::world::World::new(horizon_world::registry::BlockStateId(0), 0);
        let border = crate::worldborder::WorldBorder::default();
        let mut next = 100;
        let mut ctx = tick_context(&mut world, &border, &mut next);
        for _ in 0..200 {
            hub.drain_packets(&mut ctx);
        }
        assert_eq!(hub.guests[i].above_ground_ticks, 0);
        assert!(hub.guests[i].left_reason.is_empty(), "not kicked");
    }

    /// A frozen player's chat acknowledgements are still taken, in order with
    /// its chat message's last-seen update, and the message is dropped without
    /// a kick.
    #[test]
    fn a_frozen_player_keeps_acknowledging_chat() {
        use horizon_protocol::packets::play::{Chat, ChatAck, LastSeenUpdate, Serverbound};
        let (mut hub, i) = exported_guest();
        super::export_record(&mut hub, i, true, Default::default()).unwrap();
        let g = &mut hub.guests[i];
        // One message sent to the player: the window's 21st entry. The ack that
        // comes first drops the oldest, so the message is at index 19 when the
        // chat names it; taken the other way round, index 19 is still empty and
        // the chat fails validation (a kick).
        g.last_seen
            .add_pending(&[7; horizon_protocol::secure_chat::SIGNATURE_BYTES]);
        g.inbox
            .push_back((Serverbound::ChatAck(ChatAck { offset: 1 }), 2));
        g.inbox.push_back((
            Serverbound::Chat(Chat {
                message: "hello".into(),
                timestamp: 0,
                salt: 0,
                signature: None,
                last_seen: LastSeenUpdate {
                    offset: 0,
                    acknowledged: [0, 0, 0b1000],
                    checksum: 0,
                },
            }),
            10,
        ));
        g.inbox_bytes = 12;
        hub.keep_acknowledgements(i, &mut Vec::new());
        // Both were handled here, in the order they came; nothing is left for the inbox.
        assert!(hub.guests[i].inbox.is_empty());
        assert_eq!(hub.guests[i].inbox_bytes, 0);
        assert!(hub.guests[i].left_reason.is_empty(), "not kicked");
        assert_eq!(hub.guests[i].last_seen.tracked_count(), 20);
    }

    // ---- player-release ------------------------------------------------------

    /// A watcher and a placed player frozen for a hand-over, and the hand-over's id.
    fn frozen_with_a_watcher() -> (Hub, Receiver<Out>, Receiver<Out>, String) {
        let mut hub = hub();
        let (mut watcher, seen) = guest(1, 1, "Watcher");
        watcher.placed = true;
        hub.guests.push(watcher);
        let (mut mover, mine) = guest(2, UUID_N, "Mover");
        mover.placed = true;
        hub.guests.push(mover);
        hub.ticks = 50;
        let record = super::export_record(&mut hub, 1, true, Default::default()).unwrap();
        let handover = record["handover"].as_str().unwrap().to_owned();
        let _ = (ids(&seen), ids(&mine));
        (hub, seen, mine, handover)
    }

    fn poll(hub: &mut Hub) -> Vec<crate::lan::hub::HostEvent> {
        let mut world = crate::world::World::new(horizon_world::registry::BlockStateId(0), 0);
        let border = crate::worldborder::WorldBorder::default();
        let mut next = 100;
        let mut ctx = tick_context(&mut world, &border, &mut next);
        hub.poll(&mut ctx)
    }

    fn left_events(hub: &mut Hub) -> Vec<Guest> {
        poll(hub)
            .into_iter()
            .filter_map(|e| match e {
                crate::lan::hub::HostEvent::Left(g) => Some(*g),
                _ => None,
            })
            .collect()
    }

    /// Moved to another server: the others see it go, but nothing is said and nothing saved.
    #[test]
    fn a_moved_player_leaves_the_world_without_the_leave_line_and_is_not_saved() {
        let (mut hub, seen, mine, h) = frozen_with_a_watcher();
        let reply = super::release_request(&mut hub, &format!("{UUID} {h} moved"));
        assert_eq!(
            reply,
            serde_json::json!({"ok": true, "released": "moved", "announced": false})
        );
        assert_eq!(hub.guests.len(), 1, "the mover is out of the hub");
        let told = ids(&seen);
        assert!(told.contains(&id::PLAYER_INFO_REMOVE), "{told:?}");
        assert!(told.contains(&id::REMOVE_ENTITIES), "{told:?}");
        assert!(!told.contains(&id::SYSTEM_CHAT), "no leave line: {told:?}");
        assert!(matches!(mine.try_recv(), Ok(Out::Disconnect(_))));
        // The game is told it left, released: it saves no file and no record.
        let left = left_events(&mut hub);
        assert_eq!(left.len(), 1);
        assert!(left[0].released && left[0].placed && left[0].handover.is_none());
        assert_eq!(left[0].left_reason, "moved to another server");
    }

    #[test]
    fn a_moved_player_can_be_announced_as_leaving() {
        let (mut hub, seen, _mine, h) = frozen_with_a_watcher();
        let reply = super::release_request(&mut hub, &format!("{UUID} {h} moved announce"));
        assert_eq!(reply["announced"], true);
        assert!(ids(&seen).contains(&id::SYSTEM_CHAT));
    }

    #[test]
    fn an_aborted_hand_over_lets_the_player_go_on_as_it_was() {
        let (mut hub, seen, mine, h) = frozen_with_a_watcher();
        let reply = super::release_request(&mut hub, &format!("{UUID} {h} abort"));
        assert_eq!(reply, serde_json::json!({"ok": true, "released": "abort"}));
        assert_eq!(hub.guests.len(), 2);
        assert!(hub.guests[1].handover.is_none() && hub.guests[1].placed);
        assert_eq!(ids(&seen), Vec::<i32>::new());
        assert_eq!(ids(&mine), Vec::<i32>::new());
        assert!(left_events(&mut hub).is_empty());
        // The hand-over is over: a late `moved` for it is refused.
        let reply = super::release_request(&mut hub, &format!("{UUID} {h} moved"));
        assert!(reply["error"].is_string(), "{reply}");
        assert_eq!(hub.guests.len(), 2);
    }

    /// A release names the hand-over: one for another (an older one, or one the proxy
    /// mixed up) changes nothing.
    #[test]
    fn a_release_for_another_hand_over_is_refused_and_changes_nothing() {
        let (mut hub, seen, _mine, h) = frozen_with_a_watcher();
        let rest: String = h.chars().skip(1).collect();
        let other = format!("{}{rest}", if h.starts_with('0') { '1' } else { '0' });
        for how in ["moved", "abort"] {
            let reply = super::release_request(&mut hub, &format!("{UUID} {other} {how}"));
            assert!(
                reply["error"]
                    .as_str()
                    .unwrap()
                    .contains("not the player's current hand-over"),
                "{reply}"
            );
        }
        assert_eq!(hub.guests.len(), 2);
        assert!(hub.guests[1].handover.is_some(), "still frozen");
        assert_eq!(ids(&seen), Vec::<i32>::new());
    }

    #[test]
    fn a_release_is_refused_for_a_player_who_is_not_frozen_and_changes_nothing() {
        let (mut hub, seen, _mine, h) = frozen_with_a_watcher();
        hub.guests[1].handover = None;
        for how in ["moved", "abort"] {
            let reply = super::release_request(&mut hub, &format!("{UUID} {h} {how}"));
            assert!(
                reply["error"]
                    .as_str()
                    .unwrap()
                    .contains("not in a hand-over"),
                "{reply}"
            );
        }
        // Not connected at all.
        let reply = super::release_request(
            &mut hub,
            &format!("00000000000000000000000000000001 {h} moved"),
        );
        assert!(reply["error"].is_string());
        assert_eq!(hub.guests.len(), 2);
        assert_eq!(ids(&seen), Vec::<i32>::new());
    }

    #[test]
    fn a_release_must_say_which_hand_over_how_and_nothing_more() {
        let (mut hub, _seen, _mine, h) = frozen_with_a_watcher();
        for args in [
            String::new(),
            "nonsense moved".to_owned(),
            UUID.to_owned(),
            format!("{UUID} moved"),
            format!("{UUID} {h}"),
            format!("{UUID} not-an-id moved"),
            format!("{UUID} {h} later"),
            format!("{UUID} {h} moved quietly"),
            format!("{UUID} {h} abort announce"),
            format!("{UUID} {h} moved announce now"),
        ] {
            let reply = super::release_request(&mut hub, &args);
            assert!(reply["error"].is_string(), "{args:?} gave {reply}");
        }
        assert!(
            hub.guests[1].handover.is_some(),
            "no refusal touched the player"
        );
    }

    /// Once the freeze ran out the player played on, and the record the other server has is
    /// older than it: a late `moved` is refused, and the proxy fails the hand-over.
    #[test]
    fn a_release_that_comes_after_the_freeze_ran_out_is_refused() {
        let (mut hub, seen, mine, h) = frozen_with_a_watcher();
        let since = hub.guests[1].handover.as_ref().unwrap().since;
        hub.ticks = since + super::Settings::default().max_freeze_ticks;
        assert!(poll(&mut hub).iter().any(|e| is_warn(e, "frozen")));
        assert!(hub.guests[1].handover.is_none());
        let reply = super::release_request(&mut hub, &format!("{UUID} {h} moved"));
        assert!(
            reply["error"].as_str().unwrap().contains("freeze ran out"),
            "{reply}"
        );
        assert_eq!(hub.guests.len(), 2, "the player stays");
        assert!(!ids(&seen).contains(&id::PLAYER_INFO_REMOVE));
        assert!(!matches!(mine.try_recv(), Ok(Out::Disconnect(_))));
    }

    /// A frozen player takes no damage, but the void and `/kill` still kill it: its record is
    /// then not what is left, so `moved` is refused and the next tick ends the hand-over.
    #[test]
    fn a_player_who_died_while_frozen_cannot_be_released_as_moved() {
        let (mut hub, _seen, _mine, h) = frozen_with_a_watcher();
        hub.guests[1].dead = true;
        let reply = super::release_request(&mut hub, &format!("{UUID} {h} moved"));
        assert!(reply["error"].as_str().unwrap().contains("died"), "{reply}");
        assert_eq!(hub.guests.len(), 2);
        assert!(
            poll(&mut hub)
                .iter()
                .any(|e| is_warn(e, "died while frozen"))
        );
        assert!(hub.guests[1].handover.is_none());
    }

    // ---- the world is loaded for a held player; how a held player's connection ended --

    // ---- the world loaded around a held player; how its connection ended ----

    #[test]
    fn the_world_is_loaded_around_the_players_placed_and_the_held_ones_only() {
        let mut hub = hub();
        let overworld = |mut g: Guest, at: [f64; 3]| {
            g.position = at;
            g
        };
        // Placed in the level in place.
        let (mut placed, _a) = guest(1, 1, "Placed");
        placed.placed = true;
        hub.guests.push(overworld(placed, [8.0, 64.0, 8.0]));
        // Held: sent the level too, so the level must be loaded around it.
        let (mut held, _b) = guest(2, 2, "Held");
        held.held = true;
        hub.guests.push(overworld(held, [100.5, 64.0, -3.0]));
        // Neither (still logging in): nothing to load for it.
        let (idle, _c) = guest(3, 3, "Idle");
        hub.guests.push(overworld(idle, [500.0, 64.0, 500.0]));
        // Placed in another level.
        let (mut elsewhere, _d) = guest(4, 4, "Elsewhere");
        elsewhere.placed = true;
        elsewhere.dimension = "minecraft:the_nether".into();
        hub.guests.push(overworld(elsewhere, [900.0, 64.0, 900.0]));
        assert_eq!(hub.loading_chunks(|_| true), vec![(0, 0), (6, -1)]);
        // The caller can leave a player out (a spectator that may not load chunks).
        assert_eq!(hub.loading_chunks(|g| g.name != "Held"), vec![(0, 0)]);
    }

    #[test]
    fn a_dropped_held_player_keeps_how_its_connection_really_ended() {
        let mut hub = hub();
        let (mut held, _rx) = guest(2, UUID_N, "Held");
        held.held = true;
        hub.guests.push(held);
        assert!(super::drop_request(&mut hub, UUID)["ok"] == true);
        let gone = hub.remove_guest(0, "Invalid packet 0: unexpected end of data");
        assert_eq!(
            gone.left_reason,
            "dropped before its hand-over (the connection ended: Invalid packet 0: unexpected end of data)"
        );
    }

    #[test]
    fn a_held_player_whose_connection_ends_by_itself_logs_the_real_reason() {
        let mut hub = hub();
        let (mut held, _rx) = guest(2, UUID_N, "Held");
        held.held = true;
        hub.guests.push(held);
        let gone = hub.remove_guest(0, "Invalid packet 0: unexpected end of data");
        assert_eq!(gone.left_reason, "Invalid packet 0: unexpected end of data");
    }

    /// The control: a placed player keeps the reason it was kicked with, as vanilla logs it.
    #[test]
    fn a_placed_player_keeps_the_reason_it_was_kicked_with() {
        let mut hub = hub();
        let (mut placed, _rx) = guest(2, UUID_N, "Placed");
        placed.placed = true;
        placed.left_reason = "Timed out".into();
        hub.guests.push(placed);
        let gone = hub.remove_guest(0, "Disconnected");
        assert_eq!(gone.left_reason, "Timed out");
    }

    // ---- the held player's limits and its picture of the others ------------

    /// A held player held under the hub's own settings.
    fn held_with_a_watcher(settings: super::Settings) -> (Hub, Receiver<Out>, Receiver<Out>) {
        let mut hub = hub();
        hub.transfers = Some(super::Transfers::new(settings));
        let (mut watcher, seen) = guest(1, 1, "Watcher");
        watcher.placed = true;
        hub.guests.push(watcher);
        table(&hub)
            .expect(2, Duration::from_secs(10), Instant::now())
            .unwrap();
        let (newcomer, mine) = guest(2, 2, "Newcomer");
        hub.guests.push(newcomer);
        login_starts(&mut hub, 2);
        place(&mut hub, 2);
        assert!(hub.guests[1].held);
        let _ = (ids(&seen), ids(&mine));
        (hub, seen, mine)
    }

    /// `[transfer]`'s `max_hold_seconds` sets the wait, and a held player whose
    /// connection is slow to close is kicked and reported once, not on every tick.
    #[test]
    fn a_held_player_waits_the_servers_time_and_is_dropped_once() {
        let settings = super::Settings {
            max_hold_ticks: 40,
            ..super::Settings::default()
        };
        let (mut hub, _seen, _mine) = held_with_a_watcher(settings);
        let mut world = crate::world::World::new(horizon_world::registry::BlockStateId(0), 0);
        let border = crate::worldborder::WorldBorder::default();
        let mut next = 100;
        let mut ctx = tick_context(&mut world, &border, &mut next);
        hub.ticks = hub.guests[1].held_since + 38;
        assert!(!hub.poll(&mut ctx).iter().any(|e| is_warn(e, "dropped")));
        assert!(hub.poll(&mut ctx).iter().any(|e| is_warn(e, "dropped")));
        // The guest stays until its connection closes: no second kick.
        assert_eq!(hub.guests.len(), 2);
        for _ in 0..5 {
            assert!(!hub.poll(&mut ctx).iter().any(|e| is_warn(e, "dropped")));
        }
    }

    /// A player who joins while another is held is in the held one's list, or
    /// the held client drops its entity; and one who leaves goes from that list.
    #[test]
    fn a_held_player_hears_of_the_players_who_join_and_leave() {
        let (mut hub, seen, mine) = held_with_a_watcher(super::Settings::default());
        let (late, _late_rx) = guest(3, 3, "Late");
        hub.guests.push(late);
        place(&mut hub, 3);
        let told = ids(&mine);
        assert!(told.contains(&id::PLAYER_INFO_UPDATE), "{told:?}");
        assert!(
            !told.contains(&id::SYSTEM_CHAT),
            "the held player is not in the chat yet: {told:?}"
        );
        let _ = ids(&seen);
        let i = hub.guests.iter().position(|g| g.uuid == 1).unwrap();
        hub.remove_guest(i, "Disconnected");
        let told = ids(&mine);
        assert!(told.contains(&id::PLAYER_INFO_REMOVE), "{told:?}");
        assert!(told.contains(&id::REMOVE_ENTITIES), "{told:?}");
    }

    #[test]
    fn player_drop_is_refused_with_transfers_off() {
        let mut hub = hub();
        hub.transfers = None;
        assert_eq!(super::drop_request(&mut hub, UUID)["error"], super::OFF);
    }

    // ---- player-import ------------------------------------------------------

    /// The record of the exported guest, and a second hub in which the same player is held
    /// (with a placed watcher), ready to take it.
    fn held_in_a_second_hub() -> (serde_json::Value, Hub, Receiver<Out>, Receiver<Out>) {
        let (mut source, i) = exported_guest();
        let record = super::export_record(&mut source, i, true, Default::default()).unwrap();
        let mut hub = hub();
        let (mut watcher, seen) = guest(1, 1, "Watcher");
        watcher.placed = true;
        hub.guests.push(watcher);
        assert_eq!(
            super::expect_request(&mut hub, UUID, Instant::now())["ok"],
            true
        );
        let (newcomer, mine) = guest(2, UUID_N, "Alex");
        hub.guests.push(newcomer);
        login_starts(&mut hub, UUID_N);
        place(&mut hub, 2);
        let _ = (ids(&seen), ids(&mine));
        (record, hub, seen, mine)
    }

    /// What the game does with a request (`game/dedicated/transfer.rs`), without the disk: the
    /// import, then the placing unless `"place": false`.
    fn import_player(
        hub: &mut Hub,
        uuid: u128,
        request: &serde_json::Value,
    ) -> Result<crate::lan::import::Imported, String> {
        let done = crate::lan::import::import_player(hub, uuid, request)?;
        if done.place {
            hub.confirm_arrival(uuid, done.handover)?;
        }
        Ok(done)
    }

    /// `player-confirm` as the game runs it, without the disk. A request of the unique id alone
    /// is given the held player's hand-over id, as a proxy would.
    fn confirm(hub: &mut Hub, args: &str) -> serde_json::Value {
        let mut args = args.to_owned();
        if let [one] = args.split_whitespace().collect::<Vec<_>>().as_slice()
            && let Some(id) = super::parse_uuid(one)
                .and_then(|u| hub.guests.iter().find(|g| g.uuid == u))
                .and_then(|g| g.arrival_handover)
        {
            args = format!("{one} {}", super::handover_text(id));
        }
        match super::confirm_args(hub, &args) {
            Ok((uuid, id)) => match hub.confirm_arrival(uuid, id) {
                Ok(announced) => {
                    serde_json::json!({ "ok": true, "placed": true, "announced": announced })
                }
                Err(e) => serde_json::json!({ "error": e }),
            },
            Err(e) => serde_json::json!({ "error": e }),
        }
    }

    #[test]
    fn a_whole_record_makes_the_held_player_the_player_it_was_exported_from() {
        let (record, mut hub, seen, mine) = held_in_a_second_hub();
        let (source, i) = exported_guest();
        let done =
            import_player(&mut hub, UUID_N, &serde_json::json!({ "record": record })).unwrap();
        assert_eq!(done.parts.len(), crate::lan::import::PARTS.len());
        assert!(done.announce);
        let g = &hub.guests[1];
        assert!(g.placed && !g.held && g.handover.is_none());
        // The whole player: the compound a save would write is the same one.
        assert_eq!(
            crate::lan::playerdata::player_tag(g),
            crate::lan::playerdata::player_tag(&source.guests[i])
        );
        assert_eq!(g.position, [10.5, 70.0, -3.25]);
        assert_eq!((g.xp_level, g.food, g.game_mode, g.selected), (7, 12, 1, 3));
        // The player already there is told it has come, as for a login.
        let told = ids(&seen);
        assert!(told.contains(&id::SYSTEM_CHAT), "{told:?}");
        assert!(told.contains(&id::PLAYER_INFO_UPDATE), "{told:?}");
        // And the player is told what the import changed.
        let sent = ids(&mine);
        for wanted in [
            id::GAME_EVENT,
            id::PLAYER_ABILITIES,
            id::SET_HELD_SLOT,
            id::PLAYER_POSITION,
        ] {
            assert!(
                sent.contains(&wanted),
                "packet {wanted} missing from {sent:?}"
            );
        }
    }

    #[test]
    fn a_part_that_is_not_asked_for_keeps_what_the_held_player_has() {
        let (record, mut hub, _seen, _mine) = held_in_a_second_hub();
        let before = (
            hub.guests[1].position,
            hub.guests[1].health,
            hub.guests[1].xp_level,
            hub.guests[1].game_mode,
        );
        let request =
            serde_json::json!({ "record": record, "parts": ["inventory", "ender_chest"] });
        let done = import_player(&mut hub, UUID_N, &request).unwrap();
        assert_eq!(done.parts, vec!["inventory", "ender_chest"]);
        let g = &hub.guests[1];
        // Only the inventory came; the position, health, experience and mode are the held player's own.
        assert_eq!((g.position, g.health, g.xp_level, g.game_mode), before);
        assert!(
            g.inventory[0].is_some(),
            "the stone came with the inventory"
        );
        assert_eq!(g.selected, 3);
        assert!(
            g.placed && !g.held,
            "a partial import still lets the player in"
        );
    }

    #[test]
    fn the_parts_come_in_a_fixed_order_whatever_order_they_were_asked_in() {
        let (record, mut hub, _seen, _mine) = held_in_a_second_hub();
        let request = serde_json::json!({ "record": record, "parts": ["position", "inventory", "inventory"] });
        let done = import_player(&mut hub, UUID_N, &request).unwrap();
        assert_eq!(done.parts, vec!["inventory", "position"]);
    }

    #[test]
    fn a_silent_import_tells_no_one() {
        let (record, mut hub, seen, _mine) = held_in_a_second_hub();
        let request = serde_json::json!({ "record": record, "announce": false });
        let done = import_player(&mut hub, UUID_N, &request).unwrap();
        assert!(!done.announce);
        assert!(hub.guests[1].placed);
        // No join line, but the list entry: the watcher will be sent the player's entity, and a
        // client adds a player's entity only with its list entry.
        let told = ids(&seen);
        assert!(!told.contains(&id::SYSTEM_CHAT), "{told:?}");
        assert!(told.contains(&id::PLAYER_INFO_UPDATE), "{told:?}");
    }

    /// Every refusal leaves the held player exactly as it was, and tells the world nothing.
    #[test]
    fn a_record_that_cannot_be_taken_changes_nothing() {
        let (record, mut hub, seen, mine) = held_in_a_second_hub();
        let mut cases: Vec<(&str, serde_json::Value)> = Vec::new();
        let with = |key: &str, value: serde_json::Value| {
            let mut r = record.clone();
            r[key] = value;
            serde_json::json!({ "record": r })
        };
        cases.push(("version", with("version", serde_json::json!(2))));
        cases.push(("no version", with("version", serde_json::Value::Null)));
        cases.push(("data version", with("data_version", serde_json::json!(1))));
        cases.push((
            "another player",
            with(
                "uuid",
                serde_json::json!("00000000-0000-0000-0000-000000000001"),
            ),
        ));
        cases.push(("not base64", with("player", serde_json::json!("%%%"))));
        cases.push(("not NBT", with("player", serde_json::json!("AAAA"))));
        cases.push(("no record", serde_json::json!({})));
        cases.push((
            "unknown part",
            serde_json::json!({ "record": record, "parts": ["luck"] }),
        ));
        cases.push((
            "parts not a list",
            serde_json::json!({ "record": record, "parts": "all" }),
        ));
        cases.push((
            "announce not a bool",
            serde_json::json!({ "record": record, "announce": "yes" }),
        ));
        let before = crate::lan::playerdata::player_tag(&hub.guests[1]);
        for (what, request) in &cases {
            let error = import_player(&mut hub, UUID_N, request).expect_err(what);
            assert!(!error.is_empty(), "{what}");
            let g = &hub.guests[1];
            assert!(g.held && !g.placed, "{what}: the player must still be held");
            assert_eq!(
                crate::lan::playerdata::player_tag(g),
                before,
                "{what}: nothing may change"
            );
        }
        assert_eq!(ids(&seen), Vec::<i32>::new());
        assert_eq!(ids(&mine), Vec::<i32>::new());
    }

    #[test]
    fn a_position_in_another_dimension_is_refused_but_the_other_parts_can_come() {
        let (record, mut hub, _seen, _mine) = held_in_a_second_hub();
        // Make the held player's world the Nether: the record is in the Overworld.
        hub.guests[1].dimension = "minecraft:the_nether".into();
        let all = serde_json::json!({ "record": record });
        let error = import_player(&mut hub, UUID_N, &all).unwrap_err();
        assert!(error.contains("position cannot be taken"), "{error}");
        assert!(hub.guests[1].held);
        let without = serde_json::json!({ "record": record, "parts": ["inventory", "experience"] });
        import_player(&mut hub, UUID_N, &without).unwrap();
        assert_eq!(
            hub.guests[1].dimension, "minecraft:the_nether",
            "the client's world did not change"
        );
        assert_eq!(hub.guests[1].xp_level, 7);
    }

    #[test]
    fn only_a_held_player_can_be_imported_into() {
        let (record, mut hub, _seen, _mine) = held_in_a_second_hub();
        let request = serde_json::json!({ "record": record });
        // A placed player is not a target.
        hub.guests[1].held = false;
        hub.guests[1].placed = true;
        let error = import_player(&mut hub, UUID_N, &request).unwrap_err();
        assert!(
            error.contains("no player with that unique id is held"),
            "{error}"
        );
        // Nor a player nobody holds.
        let mut empty = self::hub();
        let error = import_player(&mut empty, UUID_N, &request).unwrap_err();
        assert!(
            error.contains("no player with that unique id is held"),
            "{error}"
        );
    }

    // ---- player-import without placing, player-confirm, and the cancel ------

    fn import_without_placing(
        request_extra: serde_json::Value,
    ) -> (Hub, Receiver<Out>, Receiver<Out>) {
        let (record, mut hub, seen, mine) = held_in_a_second_hub();
        let mut request = serde_json::json!({ "record": record, "place": false });
        for (k, v) in request_extra.as_object().unwrap() {
            request[k] = v.clone();
        }
        let done = import_player(&mut hub, UUID_N, &request).unwrap();
        assert!(!done.place);
        (hub, seen, mine)
    }

    /// The import changes the player and tells its client, but the world still does not know it.
    #[test]
    fn an_import_that_does_not_place_leaves_the_player_held_and_tells_only_its_client() {
        let (hub, seen, mine) = import_without_placing(serde_json::json!({}));
        let g = &hub.guests[1];
        assert!(g.held && !g.placed && g.imported && g.arrival_announce);
        assert_eq!(
            (g.xp_level, g.food, g.game_mode, g.position),
            (7, 12, 1, [10.5, 70.0, -3.25])
        );
        assert_eq!(
            ids(&seen),
            Vec::<i32>::new(),
            "the watcher must hear nothing"
        );
        let sent = ids(&mine);
        for wanted in [
            id::GAME_EVENT,
            id::PLAYER_ABILITIES,
            id::SET_HELD_SLOT,
            id::PLAYER_POSITION,
        ] {
            assert!(
                sent.contains(&wanted),
                "packet {wanted} missing from {sent:?}"
            );
        }
    }

    #[test]
    fn confirming_places_the_player_and_announces_it_as_the_import_asked() {
        let (mut hub, seen, mine) = import_without_placing(serde_json::json!({}));
        let _ = ids(&mine);
        let reply = confirm(&mut hub, UUID);
        assert_eq!(
            reply,
            serde_json::json!({"ok": true, "placed": true, "announced": true})
        );
        let g = &hub.guests[1];
        assert!(g.placed && !g.held && !g.imported);
        let told = ids(&seen);
        assert!(
            told.contains(&id::SYSTEM_CHAT) && told.contains(&id::PLAYER_INFO_UPDATE),
            "{told:?}"
        );
        // What the client did since the import was not acted on: it is put back where the
        // server has it, the position to confirm again and the inventory.
        let sent = ids(&mine);
        assert!(sent.contains(&id::PLAYER_POSITION), "{sent:?}");
        assert!(sent.contains(&id::CONTAINER_SET_CONTENT), "{sent:?}");
    }

    /// The arrival's join line names the player as the server's own join line does: by its
    /// nickname and team (Player.getDisplayName), not only by its account name.
    #[test]
    fn the_join_line_of_an_arrival_shows_its_nickname() {
        let (mut hub, seen, _mine) = import_without_placing(serde_json::json!({}));
        hub.set_nickname(UUID_N, Some("Captain".into()));
        let _ = packets(&seen);
        let reply = confirm(&mut hub, UUID);
        assert_eq!(reply["placed"], true);
        let lines: Vec<Vec<u8>> = packets(&seen)
            .into_iter()
            .filter(|(id, _)| *id == id::SYSTEM_CHAT)
            .map(|(_, body)| body)
            .collect();
        assert_eq!(lines.len(), 1, "one join line");
        assert!(
            lines[0].windows(b"Captain".len()).any(|w| w == b"Captain"),
            "the join line names the nickname"
        );
    }

    #[test]
    fn a_silent_import_stays_silent_when_it_is_confirmed() {
        let (mut hub, seen, _mine) =
            import_without_placing(serde_json::json!({ "announce": false }));
        let reply = confirm(&mut hub, UUID);
        assert_eq!(
            reply,
            serde_json::json!({"ok": true, "placed": true, "announced": false})
        );
        assert!(hub.guests[1].placed);
        let told = ids(&seen);
        assert!(!told.contains(&id::SYSTEM_CHAT), "{told:?}");
        assert!(told.contains(&id::PLAYER_INFO_UPDATE), "{told:?}");
    }

    /// The cancel: a failure after the import, before anything reached the client, removes the
    /// player from this server with no file and no word to anyone.
    #[test]
    fn dropping_an_imported_but_unconfirmed_player_cancels_the_arrival_without_a_word() {
        let (mut hub, seen, mine) = import_without_placing(serde_json::json!({}));
        let _ = ids(&mine);
        let reply = super::drop_request(&mut hub, UUID);
        assert_eq!(
            (reply["held"].as_bool(), reply["expected"].as_bool()),
            (Some(true), Some(false))
        );
        assert!(matches!(mine.try_recv(), Ok(Out::Disconnect(_))));
        let gone = hub.remove_guest(1, "dropped");
        assert!(
            gone.held && !gone.placed && !gone.released,
            "never placed: the game saves nothing"
        );
        assert_eq!(
            ids(&seen),
            Vec::<i32>::new(),
            "the watcher must hear nothing"
        );
        // And it cannot be confirmed any more.
        let reply = confirm(&mut hub, UUID);
        assert!(reply["error"].is_string(), "{reply}");
    }

    #[test]
    fn a_confirmation_is_refused_for_a_player_that_is_not_imported_and_changes_nothing() {
        let (record, mut hub, seen, _mine) = held_in_a_second_hub();
        let _ = record;
        for args in ["", "nonsense", &format!("{UUID} now")] {
            assert!(confirm(&mut hub, args)["error"].is_string(), "{args:?}");
        }
        // Held, but nothing was imported.
        let reply = confirm(&mut hub, &format!("{UUID} {}", "0".repeat(32)));
        assert!(
            reply["error"]
                .as_str()
                .unwrap()
                .contains("not been imported"),
            "{reply}"
        );
        assert!(hub.guests[1].held && !hub.guests[1].placed);
        // Not held at all.
        let reply = confirm(
            &mut hub,
            &format!("00000000000000000000000000000009 {}", "0".repeat(32)),
        );
        assert!(
            reply["error"].as_str().unwrap().contains("no player"),
            "{reply}"
        );
        assert_eq!(ids(&seen), Vec::<i32>::new());
    }

    #[test]
    fn place_is_true_or_false() {
        let (record, mut hub, _seen, _mine) = held_in_a_second_hub();
        let request = serde_json::json!({ "record": record, "place": "later" });
        assert!(import_player(&mut hub, UUID_N, &request).is_err());
        assert!(hub.guests[1].held && !hub.guests[1].imported);
    }

    /// A proxy that dies between the import and the confirmation leaves nothing behind.
    #[test]
    fn an_imported_player_nobody_confirms_is_dropped_like_any_held_player() {
        let (mut hub, seen, mine) = import_without_placing(serde_json::json!({}));
        let _ = ids(&mine);
        let mut world = crate::world::World::new(horizon_world::registry::BlockStateId(0), 0);
        let border = crate::worldborder::WorldBorder::default();
        let mut next = 100;
        let mut ctx = tick_context(&mut world, &border, &mut next);
        hub.ticks = hub.guests[1].held_since + super::Settings::default().max_hold_ticks - 1;
        let events = hub.poll(&mut ctx);
        assert!(events.iter().any(|e| is_warn(e, "dropped")));
        assert!(matches!(mine.try_recv(), Ok(Out::Disconnect(_))));
        assert_eq!(ids(&seen), Vec::<i32>::new());
    }

    // ---- an import replaces the inventory, it does not mix with the target's own ----

    /// A player who has played on the target before has its own inventory in the file the
    /// target loaded when it logged in. The imported inventory must REPLACE it: a slot the
    /// record leaves empty is empty afterwards, not the target's old item.
    #[test]
    fn an_imported_inventory_replaces_the_one_the_target_loaded_for_the_player() {
        use crate::items::ItemStack;
        let stack = |name: &str, count: u32| ItemStack {
            item: std::sync::Arc::from(name),
            count,
            damage: 0,
            extra: None,
        };
        let (record, mut hub, _seen, _mine) = held_in_a_second_hub();
        // What the target's file gave the held player: items in slots the record leaves empty,
        // in the armour and the off hand, and an ender chest item.
        {
            let g = &mut hub.guests[1];
            g.inventory[5] = Some(stack("minecraft:dirt", 3));
            g.inventory[20] = Some(stack("minecraft:cobblestone", 9));
            g.armor[0] = Some(stack("minecraft:iron_boots", 1));
            g.offhand = Some(stack("minecraft:shield", 1));
            g.ender_items[0] = Some(stack("minecraft:diamond", 2));
        }
        import_player(
            &mut hub,
            UUID_N,
            &serde_json::json!({ "record": record, "place": false }),
        )
        .unwrap();
        let g = &hub.guests[1];
        // The exported player had only the stone in slot 0.
        assert_eq!(
            g.inventory[0].as_ref().map(|s| (&*s.item, s.count)),
            Some(("minecraft:stone", 5))
        );
        for slot in 1..36 {
            assert!(
                g.inventory[slot].is_none(),
                "slot {slot} still holds the target's old item"
            );
        }
        assert!(g.armor.iter().all(Option::is_none), "the old armour stayed");
        assert!(g.offhand.is_none(), "the old off hand item stayed");
        assert!(
            g.ender_items.iter().all(Option::is_none),
            "the old ender chest stayed"
        );
        // And what the client is told is that inventory: the window content has no dirt.
        let items = (0..46).filter(|&s| g.menu_slot(s).is_some()).count();
        assert_eq!(items, 1, "the inventory menu holds the stone only");
    }

    /// Parts that are not imported keep the target's own, items included.
    #[test]
    fn an_inventory_that_is_not_imported_keeps_the_targets_items() {
        use crate::items::ItemStack;
        let (record, mut hub, _seen, _mine) = held_in_a_second_hub();
        hub.guests[1].inventory[5] = Some(ItemStack {
            item: std::sync::Arc::from("minecraft:dirt"),
            count: 3,
            damage: 0,
            extra: None,
        });
        let request =
            serde_json::json!({ "record": record, "parts": ["experience"], "place": false });
        import_player(&mut hub, UUID_N, &request).unwrap();
        assert!(
            hub.guests[1].inventory[5].is_some(),
            "the target's dirt must stay"
        );
        assert!(
            hub.guests[1].inventory[0].is_none(),
            "the record's stone must not come"
        );
    }

    // ---- the review's points on the import ---------------------------------

    /// The source's guest with a body (air, fire, freezing, food exhaustion and timer, a fall
    /// and a motion), a key Horizon does not model and a vehicle, exported with `freeze`.
    fn record_with_a_body() -> serde_json::Value {
        use horizon_world::nbt::Tag;
        let (mut source, i) = exported_guest();
        let g = &mut source.guests[i];
        g.loaded_body = Some(crate::lan::guest::SavedBody {
            fire_ticks: 100,
            air: 40,
            fall_distance: 2.5,
            velocity: [0.25, 0.0, -0.125],
            food_exhaustion: 1.5,
            food_tick_timer: 33,
            ticks_frozen: 60,
        });
        let mut extra = Tag::Compound(Default::default());
        if let Tag::Compound(map) = &mut extra {
            map.insert("HorizonTest".into(), Tag::Int(7));
        }
        g.saved = Some(extra);
        super::export_record(&mut source, i, true, Default::default()).unwrap()
    }

    fn held_for(record: &serde_json::Value) -> (Hub, Receiver<Out>, Receiver<Out>) {
        let (_, hub, seen, mine) = held_in_a_second_hub();
        let _ = record;
        (hub, seen, mine)
    }

    /// A whole import carries the whole compound: the body's values and the keys Horizon does
    /// not model, which the next save writes again.
    #[test]
    fn a_whole_import_keeps_the_body_and_the_keys_horizon_does_not_model() {
        use crate::save::number;
        let record = record_with_a_body();
        let (mut hub, _seen, _mine) = held_for(&record);
        import_player(&mut hub, UUID_N, &serde_json::json!({ "record": record })).unwrap();
        let g = &hub.guests[1];
        let body = g.loaded_body.unwrap();
        assert_eq!(
            (body.air, body.fire_ticks, body.ticks_frozen),
            (40, 100, 60)
        );
        assert_eq!((body.food_exhaustion, body.food_tick_timer), (1.5, 33));
        assert_eq!(
            (body.fall_distance, body.velocity),
            (2.5, [0.25, 0.0, -0.125])
        );
        let tag = crate::lan::playerdata::player_tag(g);
        assert_eq!(number(tag.get("HorizonTest")), Some(7));
        assert_eq!(number(tag.get("Air")), Some(40));
        assert_eq!(number(tag.get("Fire")), Some(100));
    }

    /// The body's values follow their parts: the fall and motion the position, the air, fire,
    /// freezing and food exhaustion and timer the health. A part left out keeps the held
    /// player's own, and its saved vehicle.
    #[test]
    fn the_body_values_follow_their_parts() {
        let record = record_with_a_body();
        let (mut hub, _seen, _mine) = held_for(&record);
        let mine = crate::lan::guest::SavedBody {
            fire_ticks: -20,
            air: 300,
            fall_distance: 0.0,
            velocity: [0.0; 3],
            food_exhaustion: 0.0,
            food_tick_timer: 0,
            ticks_frozen: 0,
        };
        hub.guests[1].loaded_body = Some(mine);
        hub.guests[1].saved_vehicle = Some([1, 2, 3, 4]);
        let request = serde_json::json!({ "record": record, "parts": ["health"], "place": false });
        import_player(&mut hub, UUID_N, &request).unwrap();
        let body = hub.guests[1].loaded_body.unwrap();
        assert_eq!(
            (body.air, body.fire_ticks, body.ticks_frozen),
            (40, 100, 60)
        );
        assert_eq!((body.food_exhaustion, body.food_tick_timer), (1.5, 33));
        assert_eq!(
            (body.fall_distance, body.velocity),
            (0.0, [0.0; 3]),
            "not the position's"
        );
        assert_eq!(hub.guests[1].saved_vehicle, Some([1, 2, 3, 4]));
    }

    /// Values no player can have are refused before the held player is touched.
    #[test]
    fn a_record_with_impossible_values_is_refused() {
        let cases: [(&str, fn(&mut crate::lan::guest::Guest)); 7] = [
            ("dead", |g| g.health = 0.0),
            ("health not a number", |g| g.health = f32::NAN),
            ("out of the world", |g| g.position = [4.0e7, 64.0, 0.0]),
            ("height out of the world", |g| {
                g.position = [0.0, 3.0e7, 0.0]
            }),
            ("rotation not a number", |g| g.yaw = f32::INFINITY),
            ("health past max_health's 1024", |g| g.health = 2000.0),
            ("food exhaustion past 40", |g| {
                g.loaded_body = Some(crate::lan::guest::SavedBody {
                    fire_ticks: -20,
                    air: 300,
                    fall_distance: 0.0,
                    velocity: [0.0; 3],
                    food_exhaustion: 41.0,
                    food_tick_timer: 0,
                    ticks_frozen: 0,
                });
            }),
        ];
        for (what, spoil) in cases {
            let (mut source, i) = exported_guest();
            spoil(&mut source.guests[i]);
            let record = super::export_record(&mut source, i, true, Default::default()).unwrap();
            let (_, mut hub, seen, _mine) = held_in_a_second_hub();
            let before = crate::lan::playerdata::player_tag(&hub.guests[1]);
            let error = import_player(&mut hub, UUID_N, &serde_json::json!({ "record": record }))
                .expect_err(what);
            assert!(error.starts_with("the record"), "{what}: {error}");
            assert_eq!(
                crate::lan::playerdata::player_tag(&hub.guests[1]),
                before,
                "{what}"
            );
            assert!(hub.guests[1].held && !hub.guests[1].imported, "{what}");
            assert_eq!(ids(&seen), Vec::<i32>::new(), "{what}");
        }
    }

    /// The record must be a hand-over's, and the held player takes one hand-over's record.
    #[test]
    fn an_import_names_its_hand_over() {
        let (source_record, mut hub, _seen, _mine) = held_in_a_second_hub();
        // A read (an export without freeze) is not a hand-over.
        let (mut source, i) = exported_guest();
        let read = super::export_record(&mut source, i, false, Default::default()).unwrap();
        let error =
            import_player(&mut hub, UUID_N, &serde_json::json!({ "record": read })).unwrap_err();
        assert!(error.contains("no hand-over id"), "{error}");
        let request = serde_json::json!({ "record": source_record, "place": false });
        let done = import_player(&mut hub, UUID_N, &request).unwrap();
        assert_eq!(
            source_record["handover"].as_str(),
            Some(super::handover_text(done.handover).as_str())
        );
        assert_eq!(hub.guests[1].arrival_handover, Some(done.handover));
        // The same record again is taken (a retry); another hand-over's is not.
        import_player(&mut hub, UUID_N, &request).unwrap();
        let mut other = source_record.clone();
        other["handover"] = serde_json::json!("0".repeat(32));
        let error = import_player(
            &mut hub,
            UUID_N,
            &serde_json::json!({ "record": other, "place": false }),
        )
        .unwrap_err();
        assert!(error.contains("another hand-over"), "{error}");
        // A confirmation of another hand-over is refused, and changes nothing.
        let wrong = confirm(&mut hub, &format!("{UUID} {}", "0".repeat(32)));
        assert!(
            wrong["error"]
                .as_str()
                .unwrap()
                .contains("another hand-over"),
            "{wrong}"
        );
        assert!(hub.guests[1].held && hub.guests[1].imported);
    }

    /// `player-drop` takes the hand-over id, which must be the imported one; without it the
    /// drop goes by the unique id alone.
    #[test]
    fn a_drop_that_names_a_hand_over_must_name_the_imported_one() {
        let (mut hub, _seen, mine) = import_without_placing(serde_json::json!({}));
        let id = super::handover_text(hub.guests[1].arrival_handover.unwrap());
        let _ = ids(&mine);
        let reply = super::drop_request(&mut hub, &format!("{UUID} {}", "0".repeat(32)));
        assert!(reply["error"].is_string(), "{reply}");
        assert!(mine.try_recv().is_err(), "nothing was dropped");
        for bad in [format!("{UUID} soon"), format!("{UUID} {id} now")] {
            assert!(
                super::drop_request(&mut hub, &bad)["error"].is_string(),
                "{bad}"
            );
        }
        let reply = super::drop_request(&mut hub, &format!("{UUID} {id}"));
        assert_eq!(reply["held"], true, "{reply}");
        assert!(matches!(mine.try_recv(), Ok(Out::Disconnect(_))));
        // While its connection closes, neither a confirmation nor an import takes it.
        let confirmed = confirm(&mut hub, &format!("{UUID} {id}"));
        assert!(confirmed["error"].is_string(), "{confirmed}");
        assert!(!hub.guests[1].placed);
    }

    /// An arrival kicked or dropped, whose connection has not closed yet, is not placed.
    #[test]
    fn a_leaving_arrival_cannot_be_imported_or_confirmed() {
        let (record, mut hub, _seen, _mine) = held_in_a_second_hub();
        hub.guests[1].left_reason = "Timed out".into();
        let error =
            import_player(&mut hub, UUID_N, &serde_json::json!({ "record": record })).unwrap_err();
        assert!(error.contains("leaving"), "{error}");
        let (mut hub, _seen, _mine) = import_without_placing(serde_json::json!({}));
        let id = super::handover_text(hub.guests[1].arrival_handover.unwrap());
        hub.guests[1].left_reason = "Timed out".into();
        let reply = confirm(&mut hub, &format!("{UUID} {id}"));
        assert!(
            reply["error"].as_str().unwrap().contains("leaving"),
            "{reply}"
        );
        assert!(!hub.guests[1].placed);
    }

    /// The login sent the target's recipe book; the import sends the imported one.
    #[test]
    fn an_import_sends_the_recipe_book_again() {
        let (_hub, _seen, mine) = import_without_placing(serde_json::json!({}));
        let sent = ids(&mine);
        assert!(sent.contains(&id::RECIPE_BOOK_SETTINGS), "{sent:?}");
    }

    /// After the commit, a retry of the confirm (or of the import) whose reply was lost finds the
    /// player placed from that same hand-over, and the game answers it the same; another
    /// hand-over's id is still refused.
    #[test]
    fn a_retry_after_the_commit_finds_the_player_placed_from_that_hand_over() {
        let (mut hub, _seen, _mine) = import_without_placing(serde_json::json!({}));
        let id = hub.guests[1].arrival_handover.unwrap();
        let args = format!("{UUID} {}", super::handover_text(id));
        assert_eq!(confirm(&mut hub, &args)["placed"], true);
        assert_eq!(hub.placed_arrival(UUID_N, id), Some(1));
        assert_eq!(super::confirm_args(&hub, &args), Ok((UUID_N, id)));
        assert_eq!(hub.placed_arrival(UUID_N, id ^ 1), None);
        let other = format!("{UUID} {}", super::handover_text(id ^ 1));
        assert!(super::confirm_args(&hub, &other).is_err());
    }

    // ---- the view follows the imported position ----------------------------

    fn held_with_a_record_at(
        at: [f64; 3],
        center: Option<(i32, i32)>,
    ) -> (serde_json::Value, Hub, Receiver<Out>) {
        held_with_a_record_of(|g| g.position = at, center)
    }

    fn held_with_a_record_of(
        source_player: impl FnOnce(&mut Guest),
        center: Option<(i32, i32)>,
    ) -> (serde_json::Value, Hub, Receiver<Out>) {
        let (mut source, i) = exported_guest();
        source_player(&mut source.guests[i]);
        let record = super::export_record(&mut source, i, true, Default::default()).unwrap();
        let mut hub = hub();
        assert_eq!(
            super::expect_request(&mut hub, UUID, Instant::now())["ok"],
            true
        );
        let (newcomer, mine) = guest(2, UUID_N, "Alex");
        hub.guests.push(newcomer);
        login_starts(&mut hub, UUID_N);
        place(&mut hub, 2);
        hub.guests[0].center = center;
        let _ = ids(&mine);
        (record, hub, mine)
    }

    #[test]
    fn an_import_to_another_chunk_moves_the_view_at_once_and_says_so() {
        // Chunk (0, 0) before, chunk (6, -1) after: x 100.5 is block 100, chunk 6; z -3.25 is chunk -1.
        let (record, mut hub, mine) = held_with_a_record_at([100.5, 70.0, -3.25], Some((0, 0)));
        let request = serde_json::json!({ "record": record, "place": false });
        let done = import_player(&mut hub, UUID_N, &request).unwrap();
        assert!(done.recentered);
        assert_eq!(
            hub.guests[0].center,
            Some((6, -1)),
            "the view is centred on the new chunk"
        );
        let sent = packets(&mine);
        let centre = sent
            .iter()
            .position(|(i, _)| *i == id::SET_CHUNK_CACHE_CENTER)
            .expect("the new chunk centre is sent with the import");
        let position = sent
            .iter()
            .position(|(i, _)| *i == id::PLAYER_POSITION)
            .unwrap();
        assert!(
            centre < position,
            "the centre goes before the position the client confirms"
        );
        // The centre is the vanilla pair of VarInts: chunk x then chunk z (6, then -1 as an unsigned VarInt).
        let mut r = horizon_protocol::buf::Reader::new(&sent[centre].1);
        assert_eq!((r.varint().unwrap(), r.varint().unwrap()), (6, -1));
    }

    #[test]
    fn an_import_inside_the_same_chunk_does_not_recentre() {
        let (record, mut hub, mine) = held_with_a_record_at([3.5, 64.0, 3.5], Some((0, 0)));
        let request = serde_json::json!({ "record": record, "place": false });
        let done = import_player(&mut hub, UUID_N, &request).unwrap();
        assert!(!done.recentered);
        assert_eq!(hub.guests[0].center, Some((0, 0)));
        assert!(!ids(&mine).contains(&id::SET_CHUNK_CACHE_CENTER));
    }

    #[test]
    fn without_the_position_part_the_view_stays_where_it_is() {
        let (record, mut hub, mine) = held_with_a_record_at([100.5, 70.0, -3.25], Some((0, 0)));
        let request =
            serde_json::json!({ "record": record, "place": false, "parts": ["inventory"] });
        let done = import_player(&mut hub, UUID_N, &request).unwrap();
        assert!(!done.recentered);
        assert_eq!(hub.guests[0].center, Some((0, 0)));
        assert!(!ids(&mine).contains(&id::SET_CHUNK_CACHE_CENTER));
    }

    /// A held player that has not been sent any chunk has no view to move: the first tick builds it
    /// around the new place.
    #[test]
    fn a_view_not_yet_built_is_not_recentred() {
        let (record, mut hub, _mine) = held_with_a_record_at([100.5, 70.0, -3.25], None);
        let request = serde_json::json!({ "record": record, "place": false });
        assert!(
            !import_player(&mut hub, UUID_N, &request)
                .unwrap()
                .recentered
        );
    }

    /// A retried import of the same hand-over (its reply was lost) finds the view already moved,
    /// and still says the chunks around the new place are on their way.
    #[test]
    fn a_retried_import_still_reports_the_recentring() {
        let (record, mut hub, _mine) = held_with_a_record_at([100.5, 70.0, -3.25], Some((0, 0)));
        let request = serde_json::json!({ "record": record, "place": false });
        for _ in 0..2 {
            let done = import_player(&mut hub, UUID_N, &request).unwrap();
            assert!(done.recentered);
        }
    }

    /// A record whose vehicle is still to be restored puts the player back on it at the vehicle's
    /// own place, so a position the request names would be undone: it is refused, and the record's
    /// own place is still taken.
    #[test]
    fn a_named_position_is_refused_for_a_record_with_a_vehicle_to_restore() {
        let (record, mut hub, _mine) = held_with_a_record_of(
            |g| {
                g.position = [10.5, 70.0, -3.25];
                g.saved_vehicle = Some([1, 2, 3, 4]);
            },
            Some((0, 0)),
        );
        let named = serde_json::json!({
            "record": record.clone(), "place": false, "position": [300.5, 64.0, 300.5],
        });
        let error = import_player(&mut hub, UUID_N, &named).unwrap_err();
        assert!(error.contains("vehicle"), "{error}");
        assert!(!hub.guests[0].imported);
        let plain = serde_json::json!({ "record": record, "place": false });
        import_player(&mut hub, UUID_N, &plain).unwrap();
        assert_eq!(hub.guests[0].position, [10.5, 70.0, -3.25]);
        assert_eq!(hub.guests[0].saved_vehicle, Some([1, 2, 3, 4]));
    }

    #[test]
    fn placing_at_once_reports_the_recentring_too() {
        let (record, mut hub, mine) = held_with_a_record_at([100.5, 70.0, -3.25], Some((0, 0)));
        let done =
            import_player(&mut hub, UUID_N, &serde_json::json!({ "record": record })).unwrap();
        assert!(done.place && done.recentered);
        assert!(ids(&mine).contains(&id::SET_CHUNK_CACHE_CENTER));
    }

    // ---- where the player arrives: the request's own position, rotation, on-ground -----

    /// The position a PlayerPosition packet carries, with its look.
    fn position_sent(mine: &Receiver<Out>) -> ([f64; 3], f32, f32) {
        let (id_, payload) = packets(mine)
            .into_iter()
            .rev()
            .find(|(i, _)| *i == id::PLAYER_POSITION)
            .expect("the import sends a position");
        match horizon_protocol::packets::play::Clientbound::decode(id_, &payload).unwrap() {
            horizon_protocol::packets::play::Clientbound::PlayerPosition(p) => {
                (p.position, p.yaw, p.pitch)
            }
            other => panic!("not a position: {other:?}"),
        }
    }

    #[test]
    fn the_requests_position_wins_over_the_records_and_the_client_is_told_it() {
        let (record, mut hub, mine) = held_with_a_record_at([10.5, 70.0, -3.25], Some((0, 0)));
        let request = serde_json::json!({
            "record": record, "place": false,
            "position": [300.5, 64.0, 300.5], "rotation": [90.0, 20.0], "on_ground": false,
        });
        let done = import_player(&mut hub, UUID_N, &request).unwrap();
        let g = &hub.guests[0];
        assert_eq!(g.position, [300.5, 64.0, 300.5]);
        assert_eq!(g.previous_position, [300.5, 64.0, 300.5]);
        assert_eq!((g.yaw, g.pitch, g.head_yaw), (90.0, 20.0, 90.0));
        assert!(!g.on_ground);
        // The chunk is the one of the request's position, not the record's.
        assert!(done.recentered);
        assert_eq!(g.center, Some((18, 18)));
        let (at, yaw, pitch) = position_sent(&mine);
        assert_eq!((at, yaw, pitch), ([300.5, 64.0, 300.5], 90.0, 20.0));
    }

    #[test]
    fn without_the_requests_fields_the_records_place_is_taken() {
        let (record, mut hub, mine) = held_with_a_record_at([10.5, 70.0, -3.25], Some((0, 0)));
        import_player(
            &mut hub,
            UUID_N,
            &serde_json::json!({ "record": record, "place": false }),
        )
        .unwrap();
        assert_eq!(hub.guests[0].position, [10.5, 70.0, -3.25]);
        assert_eq!(position_sent(&mine).0, [10.5, 70.0, -3.25]);
    }

    #[test]
    fn a_pitch_is_taken_modulo_a_turn_then_cut_to_a_quarter_turn() {
        // Entity.setXRot: Math.clamp(pitch % 360, -90, 90); setYRot keeps the yaw as it is.
        for (asked, yaw, pitch) in [
            ([10.0, 120.0], 10.0, 90.0),
            ([10.0, -120.0], 10.0, -90.0),
            ([10.0, 370.0], 10.0, 10.0),
            ([-30.0, -350.0], -30.0, -90.0),
            ([725.0, 45.0], 725.0, 45.0),
        ] {
            let (record, mut hub, _mine) = held_with_a_record_at([10.5, 70.0, -3.25], None);
            let request =
                serde_json::json!({ "record": record, "place": false, "rotation": asked });
            import_player(&mut hub, UUID_N, &request).unwrap();
            assert_eq!(
                (hub.guests[0].yaw, hub.guests[0].pitch),
                (yaw, pitch),
                "{asked:?}"
            );
        }
    }

    /// Every refusal leaves the held player as it was and tells its client nothing.
    #[test]
    fn a_place_that_is_not_one_is_refused_and_nothing_changes() {
        let (record, mut hub, mine) = held_with_a_record_at([10.5, 70.0, -3.25], Some((0, 0)));
        let before = crate::lan::playerdata::player_tag(&hub.guests[0]);
        let place = hub.guests[0].position;
        let with = |extra: serde_json::Value| {
            let mut r = serde_json::json!({ "record": record.clone(), "place": false });
            for (k, v) in extra.as_object().unwrap() {
                r[k] = v.clone();
            }
            r
        };
        let cases = [
            (
                "beyond the world",
                with(serde_json::json!({ "position": [40_000_000.0, 64.0, 0.0] })),
            ),
            (
                "far below",
                with(serde_json::json!({ "position": [0.0, -40_000_000.0, 0.0] })),
            ),
            (
                "two numbers",
                with(serde_json::json!({ "position": [1.0, 2.0] })),
            ),
            (
                "a string",
                with(serde_json::json!({ "position": ["a", "b", "c"] })),
            ),
            ("not a list", with(serde_json::json!({ "position": 5 }))),
            (
                "rotation of one",
                with(serde_json::json!({ "rotation": [1.0] })),
            ),
            (
                "a rotation past what a float holds",
                with(serde_json::json!({ "rotation": [1e39, 1e39] })),
            ),
            (
                "a velocity that is not three numbers",
                with(serde_json::json!({ "velocity": [0.25, "up", 0.0] })),
            ),
            (
                "on_ground a number",
                with(serde_json::json!({ "on_ground": 1 })),
            ),
            (
                "the position part left out",
                with(serde_json::json!({ "position": [1.0, 64.0, 1.0], "parts": ["inventory"] })),
            ),
        ];
        for (what, request) in &cases {
            let error = import_player(&mut hub, UUID_N, request).expect_err(what);
            assert!(!error.is_empty(), "{what}");
            let g = &hub.guests[0];
            assert!(
                g.held && !g.imported,
                "{what}: the player must still be held and not imported"
            );
            assert_eq!(g.position, place, "{what}");
            assert_eq!(
                crate::lan::playerdata::player_tag(g),
                before,
                "{what}: nothing may change"
            );
        }
        assert_eq!(
            ids(&mine),
            Vec::<i32>::new(),
            "a refusal tells the client nothing"
        );
    }

    #[test]
    fn a_position_is_finite_and_inside_the_worlds_limit() {
        use crate::lan::import::check_position_for_tests as check;
        assert!(check([0.0, 64.0, 0.0]));
        // Entity.load's clamp: 3.0000512E7 on x and z, 2.0E7 on y.
        assert!(check([-30_000_512.0, -64.0, 30_000_512.0]));
        assert!(check([0.0, -20_000_000.0, 0.0]));
        assert!(!check([30_000_512.5, 64.0, 0.0]));
        assert!(!check([0.0, 20_000_000.5, 0.0]));
        assert!(!check([f64::NAN, 64.0, 0.0]));
        assert!(!check([0.0, f64::INFINITY, 0.0]));
        assert!(!check([0.0, 64.0, f64::NEG_INFINITY]));
    }

    /// The request's velocity is the body's from now on, a component over 10 blocks a tick
    /// taken as 0 as `Entity.load` takes a saved `Motion`; it belongs to the position part.
    #[test]
    fn the_requests_velocity_is_taken_with_the_position() {
        let (record, mut hub, _mine) = held_with_a_record_at([10.5, 70.0, -3.25], None);
        let request = serde_json::json!({
            "record": record,
            "place": false,
            "velocity": [0.25, -0.0784, 12.0],
        });
        import_player(&mut hub, UUID_N, &request).unwrap();
        let body = hub.guests[0].loaded_body.unwrap();
        assert_eq!(body.velocity, [0.25, -0.0784, 0.0]);
        let alone = serde_json::json!({
            "record": record,
            "parts": ["inventory"],
            "velocity": [0.0, 0.0, 0.0],
        });
        let error = import_player(&mut hub, UUID_N, &alone).unwrap_err();
        assert!(error.contains("belong to the position part"), "{error}");
    }

    // ---- the lines of the log ------------------------------------------------

    use super::{dropped_line, held_line, placed_line, request_log};
    use serde_json::json;

    #[test]
    fn a_refused_request_is_a_warning_with_the_reason() {
        let reply = json!({ "error": "that player is riding" });
        let (level, line) = request_log(
            "player-export",
            Some("Alex"),
            &format!("{UUID} freeze"),
            &reply,
        );
        assert_eq!(level, "WARN");
        assert_eq!(
            line,
            "player-export Alex (123e4567-e89b-12d3-a456-426614174000): refused: that player is riding"
        );
    }

    #[test]
    fn an_accepted_request_says_what_it_did_and_never_writes_the_record() {
        let id = UUID.replace('-', "");
        let (level, line) = request_log(
            "player-expect",
            None,
            &format!("{id} 30"),
            &json!({"ok": true, "renewed": true, "ttl_seconds": 30}),
        );
        assert_eq!(level, "INFO");
        assert_eq!(
            line,
            "player-expect 123e4567-e89b-12d3-a456-426614174000: ok, expected for 30 s (renewed)"
        );
        // 8 base64 characters are 6 bytes.
        let reply = json!({"ok": true, "record": {"player": "AAAAAAAA", "name": "Alex", "handover": "0123456789abcdef0123456789abcdef"}});
        let (_, line) = request_log(
            "player-export",
            Some("Alex"),
            &format!("{UUID} freeze"),
            &reply,
        );
        assert!(
            line.ends_with(
                "ok, record of 6 bytes, frozen, hand-over 0123456789abcdef0123456789abcdef"
            ),
            "{line}"
        );
        // An export without freeze is a read.
        let read = json!({"ok": true, "record": {"player": "AAAAAAAA", "handover": null}});
        let (_, line) = request_log("player-export", None, UUID, &read);
        assert!(line.ends_with("ok, record of 6 bytes, a read"), "{line}");
        assert!(
            !line.contains("AAAA"),
            "the record itself must not be written: {line}"
        );
        let reply = json!({"ok": true, "handover": "0123456789abcdef0123456789abcdef", "parts": ["inventory", "experience"], "placed": false, "announced": true, "recentered": true});
        let (_, line) = request_log(
            "player-import",
            Some("Alex"),
            &format!("{UUID} {{\"record\":{{}}}}"),
            &reply,
        );
        assert!(
            line.ends_with(
                "ok, parts inventory,experience, placed false, announced true, recentered true, hand-over 0123456789abcdef0123456789abcdef"
            ),
            "{line}"
        );
        assert!(
            !line.contains("record"),
            "the request body must not be written: {line}"
        );
        let (_, line) = request_log(
            "player-release",
            Some("Alex"),
            &format!("{UUID} 0123456789abcdef0123456789abcdef moved"),
            &json!({"ok": true, "released": "moved", "announced": false}),
        );
        assert!(
            line.ends_with(
                "ok, moved, announced false, hand-over 0123456789abcdef0123456789abcdef"
            ),
            "{line}"
        );
        let (_, line) = request_log(
            "player-drop",
            None,
            UUID,
            &json!({"ok": true, "held": true, "expected": false}),
        );
        assert!(line.ends_with("ok, held true, expected false"), "{line}");
        let (_, line) = request_log(
            "player-drop",
            None,
            &format!("{UUID} 0123456789abcdef0123456789abcdef"),
            &json!({"ok": true, "held": true, "expected": false}),
        );
        assert!(
            line.ends_with(
                "ok, held true, expected false, hand-over 0123456789abcdef0123456789abcdef"
            ),
            "{line}"
        );
        let (_, line) = request_log(
            "player-confirm",
            None,
            &format!("{UUID} 0123456789abcdef0123456789abcdef"),
            &json!({"ok": true, "placed": true, "announced": true}),
        );
        assert!(
            line.ends_with(
                "ok, placed, announced true, hand-over 0123456789abcdef0123456789abcdef"
            ),
            "{line}"
        );
    }

    #[test]
    fn a_request_without_a_readable_id_is_logged_as_typed() {
        let (level, line) = request_log(
            "player-drop",
            None,
            "nonsense",
            &json!({"error": "player-drop needs the player's unique id"}),
        );
        assert_eq!(level, "WARN");
        assert!(
            line.starts_with("player-drop \"nonsense\": refused"),
            "{line}"
        );
    }

    #[test]
    fn the_lines_of_a_held_player_never_say_it_joined_or_left() {
        assert_eq!(
            held_line("Alex"),
            "Alex is held: it arrives from another server"
        );
        assert_eq!(
            placed_line("Alex"),
            "Alex placed: it has arrived from another server"
        );
        let dropped = dropped_line("Alex", "dropped before its hand-over");
        assert_eq!(
            dropped,
            "Alex was dropped while held: dropped before its hand-over"
        );
        for line in [held_line("Alex"), dropped] {
            assert!(
                !line.contains("joined the game") && !line.contains("left the game"),
                "{line}"
            );
        }
    }

    // ---- the view follows the imported position ------------------------------
}
