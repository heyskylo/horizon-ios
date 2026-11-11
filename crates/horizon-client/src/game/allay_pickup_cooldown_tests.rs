//! An allay saves and loads its item pickup cooldown memory
//! (`item_pickup_cooldown_ticks`, `{value: …}`), which `AllayAi` sets to 60 when
//! it throws an item (tools/test_allay_pickup_cooldown_parity.py measures the
//! same on vanilla 26.3).
use super::*;
use horizon_client::chunk::Chunk;
use horizon_client::mobs::allay;
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
    assert_eq!(allay::pickup_cooldown(&game.mobs[i]), Some(30), "loaded");
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
    // CountDownCooldownTicks: the memory reaches 0 and is erased a tick later
    // (`stop`).
    step(&mut game, 20);
    assert_eq!(
        saved(&mut game, i),
        Some(Tag::compound([("value", Tag::Int(0))])),
        "at 0"
    );
    step(&mut game, 1);
    assert_eq!(saved(&mut game, i), None, "erased the tick after");
}

#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn pickup_cooldown_saves_the_full_integer_value() {
    let mut game = game();
    let i = index(&mut game);
    for ticks in [i32::MIN, -1, 0, 1, i32::MAX] {
        allay::set_pickup_cooldown(&mut game.mobs[i], Some(ticks));
        assert_eq!(
            saved(&mut game, i),
            Some(Tag::compound([("value", Tag::Int(ticks))])),
            "saved {ticks}"
        );
        let tag = game.mob_tree_nbt(i);
        let loaded = horizon_client::entity_nbt::mob_from_nbt(
            &tag,
            game.res.mob_types(),
            game.mobs[i].id,
            None,
        )
        .unwrap();
        assert_eq!(
            allay::pickup_cooldown(&loaded),
            Some(ticks),
            "loaded {ticks}"
        );
    }
}

// Added controls (after the frozen baseline above).
fn step(game: &mut Game<'_>, ticks: u32) {
    for _ in 0..ticks {
        assert!(game.run_command("tick step 1"));
        game.tick_server(false).unwrap();
    }
}

/// The saved key follows the model: none without a cooldown (no raw memory,
/// or a raw one the model has since ended), the value a throw sets, the same
/// value after a save and load, and a NoAI allay keeps its value (no Brain tick).
#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn the_saved_pickup_cooldown_follows_the_model() {
    let mut game = game();
    assert!(game.run_command(
        "summon allay 10.5 101 8.5 {PersistenceRequired:1b,NoGravity:1b,UUID:[I;24726,0,0,3]}"
    ));
    assert!(game.run_command(
        "summon allay 7.5 101 8.5 {NoAI:1b,PersistenceRequired:1b,NoGravity:1b,UUID:[I;24726,0,0,2],Brain:{memories:{\"minecraft:item_pickup_cooldown_ticks\":{value:30}}}}"
    ));
    let at =
        |game: &mut Game<'_>, tail: u32| match game.target_with_uuid([24726, 0, 0, tail as i32]) {
            Some(super::commands::Target::Mob(id)) => {
                game.mobs.iter().position(|m| m.id == id).unwrap()
            }
            other => panic!("{other:?}"),
        };
    // No raw memory, no cooldown: no key.
    let plain = at(&mut game, 3);
    assert_eq!(saved(&mut game, plain), None, "no raw memory");
    // A loaded memory the model has ended: no key (the raw copy is not echoed).
    let supplied = index(&mut game);
    allay::set_pickup_cooldown(&mut game.mobs[supplied], None);
    assert_eq!(saved(&mut game, supplied), None, "stale raw memory");
    // A model mutation, as a throw does (`allays.rs`): saved with its value.
    allay::set_pickup_cooldown(&mut game.mobs[plain], Some(60));
    let tag = saved(&mut game, plain);
    assert_eq!(
        tag,
        Some(Tag::compound([("value", Tag::Int(60))])),
        "a throw"
    );
    // Saved and loaded again: the same value.
    let whole = game.mob_tree_nbt(plain);
    let reloaded =
        horizon_client::entity_nbt::mob_from_nbt(&whole, game.res.mob_types(), 99, None).unwrap();
    assert_eq!(
        allay::pickup_cooldown(&reloaded),
        Some(60),
        "load after save"
    );
    // NoAI: no Brain tick, the value stays.
    let frozen = at(&mut game, 2);
    step(&mut game, 5);
    assert_eq!(
        saved(&mut game, frozen),
        Some(Tag::compound([("value", Tag::Int(30))])),
        "NoAI keeps it"
    );
}
