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
    scene.raining = environment == "rain" || environment.ends_with("_rain");
    mob.position = if environment == "water" {
        Vec3::new(2.5, 3.0, 2.5)
    } else if environment.starts_with("lava_") {
        Vec3::new(7.5, 64.2, 2.5)
    } else if environment.starts_with("fire_") {
        Vec3::new(12.5, 64.2, 2.5)
    } else {
        Vec3::new(10.5, 64.0, 10.5)
    };
    mob.previous = mob.position;
    mob.velocity = Vec3::ZERO;
    assert_eq!(in_rain(mob, &scene), scene.raining);
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
            let lava = registry.default_state("minecraft:lava").unwrap();
            for x in 6..=8 {
                for z in 0..=4 {
                    world.set(x, 64, z, lava).unwrap();
                }
            }
            world
                .set(12, 64, 2, registry.default_state("minecraft:fire").unwrap())
                .unwrap();
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
            for no_ai in [true, false] {
                for environment in ["lava_dry", "lava_rain", "fire_dry", "fire_rain"] {
                    for fire in [0, 1, 20] {
                        let mut mob = pig(types, no_ai, false, fire);
                        let actual = step(&mut mob, w, environment);
                        println!(
                            "WET_FIRE NoAI={no_ai} {environment} fire={fire} actual={actual:?}"
                        );
                    }
                }
                let mut mob = pig(types, no_ai, false, 0);
                for _ in 0..2 {
                    let actual = step(&mut mob, w, "lava_rain");
                    println!("WET_FIRE NoAI={no_ai} lava_rain_repeat fire=0 actual={actual:?}");
                }
            }
            // Check the RNG state at the actual contact/extinguish boundary:
            // silent new ignition consumes no pitch draws; an existing burn
            // consumes exactly the two draws used by vanilla's hiss.
            let mut rain = w.clone();
            rain.raining = true;
            for x in [7.5, 12.5] {
                for fire in [0, 1, 20] {
                    let mut mob = pig(types, true, false, fire);
                    mob.position = Vec3::new(x, 64.2, 2.5);
                    mob.previous = mob.position;
                    assert!(in_rain(&mob, &rain));
                    let mut events = Vec::new();
                    let was_on_fire = base_tick(&mut mob, &rain, &mut events);
                    assert_eq!(was_on_fire, fire > 1);
                    let mut expected_random = mob.random.clone();
                    let sounds = usize::from(fire > 1);
                    for _ in 0..sounds * 2 {
                        expected_random.next_float();
                    }
                    events.clear();
                    extinguish_if_wet(&mut mob, &rain, was_on_fire, &mut events);
                    assert_eq!(mob.fire_ticks, 0);
                    assert_eq!(events.len(), sounds);
                    assert_eq!(mob.random.next_long(), expected_random.next_long());
                }
            }
        }
    ));
}
