//! A protocol bot joins a hosted world the way a vanilla client does:
//! handshake, status, offline login, configuration (registries and tags),
//! then the play state (login, chunks with light, player list, entities,
//! block changes). Needs the prepared resources; skipped without them.
extern crate horizon_client_lib as horizon_client;
use horizon_client::{
    chunk::{Chunk, MIN_Y},
    lan::{
        Lan, NetConfig, OwnerProfile,
        chunks::ChunkEncoder,
        data::SyncData,
        entity::EntityState,
        hub::{HostEvent, Hub, HubConfig, TickCtx},
    },
    world::World,
};
use horizon_protocol::{
    PROTOCOL_VERSION,
    adapter::ServerVersion,
    connection::Connection,
    crypto::offline_uuid,
    packets::{
        Empty,
        configuration::{self as cfg, ClientInformation},
        handshake, login,
        play::{self, Clientbound as C, Serverbound as S},
        status,
    },
};
use horizon_world::registry::BlockStateId;
use std::{
    path::Path,
    sync::Arc,
    thread,
    time::{Duration, Instant},
};

fn resources() -> Option<std::path::PathBuf> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../.local/horizon-client")
        .join(horizon_core::VANILLA_VERSION);
    root.join("items.json").exists().then_some(root)
}

const STONE: u32 = 1;

fn flat_world() -> World {
    let mut world = World::new(BlockStateId(0), 0);
    for cz in -4..=4 {
        for cx in -4..=4 {
            let mut chunk = Chunk::empty(cx, cz);
            for z in 0..16 {
                for x in 0..16 {
                    chunk.set(x, MIN_Y + 4, z, STONE);
                }
            }
            chunk.lit = true;
            world.chunks.insert((cx, cz), Arc::new(chunk));
        }
    }
    world
}

struct Bot {
    c: Connection,
    name: String,
}
impl Bot {
    fn recv_play(&mut self) -> Result<C, String> {
        let (id, payload) = self.c.recv().map_err(|e| e.to_string())?;
        match C::decode(id, &payload) {
            Ok(p) => Ok(p),
            // The command tree has argument nodes the client side does not decode.
            Err(e) if id == horizon_protocol::ids::play::clientbound::COMMANDS => {
                let _ = e;
                Ok(C::Unknown(id, payload))
            }
            Err(e) => Err(format!("packet {id}: {e}")),
        }
    }
    fn send(&mut self, p: S) {
        self.c.send(&p.encode()).unwrap();
    }
}

fn connect(port: u16, name: &str, known_pack: bool) -> Result<Bot, String> {
    let e = |e: horizon_protocol::Error| e.to_string();
    let mut c = Connection::connect("127.0.0.1", port, Duration::from_secs(5)).map_err(e)?;
    c.reader
        .set_read_timeout(Some(Duration::from_secs(20)))
        .map_err(e)?;
    c.send(
        &handshake::Serverbound::Intention(handshake::Intention {
            protocol: PROTOCOL_VERSION,
            host: "127.0.0.1".into(),
            port,
            intent: handshake::LOGIN,
        })
        .encode(),
    )
    .map_err(e)?;
    c.send(
        &login::Serverbound::Hello(login::ServerHello {
            name: name.into(),
            uuid: offline_uuid(name),
        })
        .encode(),
    )
    .map_err(e)?;
    loop {
        let (id, payload) = c.recv().map_err(e)?;
        match login::Clientbound::decode(id, &payload).map_err(e)? {
            login::Clientbound::Compression(p) => c.set_compression(p.threshold),
            login::Clientbound::LoginFinished(p) => {
                assert_eq!(p.profile.name, name);
                assert_eq!(p.profile.uuid, offline_uuid(name));
                c.send(&login::Serverbound::LoginAcknowledged(Empty).encode())
                    .map_err(e)?;
                break;
            }
            login::Clientbound::Disconnect(d) => return Err(d.reason_json),
            other => return Err(format!("unexpected login packet {other:?}")),
        }
    }
    c.send(&cfg::Serverbound::ClientInformation(ClientInformation::new(4)).encode())
        .map_err(e)?;
    let (mut registries, mut tags, mut features) = (0, 0, 0);
    loop {
        let (id, payload) = c.recv().map_err(e)?;
        match cfg::Clientbound::decode(id, &payload).map_err(e)? {
            cfg::Clientbound::SelectKnownPacks(p) => {
                assert_eq!(p.packs.len(), 1);
                assert_eq!(p.packs[0].id, "core");
                let reply = if known_pack { p.packs } else { Vec::new() };
                c.send(
                    &cfg::Serverbound::SelectKnownPacks(cfg::SelectKnownPacks { packs: reply })
                        .encode(),
                )
                .map_err(e)?;
            }
            cfg::Clientbound::RegistryData(r) => {
                registries += 1;
                // A client that knows the core pack rebuilds the entries itself
                // (RegistrySynchronization.packRegistry), dialogs included.
                let has_data = r.entries.iter().any(|e| e.data.is_some());
                assert_eq!(has_data, !known_pack, "{}", r.registry);
            }
            cfg::Clientbound::UpdateTags(t) => tags += t.registries.len(),
            cfg::Clientbound::UpdateEnabledFeatures(_) => features += 1,
            cfg::Clientbound::FinishConfiguration(_) => {
                c.send(&cfg::Serverbound::FinishConfiguration(Empty).encode())
                    .map_err(e)?;
                break;
            }
            _ => {}
        }
    }
    assert!(registries >= 28, "registries: {registries}");
    assert!(tags >= 5, "tag registries: {tags}");
    assert_eq!(features, 1);
    Ok(Bot {
        c,
        name: name.into(),
    })
}

/// Drive the hub like the game loop does until `done` is true.
struct Server {
    hub: Hub,
    world: World,
    next_entity: u32,
    placed: Vec<(u32, [i32; 3], u32, i32)>,
    /// Every packet the hub handed to the game, in order.
    packets: Vec<S>,
}
impl Server {
    fn tick(&mut self, entities: &[EntityState]) {
        let mut ctx = TickCtx {
            world: &mut self.world,
            entities,
            next_entity: &mut self.next_entity,
            spawn: [8.5, f64::from(MIN_Y + 5), 8.5],
            spawn_angle: [0.0; 2],
            difficulty: 2,
            difficulty_locked: false,
            clock_rate: 1.0,
            tick_rate: 20.0,
            tick_frozen: false,
            tick_steps: Default::default(),
            collision: None,
            border: &horizon_client::worldborder::WorldBorder::default(),
            streaming: None,
        };
        let events = self.hub.tick(&mut ctx);
        for event in events {
            match event {
                HostEvent::Joined(conn) => self.hub.place_new_player(conn, &mut ctx),
                HostEvent::Packet(conn, packet) => {
                    self.packets.push((*packet).clone());
                    if let S::UseItemOn(p) = *packet {
                        let at = [p.pos[0], p.pos[1] + 1, p.pos[2]];
                        ctx.world
                            .set(at[0], at[1], at[2], BlockStateId(STONE))
                            .unwrap();
                        self.placed.push((conn, at, STONE, p.sequence));
                        self.hub.ack(conn, p.sequence);
                    }
                }
                _ => {}
            }
        }
    }
}

fn server(resources: &Path) -> Server {
    server_with(resources, NetConfig::loopback(0, "Test - World".into(), 8))
}

fn server_with(resources: &Path, net: NetConfig) -> Server {
    server_sync(
        Arc::new(
            SyncData::load(resources, &horizon_world::loot::PackOverlay::NONE).expect("sync data"),
        ),
        net,
    )
}

/// A server over already-built sync data (a test's packs layered in).
fn server_sync(sync: Arc<SyncData>, net: NetConfig) -> Server {
    let lan = Lan::start_with(sync.clone(), net).unwrap();
    let biomes: Vec<String> = sync
        .dynamic
        .registries
        .get("minecraft:worldgen/biome")
        .map(|v| v.iter().map(|(n, _)| n.clone()).collect())
        .unwrap_or_default();
    let states = vec![
        (Arc::<str>::from("minecraft:air"), false),
        (Arc::<str>::from("minecraft:stone"), false),
        (Arc::<str>::from("minecraft:chest"), false),
    ];
    let encoder = ChunkEncoder::new(states, &sync.builtin, &biomes, &biomes);
    let hub = Hub::new(
        lan,
        sync,
        encoder,
        HubConfig {
            allow_flight: true,
            default_game_mode: 1,
            guest_commands: true,
            force_game_mode: false,
            hardcore: false,
            view_distance: 3,
            simulation_distance: 3,
            integrated: true,
            dimension: "minecraft:overworld".into(),
            seed: 1234,
            max_players: 8,
            levels: Default::default(),
        },
    );
    Server {
        hub,
        world: flat_world(),
        next_entity: 100,
        placed: Vec::new(),
        packets: Vec::new(),
    }
}

/// Tick the server while a bot thread runs.
fn run<T: Send + 'static>(
    s: &mut Server,
    bot: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> T {
    let handle = thread::spawn(bot);
    let start = Instant::now();
    while !handle.is_finished() {
        s.tick(&[]);
        thread::sleep(Duration::from_millis(10));
        assert!(start.elapsed() < Duration::from_secs(40), "timed out");
    }
    handle.join().unwrap().unwrap()
}

/// An integrated server opened to LAN with a signed-in owner: a guest's
/// player list gives the owner the account's skin (its `textures`
/// property), and the guest itself none (an offline-mode world).
#[test]
fn a_lan_guest_sees_the_signed_in_owners_skin() {
    let Some(resources) = resources() else {
        return;
    };
    let textures = login::Property {
        name: "textures".into(),
        value: "e30=".into(),
        signature: Some("c2lnbmVk".into()),
    };
    let fetched = textures.clone();
    let profile = Arc::new(OwnerProfile::new(move || vec![fetched]));
    let net = NetConfig {
        announce: false,
        owner_profile: Some(profile.clone()),
        ..NetConfig::loopback(0, "Test - World".into(), 8)
    };
    let mut s = server_with(&resources, net);
    s.hub.singleplayer_owner = Some(offline_uuid("Owner"));
    let port = s.hub.lan.port;
    let deadline = Instant::now() + Duration::from_secs(5);
    while profile.properties().is_empty() {
        assert!(
            Instant::now() < deadline,
            "the profile fetch never answered"
        );
        thread::yield_now();
    }
    // The owner (the window's connection) joins first and stays.
    let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let staying = stop.clone();
    let owner = thread::spawn(move || -> Result<(), String> {
        let mut bot = connect(port, "Owner", false)?;
        bot.c
            .reader
            .set_read_timeout(Some(Duration::from_millis(100)))
            .unwrap();
        while !staying.load(std::sync::atomic::Ordering::Relaxed) {
            let _ = bot.c.recv();
        }
        Ok(())
    });
    let deadline = Instant::now() + Duration::from_secs(10);
    while !s.hub.guests.iter().any(|g| g.name == "Owner") {
        s.tick(&[]);
        assert!(Instant::now() < deadline, "the owner did not join");
        thread::sleep(Duration::from_millis(10));
    }
    let listed = run(&mut s, move || {
        let mut bot = connect(port, "Guest", false)?;
        let (mut owner, mut guest) = (None, None);
        let deadline = Instant::now() + Duration::from_secs(20);
        while Instant::now() < deadline && (owner.is_none() || guest.is_none()) {
            if let C::PlayerInfoUpdate(p) = bot.recv_play()? {
                for e in p.entries {
                    if e.uuid == offline_uuid("Owner") {
                        owner = Some(e.properties);
                    } else if e.uuid == offline_uuid("Guest") {
                        guest = Some(e.properties);
                    }
                }
            }
        }
        Ok((owner, guest))
    });
    stop.store(true, std::sync::atomic::Ordering::Relaxed);
    owner.join().unwrap().unwrap();
    assert_eq!(listed.0, Some(vec![textures]), "the owner's entry");
    assert_eq!(listed.1, Some(Vec::new()), "the guest's entry");
}

#[test]
fn recipe_settings_packets_update_only_the_players_book_and_survive_save() {
    let Some(resources) = resources() else {
        return;
    };
    let mut s = server(&resources);
    let port = s.hub.lan.port;
    let _bot = run(&mut s, move || connect(port, "RecipeSettings", true));
    let deadline = Instant::now() + Duration::from_secs(5);
    while s.hub.guests.is_empty() {
        s.tick(&[]);
        assert!(Instant::now() < deadline, "guest did not join");
        thread::yield_now();
    }
    for book in 0..4 {
        for flags in 0..4 {
            let open = flags & 1 != 0;
            let filtering = flags & 2 != 0;
            let before = s.hub.guests[0].recipe_book.clone();
            // Vanilla's wire payload: RecipeBookType VarInt, open, filtering.
            let packet = S::decode(
                horizon_protocol::ids::play::serverbound::RECIPE_BOOK_CHANGE_SETTINGS,
                &[book, u8::from(open), u8::from(filtering)],
            )
            .unwrap();
            s.hub.guests[0].inbox.push_back((packet, 0));
            s.tick(&[]);
            let settings = &s.hub.guests[0].recipe_book;
            for i in 0..4 {
                assert_eq!(
                    settings.open[i],
                    if i == usize::from(book) {
                        open
                    } else {
                        before.open[i]
                    }
                );
                assert_eq!(
                    settings.filtering[i],
                    if i == usize::from(book) {
                        filtering
                    } else {
                        before.filtering[i]
                    }
                );
            }
            let mut saved = horizon_client::save::Level::default();
            horizon_client::lan::playerdata::store(&s.hub.guests[0], &mut saved, None);
            let mut reopened = horizon_client::save::Level::default();
            reopened.read_player(&saved.player_tag());
            assert_eq!(reopened.recipe_book.open, settings.open);
            assert_eq!(reopened.recipe_book.filtering, settings.filtering);
        }
    }
    for book_type in [-1, 4] {
        let mut wire = horizon_protocol::buf::Writer::new();
        wire.varint(book_type);
        wire.bool(true);
        wire.bool(true);
        assert!(
            S::decode(
                horizon_protocol::ids::play::serverbound::RECIPE_BOOK_CHANGE_SETTINGS,
                &wire.data,
            )
            .is_err()
        );
    }
}

/// A server whose pack carries recipes with NeoForge's ingredient shapes:
/// every guest that joins gets the shapes' items, not empty slots — the
/// book lists the shaped recipe's compound, the furnace's and the
/// stonecutter's input sets carry it — and no guest is disconnected over
/// them (#6368).
#[test]
fn neoforge_shapes_items_reach_two_guests() {
    let Some(resources) = resources() else {
        return;
    };
    let compound = r#"{"type":"neoforge:compound",
        "children":["minecraft:raw_copper","minecraft:redstone"]}"#;
    let packs = vec![
        (
            "advancement/minecraft:recipes/compound_book".to_owned(),
            vec![
                br#"{"criteria":{"tick":{"trigger":"minecraft:tick"}},
                "rewards":{"recipes":["minecraft:compound_book"]}}"#
                    .to_vec(),
            ],
        ),
        (
            "recipe/minecraft:compound_book".to_owned(),
            vec![
                format!(
                    r#"{{"type":"minecraft:crafting_shaped","pattern":["X"],
                "key":{{"X":{compound}}},
                "result":{{"id":"minecraft:copper_ingot"}}}}"#
                )
                .into_bytes(),
            ],
        ),
        (
            "recipe/minecraft:compound_smelting".to_owned(),
            vec![
                format!(
                    r#"{{"type":"minecraft:smelting","ingredient":{compound},
                "result":{{"id":"minecraft:copper_ingot"}}}}"#
                )
                .into_bytes(),
            ],
        ),
        (
            "recipe/minecraft:compound_stonecutting".to_owned(),
            vec![
                format!(
                    r#"{{"type":"minecraft:stonecutting","ingredient":{compound},
                "result":{{"id":"minecraft:copper_ingot"}}}}"#
                )
                .into_bytes(),
            ],
        ),
    ];
    let sync = Arc::new(
        SyncData::load(
            &resources,
            &horizon_world::loot::PackOverlay { files: &packs },
        )
        .expect("sync data"),
    );
    let (raw_copper, redstone) = (
        sync.id("minecraft:item", "minecraft:raw_copper").unwrap(),
        sync.id("minecraft:item", "minecraft:redstone").unwrap(),
    );
    let mut s = server_sync(sync, NetConfig::loopback(0, "Test - World".into(), 8));
    let port = s.hub.lan.port;
    // Guest one joins first and stays while guest two joins and reads its
    // own join packets, then guest one reads its own.
    let mut one = run(&mut s, move || connect(port, "CompoundOne", true));
    let expected = (raw_copper, redstone);
    let _two_stays = run(&mut s, move || {
        let mut two = connect(port, "CompoundTwo", true)?;
        read_join_recipes(&mut two, expected, "guest two")?;
        Ok(two)
    });
    assert_eq!(s.hub.guests.len(), 2, "both guests stay connected");
    run(&mut s, move || {
        read_join_recipes(&mut one, expected, "guest one")
    });
}

/// Reads a fresh guest's packets until the recipe update and the book add
/// arrived, checking the compound's items in each: the furnace's input
/// set, a stonecutting entry's input, and the book entry's requirements.
fn read_join_recipes(bot: &mut Bot, expected: (i32, i32), who: &str) -> Result<(), String> {
    let (raw_copper, redstone) = expected;
    let mut sets_ok = false;
    let mut book_ok = false;
    for _ in 0..400 {
        let packet = bot.recv_play().map_err(|e| format!("{who}: {e}"))?;
        match packet {
            C::Unknown(id, payload)
                if id == horizon_protocol::ids::play::clientbound::UPDATE_RECIPES =>
            {
                let mut r = horizon_protocol::buf::Reader::new(&payload);
                let mut furnace_input = None;
                let mut stonecutting = Vec::new();
                for _ in 0..r.varint().map_err(|e| e.to_string())? {
                    let key = r.string(64).map_err(|e| e.to_string())?;
                    let ids: Vec<i32> = (0..r.varint().map_err(|e| e.to_string())?)
                        .map(|_| r.varint().unwrap_or(-1))
                        .collect();
                    if key == "minecraft:furnace_input" {
                        furnace_input = Some(ids);
                    }
                }
                let furnace_input = furnace_input.expect("furnace_input set");
                assert!(
                    furnace_input.contains(&raw_copper) && furnace_input.contains(&redstone),
                    "{who}: furnace_input holds the compound: first ids {:?}",
                    &furnace_input[..furnace_input.len().min(8)]
                );
                for _ in 0..r.varint().map_err(|e| e.to_string())? {
                    if let horizon_protocol::packets::recipes::HolderSet::Items(ids) =
                        horizon_protocol::packets::recipes::HolderSet::read(&mut r)
                            .map_err(|e| e.to_string())?
                    {
                        stonecutting.push(ids);
                    }
                    // The result's ItemStackSlotDisplay: its type, the item,
                    // the count and the two empty component lists.
                    for _ in 0..5 {
                        r.varint().map_err(|e| e.to_string())?;
                    }
                }
                assert!(
                    stonecutting
                        .iter()
                        .any(|ids| ids.contains(&raw_copper) && ids.contains(&redstone)),
                    "{who}: a stonecutting input holds the compound"
                );
                sets_ok = true;
            }
            C::RecipeBookAdd(add) => {
                // sendInitialRecipeBook's own add arrives first, empty
                // until the first tick rewards its recipes.
                if add.entries.is_empty() {
                    continue;
                }
                let compound = add.entries.iter().any(|e| {
                    e.requirements.as_deref().unwrap_or(&[]).iter().any(|set| {
                        matches!(set,
                            horizon_protocol::packets::recipes::HolderSet::Items(ids)
                            if ids.contains(&raw_copper) && ids.contains(&redstone))
                    })
                });
                assert!(
                    compound,
                    "{who}: the book lists the compound recipe ({} entries: {:?})",
                    add.entries.len(),
                    add.entries
                        .iter()
                        .map(|e| e.requirements.clone())
                        .collect::<Vec<_>>()
                );
                book_ok = true;
            }
            _ => {}
        }
        if sets_ok && book_ok {
            return Ok(());
        }
    }
    Err(format!(
        "{who}: recipes {sets_ok} book {book_ok} never both arrived"
    ))
}

#[test]
fn sprint_commands_survive_keyboard_input_changes() {
    let Some(resources) = resources() else {
        return;
    };
    let mut s = server(&resources);
    let port = s.hub.lan.port;
    let _bot = run(&mut s, move || connect(port, "Sprinter", true));
    let deadline = Instant::now() + Duration::from_secs(5);
    while s.hub.guests.is_empty() {
        s.tick(&[]);
        assert!(Instant::now() < deadline, "guest did not join");
        thread::yield_now();
    }
    let id = s.hub.guests[0].entity_id;
    s.hub.guests[0].loaded = true;
    s.hub.guests[0].load_timeout = 0;
    s.hub.guests[0].inbox.push_back((
        S::PlayerCommand(play::PlayerCommand {
            id,
            action: 1,
            data: 0,
        }),
        0,
    ));
    s.tick(&[]);
    assert!(s.hub.guests[0].sprinting);
    // LocalPlayer keeps sprinting after Ctrl is released while W remains
    // down. Double-tap sprint also sends forward without the sprint key.
    for flags in [65, 1, 0, 33] {
        s.hub.guests[0]
            .inbox
            .push_back((S::PlayerInput(play::PlayerInput { flags }), 0));
        s.tick(&[]);
        let g = &s.hub.guests[0];
        assert!(
            g.sprinting,
            "keyboard input {flags} cleared the sprint flag"
        );
        assert_eq!(g.input, flags);
        assert_eq!(g.sneaking, flags & 32 != 0);
    }
    s.hub.guests[0].inbox.push_back((
        S::PlayerCommand(play::PlayerCommand {
            id,
            action: 2,
            data: 0,
        }),
        0,
    ));
    s.tick(&[]);
    assert!(!s.hub.guests[0].sprinting);
    s.hub.guests[0]
        .inbox
        .push_back((S::PlayerInput(play::PlayerInput { flags: 65 }), 0));
    s.tick(&[]);
    assert!(
        !s.hub.guests[0].sprinting,
        "keyboard input started a sprint"
    );
}

#[test]
fn a_move_behind_a_dig_and_its_swing_waits_for_the_next_tick() {
    let Some(resources) = resources() else {
        return;
    };
    let mut s = server(&resources);
    let port = s.hub.lan.port;
    let _bot = run(&mut s, move || connect(port, "Digger", true));
    let deadline = Instant::now() + Duration::from_secs(5);
    while s.hub.guests.is_empty() {
        s.tick(&[]);
        assert!(Instant::now() < deadline, "guest did not join");
        thread::yield_now();
    }
    s.hub.guests[0].loaded = true;
    s.hub.guests[0].load_timeout = 0;
    let at = s.hub.guests[0].position;
    let below = [
        at[0].floor() as i32,
        at[1].floor() as i32 - 1,
        at[2].floor() as i32,
    ];
    // A creative break as the client sends it in one tick: the dig, the arm swing,
    // then the move into the block it predicted gone.
    let inbox = &mut s.hub.guests[0].inbox;
    inbox.push_back((
        S::PlayerAction(play::PlayerAction {
            action: 0,
            pos: below,
            direction: 1,
            sequence: 1,
        }),
        0,
    ));
    inbox.push_back((S::Punch(Empty), 0));
    inbox.push_back((
        S::MovePlayerPos(play::MovePlayerPos {
            x: at[0],
            y: at[1] - 0.0784,
            z: at[2],
            flags: 0,
        }),
        0,
    ));
    s.tick(&[]);
    // The game breaks the block after the hub's pass: the move waits, or it would be
    // checked against the block and sent back.
    let g = &s.hub.guests[0];
    assert_eq!(g.position, at, "the move was handled before the dig");
    assert!(
        matches!(g.inbox.back(), Some((S::MovePlayerPos(_), _))),
        "the move left the queue"
    );
}

/// The first login packet a client named `name` gets after its hello.
fn first_login_packet(mut c: Connection, port: u16, name: &str) -> login::Clientbound {
    c.send(
        &handshake::Serverbound::Intention(handshake::Intention {
            protocol: PROTOCOL_VERSION,
            host: "127.0.0.1".into(),
            port,
            intent: handshake::LOGIN,
        })
        .encode(),
    )
    .unwrap();
    c.send(
        &login::Serverbound::Hello(login::ServerHello {
            name: name.into(),
            uuid: offline_uuid(name),
        })
        .encode(),
    )
    .unwrap();
    let (id, payload) = c.recv().unwrap();
    login::Clientbound::decode(id, &payload).unwrap()
}

#[test]
fn a_signed_in_hosts_lan_authenticates_guests_but_not_its_own_player() {
    use horizon_client::lan::{NetConfig, OnlineMode};
    let Some(resources) = resources() else {
        return;
    };
    let sync = Arc::new(
        SyncData::load(&resources, &horizon_world::loot::PackOverlay::NONE).expect("sync data"),
    );
    let net = NetConfig {
        online: Some(Arc::new(OnlineMode::generate(false).unwrap())),
        announce: false,
        ..NetConfig::loopback(0, "Test - World".into(), 8)
    };
    let lan = Lan::start_with(sync, net).unwrap();
    let guest = Connection::connect("127.0.0.1", lan.port, Duration::from_secs(5)).unwrap();
    match first_login_packet(guest, lan.port, "Guest") {
        login::Clientbound::Hello(h) => assert!(h.should_authenticate),
        other => panic!("expected an encryption request, got {other:?}"),
    }
    let owner = Connection::local(lan.connect_local().unwrap());
    match first_login_packet(owner, 0, "Host") {
        login::Clientbound::LoginFinished(p) => assert_eq!(p.profile.uuid, offline_uuid("Host")),
        other => panic!("expected the owner's login, got {other:?}"),
    }
}

#[test]
fn a_move_behind_a_use_on_a_block_and_its_swing_waits_for_the_next_tick() {
    let Some(resources) = resources() else {
        return;
    };
    let mut s = server(&resources);
    let port = s.hub.lan.port;
    let _bot = run(&mut s, move || connect(port, "Pillar", true));
    let deadline = Instant::now() + Duration::from_secs(5);
    while s.hub.guests.is_empty() {
        s.tick(&[]);
        assert!(Instant::now() < deadline, "guest did not join");
        thread::yield_now();
    }
    s.hub.guests[0].loaded = true;
    s.hub.guests[0].load_timeout = 0;
    // The join's teleport accepted: the hub takes moves.
    s.hub.guests[0].awaiting_teleport = None;
    let at = s.hub.guests[0].position;
    let below = [
        at[0].floor() as i32,
        at[1].floor() as i32 - 1,
        at[2].floor() as i32,
    ];
    // A jump up a pillar as the client sends it in one tick: the click on the block
    // below from where the player was, the arm swing, then the move up the jump.
    let inbox = &mut s.hub.guests[0].inbox;
    inbox.push_back((
        S::UseItemOn(play::UseItemOn {
            hand: 0,
            pos: below,
            face: 1,
            cursor: [0.5, 1.0, 0.5],
            inside: false,
            world_border: false,
            sequence: 1,
        }),
        0,
    ));
    inbox.push_back((S::Punch(Empty), 0));
    inbox.push_back((
        S::MovePlayerPos(play::MovePlayerPos {
            x: at[0],
            y: at[1] + 0.42,
            z: at[2],
            flags: 0,
        }),
        0,
    ));
    s.tick(&[]);
    // The game places the block after the hub's pass: BlockItem.canPlace sees the
    // player where they clicked from, as their client's prediction did.
    assert_eq!(s.placed.len(), 1, "the use was not handled");
    let g = &s.hub.guests[0];
    assert_eq!(g.position, at, "the move was handled before the use");
    assert!(
        matches!(g.inbox.back(), Some((S::MovePlayerPos(_), _))),
        "the move left the queue"
    );
}

#[test]
fn a_move_behind_an_attack_and_its_swing_waits_for_the_next_tick() {
    let Some(resources) = resources() else {
        return;
    };
    let mut s = server(&resources);
    let port = s.hub.lan.port;
    let _bot = run(&mut s, move || connect(port, "Striker", true));
    let deadline = Instant::now() + Duration::from_secs(5);
    while s.hub.guests.is_empty() {
        s.tick(&[]);
        assert!(Instant::now() < deadline, "guest did not join");
        thread::yield_now();
    }
    s.hub.guests[0].loaded = true;
    s.hub.guests[0].load_timeout = 0;
    s.hub.guests[0].awaiting_teleport = None;
    s.hub.guests[0].on_ground = false;
    // A falling attack sharing a server tick with the landing move behind it:
    // the attack, the arm swing, then the landing move.
    let inbox = &mut s.hub.guests[0].inbox;
    inbox.push_back((S::Attack(play::Attack { id: 1 }), 0));
    inbox.push_back((S::Punch(Empty), 0));
    inbox.push_back((S::MovePlayerStatus(play::MovePlayerStatus { flags: 1 }), 0));
    s.tick(&[]);
    // The game runs the attack after the hub's pass: the landing move waits so
    // critical_attack still sees the player off the ground.
    let g = &s.hub.guests[0];
    assert!(!g.on_ground, "the landing move ran before the attack");
    assert!(
        matches!(g.inbox.back(), Some((S::MovePlayerStatus(_), _))),
        "the move left the queue"
    );
    s.tick(&[]);
    let g = &s.hub.guests[0];
    assert!(g.on_ground, "the landing move did not run on the next tick");
    assert!(g.inbox.is_empty());
}

#[test]
fn status_and_join() {
    let Some(resources) = resources() else {
        return;
    };
    let mut s = server(&resources);
    let port = s.hub.lan.port;
    // Server list ping.
    let json = run(&mut s, move || {
        let mut c = Connection::connect("127.0.0.1", port, Duration::from_secs(5))
            .map_err(|e| e.to_string())?;
        c.send(
            &handshake::Serverbound::Intention(handshake::Intention {
                protocol: PROTOCOL_VERSION,
                host: "127.0.0.1".into(),
                port,
                intent: handshake::STATUS,
            })
            .encode(),
        )
        .unwrap();
        c.send(&status::Serverbound::StatusRequest(Empty).encode())
            .unwrap();
        let (id, payload) = c.recv().map_err(|e| e.to_string())?;
        let status::Clientbound::StatusResponse(r) =
            status::Clientbound::decode(id, &payload).map_err(|e| e.to_string())?
        else {
            return Err("no status".into());
        };
        Ok(r.json)
    });
    let v: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(
        v["version"]["protocol"],
        horizon_core::version::PROTOCOL_VERSION
    );
    // ServerStatus.CODEC writes a plain literal MOTD as a bare string.
    assert_eq!(v["description"], "Test - World");

    // `DedicatedServer.setRepliesToStatus`: disabling it closes a fresh status handshake.
    s.hub.lan.shared.set_enable_status(false);
    let closed = run(&mut s, move || {
        let mut c = Connection::connect("127.0.0.1", port, Duration::from_secs(5))
            .map_err(|e| e.to_string())?;
        c.send(
            &handshake::Serverbound::Intention(handshake::Intention {
                protocol: PROTOCOL_VERSION,
                host: "127.0.0.1".into(),
                port,
                intent: handshake::STATUS,
            })
            .encode(),
        )
        .map_err(|e| e.to_string())?;
        Ok(c.recv().is_err())
    });
    assert!(
        closed,
        "status handshake should close when replies are disabled"
    );
    s.hub.lan.shared.set_enable_status(true);

    // Join, receive the world, place a block, see another player arrive.
    let report = run(&mut s, move || {
        let mut bot = connect(port, "Steve", true)?;
        let mut chunks = 0;
        let (mut login_seen, mut teleported, mut time, mut info) = (false, false, false, false);
        let mut commands = false;
        let deadline = Instant::now() + Duration::from_secs(15);
        let mut sent_loaded = false;
        let mut block_update = None;
        while Instant::now() < deadline {
            match bot.recv_play()? {
                C::Login(l) => {
                    assert_eq!(l.chunk_radius, 3);
                    assert_eq!(l.spawn.dimension, "minecraft:overworld");
                    assert_eq!(l.spawn.game_type, 1);
                    login_seen = true;
                }
                C::PlayerPosition(p) => {
                    assert_eq!(p.position[0], 8.5);
                    bot.send(S::AcceptTeleportation(play::AcceptTeleportation {
                        id: p.teleport,
                        x: p.position[0],
                        y: p.position[1],
                        z: p.position[2],
                        yaw: 0.0,
                        pitch: 0.0,
                    }));
                    teleported = true;
                }
                // The join's full clock sync; MinecraftServer's 20-tick
                // forceGameTimeSynchronization sends no clocks and may come first.
                C::SetTime(t) if !t.clocks.is_empty() => time = true,
                C::PlayerInfoUpdate(u) => {
                    info |= u.entries.iter().any(|e| e.name.as_deref() == Some("Steve"));
                }
                C::Commands(_) => commands = true,
                C::LevelChunkWithLight(l) => {
                    chunks += 1;
                    let sections = l.chunk.sections(24, 16, 7).map_err(|e| e.to_string())?;
                    // The floor is in the lowest section.
                    assert_eq!(sections[0].states.get(4 * 256), STONE as i32);
                    assert_eq!(sections[0].non_empty, 256);
                    assert_eq!(sections[1].non_empty, 0);
                    assert!(!l.light.sky.is_empty());
                }
                C::ChunkBatchFinished(b) => {
                    assert!(b.size > 0);
                    bot.send(S::ChunkBatchReceived(play::ChunkBatchReceived {
                        chunks_per_tick: 20.0,
                    }));
                    if !sent_loaded && chunks >= 9 {
                        bot.send(S::PlayerLoaded(Empty));
                        sent_loaded = true;
                        bot.send(S::MovePlayerPosRot(play::MovePlayerPosRot {
                            x: 9.5,
                            y: f64::from(MIN_Y + 5),
                            z: 9.5,
                            yaw: 90.0,
                            pitch: 10.0,
                            flags: 1,
                        }));
                        bot.send(S::UseItemOn(play::UseItemOn {
                            hand: 0,
                            pos: [9, MIN_Y + 4, 9],
                            face: 1,
                            cursor: [0.5, 1.0, 0.5],
                            inside: false,
                            world_border: false,
                            sequence: 77,
                        }));
                    }
                }
                C::BlockUpdate(u) => {
                    block_update = Some((u.pos, u.state));
                }
                C::BlockChangedAck(a) if a.sequence == 77 && block_update.is_some() => break,
                _ => {}
            }
        }
        assert!(
            login_seen && teleported && time && info && commands,
            "missing login data"
        );
        assert!(chunks >= 9, "only {chunks} chunks");
        assert_eq!(
            block_update,
            Some(([9, MIN_Y + 5, 9], STONE as i32)),
            "placed block was not broadcast"
        );
        Ok(bot.name)
    });
    assert_eq!(report, "Steve");
    assert_eq!(s.placed.len(), 1);
}

#[test]
fn guests_see_each_other_and_the_world_entities() {
    let Some(resources) = resources() else {
        return;
    };
    let mut s = server(&resources);
    let port = s.hub.lan.port;
    let zombie = EntityState {
        id: 500,
        uuid: 9,
        kind: "minecraft:zombie".into(),
        position: [12.0, f64::from(MIN_Y + 5), 12.0],
        yaw: 0.0,
        pitch: 0.0,
        head_yaw: 0.0,
        on_ground: true,
        velocity: [0.0; 3],
        minecart_steps: None,
        data: 0,
        meta: Vec::new(),
        defaults: Vec::new(),
        equipment: Vec::new(),
        attributes: Vec::new(),
        precise: false,
        vertical_collision: false,
        horizontal_collision: false,
        needs_sync: false,
        sync_position: false,
        stepped: false,
        idle: false,
        bare: false,
        leash: None,
        spectator: false,
        watching: false,
        hidden: None,
    };
    let first = thread::spawn(move || {
        let mut bot = connect(port, "Alex", false)?;
        bot.c
            .reader
            .set_read_timeout(Some(Duration::from_millis(200)))
            .unwrap();
        let mut zombies = 0;
        let mut players = 0;
        let mut moves = 0;
        let mut at: Option<[f64; 3]> = None;
        let mut reminder = Instant::now();
        let deadline = Instant::now() + Duration::from_secs(20);
        while Instant::now() < deadline && !(zombies > 0 && players > 0 && moves > 0) {
            // A vanilla client sends its position at least every 20 ticks even
            // when standing still (LocalPlayer's positionReminder), and the
            // server checks which entities it sees again on each one.
            if let Some([x, y, z]) = at
                && reminder.elapsed() >= Duration::from_secs(1)
            {
                bot.send(S::MovePlayerPos(play::MovePlayerPos { x, y, z, flags: 1 }));
                bot.send(S::ClientTickEnd(Empty));
                reminder = Instant::now();
            }
            let Ok((id, payload)) = bot.c.recv() else {
                continue;
            };
            let Ok(packet) = C::decode(id, &payload) else {
                continue;
            };
            match packet {
                C::PlayerPosition(p) => {
                    at = Some(p.position);
                    bot.send(S::AcceptTeleportation(play::AcceptTeleportation {
                        id: p.teleport,
                        x: p.position[0],
                        y: p.position[1],
                        z: p.position[2],
                        yaw: 0.0,
                        pitch: 0.0,
                    }))
                }
                C::AddEntity(a) if a.id == 500 => zombies += 1,
                C::AddEntity(a) if a.uuid == offline_uuid("Bob") => players += 1,
                C::MoveEntityPos(_) | C::MoveEntityPosRot(_) => moves += 1,
                _ => {}
            }
        }
        if zombies > 0 && players > 0 && moves > 0 {
            Ok(())
        } else {
            Err(format!("zombies {zombies} players {players} moves {moves}"))
        }
    });
    let second = thread::spawn(move || {
        thread::sleep(Duration::from_millis(600));
        let mut bot = connect(port, "Bob", true)?;
        let deadline = Instant::now() + Duration::from_secs(20);
        let mut x = 8.5;
        while Instant::now() < deadline {
            // Keep reading and walking so the other player sees movement.
            bot.c
                .reader
                .set_read_timeout(Some(Duration::from_millis(200)))
                .unwrap();
            if let Ok((id, payload)) = bot.c.recv()
                && let Ok(C::PlayerPosition(p)) = C::decode(id, &payload)
            {
                bot.send(S::AcceptTeleportation(play::AcceptTeleportation {
                    id: p.teleport,
                    x: p.position[0],
                    y: p.position[1],
                    z: p.position[2],
                    yaw: 0.0,
                    pitch: 0.0,
                }));
            }
            x += 0.2;
            bot.send(S::MovePlayerPos(play::MovePlayerPos {
                x,
                y: f64::from(MIN_Y + 5),
                z: 8.5,
                flags: 1,
            }));
            // A client ends each tick: a second move before it is kicked.
            bot.send(S::ClientTickEnd(Empty));
        }
        Ok::<(), String>(())
    });
    let start = Instant::now();
    while !first.is_finished() {
        s.tick(std::slice::from_ref(&zombie));
        thread::sleep(Duration::from_millis(10));
        assert!(start.elapsed() < Duration::from_secs(40), "timed out");
    }
    first.join().unwrap().unwrap();
    drop(second);
}

/// A menu opened with extra data (a mod menu type's, NeoForge's `openMenu`
/// with an extra-data writer) reaches the client as a `horizon:menu_extra`
/// payload naming the window and the block, right before that window's
/// `OpenScreen`; a menu without it sends no payload.
#[test]
fn menu_extra_data_comes_right_before_its_menu() {
    use horizon_client::{items::ItemStack, lan::menu::Slots};
    use horizon_protocol::menu_extra;
    let Some(resources) = resources() else {
        return;
    };
    let mut s = server(&resources);
    let port = s.hub.lan.port;
    let bot = run(&mut s, move || connect(port, "Machinist", true));
    let deadline = Instant::now() + Duration::from_secs(5);
    while s.hub.guests.is_empty() {
        s.tick(&[]);
        assert!(Instant::now() < deadline, "guest did not join");
        thread::yield_now();
    }
    let conn = s.hub.guests[0].conn;
    bot.c
        .reader
        .set_read_timeout(Some(Duration::from_secs(3)))
        .unwrap();
    let key = (3, 70, -5);
    let items: Vec<Option<ItemStack>> = vec![None; 9];
    let title = || horizon_protocol::text::translatable("container.dispenser", &[]);
    /// The packets up to the next `OpenScreen`: the menu extra payloads met
    /// on the way, each with whether the screen came right after it, then
    /// the screen's window.
    type Extras = Vec<(menu_extra::Message, bool)>;
    fn until_screen(mut bot: Bot) -> Result<(Bot, Extras, i32), String> {
        let mut extras = Extras::new();
        loop {
            match bot.recv_play()? {
                C::CustomPayload(p) if p.channel == menu_extra::CHANNEL => {
                    let message = menu_extra::decode(&p.data).map_err(|e| e.to_string())?;
                    extras.push((message, true));
                }
                C::OpenScreen(p) => return Ok((bot, extras, p.container)),
                _ => {
                    if let Some(last) = extras.last_mut() {
                        last.1 = false;
                    }
                }
            }
        }
    }
    s.hub.open_container(
        conn,
        "minecraft:generic_3x3",
        title(),
        key,
        Slots::Block(key),
        items.clone(),
        None,
    );
    let (bot, extras, _) = run(&mut s, move || until_screen(bot));
    assert!(extras.is_empty(), "a stock menu sends no extra data");
    s.hub.open_container(
        conn,
        "minecraft:generic_3x3",
        title(),
        key,
        Slots::Block(key),
        items,
        Some(vec![7, 0, 255]),
    );
    let window = s.hub.guests[0].menu.as_ref().unwrap().window;
    let (_, extras, opened) = run(&mut s, move || until_screen(bot));
    assert_eq!(opened, window);
    assert_eq!(
        extras,
        [(
            menu_extra::Message {
                window,
                pos: [3, 70, -5],
                data: vec![7, 0, 255],
            },
            true
        )],
        "one payload, for this window and block, with nothing between it and the screen"
    );
}

/// `ServerGamePacketListenerImpl.handleContainerClick`: a spectator looks
/// into a chest but its clicks change nothing, and `sendAllDataToRemote`
/// takes back what its client predicted.
#[test]
fn a_spectators_container_click_is_taken_back() {
    use horizon_client::{containers::BlockEntity, items::ItemStack, lan::menu::Slots};
    struct NoRules;
    impl horizon_client::lan::menu::Rules for NoRules {
        fn items(&self) -> &horizon_client::items::Items {
            unreachable!("a spectator's click never reaches the menu")
        }
        fn max_stack(&self, _: &str) -> u32 {
            64
        }
        fn armor_index(&self, _: &str) -> Option<usize> {
            None
        }
    }
    let Some(resources) = resources() else {
        return;
    };
    let mut s = server(&resources);
    let port = s.hub.lan.port;
    let mut bot = run(&mut s, move || connect(port, "Watcher", true));
    let deadline = Instant::now() + Duration::from_secs(5);
    while s.hub.guests.is_empty() {
        s.tick(&[]);
        assert!(Instant::now() < deadline, "guest did not join");
        thread::yield_now();
    }
    let conn = s.hub.guests[0].conn;
    s.hub.set_game_mode(conn, 3, &s.world, None);
    let at = s.hub.guests[0].position;
    let key = (
        at[0].floor() as i32,
        at[1].floor() as i32,
        at[2].floor() as i32 + 2,
    );
    s.world.set(key.0, key.1, key.2, BlockStateId(2)).unwrap();
    let mut items = vec![None; 27];
    items[0] = Some(ItemStack::new("minecraft:diamond", 5));
    s.world
        .block_entities
        .insert(key, BlockEntity::Chest(items.clone()));
    s.hub.open_container(
        conn,
        "minecraft:generic_9x3",
        horizon_protocol::text::translatable("container.chest", &[]),
        key,
        Slots::Block(key),
        items.clone(),
        None,
    );
    let window = s.hub.guests[0].menu.as_ref().unwrap().window;
    // The menu's own contents, as the client first sees them.
    bot.c
        .reader
        .set_read_timeout(Some(Duration::from_secs(3)))
        .unwrap();
    let content = move |bot: &mut Bot| -> Result<(), String> {
        loop {
            if let C::ContainerSetContent(p) = bot.recv_play()?
                && p.container == window
            {
                assert!(p.items[0].is_some(), "the diamonds are still in slot 0");
                assert!(p.carried.is_none(), "nothing is carried");
                return Ok(());
            }
        }
    };
    let mut bot = run(&mut s, move || content(&mut bot).map(|()| bot));
    // The client predicted picking up the diamonds.
    let click = play::ContainerClick {
        container: window,
        state: 1,
        slot: 0,
        button: 0,
        input: 0,
        changed: vec![(0, None)],
        carried: None,
    };
    let drops = s.hub.container_click(conn, &click, &mut s.world, &NoRules);
    assert!(drops.is_empty());
    assert_eq!(
        s.world
            .block_entities
            .get(&key)
            .map(|b| b.slots()[0].clone()),
        Some(items[0].clone()),
        "the chest keeps its diamonds"
    );
    assert!(s.hub.guests[0].carried.is_none());
    run(&mut s, move || content(&mut bot));
}

/// `/fillbiome` on a server: `ChunkMap.resendBiomesForChunks` sends each
/// player the new biomes of the columns it has been sent, and only those,
/// in a `ClientboundChunksBiomesPacket` the client reads section by section.
#[test]
fn changed_biomes_reach_the_players_that_see_the_column() {
    let Some(resources) = resources() else {
        return;
    };
    let mut s = server(&resources);
    let port = s.hub.lan.port;
    let bot = run(&mut s, move || connect(port, "Gardener", true));
    // The column at the spawn arrives, with the batch that holds it.
    let mut bot = run(&mut s, move || {
        let mut bot = bot;
        let mut seen = false;
        loop {
            match bot.recv_play()? {
                C::PlayerPosition(p) => {
                    bot.send(S::AcceptTeleportation(play::AcceptTeleportation {
                        id: p.teleport,
                        x: p.position[0],
                        y: p.position[1],
                        z: p.position[2],
                        yaw: 0.0,
                        pitch: 0.0,
                    }))
                }
                C::LevelChunkWithLight(l) if (l.x, l.z) == (0, 0) => seen = true,
                C::ChunkBatchFinished(_) => {
                    bot.send(S::ChunkBatchReceived(play::ChunkBatchReceived {
                        chunks_per_tick: 20.0,
                    }));
                    if seen {
                        return Ok(bot);
                    }
                }
                _ => {}
            }
        }
    });
    assert!(s.hub.guests[0].sent_chunks.contains(&(0, 0)));
    assert!(!s.hub.guests[0].sent_chunks.contains(&(4, 4)));
    for key in [(0, 0), (4, 4)] {
        let chunk = Arc::make_mut(s.world.chunks.get_mut(&key).unwrap());
        // The quart cell at x 4..8, y 4..8, z 8..12 of the column.
        chunk.set_biome_cell(1, 17, 2, 5);
    }
    s.hub.resend_biomes(&s.world, &[(0, 0), (4, 4)]);
    let packet = run(&mut s, move || {
        loop {
            if let C::ChunksBiomes(p) = bot.recv_play()? {
                return Ok(p);
            }
        }
    });
    assert_eq!(packet.chunks.len(), 1, "only the column the player sees");
    assert_eq!(packet.chunks[0].pos, [0, 0]);
    let sections = packet.chunks[0].biomes(24, 7).unwrap();
    // Section 4 (y 0..16), cell (x 1, y 1, z 2).
    assert_eq!(sections[4].get(1 << 4 | 2 << 2 | 1), 5);
    assert_eq!(sections[4].get(0), 0);
    assert!(
        sections
            .iter()
            .enumerate()
            .all(|(i, b)| i == 4 || b.get(25) == 0)
    );
}

#[test]
fn a_click_past_the_menu_is_ignored() {
    use horizon_client::{containers::BlockEntity, items::ItemStack, lan::menu::Slots};
    struct NoRules;
    impl horizon_client::lan::menu::Rules for NoRules {
        fn items(&self) -> &horizon_client::items::Items {
            unreachable!("a click past the menu never reaches it")
        }
        fn max_stack(&self, _: &str) -> u32 {
            64
        }
        fn armor_index(&self, _: &str) -> Option<usize> {
            None
        }
    }
    let Some(resources) = resources() else {
        return;
    };
    let mut s = server(&resources);
    let port = s.hub.lan.port;
    let _bot = run(&mut s, move || connect(port, "Clicker", true));
    let deadline = Instant::now() + Duration::from_secs(5);
    while s.hub.guests.is_empty() {
        s.tick(&[]);
        assert!(Instant::now() < deadline, "guest did not join");
        thread::yield_now();
    }
    let conn = s.hub.guests[0].conn;
    let at = s.hub.guests[0].position;
    let key = (
        at[0].floor() as i32,
        at[1].floor() as i32,
        at[2].floor() as i32 + 2,
    );
    s.world.set(key.0, key.1, key.2, BlockStateId(2)).unwrap();
    let items = vec![None; 27];
    s.world
        .block_entities
        .insert(key, BlockEntity::Chest(items.clone()));
    s.hub.open_container(
        conn,
        "minecraft:generic_9x3",
        horizon_protocol::text::translatable("container.chest", &[]),
        key,
        Slots::Block(key),
        items,
        None,
    );
    let stone = Some(ItemStack::new("minecraft:stone", 64));
    s.hub.guests[0].inventory[9] = stone.clone();
    let window = s.hub.guests[0].menu.as_ref().unwrap().window;
    // AbstractContainerMenu.isValidSlotIndex: the chest's menu has 63 slots. Throwing
    // the whole stack (THROW, button 1) from slot 63 reached the inventory's slot 9.
    let click = play::ContainerClick {
        container: window,
        state: 1,
        slot: 63,
        button: 1,
        input: 4,
        changed: Vec::new(),
        carried: None,
    };
    let drops = s.hub.container_click(conn, &click, &mut s.world, &NoRules);
    assert!(drops.is_empty(), "nothing is thrown");
    assert_eq!(
        s.hub.guests[0].inventory[9], stone,
        "slot 9 keeps its stone"
    );
}

#[test]
fn the_prediction_records_each_client_tick_and_the_server_ticks_it_had_seen() {
    let Some(resources) = resources() else {
        return;
    };
    let mut s = server(&resources);
    let mut config = horizon_anticheat::Config {
        enabled: true,
        ..Default::default()
    };
    config.timer.enabled = false;
    config.ground.enabled = false;
    s.hub.set_anticheat(config);
    let port = s.hub.lan.port;
    let _bot = run(&mut s, move || connect(port, "Recorded", true));
    let deadline = Instant::now() + Duration::from_secs(5);
    while s.hub.guests.is_empty() {
        s.tick(&[]);
        assert!(Instant::now() < deadline, "guest did not join");
        thread::yield_now();
    }
    s.hub.guests[0].loaded = true;
    s.hub.guests[0].load_timeout = 0;
    let at = s.hub.guests[0].position;
    // The server's tick ends with a ping; the client answers it before its next tick.
    s.hub.anticheat_ping();
    let pinged = s.hub.ticks;
    let inbox = &mut s.hub.guests[0].inbox;
    inbox.push_back((
        S::Pong(horizon_protocol::packets::configuration::Ping { id: pinged as i32 }),
        0,
    ));
    inbox.push_back((S::PlayerInput(play::PlayerInput { flags: 1 }), 0));
    inbox.push_back((
        S::MovePlayerPosRot(play::MovePlayerPosRot {
            x: at[0],
            y: at[1],
            z: at[2] + 0.1,
            yaw: 45.0,
            pitch: 5.0,
            flags: 1,
        }),
        0,
    ));
    inbox.push_back((S::ClientTickEnd(Empty), 0));
    // A tick without a move.
    inbox.push_back((S::ClientTickEnd(Empty), 0));
    s.tick(&[]);
    let record = s.hub.guests[0]
        .anticheat
        .record
        .as_deref()
        .expect("the prediction records");
    let ticks: Vec<_> = record.ticks().copied().collect();
    assert_eq!(ticks.len(), 2);
    assert_eq!(ticks[0].position, Some([at[0], at[1], at[2] + 0.1]));
    assert_eq!(
        (ticks[0].yaw, ticks[0].pitch, ticks[0].flags),
        (45.0, 5.0, Some(1))
    );
    assert_eq!((ticks[0].input, ticks[0].seen), (1, Some(pinged)));
    assert_eq!(
        (ticks[1].position, ticks[1].input, ticks[1].yaw),
        (None, 1, 45.0)
    );
    // An answer to a ping never sent changes nothing.
    s.hub.guests[0].inbox.push_back((
        S::Pong(horizon_protocol::packets::configuration::Ping {
            id: (pinged + 100) as i32,
        }),
        0,
    ));
    s.tick(&[]);
    let record = s.hub.guests[0].anticheat.record.as_deref().unwrap();
    assert_eq!(record.seen(), Some(pinged));
}

/// A mod block entity reaches the players that see its chunk as vanilla's do
/// (`BlockEntityInfo.create`, then `sendBlockUpdated` and
/// `ChunkHolder.broadcastChanges`): in the chunk with its block's own type
/// and its mod's update tag, then once per tick however often it is synced,
/// with no `BlockEntityData` for one whose mod gives no tag.
#[test]
fn a_synced_mod_block_entity_reaches_the_chunks_players_once_a_tick() {
    use horizon_client::containers::{BlockEntity, ModBlockEntity, ModBlockEntityFields};
    use horizon_world::nbt::{Compound, JavaString, Tag};
    let Some(resources) = resources() else {
        return;
    };
    const MACHINE: u32 = 3;
    let mut s = server(&resources);
    let states = [
        "minecraft:air",
        "minecraft:stone",
        "minecraft:chest",
        "test:machine",
    ]
    .map(|n| (Arc::<str>::from(n), false));
    let builtin = horizon_protocol::registry::Builtin::parse(
        r#"{"minecraft:block_entity_type": {"entries": {"test:machine": {"protocol_id": 60}}}}"#,
    )
    .unwrap();
    s.hub.encoder = ChunkEncoder::new(states, &builtin, &[], &[]);
    // A machine shows whether it runs; one without `on` shows nothing.
    s.hub.encoder.mod_update_tag = Some(|_, entity| {
        let on = entity.nbt.get(&JavaString::from("on"))?.clone();
        Some(Compound::from([(JavaString::from("on"), on)]))
    });
    let (shown, silent) = ((1, MIN_Y + 5, 1), (3, MIN_Y + 5, 3));
    for (pos, on) in [(shown, Some(0)), (silent, None)] {
        s.world
            .set(pos.0, pos.1, pos.2, BlockStateId(MACHINE))
            .unwrap();
        let mut machine = ModBlockEntity::new(ModBlockEntityFields {
            id: "test:machine".into(),
            ..Default::default()
        });
        if let Some(on) = on {
            machine.nbt.insert("on".into(), Tag::Byte(on));
        }
        s.world
            .block_entities
            .insert(pos, BlockEntity::Mod(Box::new(machine)));
    }
    let on = |v: i8| Tag::Compound(Compound::from([(JavaString::from("on"), Tag::Byte(v))]));
    let port = s.hub.lan.port;
    let bot = run(&mut s, move || connect(port, "Engineer", true));
    let (mut bot, infos) = run(&mut s, move || {
        let mut bot = bot;
        let mut infos = None;
        loop {
            match bot.recv_play()? {
                C::PlayerPosition(p) => {
                    bot.send(S::AcceptTeleportation(play::AcceptTeleportation {
                        id: p.teleport,
                        x: p.position[0],
                        y: p.position[1],
                        z: p.position[2],
                        yaw: 0.0,
                        pitch: 0.0,
                    }))
                }
                C::LevelChunkWithLight(l) if (l.x, l.z) == (0, 0) => {
                    infos = Some(l.chunk.block_entities)
                }
                C::ChunkBatchFinished(_) => {
                    bot.send(S::ChunkBatchReceived(play::ChunkBatchReceived {
                        chunks_per_tick: 20.0,
                    }));
                    if let Some(infos) = infos.take() {
                        return Ok((bot, infos));
                    }
                }
                _ => {}
            }
        }
    });
    let mut sent: Vec<_> = infos
        .iter()
        .map(|e| (e.packed_xz, e.kind, e.tag.clone()))
        .collect();
    sent.sort_by_key(|e| e.0);
    assert_eq!(sent, [(0x11, 60, on(0)), (0x33, 60, Tag::End)]);
    // The machine starts and is synced twice; the silent one once. A stone
    // in the section above comes after them, in the same tick.
    if let Some(BlockEntity::Mod(m)) = s.world.block_entities.get_mut(&shown) {
        m.nbt.insert("on".into(), Tag::Byte(1));
    }
    let log = s
        .world
        .block_log
        .as_mut()
        .expect("the hub logs block changes");
    log.extend([shown, silent, shown]);
    let marker = [2, MIN_Y + 21, 2];
    s.world
        .set(marker[0], marker[1], marker[2], BlockStateId(STONE))
        .unwrap();
    let data = run(&mut s, move || {
        let mut data = Vec::new();
        loop {
            match bot.recv_play()? {
                C::BlockEntityData(p) => data.push((p.pos, p.tag)),
                C::BlockUpdate(p) if p.pos == marker => return Ok(data),
                _ => {}
            }
        }
    });
    assert_eq!(data, [([shown.0, shown.1, shown.2], on(1))]);
}

/// #4301: the damage-immunity term `ServerPlayer.isInvulnerableTo` takes from
/// its connection (`!hasClientLoaded`) is cleared by a respawn, kept through a
/// level change, and comes back with ServerboundPlayerLoadedPacket: a respawned
/// guest cannot be hurt at the spot it died in before its client is back.
#[test]
fn a_respawned_guest_is_immune_until_its_client_loads_the_world() {
    let Some(resources) = resources() else {
        return;
    };
    let mut s = server(&resources);
    let port = s.hub.lan.port;
    let _bot = run(&mut s, move || connect(port, "Respawned", true));
    let deadline = Instant::now() + Duration::from_secs(5);
    while s.hub.guests.is_empty() {
        s.tick(&[]);
        assert!(Instant::now() < deadline, "guest did not join");
        thread::yield_now();
    }
    s.hub.guests[0].load_timeout = 0;
    assert!(s.hub.guests[0].client_loaded());
    let border = horizon_client::worldborder::WorldBorder::default();
    let at = [8.5, f64::from(MIN_Y + 5), 8.5];
    let ctx = TickCtx {
        world: &mut s.world,
        entities: &[],
        next_entity: &mut s.next_entity,
        spawn: at,
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
    s.hub.respawn(0, at, [0.0; 2], &ctx);
    assert!(
        !s.hub.guests[0].client_loaded(),
        "a respawn unloads the client"
    );
    // A level change does not clear it: vanilla resets the loaded state on death and respawn
    // only, and covers a portal with its separate `isChangingDimension` term.
    s.hub.change_level(0, at, 0.0, None, false, 0, &ctx);
    assert!(
        !s.hub.guests[0].client_loaded(),
        "a level change keeps the cleared state from the respawn"
    );
    // The client's ServerboundPlayerLoadedPacket once the new level is in.
    s.hub.guests[0].inbox.push_back((S::PlayerLoaded(Empty), 0));
    s.tick(&[]);
    assert!(
        s.hub.guests[0].client_loaded(),
        "the packet marks it loaded"
    );
}

/// `tickPlayer`'s vehicle check (BoatFly): the driver of a boat its last accepted move left hovering
/// over air is kicked once the boat has floated longer than `getMaximumFlyingTicks` (160 ticks at a
/// boat's gravity), and a move that doesn't float starts the count again.
#[test]
fn a_boat_floating_too_long_kicks_its_driver() {
    let Some(resources) = resources() else {
        return;
    };
    let mut s = server(&resources);
    let port = s.hub.lan.port;
    let _bot = run(&mut s, move || connect(port, "Boater", true));
    let deadline = Instant::now() + Duration::from_secs(5);
    while s.hub.guests.is_empty() {
        s.tick(&[]);
        assert!(Instant::now() < deadline, "guest did not join");
        thread::yield_now();
    }
    let g = &mut s.hub.guests[0];
    (g.vehicle, g.vehicle_floating, g.vehicle_gravity) = (Some(7), true, 0.04);
    for _ in 0..100 {
        s.tick(&[]);
    }
    assert_eq!(s.hub.guests[0].above_ground_vehicle_ticks, 100);
    // The boat touched water: the next floating move counts from zero.
    s.hub.guests[0].vehicle_floating = false;
    s.tick(&[]);
    assert_eq!(s.hub.guests[0].above_ground_vehicle_ticks, 0);
    s.hub.guests[0].vehicle_floating = true;
    for _ in 0..160 {
        s.tick(&[]);
    }
    assert!(
        s.hub.guests[0].left_reason.is_empty(),
        "160 ticks are a boat's allowance: {}",
        s.hub.guests[0].left_reason
    );
    s.tick(&[]);
    assert!(
        !s.hub.guests[0].left_reason.is_empty(),
        "the 161st floating tick kicks the driver"
    );
}

/// A client that numbers its packets as `version` does, from the protocol adapter's own tables: for 26.2 its
/// handshake states 776 and every id it sends and reads is 26.2's. [`Self::send`]'s payloads are the pinned
/// ones; [`Self::send_raw`] sends a packet as the version lays it out. The clientbound world packets arrive
/// in 26.2's layout.
struct Numbered {
    c: Connection,
    version: ServerVersion,
    state: horizon_protocol::State,
    /// Reads and writes packets in the version's own layout (Horizon's adapter for older servers), not
    /// only its ids.
    session: Option<horizon_protocol::adapter::session::Session>,
}
impl Numbered {
    fn send(&mut self, body: Vec<u8>) -> Result<(), String> {
        if let Some(session) = &self.session {
            let e = |e: horizon_protocol::Error| e.to_string();
            if let Some(body) = session.serverbound(&body).map_err(e)? {
                self.c.send(&body).map_err(e)?;
            }
            for body in session.take_outgoing() {
                self.c.send(&body).map_err(e)?;
            }
            return Ok(());
        }
        let body = self
            .version
            .serverbound_body(self.state, &body)
            .map_err(|e| e.to_string())?
            .ok_or("the client's version has no such packet")?;
        self.c.send(&body).map_err(|e| e.to_string())
    }
    /// A packet by the version's own `id`, its payload in the version's layout.
    fn send_raw(&mut self, id: i32, payload: &[u8]) -> Result<(), String> {
        let mut w = horizon_protocol::buf::Writer::new();
        w.varint(id);
        w.bytes(payload);
        self.c.send(&w.data).map_err(|e| e.to_string())
    }
    /// The next packet with its pinned id (the version's own id read off the wire).
    fn recv(&mut self) -> Result<(i32, Vec<u8>), String> {
        if let Some(session) = &self.session {
            let e = |e: horizon_protocol::Error| e.to_string();
            loop {
                if let Some(p) = session.next_incoming() {
                    return Ok(p);
                }
                let (id, payload) = self.c.recv().map_err(e)?;
                if let Some(p) = session.clientbound(id, payload).map_err(e)? {
                    return Ok(p);
                }
            }
        }
        let (id, payload) = self.c.recv().map_err(|e| e.to_string())?;
        let pinned = self
            .version
            .clientbound_id(self.state, id)
            .ok_or_else(|| format!("no clientbound packet {id} in {:?}", self.state))?;
        Ok((pinned, payload))
    }
}

/// A login as `version`: `Err` with the refusal (login's JSON, configuration's text), or the client once in
/// play, with the ids of the item tag `beacon_payment_items` it was sent.
fn join_as(
    version: ServerVersion,
    port: u16,
    name: &str,
) -> Result<Result<(Numbered, Vec<i32>), String>, String> {
    join_with(version, port, name, false)
}

/// [`join_as`], the packets read and written in `version`'s layout when `layouts`.
fn join_with(
    version: ServerVersion,
    port: u16,
    name: &str,
    layouts: bool,
) -> Result<Result<(Numbered, Vec<i32>), String>, String> {
    use horizon_protocol::State;
    let e = |e: horizon_protocol::Error| e.to_string();
    let hello = |intent| {
        handshake::Serverbound::Intention(handshake::Intention {
            protocol: version.protocol(),
            host: "127.0.0.1".into(),
            port,
            intent,
        })
        .encode()
    };
    let c = Connection::connect("127.0.0.1", port, Duration::from_secs(5)).map_err(e)?;
    c.reader
        .set_read_timeout(Some(Duration::from_secs(20)))
        .map_err(e)?;
    let mut bot = Numbered {
        c,
        version,
        state: State::Login,
        session: layouts.then(|| horizon_protocol::adapter::session::Session::new(version)),
    };
    bot.send(hello(handshake::LOGIN))?;
    bot.send(
        login::Serverbound::Hello(login::ServerHello {
            name: name.into(),
            uuid: offline_uuid(name),
        })
        .encode(),
    )?;
    loop {
        let (id, payload) = bot.recv()?;
        match login::Clientbound::decode(id, &payload).map_err(e)? {
            login::Clientbound::Compression(p) => bot.c.set_compression(p.threshold),
            login::Clientbound::LoginFinished(_) => break,
            login::Clientbound::Disconnect(d) => return Ok(Err(d.reason_json)),
            other => return Err(format!("unexpected login packet {other:?}")),
        }
    }
    bot.send(login::Serverbound::LoginAcknowledged(Empty).encode())?;
    bot.state = State::Configuration;
    bot.send(cfg::Serverbound::ClientInformation(ClientInformation::new(4)).encode())?;
    let mut payment = Vec::new();
    loop {
        let (id, payload) = bot.recv()?;
        match cfg::Clientbound::decode(id, &payload).map_err(e)? {
            // A 26.2 client knows its own core pack, not 26.3's: it is sent the registries' data (and so
            // is this 26.3 client, alike).
            cfg::Clientbound::SelectKnownPacks(_) => bot.send(
                cfg::Serverbound::SelectKnownPacks(cfg::SelectKnownPacks { packs: Vec::new() })
                    .encode(),
            )?,
            cfg::Clientbound::UpdateTags(t) => {
                for registry in t.registries {
                    if registry.registry == "minecraft:item" {
                        payment.extend(
                            registry
                                .tags
                                .into_iter()
                                .filter(|t| t.name == "minecraft:beacon_payment_items")
                                .flat_map(|t| t.ids),
                        );
                    }
                }
            }
            cfg::Clientbound::Disconnect(d) => {
                return Ok(Err(match d.reason {
                    horizon_world::nbt::Tag::String(s) => s.to_string_lossy(),
                    other => format!("{other:?}"),
                }));
            }
            cfg::Clientbound::FinishConfiguration(_) => {
                bot.send(cfg::Serverbound::FinishConfiguration(Empty).encode())?;
                bot.state = State::Play;
                return Ok(Ok((bot, payment)));
            }
            _ => {}
        }
    }
}

/// The protocol a status ping stating `protocol` is answered with.
fn status_protocol(port: u16, protocol: i32) -> Result<i64, String> {
    let e = |e: horizon_protocol::Error| e.to_string();
    let mut c = Connection::connect("127.0.0.1", port, Duration::from_secs(5)).map_err(e)?;
    c.send(
        &handshake::Serverbound::Intention(handshake::Intention {
            protocol,
            host: "127.0.0.1".into(),
            port,
            intent: handshake::STATUS,
        })
        .encode(),
    )
    .map_err(e)?;
    c.send(&status::Serverbound::StatusRequest(Empty).encode())
        .map_err(e)?;
    let (id, payload) = c.recv().map_err(e)?;
    let status::Clientbound::StatusResponse(r) =
        status::Clientbound::decode(id, &payload).map_err(e)?
    else {
        return Err("no status".into());
    };
    let v: serde_json::Value = serde_json::from_str(&r.json).map_err(|e| e.to_string())?;
    v["version"]["protocol"]
        .as_i64()
        .ok_or_else(|| "no protocol".into())
}

/// `accept_older_clients` off (the default), a 26.2 client is turned away as vanilla turns it away and the
/// server list shows 26.3's protocol; on, the client is told its own protocol, logs in in 26.2's packet ids
/// and is told, before any registry data, that 26.2 isn't translated yet, while a 26.3 client is served as
/// before. Let past that (the tests-only `older_clients_play`), it is sent the tags in 26.2's item ids, plays in
/// 26.2's packet ids and sees the block it places.
#[test]
fn a_26_2_client_joins_only_a_server_that_accepts_older_clients() {
    let Some(resources) = resources() else {
        return;
    };
    const OLD: ServerVersion = ServerVersion::V26_2;
    const DIAMOND_26_2: i32 = 926;
    const DIAMOND_26_3: i32 = 1012;
    let mut off = server(&resources);
    let port = off.hub.lan.port;
    let (status, refused) = run(&mut off, move || {
        let status = status_protocol(port, 776)?;
        let refused = join_as(OLD, port, "Old")?
            .err()
            .ok_or("a 26.2 client got in")?;
        Ok((status, refused))
    });
    assert_eq!(status, i64::from(PROTOCOL_VERSION));
    assert!(
        refused.contains("multiplayer.disconnect.incompatible"),
        "{refused}"
    );

    let net = NetConfig {
        accept_older_clients: true,
        ..NetConfig::loopback(0, "Test - World".into(), 8)
    };
    let mut on = server_with(&resources, net);
    let port = on.hub.lan.port;
    let statuses = run(&mut on, move || {
        Ok((
            status_protocol(port, 776)?,
            status_protocol(port, PROTOCOL_VERSION)?,
        ))
    });
    assert_eq!(statuses, (776, i64::from(PROTOCOL_VERSION)));
    let unfinished = run(&mut on, move || {
        join_as(OLD, port, "Old")?
            .err()
            .ok_or_else(|| "a 26.2 client got past configuration".to_owned())
    });
    assert!(
        unfinished.contains(horizon_client::lan::OLDER_CLIENTS_UNFINISHED),
        "{unfinished}"
    );
    // A 26.3 client on the same server: its own ids, as on any server.
    let payment = run(&mut on, move || {
        let (_bot, payment) =
            join_as(ServerVersion::Current, port, "New")?.map_err(|r| format!("refused: {r}"))?;
        Ok(payment)
    });
    assert!(payment.contains(&DIAMOND_26_3), "{payment:?}");

    let net = NetConfig {
        accept_older_clients: true,
        older_clients_play: true,
        ..NetConfig::loopback(0, "Test - World".into(), 8)
    };
    let mut play = server_with(&resources, net);
    let port = play.hub.lan.port;
    let (payment, placed) = run(&mut play, move || {
        let (mut bot, payment) = join_as(OLD, port, "Old")?.map_err(|r| format!("refused: {r}"))?;
        let (mut login, mut chunks, mut placed) = (false, 0, None);
        let deadline = Instant::now() + Duration::from_secs(15);
        while Instant::now() < deadline {
            let (id, payload) = bot.recv()?;
            // The command tree has argument nodes the client side does not decode.
            if id == horizon_protocol::ids::play::clientbound::COMMANDS {
                continue;
            }
            // World packets come in 26.2's own layout, which the pinned readers don't read (the adapter's
            // unit tests read them back with translate's 26.2 readers).
            {
                use horizon_protocol::ids::play::clientbound as cb;
                match id {
                    cb::LEVEL_CHUNK_WITH_LIGHT => {
                        chunks += 1;
                        continue;
                    }
                    cb::LIGHT_UPDATE
                    | cb::LEVEL_PARTICLES
                    | cb::EXPLODE
                    | cb::MOVE_ENTITY_POS
                    | cb::MOVE_ENTITY_POS_ROT
                    | cb::MOVE_ENTITY_ROT
                    | cb::ENTITY_POSITION_SYNC
                    | cb::ANIMATE
                    // Its tag displays are 26.2's bare tag ids.
                    | cb::RECIPE_BOOK_ADD => continue,
                    _ => {}
                }
            }
            match C::decode(id, &payload).map_err(|e| format!("packet {id}: {e}"))? {
                C::Login(_) => login = true,
                C::PlayerPosition(p) => bot.send(
                    S::AcceptTeleportation(play::AcceptTeleportation {
                        id: p.teleport,
                        x: p.position[0],
                        y: p.position[1],
                        z: p.position[2],
                        yaw: 0.0,
                        pitch: 0.0,
                    })
                    .encode(),
                )?,
                C::ChunkBatchFinished(_) => {
                    bot.send(
                        S::ChunkBatchReceived(play::ChunkBatchReceived {
                            chunks_per_tick: 20.0,
                        })
                        .encode(),
                    )?;
                    if chunks >= 9 && placed.is_none() {
                        placed = Some(None);
                        bot.send(S::PlayerLoaded(Empty).encode())?;
                        bot.send(
                            S::MovePlayerPosRot(play::MovePlayerPosRot {
                                x: 9.5,
                                y: f64::from(MIN_Y + 5),
                                z: 9.5,
                                yaw: 90.0,
                                pitch: 10.0,
                                flags: 1,
                            })
                            .encode(),
                        )?;
                        bot.send(
                            S::UseItemOn(play::UseItemOn {
                                hand: 0,
                                pos: [9, MIN_Y + 4, 9],
                                face: 1,
                                cursor: [0.5, 1.0, 0.5],
                                inside: false,
                                world_border: false,
                                sequence: 77,
                            })
                            .encode(),
                        )?;
                    }
                }
                C::BlockUpdate(u) => placed = Some(Some((u.pos, u.state))),
                C::BlockChangedAck(a) if a.sequence == 77 => break,
                _ => {}
            }
        }
        assert!(login, "no login packet");
        Ok((payment, placed.flatten()))
    });
    assert!(payment.contains(&DIAMOND_26_2), "{payment:?}");
    assert!(!payment.contains(&DIAMOND_26_3), "{payment:?}");
    assert_eq!(placed, Some(([9, MIN_Y + 5, 9], STONE as i32)));
    assert_eq!(play.placed.len(), 1);
}

/// The serverbound packets whose layout 26.3 changed reach the game from a 26.2 client as 26.3's: its
/// teleport confirmations (the id alone) are taken, so its walk counts; its digging actions past 0 are one
/// up (start 0 stays 0, stop 2 is 3); its main-hand swing is a punch and its off-hand swing nothing.
#[test]
fn a_26_2_clients_walk_dig_swing_and_teleport_confirmations_read_as_26_3s() {
    let Some(resources) = resources() else {
        return;
    };
    // 26.2's serverbound ids (play) of the packets whose layout changed.
    const ACCEPT_TELEPORTATION_26_2: i32 = 0;
    const PLAYER_ACTION_26_2: i32 = 41;
    const SWING_26_2: i32 = 63;
    let net = NetConfig {
        accept_older_clients: true,
        older_clients_play: true,
        ..NetConfig::loopback(0, "Test - World".into(), 8)
    };
    let mut s = server_with(&resources, net);
    let port = s.hub.lan.port;
    let below = [9, MIN_Y + 4, 9];
    let walked = [9.5, f64::from(MIN_Y + 5), 9.5];
    let (_bot, confirmed) = run(&mut s, move || {
        let (mut bot, _) =
            join_as(ServerVersion::V26_2, port, "Old")?.map_err(|r| format!("refused: {r}"))?;
        let (mut confirmed, mut chunks, mut sent) = (0, 0, false);
        let deadline = Instant::now() + Duration::from_secs(15);
        while Instant::now() < deadline {
            let (id, payload) = bot.recv()?;
            // The command tree has argument nodes the client side does not decode.
            if id == horizon_protocol::ids::play::clientbound::COMMANDS {
                continue;
            }
            // The packets written in 26.2's layout, which the pinned readers don't read.
            {
                use horizon_protocol::ids::play::clientbound as cb;
                if id == cb::LEVEL_CHUNK_WITH_LIGHT {
                    chunks += 1;
                }
                if matches!(
                    id,
                    cb::LEVEL_CHUNK_WITH_LIGHT
                        | cb::LIGHT_UPDATE
                        | cb::LEVEL_PARTICLES
                        | cb::EXPLODE
                        | cb::MOVE_ENTITY_POS
                        | cb::MOVE_ENTITY_POS_ROT
                        | cb::MOVE_ENTITY_ROT
                        | cb::ENTITY_POSITION_SYNC
                        | cb::ANIMATE
                        | cb::RECIPE_BOOK_ADD
                ) {
                    continue;
                }
            }
            match C::decode(id, &payload).map_err(|e| format!("packet {id}: {e}"))? {
                C::PlayerPosition(p) => {
                    let mut w = horizon_protocol::buf::Writer::new();
                    w.varint(p.teleport);
                    bot.send_raw(ACCEPT_TELEPORTATION_26_2, &w.data)?;
                    confirmed += 1;
                }
                C::ChunkBatchFinished(_) => {
                    bot.send(
                        S::ChunkBatchReceived(play::ChunkBatchReceived {
                            chunks_per_tick: 20.0,
                        })
                        .encode(),
                    )?;
                }
                C::BlockChangedAck(a) if a.sequence == 77 => break,
                _ => {}
            }
            if chunks >= 9 && confirmed > 0 && !sent {
                sent = true;
                bot.send(S::PlayerLoaded(Empty).encode())?;
                // A walk from the spawn (8.5, 8.5), on the stone: one move, as vanilla kicks a second in a tick.
                bot.send(
                    S::MovePlayerPos(play::MovePlayerPos {
                        x: walked[0],
                        y: walked[1],
                        z: walked[2],
                        flags: 1,
                    })
                    .encode(),
                )?;
                // Start and stop destroying the block below, as 26.2 numbers the actions.
                for (action, sequence) in [(0, 5), (2, 6)] {
                    let mut w = horizon_protocol::buf::Writer::new();
                    w.varint(action);
                    w.i64(horizon_protocol::buf::pack_block_pos(below));
                    w.u8(1);
                    w.varint(sequence);
                    bot.send_raw(PLAYER_ACTION_26_2, &w.data)?;
                }
                // A main-hand swing, then an off-hand one.
                bot.send_raw(SWING_26_2, &[0])?;
                bot.send_raw(SWING_26_2, &[1])?;
                // Its acknowledgement says the hub has handled everything before it.
                bot.send(
                    S::UseItemOn(play::UseItemOn {
                        hand: 0,
                        pos: [12, MIN_Y + 4, 12],
                        face: 1,
                        cursor: [0.5, 1.0, 0.5],
                        inside: false,
                        world_border: false,
                        sequence: 77,
                    })
                    .encode(),
                )?;
            }
        }
        assert!(sent, "the client never got its chunks");
        // The bot is kept, so its guest stays to be looked at.
        Ok((bot, confirmed))
    });
    assert!(confirmed > 0, "no teleport to confirm");
    let g = &s.hub.guests[0];
    assert!(g.left_reason.is_empty(), "{}", g.left_reason);
    assert_eq!(g.position, walked, "the walk was not taken");
    let actions: Vec<i32> = s
        .packets
        .iter()
        .filter_map(|p| match p {
            S::PlayerAction(a) if a.pos == below => Some(a.action),
            _ => None,
        })
        .collect();
    assert_eq!(actions, [0, 3]);
    let punches = s
        .packets
        .iter()
        .filter(|p| matches!(p, S::Punch(_)))
        .count();
    assert_eq!(punches, 1);
}

/// A client of each older release the adapter knows (26.1 down to 1.20.5) gets past configuration and is sent chunks without being
/// disconnected, on a server that accepts older clients.
#[test]
fn older_clients_of_each_release_join_and_are_sent_chunks() {
    let Some(resources) = resources() else {
        return;
    };
    for version in [
        ServerVersion::V26_1,
        ServerVersion::V1_21_11,
        ServerVersion::V1_21_10,
        ServerVersion::V1_21_8,
        ServerVersion::V1_21_6,
        ServerVersion::V1_21_5,
        ServerVersion::V1_21_4,
        ServerVersion::V1_21_3,
        ServerVersion::V1_21_1,
        ServerVersion::V1_20_6,
    ] {
        let net = NetConfig {
            accept_older_clients: true,
            older_clients_play: true,
            ..NetConfig::loopback(0, "Test - World".into(), 8)
        };
        let mut s = server_with(&resources, net);
        let port = s.hub.lan.port;
        let chunks = run(&mut s, move || {
            let (mut bot, _) = join_with(version, port, "Old", true)
                .map_err(|e| format!("{}: join: {e}", version.name()))?
                .map_err(|r| format!("refused: {r}"))?;
            let mut chunks = 0;
            let mut last = -1;
            let deadline = Instant::now() + Duration::from_secs(10);
            while Instant::now() < deadline && chunks < 9 {
                let (id, _) = bot
                    .recv()
                    .map_err(|e| format!("{} after packet {last}: {e}", version.name()))?;
                last = id;
                use horizon_protocol::ids::play::clientbound as cb;
                if id == cb::DISCONNECT {
                    return Err("disconnected".into());
                }
                if id == cb::LEVEL_CHUNK_WITH_LIGHT {
                    chunks += 1;
                }
            }
            Ok(chunks)
        });
        assert!(chunks > 0, "{}: no chunks", version.name());
    }
}
