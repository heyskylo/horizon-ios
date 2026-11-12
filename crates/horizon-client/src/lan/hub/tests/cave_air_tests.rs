//! `ServerGamePacketListenerImpl.noBlocksAround` filters the player's box on
//! `BlockBehaviour.BlockStateBase::isAir`, so the air of a cave (`cave_air`) lets nothing
//! block it: a survival guest hovering in a mineshaft is floating, and the server kicks it
//! after `getMaximumFlyingTicks` (80) ticks with "Flying is not enabled on this server".
use super::*;

#[test]
fn a_guest_hovering_in_cave_air_is_kicked_for_flying() {
    // The prepared 26.3 data: the real cave_air state, its facts and collision shapes.
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../.local/horizon-client")
        .join(horizon_core::VANILLA_VERSION);
    if !root.join("cached-collision.nbt").exists() {
        return; // CI has no vanilla data.
    }
    let assets = crate::assets::Assets::load(&root).unwrap();
    let registry = assets.registry;
    let facts = crate::blocks::BlockFacts::load(&root, registry).unwrap();
    let collision = crate::collisions::load(&root, registry).unwrap();
    let air = registry.default_state("minecraft:air").unwrap();
    let cave_air = registry.default_state("minecraft:cave_air").unwrap();
    let stone = registry.default_state("minecraft:stone").unwrap();
    let (mut hub, _rx) = hub_with_a_guest();
    // The hub's state table knows the real air blocks, as the game's does.
    hub.encoder = ChunkEncoder::new(
        facts
            .states
            .iter()
            .map(|s| (s.name.clone(), s.fluid_level > 0 || s.waterlogged)),
        &horizon_protocol::registry::Builtin::default(),
        &[],
        &[],
    );
    hub.guests[0].position = [0.5, 64.0, 0.5];
    // The body's box (0.6 wide, 1.8 tall at 64) covers (0, 63..=65, 0): a cave's air.
    let mut world = World::new(air, 0);
    let mut chunk = crate::chunk::Chunk::empty(0, 0);
    for y in 63..=65 {
        chunk.set(0, y, 0, cave_air.0);
    }
    chunk.lit = true;
    world.chunks.insert((0, 0), std::sync::Arc::new(chunk));
    assert!(
        hub.no_blocks_around(0, [0.5, 64.0, 0.5], &world),
        "cave_air is air"
    );
    // One stone block in the box is not air (a cave_air block is not a hole in this test).
    world.set(0, 64, 0, stone).unwrap();
    assert!(!hub.no_blocks_around(0, [0.5, 64.0, 0.5], &world));
    world.set(0, 64, 0, cave_air).unwrap();

    // A survival guest hovering there: no step up, nothing to stand on, nothing around.
    let mut events = Vec::new();
    let border = crate::worldborder::WorldBorder::default();
    let mut next_entity = 2;
    let ctx = TickCtx {
        sea_level: 63,
        world: &mut world,
        entities: &[],
        next_entity: &mut next_entity,
        spawn: [0.5, 64.0, 0.5],
        spawn_angle: [0.0; 2],
        difficulty: 2,
        difficulty_locked: false,
        clock_rate: 1.0,
        tick_rate: 20.0,
        tick_frozen: false,
        tick_steps: Default::default(),
        collision: Some(&collision),
        border: &border,
        streaming: None,
    };
    hub.position_change(0, [0.5, 64.0, 0.5], (0.0, 0.0), 0, &ctx, &mut events);
    assert!(hub.guests[0].client_is_floating);
    // getMaximumFlyingTicks(0.08) is 80: the 81st tick is the one that kicks.
    for _ in 0..80 {
        hub.tick_floating(0, &mut events);
    }
    assert!(events.is_empty(), "80 ticks of hovering are allowed");
    hub.tick_floating(0, &mut events);
    assert!(
        matches!(&events[..], [HostEvent::Warn(line)] if line.contains("floating too long")),
        "one kick warning"
    );
    assert!(
        matches!(&hub.pending_kicks[..],
            [HostEvent::Kicked { reason, .. }] if reason.to_lowercase().contains("flying")),
        "the guest is kicked for flying"
    );
}
