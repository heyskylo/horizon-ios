//! `LootCommand.dropKillLoot` under `execute as <guest>`: the source's
//! entity is the killer, so a connected player is `LAST_DAMAGE_PLAYER`
//! (`killed_by_player` holds) and its own weapon counts for looting, while
//! the message goes to the issuer (tools/test_loot_kill_guest_parity.py
//! measures the same on vanilla 26.3).
use super::*;
use horizon_client::{
    chunk::Chunk,
    items::ItemStack,
    lan::{Joined, NetConfig, PlayerSender, guest::Guest, hub::HubConfig},
    motion::PlayerMotion,
};
use horizon_protocol::packets::configuration::ClientInformation;
use std::sync::{Arc, atomic::AtomicI32, mpsc};

const GUEST: &str = "H038Guest";
const ELDER: &str = "00000038-0000-0000-0000-000000000001";
const PIG: &str = "00000038-0000-0000-0000-000000000002";

/// A host on a stone floor, holding a plain sword, with one connected,
/// placed guest (conn 2), an elder guardian, a pig and a chest.
fn game() -> Game<'static> {
    let root = std::path::PathBuf::from(
        std::env::var_os("HORIZON_TEST_RESOURCES")
            .expect("set HORIZON_TEST_RESOURCES to prepared resources"),
    );
    let settings = Settings::default();
    let loaded = load_resources(
        root.clone(),
        None,
        &root,
        &settings,
        std::time::Instant::now(),
        false,
        true,
        &Progress::new(),
        &mut |_, _| unreachable!(),
    )
    .unwrap();
    let mut options = Options::parse(std::iter::empty()).unwrap().unwrap();
    options.resources = root.clone();
    options.execution_role = execution_role::ExecutionRole::IntegratedServer;
    let res: &'static _ = Box::leak(Box::new(loaded.resources));
    let mut game = Game::new(
        res,
        loaded.pack,
        loaded.gui.unwrap(),
        options,
        settings,
        Audio::silent(&root),
        None,
    )
    .unwrap();
    game.build_registries();
    game.set_allow_commands(true);
    for x in 0..2 {
        game.world
            .chunks
            .insert((x, 0), Arc::new(Chunk::empty(x, 0)));
        game.block_ticks
            .add_empty_chunk(horizon_core::position::ChunkPos::new(x, 0))
            .unwrap();
    }
    assert!(game.run_command("fill 0 99 0 31 99 15 stone"));
    game.player = Some(PlayerMotion::new(game.motion, Vec3::new(4.5, 100.0, 6.5)).unwrap());
    let net = NetConfig {
        announce: false,
        ..NetConfig::lan(0, "h038-test".into(), 2)
    };
    game.lan_host(
        super::lan::LanSettings {
            port: 0,
            guest_commands: true,
            force_game_mode: false,
        },
        net,
        HubConfig {
            default_game_mode: 1,
            guest_commands: true,
            force_game_mode: false,
            hardcore: false,
            view_distance: 2,
            simulation_distance: 2,
            dimension: Dim::Overworld.id().into(),
            allow_flight: false,
            seed: 0,
            max_players: 2,
            integrated: false,
            levels: Default::default(),
        },
    )
    .unwrap();
    let (tx, _rx) = mpsc::channel();
    let mut guest = Guest::new(
        Joined {
            conn: 2,
            uuid: 2,
            name: GUEST.into(),
            properties: Vec::new(),
            info: ClientInformation::new(2),
            brand: "test".into(),
            sender: PlayerSender::new(tx, Arc::default()),
            latency: Arc::new(AtomicI32::new(0)),
            address: "127.0.0.1:25565".parse().unwrap(),
            expected: false,
            configured: Default::default(),
        },
        2,
        [22.5, 101.0, 9.5],
        1,
    );
    guest.placed = true;
    guest.loaded = true;
    // Its own weapon: a diamond sword with Looting III.
    let mut sword = ItemStack::new("minecraft:diamond_sword", 1);
    sword.data_mut().enchantments = vec![("minecraft:looting".into(), 3)];
    guest.inventory[0] = Some(sword);
    game.lan.as_mut().unwrap().hub.guests.push(guest);
    game.player.as_mut().unwrap().position = Vec3::new(4.5, 100.0, 4.5);
    game.inventory[game.selected] = Some(ItemStack::new("minecraft:diamond_sword", 1));
    assert!(game.run_command("setblock 8 100 8 chest"));
    assert!(game.run_command("difficulty normal"));
    let still = "NoAI:1b,NoGravity:1b,PersistenceRequired:1b,Silent:1b";
    let mut setup = |command: String| {
        game.chat_capture = Some(Vec::new());
        let ok = game.run_command(&command);
        let said = game.chat_capture.take().unwrap_or_default();
        let said: Vec<_> = said.iter().map(horizon_client::text::Text::plain).collect();
        assert!(ok, "{command}: {said:?}");
    };
    setup(format!(
        "summon elder_guardian 12.5 100 12.5 {{{still},UUID:[I;56,0,0,1]}}"
    ));
    setup(format!(
        "summon pig 12.5 100 4.5 {{{still},UUID:[I;56,0,0,2]}}"
    ));
    game
}

/// Runs `command`: whether it succeeded and what the issuer was told.
fn run(game: &mut Game<'_>, command: &str) -> (bool, Vec<String>) {
    game.chat_capture = Some(Vec::new());
    let ok = game.run_command(command);
    let said = game.chat_capture.take().unwrap_or_default();
    (
        ok,
        said.iter().map(horizon_client::text::Text::plain).collect(),
    )
}

/// The chest's items after `command` puts the guardian's kill loot in it,
/// and what the issuer was told.
fn loot(game: &mut Game<'_>, command: &str) -> (Vec<String>, Vec<String>) {
    // An empty chest answers "Nothing changed" here; that is fine.
    let _ = game.run_command("data merge block 8 100 8 {Items:[]}");
    let (_, said) = run(game, command);
    let items = (0..27)
        .filter_map(|i| game.slot_get(&commands::items::Slot::Block([8, 100, 8], i)))
        .map(|s| s.item.to_string())
        .collect();
    (items, said)
}

#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn loot_kill_as_a_guest_is_killed_by_that_player() {
    let mut game = game();
    let insert = format!("loot insert 8 100 8 kill {ELDER}");
    let sponge = |items: &[String]| items.iter().any(|i| i == "minecraft:wet_sponge");
    // The host: a player kill.
    let (host, _) = loot(&mut game, &insert);
    assert!(sponge(&host), "{host:?}");
    // A pig: no player kill.
    let (pig, _) = loot(&mut game, &format!("execute as {PIG} run {insert}"));
    assert!(!sponge(&pig), "{pig:?}");
    // As the guest: the guest is the killer, and the issuer is told.
    let (guest, said) = loot(&mut game, &format!("execute as {GUEST} run {insert}"));
    assert!(sponge(&guest), "killed by the guest: {guest:?}");
    assert_eq!(said.len(), 1, "{said:?}");
    assert!(said[0].starts_with("Dropped "), "{said:?}");
    // The host's own place and stack are its own again.
    let p = game.player.as_ref().unwrap().position;
    assert_eq!([p.x, p.y, p.z], [4.5, 100.0, 4.5]);
    let held = game.inventory[game.selected].as_ref().unwrap();
    assert_eq!(
        (&*held.item, held.data().is_some()),
        ("minecraft:diamond_sword", false)
    );
}

/// The full stacks (item, count, components) after `command`.
fn loot_stacks(game: &mut Game<'_>, command: &str) -> Vec<(String, u32, bool)> {
    let _ = game.run_command("data merge block 8 100 8 {Items:[]}");
    assert!(game.run_command(command));
    (0..27)
        .filter_map(|i| game.slot_get(&commands::items::Slot::Block([8, 100, 8], i)))
        .map(|s| (s.item.to_string(), s.count, s.data().is_some()))
        .collect()
}

/// Added controls (after the baseline): an empty-handed guest is still the
/// killer, with its empty hand and not the host's sword, and reading its
/// stack draws nothing from the loot's random sequence; a guest that is not
/// there keeps the old rule; every player's state is back afterwards.
#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn an_empty_handed_guest_is_the_killer_and_a_missing_one_is_not() {
    let insert = format!("loot insert 8 100 8 kill {ELDER}");
    let mut looting = ItemStack::new("minecraft:diamond_sword", 1);
    looting.data_mut().enchantments = vec![("minecraft:looting".into(), 3)];
    // As the empty-handed guest, while the host holds a Looting III sword.
    let mut first = game();
    first
        .lan
        .as_mut()
        .unwrap()
        .hub
        .guest_mut(2)
        .unwrap()
        .inventory[0] = None;
    first.inventory[first.selected] = Some(looting.clone());
    let as_guest = loot_stacks(&mut first, &format!("execute as {GUEST} run {insert}"));
    assert!(
        as_guest.iter().any(|s| s.0 == "minecraft:wet_sponge"),
        "{as_guest:?}"
    );
    // The host's own kill with an empty hand, from the same sequence state.
    let mut other = game();
    other.inventory[other.selected] = None;
    let as_host = loot_stacks(&mut other, &insert);
    assert_eq!(
        as_guest, as_host,
        "the guest's empty hand, and no extra draw"
    );
    // Everyone is as before.
    let held = first.inventory[first.selected].as_ref().unwrap();
    assert_eq!(
        (&*held.item, held.count, held.data().is_some()),
        ("minecraft:diamond_sword", 1, true)
    );
    let p = first.player.as_ref().unwrap().position;
    assert_eq!(
        ([p.x, p.y, p.z], first.dim),
        ([4.5, 100.0, 4.5], Dim::Overworld)
    );
    let g = first.lan.as_ref().unwrap().hub.guest(2).unwrap();
    assert_eq!(
        (g.position, g.selected, g.inventory[0].is_none()),
        ([22.5, 101.0, 9.5], 0, true)
    );
    // A connected player that is not there: the old rule (no player kill).
    let mut missing = first.player_source();
    missing.entity = Some(commands::Target::Guest(99));
    let _ = first.run_command("data merge block 8 100 8 {Items:[]}");
    assert!(first.execute_line(&insert, missing).is_ok());
    let items: Vec<String> = (0..27)
        .filter_map(|i| first.slot_get(&commands::items::Slot::Block([8, 100, 8], i)))
        .map(|s| s.item.to_string())
        .collect();
    assert!(
        !items.iter().any(|i| i == "minecraft:wet_sponge"),
        "{items:?}"
    );
}
