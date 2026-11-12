//! An allay dances to the jukebox it heard first: `Allay.setJukeboxPlaying`
//! keeps that jukebox's position while it dances, and only a stop from there
//! ends the dance; another jukebox's `jukebox_play` (every 20 ticks of its
//! song) starts it again (tools/test_allay_jukebox_source_parity.py measures
//! the same on vanilla 26.3).
use crate::game::*;
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
    for command in [
        "tick freeze",
        "forceload add 0 0",
        "fill 0 99 0 15 99 15 stone",
        "fill 7 99 7 9 102 9 glass",
        "fill 8 100 8 8 101 8 air",
        "setblock 5 100 8 jukebox",
        "setblock 12 100 8 jukebox",
        "summon allay 8.5 100 8.5 {PersistenceRequired:1b,UUID:[I;2577,0,0,1]}",
    ] {
        assert!(game.run_command(command), "{command}");
    }
    step(&mut game, 12);
    game
}

fn step(game: &mut Game<'_>, ticks: usize) {
    for _ in 0..ticks {
        assert!(game.run_command("tick step 1"));
        game.tick_server(false).unwrap();
    }
}

fn dancing(game: &Game<'_>) -> bool {
    game.mobs
        .iter()
        .find(|m| m.species() == horizon_client::mobs::Species::Allay)
        .expect("the allay")
        .extra
        .flag
}

fn play(game: &mut Game<'_>, x: i32, item: &str) {
    assert!(game.run_command(&format!(
        "item replace block {x} 100 8 container.0 with {item}"
    )));
}

#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn stopping_the_first_jukebox_stops_the_dance_until_the_other_plays_again() {
    let mut game = game();
    assert!(!dancing(&game));
    play(&mut game, 5, "minecraft:music_disc_cat");
    step(&mut game, 5);
    assert!(dancing(&game), "jukebox A starts the dance");
    play(&mut game, 12, "minecraft:music_disc_cat");
    step(&mut game, 25);
    assert!(dancing(&game));
    // B has played for 25 ticks: its next jukebox_play is 15 ticks away.
    play(&mut game, 5, "minecraft:air");
    step(&mut game, 1);
    assert!(
        !dancing(&game),
        "the stop of A, the dance's jukebox, ends it though B still plays"
    );
    step(&mut game, 20);
    assert!(dancing(&game), "B's next jukebox_play starts it again");
}

// Controls added after the baseline: the source through an in-place load, a
// NoAI allay, and the out-of-reach check of `aiStep`.

const ALLAY: &str = "00000a11-0000-0000-0000-000000000001";

#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn control_data_merge_keeps_the_dance_and_its_jukebox() {
    let mut game = game();
    play(&mut game, 5, "minecraft:music_disc_cat");
    step(&mut game, 5);
    play(&mut game, 12, "minecraft:music_disc_cat");
    step(&mut game, 5);
    assert!(game.run_command(&format!("data merge entity {ALLAY} {{CustomName:\"x\"}}")));
    step(&mut game, 1);
    assert!(dancing(&game), "Entity.load keeps DATA_DANCING");
    // The kept jukebox is still A: B's stop does not end the dance, A's does.
    play(&mut game, 12, "minecraft:air");
    step(&mut game, 1);
    assert!(dancing(&game));
    play(&mut game, 5, "minecraft:air");
    step(&mut game, 1);
    assert!(!dancing(&game));
}

#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn control_a_no_ai_allay_never_dances() {
    let mut game = game();
    assert!(game.run_command(&format!("data merge entity {ALLAY} {{NoAI:1b}}")));
    play(&mut game, 5, "minecraft:music_disc_cat");
    step(&mut game, 25);
    assert!(!dancing(&game));
}

#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn control_an_allay_out_of_reach_stops_on_a_twentieth_tick() {
    let mut game = game();
    play(&mut game, 5, "minecraft:music_disc_cat");
    step(&mut game, 2);
    assert!(dancing(&game));
    assert!(game.run_command("fill 7 99 7 9 102 9 air"));
    assert!(game.run_command(&format!("tp {ALLAY} 5.5 112 8.5")));
    // Farther than 10 blocks from A's centre: stopped by the next tickCount % 20.
    step(&mut game, 20);
    assert!(!dancing(&game));
}
