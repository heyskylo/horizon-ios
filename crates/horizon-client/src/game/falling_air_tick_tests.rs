use super::*;
use horizon_client::chunk::Chunk;
use horizon_core::position::ChunkPos;
use horizon_world::nbt::Tag;
use std::sync::Arc;

#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES and scratch homes/cwd"]
fn loaded_air_is_discarded_on_first_actual_game_tick() {
    let root = std::path::PathBuf::from(
        std::env::var_os("HORIZON_TEST_RESOURCES").expect("prepared resources required"),
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
    game.build_registries();
    game.set_allow_commands(true);
    game.world
        .chunks
        .insert((0, 0), Arc::new(Chunk::empty(0, 0)));
    game.block_ticks
        .add_empty_chunk(ChunkPos::new(0, 0))
        .unwrap();
    assert!(game.run_command("forceload add 0 0"));
    assert!(game.run_command("fill 0 199 0 15 199 15 stone"));
    assert!(game.res.facts().is_air(game.world.block(4, 204, 4)));
    assert!(game.res.facts().is_air(game.world.block(8, 204, 4)));
    for (last, state, x) in [(1, "air", 4), (2, "sand", 8)] {
        assert!(game.run_command(&format!(
            "summon falling_block {x}.5 204 4.5 {{UUID:[I;97,0,0,{last}],BlockState:\"minecraft:{state}\",Time:0,NoGravity:1b,Motion:[0d,0d,0d]}}"
        )));
    }
    assert_eq!(
        game.falling.len(),
        2,
        "both actual command loads must be admitted"
    );
    let keys = [(0, 0)].into_iter().collect();
    let ids: Vec<_> = game.falling.iter().map(|f| f.id).collect();
    for (i, state) in ["minecraft:air", "minecraft:sand"].into_iter().enumerate() {
        let f = &game.falling[i];
        assert_eq!(&*game.res.facts().get(f.state).name, state);
        assert_eq!(f.age, 0);
        assert!(f.no_gravity);
        assert_eq!(f.velocity, Vec3::ZERO);
        assert_eq!(
            f.position,
            Vec3::new(if i == 0 { 4.5 } else { 8.5 }, 204.0, 4.5)
        );
        assert_eq!(game.uuid_for(ids[i]), [97, 0, 0, i as i32 + 1]);
        let (_, saved) = game.save_falling(i, &keys).expect("actual saved state");
        assert_eq!(saved.get("BlockState"), Some(&Tag::String(state.into())));
        assert_eq!(saved.get("Time"), Some(&Tag::Int(0)));
    }
    assert!(!game.res.facts().is_air(game.world.block(4, 199, 4)));
    let clock = game.world.game_time;
    game.tick().unwrap();
    assert_eq!(game.world.game_time, clock + 1);
    let sand = game
        .falling
        .iter()
        .find(|f| f.id == ids[1])
        .expect("nonair control survives");
    assert_eq!(&*game.res.facts().get(sand.state).name, "minecraft:sand");
    assert_eq!(sand.age, 1);
    assert!(sand.no_gravity);
    assert_eq!(sand.position, Vec3::new(8.5, 204.0, 4.5));
    assert_eq!(sand.velocity, Vec3::ZERO);
    assert!(
        !game.falling.iter().any(|f| f.id == ids[0]),
        "admitted air must disappear on its first tick"
    );
}
