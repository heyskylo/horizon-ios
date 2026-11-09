//! `LevelChunk.registerTickContainerInLevel`: a chunk's block and fluid tick
//! containers are registered when it becomes a full chunk, before a command
//! can reach it. Water placed in a newly loaded chunk of a frozen level,
//! before the level's next world tick, keeps its flow tick
//! (tools/test_frozen_first_tick_water_parity.py measures the same on
//! vanilla 26.3: it flows 5 ticks after placement).
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
    game
}

/// The fluid ticks queued in chunk `key`, or None when the chunk has no
/// fluid tick container.
fn fluid_ticks(game: &Game<'_>, key: (i32, i32)) -> Option<usize> {
    game.block_ticks
        .fluids
        .chunk(ChunkPos::new(key.0, key.1))
        .map(|c| c.count())
}

/// A chunk that becomes resident is registered with both tick schedulers by
/// the time the next command runs, with no world tick in between.
#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn water_placed_before_the_first_world_tick_keeps_its_flow_tick() {
    let mut game = game();
    let key = (80, 80);
    assert!(game.run_command("forceload add 1280 1280"));
    // The streaming worker's chunk, made resident as `streaming.update` does.
    game.world
        .chunks
        .insert(key, Arc::new(Chunk::empty(key.0, key.1)));
    assert_eq!(fluid_ticks(&game, key), None, "not registered yet");
    let time = game.world.game_time;
    game.update_world();
    assert!(
        game.world.chunks.contains_key(&key),
        "the chunk stays loaded"
    );
    assert_eq!(game.world.game_time, time, "no world tick ran");
    assert!(game.run_command("setblock 1283 100 1283 stone"));
    assert!(game.run_command("setblock 1283 101 1283 water"));
    assert_eq!(
        fluid_ticks(&game, key),
        Some(1),
        "the water's flow tick is queued"
    );
    assert!(
        game.block_ticks
            .blocks
            .chunk(ChunkPos::new(key.0, key.1))
            .is_some(),
        "the block tick container too"
    );
}

/// Registration takes a chunk's saved ticks with their saved delays (they
/// count down from the first world tick that unpacks them, as before), runs
/// no world tick and draws nothing from the level random, and a second
/// `update_world` registers nothing again.
#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn early_registration_keeps_saved_ticks_and_ticks_nothing() {
    use horizon_core::random::RandomSource;
    use horizon_core::ticks::{SavedTick, TickPriority, TickTypeId};
    let mut game = game();
    let key = (80, 80);
    assert!(game.run_command("forceload add 1280 1280"));
    let saved = SavedTick {
        kind: TickTypeId(horizon_world::fluid::FluidKind::Water.index() as u32),
        pos: horizon_core::position::BlockPos::new(1285, 101, 1285),
        delay: 3,
        priority: TickPriority::Normal,
    };
    let mut chunk = Chunk::empty(key.0, key.1);
    chunk.saved = Some(Arc::new(horizon_client::chunk::ChunkExtras {
        fluid_ticks: vec![saved],
        ..Default::default()
    }));
    game.world.chunks.insert(key, Arc::new(chunk));
    let time = game.world.game_time;
    let random = game.world.random.clone().next_long();
    game.update_world();
    game.update_world();
    assert_eq!(game.world.game_time, time, "no world tick ran");
    assert_eq!(
        game.world.random.clone().next_long(),
        random,
        "no level random draw"
    );
    let ticks = game
        .block_ticks
        .fluids
        .chunk(horizon_core::position::ChunkPos::new(key.0, key.1))
        .expect("registered");
    assert_eq!(
        ticks.pack(time),
        vec![saved],
        "the saved tick, its delay unchanged"
    );
    let extras = game.world.chunks[&key].saved.clone().unwrap();
    assert!(
        extras.fluid_ticks.is_empty(),
        "the scheduler holds the saved tick now"
    );
}

/// `ServerLevel.startTickingChunk` unpacks a chunk's saved ticks at the
/// game time it starts ticking (`unpackTicks(getGameTime())`; Horizon reads
/// it as `game_time - 1` inside the world tick). Registering the chunk
/// earlier, in `update_world`, leaves that unpack where it was: a saved block
/// tick with delay 2 and a saved fluid tick with delay 3, registered at game
/// time T, trigger at T - 1 + 2 and T - 1 + 3 once the chunk starts ticking.
#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn saved_block_and_fluid_ticks_unpack_when_the_chunk_starts_ticking() {
    use horizon_core::ticks::{SavedTick, TickPriority, TickTypeId};
    let mut game = game();
    let key = (80, 80);
    assert!(game.run_command("forceload add 1280 1280"));
    let at = |y| horizon_core::position::BlockPos::new(1285, y, 1285);
    let block = SavedTick {
        kind: TickTypeId(1),
        pos: at(100),
        delay: 2,
        priority: TickPriority::Normal,
    };
    let fluid = SavedTick {
        kind: TickTypeId(horizon_world::fluid::FluidKind::Water.index() as u32),
        pos: at(101),
        delay: 3,
        priority: TickPriority::Normal,
    };
    let mut chunk = Chunk::empty(key.0, key.1);
    chunk.saved = Some(Arc::new(horizon_client::chunk::ChunkExtras {
        block_ticks: vec![block],
        fluid_ticks: vec![fluid],
        ..Default::default()
    }));
    // `ticking_chunk`: a lit chunk whose 3x3 neighbours are loaded.
    chunk.lit = true;
    for dx in -1..=1 {
        for dz in -1..=1 {
            if (dx, dz) != (0, 0) {
                let (x, z) = (key.0 + dx, key.1 + dz);
                game.world
                    .chunks
                    .insert((x, z), Arc::new(Chunk::empty(x, z)));
            }
        }
    }
    game.world.chunks.insert(key, Arc::new(chunk));
    game.world.game_time = 500;
    game.update_world();
    game.update_chunk_tickets();
    assert!(
        super::ticking_chunk(&game.world, key.0, key.1),
        "a ticking chunk"
    );
    assert!(
        game.tickets.levels().loaded_ticking(key.0, key.1),
        "a ticking ticket level"
    );
    let pos = ChunkPos::new(key.0, key.1);
    assert_eq!(
        game.block_ticks.blocks.chunk(pos).unwrap().pack(500),
        vec![block],
        "waiting"
    );
    game.start_ticking_chunks();
    let triggers = |ticks: &horizon_core::ticks::ChunkTicks| {
        ticks
            .queued()
            .iter()
            .map(|t| (t.pos, t.trigger_tick))
            .collect::<Vec<_>>()
    };
    assert_eq!(
        triggers(game.block_ticks.blocks.chunk(pos).unwrap()),
        vec![(at(100), 501)]
    );
    assert_eq!(
        triggers(game.block_ticks.fluids.chunk(pos).unwrap()),
        vec![(at(101), 502)]
    );
}

/// The unload route saves a chunk's queued ticks before its queue goes
/// (`with_entities` -> `attach_save_data`, then the removal), and the chunk
/// loaded back from that save gets them again. Registering twice, or a chunk
/// that left and came back before `update_world`, queues nothing twice.
#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn an_unloaded_chunk_saves_its_ticks_and_a_reloaded_one_gets_them_back() {
    let mut game = game();
    let key = (80, 80);
    let pos = ChunkPos::new(key.0, key.1);
    assert!(game.run_command("forceload add 1280 1280"));
    game.world
        .chunks
        .insert(key, Arc::new(Chunk::empty(key.0, key.1)));
    game.update_world();
    assert!(game.run_command("setblock 1283 100 1283 stone"));
    assert!(game.run_command("setblock 1283 101 1283 water"));
    assert_eq!(fluid_ticks(&game, key), Some(1));
    // Left and back before the next update_world: the same queue, once.
    let resident = game.world.chunks.remove(&key).unwrap();
    game.world.chunks.insert(key, resident);
    game.update_world();
    game.update_world();
    assert_eq!(fluid_ticks(&game, key), Some(1), "no second registration");
    // The save an unloaded chunk is written with.
    let mut saved = (*game.world.chunks[&key]).clone();
    game.attach_save_data(&mut saved);
    let ticks = saved.saved.as_deref().unwrap().fluid_ticks.clone();
    assert_eq!(ticks.len(), 1, "the water's tick goes into the save");
    assert_eq!(
        ticks[0].pos,
        horizon_core::position::BlockPos::new(1283, 101, 1283)
    );
    assert_eq!(ticks[0].delay, 5, "saved relative to the save time");
    // Unloaded: the queue goes with the chunk.
    game.world.chunks.remove(&key);
    game.update_world();
    assert!(
        game.block_ticks.fluids.chunk(pos).is_none(),
        "dropped after the save"
    );
    // Loaded back from that save.
    game.world.chunks.insert(key, Arc::new(saved));
    game.update_world();
    assert_eq!(
        game.block_ticks
            .fluids
            .chunk(pos)
            .unwrap()
            .pack(game.world.game_time),
        ticks
    );
}
