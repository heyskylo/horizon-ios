//! A loaded sulfur cube's body primes it one tick after it is loaded (H249):
//! `SulfurCube.tick` checks the power (`primeWhenOnPoweredPosition`, which
//! needs the body's `explosionData`) before `super.tick()` adopts the body
//! (`detectEquipmentUpdates` → `collectEquipmentChanges`), so a cube loaded
//! with a TNT body on a redstone block primes on its second tick
//! (tools/test_sulfur_prime_phase_parity.py measures the same on vanilla 26.3).
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

fn fuse(game: &Game<'_>) -> i32 {
    let cube = game
        .mobs
        .iter()
        .find(|m| m.species() == horizon_client::mobs::Species::SulfurCube)
        .expect("the cube");
    horizon_client::mobs::sulfur_cube::fuse(cube)
}

fn step(game: &mut Game<'_>) {
    assert!(game.run_command("tick step 1"));
    game.tick_server(false).unwrap();
}

/// A NoAI cube with a TNT body summoned at 4.5 100 8.5, on `under`.
fn cube_on(game: &mut Game<'_>, under: &str) {
    assert!(game.run_command("forceload add 0 0"));
    assert!(game.run_command("fill 0 98 0 15 99 15 stone"));
    if under != "stone" {
        assert!(game.run_command(&format!("setblock 4 99 8 {under}")));
    }
    for command in [
        "tick freeze".to_owned(),
        "summon sulfur_cube 4.5 100 8.5 {NoAI:1b,Silent:1b,UUID:[I;45641,0,0,1],\
         equipment:{body:{id:\"minecraft:tnt\",count:1}}}"
            .to_owned(),
    ] {
        assert!(game.run_command(&command), "{command}");
    }
    assert_eq!(fuse(game), -1, "not primed when loaded");
}

#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn a_cube_loaded_with_a_tnt_body_on_power_primes_on_its_second_tick() {
    let mut game = game();
    cube_on(&mut game, "redstone_block");
    step(&mut game);
    assert_eq!(fuse(&game), -1, "its body is adopted after the power check");
    step(&mut game);
    assert_eq!(fuse(&game), 120);
    step(&mut game);
    assert_eq!(fuse(&game), 119);
}

#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn a_warmed_cube_primes_on_the_first_powered_tick() {
    let mut game = game();
    cube_on(&mut game, "stone");
    for _ in 0..5 {
        step(&mut game);
    }
    assert_eq!(fuse(&game), -1, "no power");
    assert!(game.run_command("setblock 5 100 8 redstone_block"));
    step(&mut game);
    assert_eq!(fuse(&game), 120);
}
