//! Players that are about to arrive from another server, and a connected
//! player read out of this one and sent away: the control endpoint's
//! `player-expect`, `player-export` and `player-release`
//! (`game/dedicated/transfer.rs` runs them).
//!
//! A proxy that moves a player between two servers first tells the target
//! server who is coming (`player-expect <uuid>`), and reads the player out of
//! the source (`player-export <uuid> [freeze]`). Nothing here may last without
//! bound, because a proxy that dies in the middle of a move leaves its
//! expectations and its freezes behind: every expectation expires, the table
//! has a size cap, and a freeze is let go by itself.
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
/// expect_seconds=10,max_expect_seconds=60,max_expected=64,max_freeze_seconds=30`.
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
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            expect: Duration::from_secs(10),
            max_expect: Duration::from_secs(60),
            max_expected: 64,
            max_freeze_ticks: 20 * 30,
        }
    }
}

impl Settings {
    /// Reads `--transfers`' value: `key=value` pairs separated by commas, each a
    /// whole number above zero.
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
                .filter(|n| *n > 0)
                .ok_or_else(|| format!("{key} must be a whole number above zero"))?;
            match key {
                "expect_seconds" => s.expect = Duration::from_secs(n),
                "max_expect_seconds" => s.max_expect = Duration::from_secs(n),
                "max_expected" => {
                    s.max_expected = usize::try_from(n).map_err(|_| "max_expected is too large")?
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
}

impl Transfers {
    pub fn new(settings: Settings) -> Self {
        let expected = Arc::new(Mutex::new(Expectations::new(
            settings.max_expected,
            settings.max_expect,
        )));
        Self { settings, expected }
    }

    /// The table, locked. A thread that panicked while holding it left it
    /// whole: every change is one map operation.
    pub fn table(&self) -> std::sync::MutexGuard<'_, Expectations> {
        self.expected.lock().unwrap_or_else(|e| e.into_inner())
    }
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

/// Whether a player frozen at tick `since` has been frozen too long at tick
/// `now`, for a freeze of at most `max` ticks.
pub fn frozen_too_long(since: u64, now: u64, max: u64) -> bool {
    now.saturating_sub(since) >= max
}

/// Why the connected player `uuid` cannot be exported now, or the index of its
/// guest.
pub fn check_exportable(hub: &Hub, uuid: u128) -> Result<usize, &'static str> {
    let Some(i) = hub.guests.iter().position(|g| g.uuid == uuid) else {
        return Err("that player is not connected to this server");
    };
    let g = &hub.guests[i];
    if !g.placed {
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

/// `player-expect <uuid> [seconds]`: the player will log in within that time
/// (`[transfer]`'s `expect_seconds` by default, `max_expect_seconds` at most).
/// The reply says whether an earlier expectation was renewed. The login uses
/// the expectation up when it starts; one that no login used is reported when
/// it runs out.
pub fn expect_request(hub: &mut Hub, args: &str, now: Instant) -> serde_json::Value {
    let Some(transfers) = &hub.transfers else {
        return error(OFF);
    };
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
    let mut table = transfers.table();
    match table.expect(uuid, ttl, now) {
        Ok(renewed) => serde_json::json!({
            "ok": true,
            "renewed": renewed,
            "ttl_seconds": ttl.min(table.max_ttl()).as_secs(),
        }),
        Err(ExpectError::Full) => error("too many players are expected at once"),
    }
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
        ] {
            assert!(Settings::parse(bad).is_err(), "{bad:?} was taken");
        }
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
        let config = NetConfig {
            announce: false,
            ..NetConfig::loopback(0, "ArrivalsTest".into(), 4)
        };
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

    fn place(hub: &mut Hub, conn: u32) {
        let mut world = crate::world::World::new(horizon_world::registry::BlockStateId(0), 0);
        let border = crate::worldborder::WorldBorder::default();
        let mut next = 100;
        let mut ctx = TickCtx {
            world: &mut world,
            entities: &[],
            next_entity: &mut next,
            spawn: [0.5, 64.0, 0.5],
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

    /// A login takes the expectation from the table the network threads share
    /// with the hub (`lan/conn.rs`, as the login starts), so nothing is reported
    /// when its time would have run out; another player's stays.
    #[test]
    fn a_login_uses_up_the_expectation_of_that_player_only() {
        let mut hub = hub();
        let now = Instant::now();
        table(&hub).expect(2, Duration::from_secs(10), now).unwrap();
        table(&hub).expect(3, Duration::from_secs(10), now).unwrap();
        let shared = hub.transfers.as_ref().unwrap().expected.clone();
        assert!(shared.lock().unwrap().take(2, now));
        let (mut newcomer, _mine) = guest(2, 2, "Newcomer");
        newcomer.expected = true;
        hub.guests.push(newcomer);
        place(&mut hub, 2);
        assert!(hub.guests[0].placed);
        assert_eq!(table(&hub).len(), 1, "the other player is still expected");
        assert!(table(&hub).take(3, now));
    }

    /// While `[transfer]` is off every request is refused, there is no table,
    /// and a tick does nothing for transfers.
    #[test]
    fn with_transfers_off_every_request_is_refused() {
        let mut hub = hub();
        hub.transfers = None;
        let reply = super::expect_request(&mut hub, UUID, Instant::now());
        assert_eq!(reply["error"], super::OFF);
        let mut world = crate::world::World::new(horizon_world::registry::BlockStateId(0), 0);
        let border = crate::worldborder::WorldBorder::default();
        let mut next = 100;
        let mut ctx = tick_context(&mut world, &border, &mut next);
        assert!(hub.poll(&mut ctx).is_empty());
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
}
