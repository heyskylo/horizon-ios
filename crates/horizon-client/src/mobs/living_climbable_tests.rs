//! `AbstractHorse.onClimbable` and the `onClimbable` of `Ghast`, `Phantom` and
//! `HappyGhast` answer false: no horse, donkey, mule, undead horse, llama,
//! trader llama, camel or camel husk (nor those flying monsters) ever takes
//! the climbable branch of `LivingEntity` — `handleOnClimbable`'s 0.15 clamp
//! and fall reset, the 0.2 climb against a wall, the ridden fall reset of
//! `handleMoveVehicle`, and the `isPushable` veto.
use super::*;
use std::sync::Arc;

/// Every `AbstractHorse`: horses, donkeys, mules, the undead horses, camels
/// and the llamas.
const EQUINES: [&str; 9] = [
    "minecraft:horse",
    "minecraft:donkey",
    "minecraft:mule",
    "minecraft:skeleton_horse",
    "minecraft:zombie_horse",
    "minecraft:camel",
    "minecraft:camel_husk",
    "minecraft:llama",
    "minecraft:trader_llama",
];

/// The equines, then the flying monsters whose own `onClimbable` is false.
const NEVER_CLIMBS: [&str; 12] = [
    "minecraft:horse",
    "minecraft:donkey",
    "minecraft:mule",
    "minecraft:skeleton_horse",
    "minecraft:zombie_horse",
    "minecraft:camel",
    "minecraft:camel_husk",
    "minecraft:llama",
    "minecraft:trader_llama",
    "minecraft:ghast",
    "minecraft:happy_ghast",
    "minecraft:phantom",
];

/// A stone floor at y = 0 and a wall at z = 2, the column at z = 1 from y = 1
/// up holding `block` when one is given: a mob at z = 1 stands in that block.
fn wall_with(
    block: Option<&str>,
    world: &mut crate::world::World,
    registry: &horizon_world::registry::BlockRegistry,
) {
    world
        .chunks
        .insert((0, 0), Arc::new(crate::chunk::Chunk::empty(0, 0)));
    let stone = registry.default_state("minecraft:stone").unwrap();
    for x in 0..=4 {
        for z in 0..=4 {
            world.set(x, 0, z, stone).unwrap();
        }
    }
    let state = block.map(|block| {
        let mut state = registry
            .state(registry.default_state(block).unwrap())
            .unwrap()
            .clone();
        if block == "minecraft:vine" {
            state.properties.insert("south".into(), "true".into());
        }
        if block == "minecraft:ladder" {
            state.properties.insert("facing".into(), "south".into());
        }
        registry.resolve(&state).unwrap()
    });
    for x in 0..=4 {
        for y in 1..=14 {
            world.set(x, y, 2, stone).unwrap();
            if let Some(state) = state {
                world.set(x, y, 1, state).unwrap();
            }
        }
    }
}

/// `AbstractHorse.onClimbable` / `Ghast.onClimbable` / `Phantom.onClimbable` /
/// `HappyGhast.onClimbable` are a plain false: standing in vines does not make
/// any of them climbable, so the climbable branch never runs for them. A cow
/// in the same block is on a climbable, and `Spider.onClimbable` is its own
/// `isClimbing` flag, whatever the block.
#[test]
fn an_equine_and_a_flying_monster_are_never_on_a_climbable() {
    let ran = crate::mobs::world::with_built_test_world(
        &[],
        |world, registry| wall_with(Some("minecraft:vine"), world, registry),
        |w, types| {
            let at = Vec3::new(1.5, 2.0, 1.5);
            let mob = |id: &str| Mob::new(1, Arc::new(types.types[id].clone()), at, 0.0);
            // The control: a cow's block is a vine, so it is on a climbable.
            assert!(
                mob("minecraft:cow").on_climbable(w),
                "a cow in vines is on a climbable"
            );
            for id in NEVER_CLIMBS {
                let m = mob(id);
                assert!(!m.on_climbable(w), "{id} is on a climbable");
            }
            // Spider.onClimbable: the flag, not the block.
            let mut spider = mob("minecraft:spider");
            assert!(!spider.on_climbable(w), "a spider at rest is not climbing");
            spider.climbing = true;
            assert!(spider.on_climbable(w), "a climbing spider is climbable");
        },
    );
    // Without the vanilla data under .local/horizon-client the world cannot
    // be built: the test has nothing to check.
    let _ = ran;
}

/// `LivingEntity.isPushable` vetoes a mob that is `onClimbable`. A horse
/// standing in vines is still pushed by others (the cache `pushEntities`
/// refreshes each tick stays false), where a cow in the same block is not.
#[test]
fn a_horse_in_vines_is_still_pushable() {
    let ran = crate::mobs::world::with_built_test_world(
        &[],
        |world, registry| wall_with(Some("minecraft:vine"), world, registry),
        |w, types| {
            let pushable_after_a_tick = |id: &str| {
                let kind = Arc::new(types.types[id].clone());
                let mut m = Mob::new(1, kind, Vec3::new(1.5, 2.0, 1.5), 0.0);
                m.on_ground = true;
                let mut events = Vec::new();
                crate::mobs::ai::tick(
                    &mut m,
                    w,
                    &mut horizon_core::random::LegacyRandom::new(1),
                    &mut events,
                );
                (m.on_climbable, m.pushable())
            };
            let cow = pushable_after_a_tick("minecraft:cow");
            assert_eq!(cow, (true, false), "a cow in vines is held by them");
            for id in NEVER_CLIMBS {
                assert_eq!(pushable_after_a_tick(id), (false, true), "{id} in vines");
            }
        },
    );
    let _ = ran;
}

/// A horse dropped through a column of vines falls exactly as it does through
/// open air: `handleOnClimbable` never holds it to 0.15 a tick. A cow in the
/// same column slides down at 0.15, so the column is a real one.
#[test]
fn a_horse_falls_through_vines_at_full_speed() {
    let drop = |vines: bool, id: &str| {
        let mut out = None;
        let build = |world: &mut crate::world::World,
                     registry: &horizon_world::registry::BlockRegistry| {
            wall_with(vines.then_some("minecraft:vine"), world, registry);
        };
        crate::mobs::world::with_built_test_world(&[], build, |w, types| {
            let kind = Arc::new(types.types[id].clone());
            let mut m = Mob::new(1, kind, Vec3::new(1.5, 12.0, 1.1), 0.0);
            m.on_ground = false;
            let mut drops = Vec::new();
            while !m.on_ground && drops.len() < 200 {
                let before = m.position.y;
                m.travel(w, Vec3::ZERO, &mut Vec::new());
                drops.push(before - m.position.y);
            }
            out = Some(drops);
        });
        out
    };
    let (Some(open), Some(vines), Some(cow)) = (
        drop(false, "minecraft:horse"),
        drop(true, "minecraft:horse"),
        drop(true, "minecraft:cow"),
    ) else {
        return;
    };
    assert!(
        open.len() < 40 && open.iter().cloned().fold(0.0, f64::max) > 0.5,
        "open air: {open:?}"
    );
    let limit = f64::from(0.15f32);
    assert!(
        cow.len() > 3
            && cow[2..cow.len() - 1]
                .iter()
                .all(|d| (d - limit).abs() < 1.0e-9),
        "a cow in the same vines slides down at 0.15: {cow:?}"
    );
    assert_eq!(vines, open, "vines change the horse's fall");
}

/// `handleRelativeFrictionAndCalculateMovement` lifts a mob that is on a
/// climbable by 0.2 when it is pressed against a wall. A horse pushed into a
/// ladder stays at the bottom where a cow climbs it.
#[test]
fn a_horse_pressed_into_a_ladder_gains_no_height() {
    let ran = crate::mobs::world::with_built_test_world(
        &[],
        |world, registry| wall_with(Some("minecraft:ladder"), world, registry),
        |w, types| {
            let climbed = |id: &str| {
                let kind = Arc::new(types.types[id].clone());
                // In the ladder block, facing the wall at z = 2.
                let mut m = Mob::new(1, kind, Vec3::new(1.5, 1.0, 1.0), 0.0);
                m.on_ground = true;
                // The speed its move control would ask for this tick.
                m.ai.speed = 0.3;
                let start = m.position.y;
                let mut pressed = false;
                for _ in 0..10 {
                    m.travel(w, Vec3::new(0.0, 0.0, 1.0), &mut Vec::new());
                    pressed |= m.horizontal_collision;
                }
                (pressed, m.position.y - start)
            };
            let (cow_pressed, cow_rise) = climbed("minecraft:cow");
            assert!(cow_pressed, "the cow reached the wall");
            assert!(cow_rise > 0.5, "a cow climbs the ladder: {cow_rise}");
            for id in EQUINES {
                let (pressed, rise) = climbed(id);
                assert!(pressed, "{id} reached the wall");
                assert!(rise <= 0.0, "{id} climbed {rise}");
            }
        },
    );
    let _ = ran;
}
