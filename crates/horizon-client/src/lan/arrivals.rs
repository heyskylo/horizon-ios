//! Players that are about to arrive from another server, and a connected
//! player read out of this one: the control endpoint's `player-expect` and
//! `player-export` (`game/dedicated/transfer.rs` runs them).
//!
//! A proxy that moves a player between two servers first tells the target
//! server who is coming (`player-expect <uuid>`), and reads the player out of
//! the source (`player-export <uuid> [freeze]`). Nothing here may last without
//! bound, because a proxy that dies in the middle of a move leaves its
//! expectations and its freezes behind: every expectation expires, the table
//! has a size cap, and a freeze is let go by itself.
use super::hub::Hub;
use std::collections::HashMap;
use std::time::{Duration, Instant};

/// How long an expectation lives when the request names no time.
pub const DEFAULT_TTL: Duration = Duration::from_secs(10);
/// The longest an expectation may be asked to live.
pub const MAX_TTL: Duration = Duration::from_secs(60);
/// The most expectations the table keeps at once.
pub const MAX_EXPECTED: usize = 64;

/// Why an expectation could not be recorded.
#[derive(Debug, PartialEq, Eq)]
pub enum ExpectError {
    /// The table is full of live expectations.
    Full,
}

/// The players a server was told to expect, by unique id.
#[derive(Debug, Default)]
pub struct Expectations {
    until: HashMap<u128, Instant>,
}

impl Expectations {
    /// Records that `uuid` will log in within `ttl` (cut to [`MAX_TTL`]). A second
    /// request for the same player replaces the first one's deadline. Returns
    /// whether the player was already expected.
    pub fn expect(&mut self, uuid: u128, ttl: Duration, now: Instant) -> Result<bool, ExpectError> {
        self.expire(now);
        if !self.until.contains_key(&uuid) && self.until.len() >= MAX_EXPECTED {
            return Err(ExpectError::Full);
        }
        let deadline = now + ttl.min(MAX_TTL);
        Ok(self.until.insert(uuid, deadline).is_some())
    }

    /// Consumes the expectation of `uuid`, and says whether it was still alive:
    /// one login takes it, and a second login of the same player finds none.
    pub fn take(&mut self, uuid: u128, now: Instant) -> bool {
        match self.until.remove(&uuid) {
            Some(deadline) => now < deadline,
            None => false,
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

    /// How many expectations are recorded (some may be past their time but not
    /// yet swept).
    pub fn len(&self) -> usize {
        self.until.len()
    }

    pub fn is_empty(&self) -> bool {
        self.until.is_empty()
    }
}

/// Server ticks a player may stay frozen for a hand-over (thirty seconds): a proxy
/// that dies in the middle of one must not leave the player stuck.
pub const FROZEN_MAX_TICKS: u64 = 20 * 30;
/// The version of the record `player-export` answers with.
pub const RECORD_VERSION: i64 = 1;

/// Whether a player frozen at tick `since` has been frozen too long at tick `now`.
pub fn frozen_too_long(since: u64, now: u64) -> bool {
    now.saturating_sub(since) >= FROZEN_MAX_TICKS
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

/// The record of the player at guest index `i` (which [`check_exportable`] gave,
/// with its menus closed): the player compound as the server saves it, as the
/// uncompressed NBT of the compound in base64, and the few values a proxy reads
/// without opening it. `advancements` is the JSON the server keeps for the
/// player, or null. With `freeze`, the player's own inputs stop being acted on
/// until the freeze is let go.
pub fn export_record(
    hub: &mut Hub,
    i: usize,
    freeze: bool,
    advancements: serde_json::Value,
) -> Result<serde_json::Value, String> {
    use horizon_world::nbt::{Limits, NamedTag, write_named};
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
        "uuid": horizon_protocol::crypto::uuid_string(g.uuid),
        "name": g.name,
        "dimension": g.dimension,
        "position": g.position,
        "rotation": [g.yaw, g.pitch],
        "velocity": velocity,
        "game_mode": g.game_mode,
        "player": super::base64(&bytes),
        "advancements": advancements,
    });
    if freeze {
        let since = hub.ticks;
        hub.guests[i].frozen_since = Some(since);
    }
    Ok(record)
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

/// `player-expect <uuid> [seconds]`: the player will log in within that time
/// (10 seconds by default, 60 at most). The reply says whether an earlier
/// expectation was renewed. The login uses the expectation up; one that no
/// login used is reported when it runs out.
pub fn expect_request(hub: &mut Hub, args: &str, now: Instant) -> serde_json::Value {
    let mut parts = args.split_whitespace();
    let Some(uuid) = parts.next().and_then(parse_uuid) else {
        return error("player-expect needs the player's unique id");
    };
    let ttl = match parts.next() {
        None => DEFAULT_TTL,
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
    match hub.arrivals.expect(uuid, ttl, now) {
        Ok(renewed) => serde_json::json!({
            "ok": true,
            "renewed": renewed,
            "ttl_seconds": ttl.min(MAX_TTL).as_secs(),
        }),
        Err(ExpectError::Full) => error("too many players are expected at once"),
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
        assert!(!e.take(A, t0 + MAX_TTL));
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
        for n in 0..MAX_EXPECTED as u128 {
            e.expect(100 + n, secs(10), t0).unwrap();
        }
        assert_eq!(e.expect(A, secs(10), t0), Err(ExpectError::Full));
        // A player already in the table can still be renewed when it is full.
        assert_eq!(e.expect(100, secs(10), t0), Ok(true));
        // Once the old ones are past their time, there is room again.
        assert_eq!(e.expect(A, secs(10), t0 + secs(11)), Ok(false));
        assert_eq!(e.len(), 1);
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
    use std::sync::mpsc::{Receiver, channel};
    use std::time::{Duration, Instant};

    fn hub() -> Hub {
        let config = NetConfig {
            announce: false,
            ..NetConfig::lan(0, "ArrivalsTest".into(), 4)
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
        Hub::new(lan, sync, encoder, config)
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
            },
            conn as i32,
            [0.5, 64.0, 0.5],
            4,
        );
        g.dimension = "minecraft:overworld".into();
        (g, rx)
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
        hub.arrivals
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
        assert!(hub.arrivals.is_empty());
    }

    /// The login of an expected player uses the expectation up, so nothing is
    /// reported when its time would have run out; another player's stays.
    #[test]
    fn a_login_uses_up_the_expectation_of_that_player_only() {
        let mut hub = hub();
        let (newcomer, _mine) = guest(2, 2, "Newcomer");
        hub.guests.push(newcomer);
        let now = Instant::now();
        hub.arrivals
            .expect(2, Duration::from_secs(10), now)
            .unwrap();
        hub.arrivals
            .expect(3, Duration::from_secs(10), now)
            .unwrap();
        place(&mut hub, 2);
        assert!(hub.guests[0].placed);
        assert_eq!(hub.arrivals.len(), 1, "the other player is still expected");
        assert!(hub.arrivals.take(3, now));
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
        assert_eq!(hub.arrivals.len(), 1);
        assert!(hub.arrivals.take(UUID_N, now));
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
        assert!(hub.arrivals.is_empty());
    }

    #[test]
    fn player_expect_refuses_when_the_table_is_full() {
        let mut hub = hub();
        let now = Instant::now();
        for n in 0..super::MAX_EXPECTED as u128 {
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
        let record =
            super::export_record(&mut hub, i, false, serde_json::json!({"done": []})).unwrap();
        assert_eq!(record["version"], 1);
        assert_eq!(record["data_version"], horizon_world::DATA_VERSION);
        assert_eq!(record["uuid"], UUID);
        assert_eq!(record["name"], "Alex");
        assert_eq!(record["dimension"], "minecraft:overworld");
        assert_eq!(record["position"], serde_json::json!([10.5, 70.0, -3.25]));
        assert_eq!(record["rotation"], serde_json::json!([90.0, 15.0]));
        assert_eq!(record["game_mode"], 1);
        assert_eq!(record["advancements"], serde_json::json!({"done": []}));
        assert_eq!(hub.guests[i].frozen_since, None, "no freeze was asked for");

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
        super::export_record(&mut hub, i, true, serde_json::Value::Null).unwrap();
        assert_eq!(hub.guests[i].frozen_since, Some(hub.ticks));
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
        hub.ticks = hub.guests[i].frozen_since.unwrap() + super::FROZEN_MAX_TICKS - 2;
        assert!(!hub.poll(&mut ctx).iter().any(|e| is_warn(e, "frozen")));
        assert!(hub.guests[i].frozen_since.is_some());
        assert!(hub.poll(&mut ctx).iter().any(|e| is_warn(e, "frozen")));
        assert_eq!(hub.guests[i].frozen_since, None);
    }

    #[test]
    fn the_freeze_lasts_thirty_seconds_of_ticks() {
        assert!(!super::frozen_too_long(10, 10 + 599));
        assert!(super::frozen_too_long(10, 10 + 600));
        assert!(!super::frozen_too_long(500, 100));
    }
}
