use super::*;
use horizon_core::random::LegacyRandom;
use std::sync::Arc;

#[test]
#[ignore = "requires prepared vanilla resources in .local/horizon-client"]
fn mob_tick_keeps_glow_squid_countdowns_with_no_ai() {
    let ran = crate::mobs::world::with_built_test_world(
        &[],
        |world, registry| {
            world
                .chunks
                .insert((0, 0), Arc::new(crate::chunk::Chunk::empty(0, 0)));
            let water = registry.default_state("minecraft:water").unwrap();
            for x in 0..16 {
                for y in 60..69 {
                    for z in 0..16 {
                        world.set(x, y, z, water).unwrap();
                    }
                }
            }
        },
        |world, types| {
            // Normal and negative controls run before the old NoAI positive failure.
            for no_ai in [false, true] {
                for initial in [-2, 40] {
                    let kind = Arc::new(types.types["minecraft:glow_squid"].clone());
                    let mut squid = Mob::new(1, kind, Vec3::new(8.5, 64.0, 8.5), 0.0);
                    squid.no_ai = no_ai;
                    squid.no_gravity = true;
                    squid.persistent = true;
                    set_dark_ticks(&mut squid, initial);
                    let mut random = LegacyRandom::new(0);
                    let mut events = Vec::new();
                    for elapsed in 1..=10 {
                        assert!(squid.tick(world, &mut random, &mut events));
                        assert_eq!(squid.health, 10.0);
                        let expected = if initial > 0 {
                            initial - elapsed
                        } else {
                            initial
                        };
                        assert_eq!(
                            dark_ticks(&squid),
                            expected,
                            "NoAI={no_ai}, initial={initial}, elapsed={elapsed}"
                        );
                    }
                    let glows = events
                        .iter()
                        .filter(|event| {
                            matches!(event,
                        MobEvent::ParticleAt { kind, .. } if *kind == ParticleType::Glow.name())
                        })
                        .count();
                    assert_eq!(
                        glows,
                        if no_ai { 0 } else { 10 },
                        "the timer-only NoAI path must preserve existing particle dispatch"
                    );
                }
            }
        },
    );
    assert!(
        ran,
        "prepared vanilla resources are required; do not silently skip this regression"
    );
}
