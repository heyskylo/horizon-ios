//! An allay saves and loads its item pickup cooldown memory
//! (`item_pickup_cooldown_ticks`, `{value: …}`), which `AllayAi` sets to 60 when
//! it throws an item (tools/test_allay_pickup_cooldown_parity.py measures the
//! same on vanilla 26.3).
use super::*;
use horizon_client::chunk::Chunk;
use horizon_core::position::ChunkPos;
use horizon_world::nbt::Tag;
use std::sync::Arc;

const SUPPLIED: &str = "00006096-0000-0000-0000-000000000001";

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
    game.world
        .chunks
        .insert((0, 0), Arc::new(Chunk::empty(0, 0)));
    game.block_ticks
        .add_empty_chunk(ChunkPos::new(0, 0))
        .unwrap();
    for command in [
        "forceload add 0 0",
        "fill 0 99 0 15 99 15 stone",
        "tick freeze",
        "summon allay 4.5 101 8.5 {PersistenceRequired:1b,NoGravity:1b,UUID:[I;24726,0,0,1],Brain:{memories:{\"minecraft:item_pickup_cooldown_ticks\":{value:30}}}}",
    ] {
        assert!(game.run_command(command), "{command}");
    }
    game
}

fn index(game: &mut Game<'_>) -> usize {
    match game.target_with_uuid(horizon_client::save::uuid_from_text(SUPPLIED).unwrap()) {
        Some(super::commands::Target::Mob(id)) => {
            game.mobs.iter().position(|m| m.id == id).unwrap()
        }
        other => panic!("{other:?}"),
    }
}

fn saved(game: &mut Game<'_>, i: usize) -> Option<Tag> {
    game.mob_tree_nbt(i)
        .get("Brain")
        .and_then(|b| b.get("memories"))
        .and_then(|m| m.get("minecraft:item_pickup_cooldown_ticks"))
        .cloned()
}

#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn an_allay_keeps_its_item_pickup_cooldown_memory() {
    let mut game = game();
    let i = index(&mut game);
    // The supplied memory is loaded into the pickup cooldown and saved back.
    assert_eq!(game.mobs[i].extra.ints[3], 30, "loaded");
    assert_eq!(
        saved(&mut game, i),
        Some(Tag::compound([("value", Tag::Int(30))])),
        "saved"
    );
    // It counts down with the allay's ticks.
    for _ in 0..10 {
        assert!(game.run_command("tick step 1"));
        game.tick_server(false).unwrap();
    }
    assert_eq!(
        saved(&mut game, i),
        Some(Tag::compound([("value", Tag::Int(20))])),
        "after 10 ticks"
    );
}
