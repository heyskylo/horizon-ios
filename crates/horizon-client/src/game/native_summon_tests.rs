//! `native_summon` and `native_summon_with_uuid` build an entity tree from data as `/summon` does, and
//! `native_entity_merge_nbt` changes a living entity as `/data merge entity` does and refuses players.
use super::*;
use horizon_client::mod_api::NativeAuthority;
use horizon_world::nbt::{Compound, Tag};

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
    options.dedicated = Some(Vec::new());
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
    game.native_lifecycle
        .begin_open(NativeAuthority::IntegratedServer);
    game.world.chunks.insert(
        (0, 0),
        std::sync::Arc::new(horizon_client::chunk::Chunk::empty(0, 0)),
    );
    game.block_ticks
        .add_empty_chunk(horizon_core::position::ChunkPos::new(0, 0))
        .unwrap();
    game
}

fn compound(kind: &str) -> Compound {
    let mut c = Compound::new();
    c.insert("id".into(), Tag::String(kind.into()));
    c
}

fn pig_with_chicken() -> Compound {
    let mut pig = Compound::new();
    pig.insert(
        "Passengers".into(),
        Tag::List(vec![Tag::Compound(compound("minecraft:chicken"))]),
    );
    pig
}

const AT: [f64; 3] = [4.5, 200.0, 4.5];

#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn a_summoned_passenger_exists_and_rides() {
    let mut game = game();
    let dim = game.dim.id();
    let root = game
        .native_summon(dim, "minecraft:pig", AT, pig_with_chicken())
        .unwrap();
    assert_eq!(game.mobs.len(), 2);
    let rider = game.mobs.iter().find(|m| m.id != root).unwrap();
    assert_eq!(rider.extra.mount, Some(root));
}

#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn a_chosen_uuid_is_used_and_a_taken_one_is_refused() {
    let mut game = game();
    let dim = game.dim.id();
    let uuid = 0x1234_5678_9abc_def0_1234_5678_9abc_def0_u128;
    let id = game
        .native_summon_with_uuid(dim, "minecraft:pig", AT, pig_with_chicken(), uuid)
        .unwrap();
    assert_eq!(
        game.native_entity_by_uuid(uuid),
        Some((dim.to_owned(), id, "minecraft:pig".to_owned()))
    );
    assert_eq!(game.mobs.len(), 2);
    // The UUID is in use now: nothing more spawns.
    assert_eq!(
        game.native_summon_with_uuid(dim, "minecraft:cow", AT, Compound::new(), uuid),
        None
    );
    assert_eq!(game.mobs.len(), 2);
    // A passenger that takes the UUID of an entity of the level refuses the tree.
    let mut cow = Compound::new();
    let mut rider = compound("minecraft:chicken");
    rider.insert(
        "UUID".into(),
        Tag::IntArray(horizon_client::lan::playerdata::uuid_ints(uuid).to_vec()),
    );
    cow.insert("Passengers".into(), Tag::List(vec![Tag::Compound(rider)]));
    assert_eq!(
        game.native_summon_with_uuid(dim, "minecraft:cow", AT, cow, uuid + 1),
        None
    );
    assert_eq!(game.mobs.len(), 2);
}

#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn a_merge_changes_a_field_and_a_player_is_refused() {
    let mut game = game();
    let dim = game.dim.id();
    let id = game
        .native_summon(dim, "minecraft:item_frame", AT, {
            let mut c = Compound::new();
            c.insert("Facing".into(), Tag::Byte(3));
            c
        })
        .unwrap();
    let entity = game.native_entity(id).unwrap();
    let mut merge = Compound::new();
    merge.insert("ItemRotation".into(), Tag::Byte(5));
    assert!(game.native_entity_merge_nbt(entity, merge.clone()));
    assert_eq!(
        game.native_entity_nbt(entity)
            .unwrap()
            .get(&"ItemRotation".into()),
        Some(&Tag::Byte(5))
    );
    // Merging what is there already changes nothing.
    assert!(!game.native_entity_merge_nbt(entity, merge));
    // `/data merge entity` can't modify a player.
    assert!(
        game.apply_entity_nbt(commands::Target::Player, Tag::Compound(Compound::new()))
            .is_err()
    );
}
