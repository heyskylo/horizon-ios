//! `/setblock` and `/fill` place a block with `UPDATE_CLIENTS | UPDATE_SKIP_BLOCK_ENTITY_SIDEEFFECTS`
//! and send the neighbours' notice themselves, after the block's shape updates (`BlockInput.place`,
//! `ServerLevel.updateNeighboursOnBlockSet`): what a block pops off in answer to that notice has
//! already been seen by the wire next to it, which then ends as a cross, not a dot
//! (tools/redstone_diff.py found it on vanilla 26.3 with two random circuits and reduced it to these
//! four commands).
use super::*;
use horizon_client::chunk::Chunk;
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
    assert!(game.run_command("fill 0 99 0 15 99 15 stone"));
    game
}

fn at(game: &Game<'_>, x: i32, y: i32, z: i32) -> String {
    game.state_string(game.world.block(x, y, z))
}

fn run(game: &mut Game<'_>, commands: &[&str]) {
    for command in commands {
        assert!(game.run_command(command), "{command}");
    }
}

/// A comparator with nothing under it breaks at the first notice, which takes the wall torch attached
/// to it with it. The wire between connected to the torch when it was placed, and loses it again: a
/// wire that has lost its connection is a cross, not a dot (`RedStoneWireBlock.getConnectionState`).
#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn a_block_that_pops_off_in_the_neighbours_notice_leaves_the_wire_a_cross() {
    let mut game = game();
    run(
        &mut game,
        &[
            "setblock 5 100 2 glass",
            "setblock 5 101 2 redstone_wire",
            "setblock 6 101 1 comparator[facing=east,mode=subtract]",
            "setblock 6 101 2 redstone_wall_torch[facing=south]",
        ],
    );
    assert_eq!(at(&game, 6, 101, 1), "minecraft:air", "the comparator");
    assert_eq!(at(&game, 6, 101, 2), "minecraft:air", "the torch");
    assert_eq!(
        at(&game, 5, 101, 2),
        "minecraft:redstone_wire[east=side,north=side,power=0,south=side,west=side]"
    );
}

/// With nothing to take it off again the torch stays, and the wire is the line it connected.
#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn a_torch_that_stays_connects_the_wire_in_a_line_and_powers_it() {
    let mut game = game();
    run(
        &mut game,
        &[
            "setblock 5 100 2 glass",
            "setblock 5 101 2 redstone_wire",
            "setblock 6 101 2 redstone_wall_torch[facing=south]",
        ],
    );
    assert_eq!(
        at(&game, 6, 101, 2),
        "minecraft:redstone_wall_torch[facing=south,lit=true]"
    );
    assert_eq!(
        at(&game, 5, 101, 2),
        "minecraft:redstone_wire[east=side,north=none,power=15,south=none,west=side]"
    );
}

/// `handleSetCommandBlock` edits the block entity through
/// `BaseCommandBlock.setCommand`/`onUpdated`: the command block is marked
/// changed and its update tag reaches the clients. Horizon applied the edit
/// without marking the chunk.
#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn editing_a_command_block_marks_its_chunk() {
    let mut game = game();
    run(&mut game, &["setblock 5 100 2 command_block[facing=up]"]);
    game.world.modified.clear();
    if let Some(log) = game.world.block_log.as_mut() {
        log.clear();
    }
    game.apply_command_block(&super::command_blocks::CommandBlockEdit {
        pos: (5, 100, 2),
        command: "say hi".into(),
        track_output: false,
        mode: horizon_client::command_block::Mode::Redstone,
        conditional: false,
        auto: false,
    });
    assert!(
        game.world.modified.contains(&(0, 0)),
        "the edited command block's chunk is dirty"
    );
}
