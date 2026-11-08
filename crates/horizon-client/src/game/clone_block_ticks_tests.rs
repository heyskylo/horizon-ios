//! `CloneCommands.clone` ends with `LevelTicks.copyAreaFrom`: every block
//! tick pending in the source box is scheduled at the same offset in the
//! destination, with its trigger time, whatever the clone placed there
//! (tools/test_clone_block_ticks_parity.py measures the same on vanilla 26.3).
use super::*;
use horizon_client::chunk::Chunk;
use horizon_core::position::{BlockPos, ChunkPos};
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
    // A loaded chunk whose block tick queue is registered.
    game.world
        .chunks
        .insert((0, 0), Arc::new(Chunk::empty(0, 0)));
    game.block_ticks
        .add_empty_chunk(ChunkPos::new(0, 0))
        .unwrap();
    assert!(game.run_command("fill 0 99 0 15 99 15 stone"));
    game
}

/// The block ticks queued at `pos`: (block, trigger tick).
fn ticks_at(game: &Game<'_>, pos: BlockPos) -> Vec<(u32, i64)> {
    game.block_ticks
        .blocks
        .chunk(ChunkPos::new(0, 0))
        .unwrap()
        .queued()
        .iter()
        .filter(|t| t.pos == pos)
        .map(|t| (t.kind.0, t.trigger_tick))
        .collect()
}

#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn a_strict_clone_copies_the_pending_block_ticks_of_its_source() {
    let mut game = game();
    let source = BlockPos::new(1, 101, 1);
    assert!(game.run_command("setblock 1 101 1 sand"));
    let pending = ticks_at(&game, source);
    assert_eq!(pending.len(), 1, "the placed sand's fall is queued");
    // Strict: the clone places the sand without its own tick.
    assert!(game.run_command("clone 1 101 1 1 101 1 3 101 1 strict replace"));
    assert_eq!(
        ticks_at(&game, BlockPos::new(3, 101, 1)),
        pending,
        "copied as it was"
    );
    assert_eq!(ticks_at(&game, source), pending, "the source keeps its own");
    // Filtered: the sand's tick comes with it, the dirt below has none.
    assert!(game.run_command("setblock 5 100 1 dirt"));
    assert!(game.run_command("setblock 5 101 1 sand"));
    let pending = ticks_at(&game, BlockPos::new(5, 101, 1));
    assert!(game.run_command("clone 5 100 1 5 101 1 7 100 1 strict filtered minecraft:sand"));
    assert_eq!(ticks_at(&game, BlockPos::new(7, 101, 1)), pending);
    assert_eq!(ticks_at(&game, BlockPos::new(7, 100, 1)), vec![]);
    // Move: the copy is made after the source was cleared.
    assert!(game.run_command("setblock 9 101 1 sand"));
    let pending = ticks_at(&game, BlockPos::new(9, 101, 1));
    assert!(game.run_command("clone 9 101 1 9 101 1 11 101 1 strict replace move"));
    assert_eq!(ticks_at(&game, BlockPos::new(11, 101, 1)), pending);
}

/// Added controls (after the baseline). A destination that already holds a
/// tick for the same block keeps that one (`LevelChunkTicks.schedule` keeps
/// one tick per position and type). A copied tick keeps its priority and
/// comes after the ticks queued before it. The copy is made before the
/// command fails on an empty count, as `copyAreaFrom` is called before
/// `ERROR_FAILED`.
#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn copied_block_ticks_keep_order_priority_and_dedup() {
    let mut game = game();
    let queued = |game: &Game<'_>, pos: BlockPos| {
        game.block_ticks
            .blocks
            .chunk(ChunkPos::new(0, 0))
            .unwrap()
            .queued()
            .iter()
            .filter(|t| t.pos == pos)
            .copied()
            .collect::<Vec<_>>()
    };
    // The destination's own tick stays the only one.
    assert!(game.run_command("setblock 7 101 5 sand"));
    let own = queued(&game, BlockPos::new(7, 101, 5));
    assert_eq!(own.len(), 1);
    assert!(game.run_command("setblock 5 101 5 sand"));
    assert!(game.run_command("clone 5 101 5 5 101 5 7 101 5 strict replace"));
    assert_eq!(
        queued(&game, BlockPos::new(7, 101, 5)),
        own,
        "kept, not doubled"
    );
    // Priority kept; ordered after what was queued before.
    assert!(game.run_command("setblock 1 101 9 sand"));
    let source = queued(&game, BlockPos::new(1, 101, 9))[0];
    assert!(game.run_command("clone 1 101 9 1 101 9 3 101 9 strict replace"));
    let copied = queued(&game, BlockPos::new(3, 101, 9));
    assert_eq!(copied.len(), 1);
    assert_eq!(
        (copied[0].kind, copied[0].trigger_tick, copied[0].priority),
        (source.kind, source.trigger_tick, source.priority)
    );
    assert!(copied[0].sub_tick_order > source.sub_tick_order);
    // A clone that places nothing (a barrier) fails, its tick copied first.
    assert!(game.run_command("setblock 9 101 9 sand"));
    let pending = queued(&game, BlockPos::new(9, 101, 9));
    assert!(game.run_command("setblock 9 101 9 barrier strict"));
    assert_eq!(
        queued(&game, BlockPos::new(9, 101, 9)),
        pending,
        "the tick stays"
    );
    assert!(!game.run_command("clone 9 101 9 9 101 9 11 101 9 strict replace"));
    let moved: Vec<_> = queued(&game, BlockPos::new(11, 101, 9))
        .iter()
        .map(|t| (t.kind, t.trigger_tick))
        .collect();
    assert_eq!(moved, vec![(pending[0].kind, pending[0].trigger_tick)]);
}
