//! Actual Game/guest/menu regression: vehicle loot must precede the live slot snapshot.
use super::*;
use crate::game::{Audio, Options, Progress, Settings, boats::Boat, load_resources};
use horizon_client::{
    advancements::{Advancement, Criterion, Rewards, Tree},
    chunk::Chunk,
    lan::{
        Joined, Lan, NetConfig, PlayerSender,
        chunks::ChunkEncoder,
        data::SyncData,
        guest::Guest,
        hub::{Hub, HubConfig},
    },
};
use horizon_world::nbt::Tag;
use std::sync::Arc;

const ADV: &str = "test:boat_loot";
const TABLE: &str = "minecraft:chests/simple_dungeon";
const KEY: (i32, i32, i32) = (i32::MAX - 15, super::super::minecarts::CART_KEY_Y, 100);

fn fixture() -> (Game<'static>, LanHost) {
    let root = std::path::PathBuf::from(
        std::env::var_os("HORIZON_TEST_RESOURCES").expect("prepared resources"),
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
    let mut options = Options::parse(
        [
            "--game-dir".to_owned(),
            std::env::current_dir()
                .unwrap()
                .to_string_lossy()
                .into_owned(),
        ]
        .into_iter(),
    )
    .unwrap()
    .unwrap();
    options.resources = root.clone();
    options.execution_role = crate::game::execution_role::ExecutionRole::IntegratedServer;
    let mut game = Game::new(
        &loaded.resources,
        loaded.pack,
        loaded.gui.unwrap(),
        options,
        settings,
        Audio::silent(&root),
        None,
    )
    .unwrap();
    game.options.dedicated = Some(Vec::new());
    game.world
        .chunks
        .insert((0, 0), Arc::new(Chunk::empty(0, 0)));
    game.boats
        .push(Boat::new(100, "oak".into(), Vec3::new(8.5, 65.0, 8.5), 0.0).with_chest());
    let mut tree = Tree::default();
    tree.list.push(Advancement {
        id: ADV.into(),
        parent: None,
        display: None,
        criteria: vec![Criterion {
            name: "opened".into(),
            trigger: "minecraft:player_generates_container_loot".into(),
            conditions: serde_json::json!({"loot_tables": TABLE}).into(),
        }],
        requirements: vec![vec!["opened".into()]],
        rewards: Rewards::default(),
    });
    tree.by_id.insert(ADV.into(), 0);
    tree.by_trigger.insert(
        "minecraft:player_generates_container_loot".into(),
        vec![(0, 0)],
    );
    game.adv.tree = Arc::new(tree);
    game.adv.loaded = true;
    let mut sync = SyncData::empty();
    sync.builtin.registries.insert(
        "minecraft:menu".into(),
        horizon_protocol::registry::Ids::from_names(vec!["minecraft:generic_9x3".into()]),
    );
    let sync = Arc::new(sync);
    let lan = Lan::start_with(
        sync.clone(),
        NetConfig {
            announce: false,
            ..NetConfig::loopback(0, "BoatLootFixture".into(), 2)
        },
    )
    .unwrap();
    let encoder = ChunkEncoder::new(
        [] as [(Arc<str>, bool); 0],
        &horizon_protocol::registry::Builtin::default(),
        &[],
        &[],
    );
    let mut hub = Hub::new(
        lan,
        sync,
        encoder,
        HubConfig {
            default_game_mode: 0,
            guest_commands: false,
            force_game_mode: false,
            hardcore: false,
            view_distance: 2,
            simulation_distance: 2,
            dimension: "minecraft:overworld".into(),
            allow_flight: false,
            seed: 0,
            max_players: 2,
            integrated: false,
            levels: Default::default(),
        },
    );
    for conn in [1, 2] {
        let (tx, _rx) = std::sync::mpsc::channel();
        let mut guest = Guest::new(
            Joined {
                conn,
                uuid: u128::from(conn),
                name: format!("Viewer{conn}"),
                properties: Vec::new(),
                info: horizon_protocol::packets::configuration::ClientInformation::new(2),
                brand: "vanilla".into(),
                sender: PlayerSender::new(tx, Arc::default()),
                latency: Arc::new(std::sync::atomic::AtomicI32::new(0)),
                address: "127.0.0.1:25565".parse().unwrap(),
                expected: false,
            },
            conn as i32,
            [8.5, 65.0, 8.5],
            2,
        );
        guest.placed = true;
        guest.dimension = "minecraft:overworld".into();
        hub.guests.push(guest);
    }
    (game, LanHost::fixture(hub))
}

fn pending(game: &mut Game<'_>) {
    game.persist.raw.insert(
        100,
        Tag::Compound(
            [
                ("LootTable".into(), Tag::String(TABLE.into())),
                ("LootTableSeed".into(), Tag::Long(123)),
            ]
            .into_iter()
            .collect(),
        ),
    );
}

fn open(game: &mut Game<'_>, host: &mut LanHost, conn: u32) {
    assert_eq!(
        game.as_guest(host, conn, |g| g.open_chest_boat_container(0)),
        Some(())
    );
}

fn contents(game: &Game<'_>, host: &LanHost, conn: u32) -> Vec<Option<ItemStack>> {
    let guest = host.hub.guest(conn).unwrap();
    let menu = guest.menu.as_ref().expect("real guest menu opens");
    assert_eq!(menu.slots, Slots::Block(KEY));
    menu.slots.read(&game.world, guest).unwrap()
}

#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn boat_loot_is_visible_to_its_opener_and_survives_close_reopen() {
    let (mut game, mut host) = fixture();
    pending(&mut game);
    open(&mut game, &mut host, 1);
    let rolled = contents(&game, &host, 1);
    assert!(
        rolled.iter().any(Option::is_some),
        "first-open loot must be in the actual menu's live slots"
    );
    assert!(
        host.hub.guest(1).unwrap().advancements.has(ADV, "opened"),
        "the opening guest owns the criterion"
    );
    assert!(!host.hub.guest(2).unwrap().advancements.has(ADV, "opened"));
    assert!(
        !game.adv.progress.has(ADV, "opened"),
        "the host must not inherit the guest's criterion"
    );
    assert!(!game.boat_loot_pending(100));
    game.guest_close_all(&mut host, 1);
    assert!(game.world.block_entities.get(&KEY).is_none());
    assert_eq!(game.boats[0].items, rolled);
    open(&mut game, &mut host, 2);
    assert_eq!(contents(&game, &host, 2), rolled);
    assert!(
        !host.hub.guest(2).unwrap().advancements.has(ADV, "opened"),
        "reopening does not generate loot twice"
    );
    game.guest_close_all(&mut host, 2);
}

#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn pending_boat_loot_cannot_be_consumed_by_a_spectator() {
    let (mut game, mut host) = fixture();
    pending(&mut game);
    host.hub.guests[0].game_mode = 3;
    open(&mut game, &mut host, 1);
    assert!(host.hub.guests[0].menu.is_none());
    assert!(game.boat_loot_pending(100));
    assert!(
        game.world.block_entities.get(&KEY).is_none(),
        "rejected spectator creates no orphan live inventory"
    );
    assert!(!host.hub.guests[0].advancements.has(ADV, "opened"));
    host.hub.guests[0].game_mode = 0;
    open(&mut game, &mut host, 1);
    assert!(contents(&game, &host, 1).iter().any(Option::is_some));
    game.guest_close_all(&mut host, 1);
    host.hub.guests[0].game_mode = 3;
    open(&mut game, &mut host, 1);
    assert!(
        host.hub.guests[0].menu.is_some(),
        "spectators may inspect already generated loot"
    );
    game.guest_close_all(&mut host, 1);
}

#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn rolling_into_a_live_boat_preserves_other_viewers_edited_slots() {
    let (mut game, mut host) = fixture();
    open(&mut game, &mut host, 1);
    let kept = Some(ItemStack::new("minecraft:diamond", 7));
    // Actual menu/world authority, with the vehicle backing intentionally stale.
    if let Some(BlockEntity::Chest(items)) = game.world.block_entities.get_mut(&KEY) {
        items[3] = kept.clone();
    } else {
        panic!("missing live inventory");
    }
    assert!(game.boats[0].items[3].is_none());
    pending(&mut game);
    open(&mut game, &mut host, 2);
    let second = contents(&game, &host, 2);
    assert_eq!(second[3], kept);
    assert!(
        second
            .iter()
            .enumerate()
            .any(|(i, s)| i != 3 && s.is_some()),
        "the remaining empty slots still receive loot"
    );
    assert_eq!(contents(&game, &host, 1), second);
    assert!(host.hub.guests[1].advancements.has(ADV, "opened"));
    assert!(!host.hub.guests[0].advancements.has(ADV, "opened"));
    game.guest_close_all(&mut host, 1);
    assert_eq!(contents(&game, &host, 2), second);
    game.guest_close_all(&mut host, 2);
    assert_eq!(game.boats[0].items, second);
    open(&mut game, &mut host, 1);
    assert_eq!(contents(&game, &host, 1), second);
    game.guest_close_all(&mut host, 1);
}

#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn owner_and_guest_vehicle_viewers_keep_the_same_live_inventory_until_last_close() {
    for owner_closes_first in [false, true] {
        let (mut game, mut host) = fixture();
        game.boats[0].items[3] = Some(ItemStack::new("minecraft:diamond", 7));
        game.open_chest_boat_container(0);
        assert_eq!(game.container, Some(KEY));
        open(&mut game, &mut host, 1);
        let edited = Some(ItemStack::new("minecraft:emerald", 9));
        let mut live = contents(&game, &host, 1);
        live[3] = edited.clone();
        Slots::Block(KEY).write(&mut game.world, &mut host.hub.guests[0], live.clone());
        if owner_closes_first {
            game.lan = Some(host);
            let key = game.container.take().unwrap();
            game.close_cart_container(key);
            host = game.lan.take().unwrap();
            assert_eq!(
                contents(&game, &host, 1),
                live,
                "owner close erased the guest's live slots"
            );
            game.guest_close_all(&mut host, 1);
        } else {
            game.guest_close_all(&mut host, 1);
            assert!(
                game.world.block_entities.get(&KEY).is_some(),
                "guest close erased the owner's live inventory"
            );
            assert_eq!(game.container, Some(KEY));
            assert_eq!(game.boats[0].items[3], edited);
            let key = game.container.take().unwrap();
            game.close_cart_container(key);
        }
        assert!(
            game.world.block_entities.get(&KEY).is_none(),
            "last close releases the shared placeholder"
        );
        assert_eq!(game.boats[0].items, live);
        open(&mut game, &mut host, 2);
        assert_eq!(
            contents(&game, &host, 2),
            live,
            "reopen must retain both viewers' edits"
        );
        game.guest_close_all(&mut host, 2);
    }
}

#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn foreign_or_unplaced_vehicle_menus_cannot_pin_this_levels_live_inventory() {
    for foreign_dimension in [false, true] {
        let (mut game, mut host) = fixture();
        game.boats[0].items[3] = Some(ItemStack::new("minecraft:diamond", 7));
        game.open_chest_boat_container(0);
        open(&mut game, &mut host, 1);
        let live = contents(&game, &host, 1);
        if foreign_dimension {
            host.hub.guests[0].dimension = "minecraft:the_nether".into();
        } else {
            host.hub.guests[0].placed = false;
        }
        game.lan = Some(host);
        let key = game.container.take().unwrap();
        game.close_cart_container(key);
        assert!(
            game.world.block_entities.get(&KEY).is_none(),
            "another level or an unplaced body must not count as a viewer here"
        );
        assert_eq!(game.boats[0].items, live);
    }
}
