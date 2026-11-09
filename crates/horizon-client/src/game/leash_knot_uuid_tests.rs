//! A mob or boat whose `leash` names a leash knot by UUID is tied to that knot
//! (#13113): `Leashable.restoreLeashFromSave` finds it with
//! `ServerLevel.getEntity`, as any other holder.
use super::*;
use horizon_client::chunk::Chunk;
use horizon_client::mobs::leash::Holder;
use horizon_core::position::ChunkPos;
use std::sync::Arc;

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
        "fill 0 63 0 15 63 15 stone",
        "setblock 4 64 6 minecraft:oak_fence",
        "summon minecraft:leash_knot 4.5 64 6.5 {UUID:[I;7193,0,0,1]}",
    ] {
        assert!(game.run_command(command), "{command}");
    }
    game
}

#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn a_mob_leashed_to_a_knots_uuid_is_tied_to_its_post() {
    let mut game = game();
    assert!(
        game.run_command("summon minecraft:wolf 2.5 64 6.5 {NoAI:1b,leash:{UUID:[I;7193,0,0,1]}}")
    );
    let id = game.mobs.last().unwrap().id;
    let dropped = game.dropped.len();
    for _ in 0..150 {
        game.tick_server(false).unwrap();
    }
    let wolf = game.mobs.iter().find(|m| m.id == id).unwrap();
    assert_eq!(wolf.extra.leash, Some(Holder::Knot([4, 64, 6])));
    assert!(wolf.extra.leash_pending.is_none());
    // Past the 100 ticks an unfound holder gets, no lead was dropped.
    assert_eq!(game.dropped.len(), dropped);
}

#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn a_boat_leashed_to_a_knots_uuid_is_tied_to_its_post() {
    let mut game = game();
    assert!(game.run_command("summon minecraft:oak_boat 2.5 64 9.5 {leash:{UUID:[I;7193,0,0,1]}}"));
    for _ in 0..150 {
        game.tick_server(false).unwrap();
    }
    let boat = game.boats.last().unwrap();
    assert_eq!(boat.leash, Some(Holder::Knot([4, 64, 6])));
    assert!(boat.leash_pending.is_none());
}
