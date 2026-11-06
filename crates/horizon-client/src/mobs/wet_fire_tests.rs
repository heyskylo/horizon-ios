use super::*;
use std::sync::Arc;

fn pig(types: &crate::mobs::MobTypes, no_ai: bool, immune: bool, fire: i32) -> Mob {
    let mut kind = types.types["minecraft:pig"].clone();
    kind.flags.fire_immune = immune;
    let mut mob = Mob::new(1, Arc::new(kind), Vec3::ZERO, 0.0);
    mob.no_ai = no_ai;
    mob.no_gravity = true;
    mob.persistent = true;
    mob.ai.goals = GoalSelector::default();
    mob.ai.targets = GoalSelector::default();
    mob.fire_ticks = fire;
    mob.random = LegacyRandom::new(1);
    mob
}

fn step(mob: &mut Mob, w: &MobWorld, environment: &str) -> (f32, i32, usize) {
    let mut scene = w.clone();
    scene.raining = environment == "rain";
    mob.position = if environment == "water" {
        Vec3::new(2.5, 3.0, 2.5)
    } else {
        Vec3::new(10.5, 64.0, 10.5)
    };
    mob.previous = mob.position;
    mob.velocity = Vec3::ZERO;
    assert_eq!(in_rain(mob, &scene), environment == "rain");
    let mut events = Vec::new();
    assert!(tick(mob, &scene, &mut LegacyRandom::new(2), &mut events));
    let sounds = events
        .iter()
        .filter(|event| matches!(event, MobEvent::Sound("entity.generic.extinguish_fire", ..)))
        .count();
    (mob.health, mob.fire_ticks, sounds)
}

#[test]
#[ignore = "requires prepared 26.3 resources in HORIZON_TEST_RESOURCES"]
fn wet_fire_damage_and_extinguishing_follow_the_full_mob_tick() {
    let root = std::env::var_os("HORIZON_TEST_RESOURCES").expect("HORIZON_TEST_RESOURCES");
    assert!(crate::mobs::world::with_resource_test_world(
        std::path::Path::new(&root),
        &[],
        true,
        true,
        |world, registry| {
            let biomes = crate::biome::Biomes::load(std::path::Path::new(&root)).unwrap();
            let plains = biomes.id("minecraft:plains");
            let mut chunk = crate::chunk::Chunk::empty(0, 0);
            for section in &mut chunk.sections {
                for i in 0..64 {
                    Arc::make_mut(section).biomes.set(i, u32::from(plains));
                }
            }
            world.chunks.insert((0, 0), Arc::new(chunk));
            let water = registry.default_state("minecraft:water").unwrap();
            for x in 0..=4 {
                for z in 0..=4 {
                    for y in 0..=8 {
                        world.set(x, y, z, water).unwrap();
                    }
                }
            }
        },
        |w, types| {
            let mut failures = Vec::new();
            let mut observations = 0;
            let mut check = |label: String, actual, expected| {
                observations += 1;
                println!("WET_FIRE {label} actual={actual:?} expected={expected:?}");
                if actual != expected {
                    failures.push(format!("{label}: {actual:?} != {expected:?}"));
                }
            };
            for no_ai in [true, false] {
                for environment in ["dry", "water", "rain"] {
                    for fire in [0, 1, 19, 20, 21, 40] {
                        let mut mob = pig(types, no_ai, false, fire);
                        let damage = if fire > 0 && fire % 20 == 0 { 1.0 } else { 0.0 };
                        let remaining = if environment == "dry" {
                            (fire - 1).max(0)
                        } else {
                            0
                        };
                        let sound = usize::from(environment != "dry" && fire > 1);
                        check(
                            format!("NoAI={no_ai} {environment} fire={fire}"),
                            step(&mut mob, w, environment),
                            (10.0 - damage, remaining, sound),
                        );
                    }
                }
                for wet in ["water", "rain"] {
                    let mut mob = pig(types, no_ai, false, 21);
                    for (environment, expected) in [
                        ("dry", (10.0, 20, 0)),
                        (wet, (9.0, 0, 1)),
                        ("dry", (9.0, 0, 0)),
                    ] {
                        check(
                            format!("NoAI={no_ai} transition={wet}/{environment}"),
                            step(&mut mob, w, environment),
                            expected,
                        );
                    }
                }
                for environment in ["dry", "water", "rain"] {
                    let mut mob = pig(types, no_ai, true, 20);
                    check(
                        format!("NoAI={no_ai} immune {environment}"),
                        step(&mut mob, w, environment),
                        (10.0, 0, 0),
                    );
                }
            }
            assert_eq!(observations, 54);
            assert!(failures.is_empty(), "{}", failures.join("\n"));
        }
    ));
}
