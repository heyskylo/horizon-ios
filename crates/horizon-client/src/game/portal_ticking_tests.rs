//! `Entity.handlePortal` runs in the entity's own tick, and `ServerLevel.tick`
//! ticks only the entities in entity-ticking chunks: past the simulation
//! distance a mob in a nether portal doesn't cross, and a mob cooling down
//! from one keeps its cooldown until it ticks again.

use super::native_machines::tests::server_game;
use horizon_client::chunk::Chunk;
use horizon_client::mobs::Mob;
use horizon_client::nether_portal::ENTITY_DELAY;
use horizon_core::geometry::Vec3;
use std::sync::Arc;

#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn mobs_past_the_simulation_distance_neither_enter_portals_nor_cool_down() {
    let mut game = server_game();
    game.settings.simulation_distance = 4;
    game.settings.render_distance = 12;
    let registry = game.pack.assets.registry;
    let stone = registry.default_state("minecraft:stone").unwrap();
    let portal = registry.default_state("minecraft:nether_portal").unwrap();
    let zombie = game
        .res
        .mob_types()
        .get("minecraft:zombie")
        .unwrap()
        .clone();
    // The player in chunk (0, 0); chunk (2, 0) is inside the simulation
    // distance, chunk (8, 0) loaded and in view but past it. In each, one
    // zombie stands in a portal (z 8) and one on stone, cooling down (z 12).
    game.player = Some(
        horizon_client::motion::PlayerMotion::new(&game.motion, Vec3::new(8.5, 65.0, 8.5)).unwrap(),
    );
    for (cx, x, in_portal, cooling) in [(2, 40, 1, 3), (8, 136, 2, 4)] {
        game.world
            .chunks
            .insert((cx, 0), Arc::new(Chunk::empty(cx, 0)));
        for z in [8, 12] {
            game.world.set(x, 63, z, stone).unwrap();
        }
        game.world.set(x, 64, 8, portal).unwrap();
        game.world.set(x, 65, 8, portal).unwrap();
        let at = |z: f64| Vec3::new(f64::from(x) + 0.5, 64.0, z);
        game.mobs
            .push(Mob::new(in_portal, zombie.clone(), at(8.5), 0.0));
        game.mobs
            .push(Mob::new(cooling, zombie.clone(), at(12.5), 0.0));
        game.set_portal_cooldown(cooling);
    }
    game.update_chunk_tickets();
    let ticking = game.entity_ticking_area();
    assert!(ticking(40, 8), "chunk (2, 0) ticks entities");
    assert!(
        !ticking(136, 8),
        "chunk (8, 0) is past the simulation distance"
    );
    for _ in 0..3 {
        game.handle_entity_portals();
        game.find_entity_portals();
    }
    let state = |id| game.portals.entities.get(&id);
    let here = |id| game.mobs.iter().any(|m| m.id == id);
    // A mob crosses a nether portal at once: the ticking zombie has gone.
    assert!(!here(1), "the ticking zombie went through its portal");
    assert!(here(2), "the far zombie is still here");
    assert!(
        state(2).is_none(),
        "the far zombie never entered its portal"
    );
    assert_eq!(state(3).map(|s| s.cooldown), Some(ENTITY_DELAY - 3));
    assert_eq!(state(4).map(|s| s.cooldown), Some(ENTITY_DELAY));
}
