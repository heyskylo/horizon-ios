//! A new mob's yaw as its vanilla spawn path sets it (#12973): `/summon`
//! without NBT loads the mob from `{id:..}` and keeps yaw 0 with no
//! level-random draw (`SummonCommand.createEntity`), and `EntityType.create`
//! (spawn eggs, a dispenser's eggs, mob buckets) wraps its draw with
//! `Mth.wrapDegrees`.
use super::creatures::SpawnYaw;
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
    for command in [
        "forceload add 0 0",
        "fill 0 99 0 15 99 15 stone",
        "tick freeze",
    ] {
        assert!(game.run_command(command), "{command}");
    }
    game
}

fn yaw_of(game: &Game<'_>, id: u32) -> f32 {
    game.mobs.iter().find(|m| m.id == id).expect("spawned").yaw
}

#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn a_summoned_mob_without_nbt_faces_south() {
    let mut game = game();
    let before = game.mobs.len();
    assert!(game.run_command("summon pig 4.5 100 8.5"));
    let pig = &game.mobs[before];
    assert_eq!(pig.yaw, 0.0);
}

#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn a_spawn_eggs_yaw_is_the_wrapped_level_draw() {
    let mut game = game();
    // Draws until one lands in [180, 360), where wrapping changes the value.
    let mut checked = false;
    for _ in 0..32 {
        let mut level = game.world.random.clone();
        let draw = level.next_float() * 360.0;
        let id = game
            .spawn_mob_spawned("pig", [4.5, 100.0, 8.5], SpawnYaw::Created)
            .expect("pig");
        let yaw = yaw_of(&game, id);
        assert_eq!(yaw, horizon_client::camera::wrap_degrees(draw));
        assert!((-180.0..180.0).contains(&yaw), "{yaw}");
        if draw >= 180.0 {
            checked = true;
            break;
        }
    }
    assert!(checked, "no draw of 180 or more in 32 spawns");
}

#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn a_dispensed_mob_takes_the_wrapped_draw_too() {
    let mut game = game();
    let mut level = game.world.random.clone();
    let draw = level.next_float() * 360.0;
    let id = game.spawn_mob_at("pig", (4, 100, 8), false).expect("pig");
    assert_eq!(
        yaw_of(&game, id),
        horizon_client::camera::wrap_degrees(draw)
    );
}
