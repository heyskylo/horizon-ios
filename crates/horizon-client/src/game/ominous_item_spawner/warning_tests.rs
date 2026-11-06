use horizon_core::geometry::Vec3;

#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn warning_dispatch_uses_neutral_at_tick_four() {
    use crate::game::{Audio, Game, Options, Progress, Settings, load_resources};
    use horizon_client::motion::PlayerMotion;
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
    let scratch = std::env::temp_dir().join(format!(
        "horizon-ominous-sound-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&scratch).unwrap();
    game.world_root = scratch.clone();
    game.player = Some(PlayerMotion::new(game.motion, Vec3::new(4.5, 100.0, 4.5)).unwrap());

    const WARNING: &str = "minecraft:block.trial_spawner.about_to_spawn_item";
    let center = [6.5, 102.5, 4.5];
    game.audio.capture = Some(Vec::new());
    game.audio
        .play("minecraft:block.stone.break", Some(center), 1.0, 1.0);
    game.audio.play_on(
        6,
        "minecraft:block.note_block.pling",
        Some(center),
        1.0,
        1.0,
    );
    assert_eq!(
        game.audio
            .capture
            .as_ref()
            .unwrap()
            .iter()
            .map(|s| s.source)
            .collect::<Vec<_>>(),
        [4, 6]
    );
    game.audio.capture.as_mut().unwrap().clear();
    let (tag, _) = horizon_world::nbt::parse_snbt(r#"{id:"minecraft:ominous_item_spawner",UUID:[I;128,0,0,1],Pos:[6.25d,102.75d,4.875d],item:{id:"minecraft:diamond",count:1},spawn_item_after_ticks:40L}"#).unwrap();
    game.load_ominous_item_spawner(tag);
    assert_eq!(game.ominous_spawners.len(), 1);
    let s = &game.ominous_spawners[0];
    assert_eq!(s.uuid, [128, 0, 0, 1]);
    assert_eq!(s.position, [6.25, 102.75, 4.875]);
    assert_eq!(&*s.item.item, "minecraft:diamond");
    assert_eq!(s.item.count, 1);
    assert_eq!(s.spawn_item_after, 40);
    assert_eq!(s.tick_count, 0);
    for tick in 1..=5 {
        game.tick_ominous_item_spawners();
        assert_eq!(game.ominous_spawners[0].tick_count, tick);
        let warning: Vec<_> = game
            .audio
            .capture
            .as_ref()
            .unwrap()
            .iter()
            .filter(|s| s.event == WARNING)
            .collect();
        assert_eq!(warning.len(), if tick >= 4 { 1 } else { 0 });
        if tick == 4 {
            let sound = warning[0];
            assert_eq!(sound.position, center);
            assert_eq!(sound.volume.to_bits(), 1.0f32.to_bits());
            assert_eq!(sound.pitch.to_bits(), 1.0f32.to_bits());
            assert_eq!(
                sound.source, 6,
                "OminousItemSpawner.tickServer explicitly selects NEUTRAL"
            );
        }
    }
    game.audio
        .play("minecraft:block.stone.break", Some(center), 1.0, 1.0);
    assert_eq!(
        game.audio.capture.as_ref().unwrap().last().unwrap().source,
        4,
        "the explicit warning source must not change later event-family inference"
    );
    drop(game);
    std::fs::remove_dir_all(scratch).unwrap();
}
