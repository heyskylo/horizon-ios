//! An experience orb is a score holder: `execute as <orb> store … score @s`
//! stores into the score named by the orb's UUID, as an armor stand's does
//! (tools/test_orb_score_store_parity.py measures the same on vanilla 26.3).
use super::*;
use horizon_client::chunk::Chunk;
use horizon_core::position::ChunkPos;
use std::sync::Arc;

const ORB: &str = "00000000-0000-0000-0000-0000000000a7";
const STAND: &str = "00000000-0000-0000-0000-0000000000a8";

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
    assert!(game.run_command("scoreboard objectives add os dummy"));
    assert!(game.run_command(
        "summon experience_orb 4.5 100 8.5 {UUID:[I;0,0,0,167],Value:7s,Count:2,Age:0s,Health:5s,NoGravity:1b,Motion:[0d,0d,0d]}"
    ));
    assert!(game.run_command("summon armor_stand 10.5 100 8.5 {UUID:[I;0,0,0,168],NoGravity:1b}"));
    // The orb is the source, and its getter works.
    assert!(game.run_command(&format!(
        "execute as {ORB} if entity @s[type=experience_orb]"
    )));
    assert!(game.run_command(&format!("execute as {ORB} run data get entity @s Value")));
    game
}

fn score(game: &Game<'_>, name: &str) -> Option<i32> {
    game.score.board.score(name, "os").map(|s| s.value)
}

/// `execute as <subject> store <kind> score @s os run data get entity @s <getter>`
/// from 31: whether it ran and the subject's score after it.
fn store(game: &mut Game<'_>, subject: &str, kind: &str, getter: &str) -> (bool, Option<i32>) {
    assert!(game.run_command(&format!("scoreboard players set {subject} os 31")));
    let ok = game.run_command(&format!(
        "execute as {subject} store {kind} score @s os run data get entity @s {getter}"
    ));
    (ok, score(game, subject))
}

#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn an_orb_stores_into_its_own_score() {
    let mut game = game();
    // The supported holder: the armor stand's own score.
    assert_eq!(store(&mut game, STAND, "result", "Air"), (true, Some(300)));
    assert_eq!(store(&mut game, STAND, "success", "Air"), (true, Some(1)));
    // The orb's own score, by its UUID.
    assert_eq!(store(&mut game, ORB, "result", "Value"), (true, Some(7)));
    assert_eq!(store(&mut game, ORB, "success", "Value"), (true, Some(1)));
}
