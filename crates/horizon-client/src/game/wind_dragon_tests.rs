use crate::game::{Audio, Game, Options, Progress, Settings, load_resources};
use horizon_client::chunk::Chunk;
use horizon_core::geometry::Vec3;
use std::sync::Arc;

fn end_game() -> Game<'static> {
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
    options.execution_role = crate::game::execution_role::ExecutionRole::IntegratedServer;
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
    assert_eq!(
        game.enter_command_level(crate::game::Dim::End),
        Some(crate::game::Dim::Overworld)
    );
    assert!(game.dimension_type().has_ender_dragon_fight, "the End");
    for x in -1..2 {
        for z in -1..2 {
            game.world
                .chunks
                .insert((x, z), Arc::new(Chunk::empty(x, z)));
        }
    }
    assert!(game.run_command("forceload add 0 0"));
    game
}

#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn a_wind_burst_pushes_the_dragon_without_damage() {
    let mut game = end_game();
    game.spawn_dragon_spawned([0.0, 70.0, 0.0])
        .expect("spawned dragon");
    game.dragon.as_mut().expect("dragon").velocity = Vec3::ZERO;

    game.wind_burst([0.0, 66.0, 0.0], 5.0, 1.0, false, false)
        .unwrap();

    let dragon = game.dragon.as_ref().expect("dragon");
    assert_eq!(dragon.health, 200.0);
    assert!(
        dragon.velocity.y > 0.0,
        "Wind Burst did not push the dragon"
    );
}

#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn a_wind_burst_scales_the_dragons_push_by_its_multiplier() {
    let mut game = end_game();
    game.spawn_dragon_spawned([0.0, 70.0, 0.0])
        .expect("spawned dragon");
    game.dragon.as_mut().expect("dragon").velocity = Vec3::ZERO;
    game.wind_burst([0.0, 66.0, 0.0], 5.0, 1.22, false, false)
        .unwrap();

    let push = game.dragon.as_ref().expect("dragon").velocity.y;
    assert!(push > 0.0, "Wind Burst did not push the dragon");
    assert!(push > 0.6, "Wind Burst multiplier was not applied: {push}");
}
