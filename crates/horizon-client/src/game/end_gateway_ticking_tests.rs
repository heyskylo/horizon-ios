//! `EndGatewayBlock.entityInside` comes from the entity's own tick
//! (`Entity.applyEffectsFromBlocks`), and `ServerLevel.tick` ticks only the
//! entities in entity-ticking chunks: past the simulation distance an item in
//! an End gateway stays where it is, and only the ticking one goes in.

use super::super::Dim;
use super::super::native_machines::tests::server_game;
use horizon_client::chunk::Chunk;
use horizon_client::entities::ItemEntity;
use horizon_client::items::ItemStack;
use horizon_core::geometry::Vec3;
use std::sync::Arc;

fn item(id: u32, at: Vec3) -> ItemEntity {
    ItemEntity {
        rotation: [0.0, 0.0],
        join: Default::default(),
        no_gravity: true,
        id,
        thrower: None,
        target: None,
        stack: ItemStack {
            item: Arc::from("minecraft:white_carpet"),
            count: 1,
            damage: 0,
            extra: None,
        },
        health: 5,
        fire_ticks: 0,
        position: at,
        previous: at,
        velocity: Vec3::ZERO,
        on_ground: false,
        vertical_collision: false,
        horizontal_collision: false,
        fall_distance: 0.0,
        age: 97,
        pickup_delay: 0,
        bob_offset: 0.0,
        ticks: 97,
        needs_sync: false,
        stuck: Vec3::ZERO,
        was_touching_water: false,
    }
}

#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn items_past_the_simulation_distance_enter_no_end_gateway() {
    let mut game = server_game();
    game.dim = Dim::End;
    game.settings.simulation_distance = 4;
    game.settings.render_distance = 12;
    let gateway = game
        .pack
        .assets
        .registry
        .default_state("minecraft:end_gateway")
        .unwrap();
    // The player in chunk (0, 0); chunk (2, 0) is inside the simulation
    // distance, chunk (8, 0) loaded and in view but past it. In each, an item
    // lies in an End gateway block.
    game.player = Some(
        horizon_client::motion::PlayerMotion::new(game.motion, Vec3::new(8.5, 65.0, 8.5)).unwrap(),
    );
    for (id, cx, x) in [(1, 2, 40), (2, 8, 136)] {
        game.world
            .chunks
            .insert((cx, 0), Arc::new(Chunk::empty(cx, 0)));
        game.world.set(x, 64, 8, gateway).unwrap();
        game.dropped
            .push(item(id, Vec3::new(f64::from(x) + 0.5, 64.2, 8.5)));
    }
    game.update_chunk_tickets();
    let ticking = game.entity_ticking_area();
    assert!(ticking(40, 8), "chunk (2, 0) ticks entities");
    assert!(
        !ticking(136, 8),
        "chunk (8, 0) is past the simulation distance"
    );
    game.find_end_gateway_portals();
    assert!(
        game.end.gateways.contains_key(&1),
        "the ticking item went into its gateway"
    );
    assert!(
        !game.end.gateways.contains_key(&2),
        "the item past the simulation distance never entered its gateway"
    );
    // It waits there: the gateways' pass leaves it alone.
    game.handle_end_gateway_portals();
    assert!(game.dropped.iter().any(|d| d.id == 2));
}

/// `TheEndGatewayBlockEntity.portalTick` counts the Age up in the block entity
/// itself, so `/data get block` reads 0, then 5, then 25 (#13202), also far
/// from the player, where nothing draws the gateway.
#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn a_ticking_gateway_counts_its_age_in_its_block_entity() {
    use horizon_client::containers::BlockEntity;
    let mut game = server_game();
    game.dim = Dim::End;
    game.player = Some(
        horizon_client::motion::PlayerMotion::new(game.motion, Vec3::new(8.5, 65.0, 8.5)).unwrap(),
    );
    game.world
        .chunks
        .insert((2, 0), Arc::new(Chunk::empty(2, 0)));
    assert!(game.run_command("setblock 40 64 8 minecraft:end_gateway{Age:0L}"));
    game.update_chunk_tickets();
    assert!(game.block_ticking_area()(40, 8));
    // What `/data get block` and the chunk save encode (`TheEndGatewayBlockEntity.saveAdditional`).
    let age = |game: &super::super::Game<'_>| match game.world.block_entities.get(&(40, 64, 8)) {
        Some(BlockEntity::EndGateway(g)) => Some(g.age),
        _ => None,
    };
    assert_eq!(age(&game), Some(0));
    for _ in 0..5 {
        game.tick_block_entities().unwrap();
    }
    assert_eq!(age(&game), Some(5));
    for _ in 0..20 {
        game.tick_block_entities().unwrap();
    }
    assert_eq!(age(&game), Some(25));
}
