#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn overworld_spawnpoint_is_validated_at_both_player_blocks() {
    use crate::game::{Audio, Game, Options, Progress, Settings, load_resources};
    use horizon_client::chunk::Chunk;
    use horizon_core::geometry::Vec3;
    use std::sync::Arc;
    let root = std::path::PathBuf::from(std::env::var_os("HORIZON_TEST_RESOURCES").unwrap());
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
    let mut game = Game::new(
        &loaded.resources,
        loaded.pack,
        loaded.gui.unwrap(),
        options,
        settings,
        Audio::silent(&root),
        None,
    )
    .unwrap();
    game.player = Some(
        horizon_client::motion::PlayerMotion::new(game.motion, Vec3::new(5.5, 64.0, 5.5)).unwrap(),
    );
    game.build_registries();
    game.set_allow_commands(true);
    game.world
        .chunks
        .insert((0, 0), Arc::new(Chunk::empty(0, 0)));
    game.level.world_spawn = Some([0, 80, 0]);
    let registry = game.pack.assets.registry;
    let air = registry.default_state("minecraft:air").unwrap();
    // Liquid is a block property: aquatic plants such as kelp still permit
    // a forced point, while a bubble column does not, at feet or head.
    for (block, allowed) in [
        ("air", true),
        ("water", false),
        ("lava", false),
        ("bubble_column", false),
        ("kelp", true),
        ("stone", false),
    ] {
        let state = registry
            .default_state(&format!("minecraft:{block}"))
            .unwrap();
        for height in [0, 1] {
            game.world.set(5, 64, 5, air).unwrap();
            game.world.set(5, 65, 5, air).unwrap();
            game.world.set(5, 64 + height, 5, state).unwrap();
            assert!(game.run_command("spawnpoint @s 5 64 5"));
            let config = game
                .respawn_config
                .expect("Overworld commands must retain a forced configuration");
            assert!(config.forced);
            assert_eq!(config.pos, [5, 64, 5]);
            let actual = game.resolve_server_respawn(config);
            assert_eq!(actual.is_some(), allowed, "{block} at height {height}");
            assert_eq!(game.respawn_config.is_some(), allowed);
            assert_eq!(game.level.world_spawn, Some([0, 80, 0]));
            if let Some((at, yaw, pitch)) = actual {
                assert_eq!(at, Vec3::new(5.5, 64.1, 5.5));
                assert_eq!(yaw, 0.0);
                assert_eq!(pitch.to_bits(), 0.0f32.to_bits());
            }
        }
    }
}
