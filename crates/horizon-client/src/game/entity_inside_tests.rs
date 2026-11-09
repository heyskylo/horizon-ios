//! `Entity.applyEffectsFromBlocks` runs in the entity's own tick, and
//! `ServerLevel.tick` ticks only the entities in entity-ticking chunks: a mob
//! past the simulation distance presses no plate, however long it stands on it.

use super::Game;
use super::native_machines::tests::server_game;
use horizon_client::chunk::Chunk;
use horizon_client::mobs::Mob;
use horizon_core::geometry::Vec3;
use std::sync::Arc;

#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn a_mob_presses_a_plate_only_inside_the_simulation_distance() {
    let mut game = server_game();
    game.settings.simulation_distance = 4;
    game.settings.render_distance = 12;
    let registry = game.pack.assets.registry;
    let stone = registry.default_state("minecraft:stone").unwrap();
    let plate = registry
        .default_state("minecraft:stone_pressure_plate")
        .unwrap();
    let zombie = game
        .res
        .mob_types()
        .get("minecraft:zombie")
        .unwrap()
        .clone();
    // The player in chunk (0, 0); a zombie on a plate in chunk (2, 0), inside
    // the simulation distance, and another in chunk (8, 0): loaded and in
    // view, but past it.
    game.player = Some(
        horizon_client::motion::PlayerMotion::new(&game.motion, Vec3::new(8.5, 65.0, 8.5)).unwrap(),
    );
    for (cx, x, id) in [(2, 40, 1), (8, 136, 2)] {
        game.world
            .chunks
            .insert((cx, 0), Arc::new(Chunk::empty(cx, 0)));
        game.world.set(x, 63, 8, stone).unwrap();
        game.world.set(x, 64, 8, plate).unwrap();
        game.mobs.push(Mob::new(
            id,
            zombie.clone(),
            Vec3::new(f64::from(x) + 0.5, 64.0, 8.5),
            0.0,
        ));
    }
    game.update_chunk_tickets();
    let ticking = game.entity_ticking_area();
    assert!(ticking(40, 8), "chunk (2, 0) ticks entities");
    assert!(
        !ticking(136, 8),
        "chunk (8, 0) is past the simulation distance"
    );
    for _ in 0..3 {
        game.tick_blocks().unwrap();
    }
    let powered = |game: &Game<'_>, x: i32| {
        let state = game.world.block(x, 64, 8);
        registry
            .state(state)
            .unwrap()
            .properties
            .get("powered")
            .cloned()
    };
    assert_eq!(powered(&game, 40).as_deref(), Some("true"));
    assert_eq!(powered(&game, 136).as_deref(), Some("false"));
}
