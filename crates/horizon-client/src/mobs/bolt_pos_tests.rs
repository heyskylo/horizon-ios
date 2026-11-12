//! `RunAroundLikeCrazyGoal.canUse`: `DefaultRandomPos.getPos(horse, 5, 4)`,
//! ten attempts whose spot must be inside the build height, within the home,
//! a stable destination and of no pathfinding malus, the best walk-target
//! value winning; `start` walks to the spot. A raw offset lands inside a hill
//! or over a pond instead.
use super::*;
use horizon_core::geometry::Vec3;
use horizon_core::random::{EntityRandom, LegacyRandom};
use std::sync::Arc;

const SEEDS: i64 = 200;

/// A shelf of stone with a hill (north-west) and a two-deep pond
/// (south-east) on it, and an untamed horse with a mob on its back.
fn shelf(world: &mut crate::world::World, registry: &horizon_world::registry::BlockRegistry) {
    for (cx, cz) in [(-1, -1), (-1, 0), (-1, 1), (0, -1), (0, 0), (0, 1)] {
        world
            .chunks
            .insert((cx, cz), Arc::new(crate::chunk::Chunk::empty(cx, cz)));
    }
    let stone = registry.default_state("minecraft:stone").unwrap();
    let water = registry.default_state("minecraft:water").unwrap();
    for x in -16..16 {
        for z in -16..16 {
            world.set(x, 63, z, stone).unwrap();
        }
    }
    // A hill two blocks above the shelf, and a pond two blocks into it.
    for x in -5..-2 {
        for z in -5..-2 {
            for y in 64..66 {
                world.set(x, y, z, stone).unwrap();
            }
        }
    }
    for x in 3..6 {
        for z in 3..6 {
            for y in 64..66 {
                world.set(x, y, z, water).unwrap();
            }
        }
    }
}

fn horse(seed: i64, types: &crate::mobs::MobTypes) -> Mob {
    let kind = Arc::new(types.types["minecraft:horse"].clone());
    let mut m = Mob::with_random(
        1,
        kind,
        Vec3::new(0.5, 64.0, 0.5),
        0.0,
        EntityRandom::own(LegacyRandom::new(seed)),
    );
    m.on_ground = true;
    // `isVehicle`: a mob's back is enough to bolt.
    m.extra.rider = Some(9);
    m
}

/// The spot the bolt picks is a dry, standable cell (`isOutsideLimits`,
/// `isRestricted`, `isNotStable` and `hasMalus` all pass it) reached by
/// `RandomPos.generateRandomDirection`'s x, y, z draws, ten times: no further
/// and no fewer, so a horse never bolts into the hill or onto the pond.
#[test]
fn a_bucking_horse_bolts_only_where_the_filters_allow() {
    use crate::mobs::world::with_built_test_world;
    let mut out = None;
    if !with_built_test_world(&[], shelf, |w, types| {
        let mut trials = Vec::new();
        for seed in 0..SEEDS {
            let mut m = horse(seed, types);
            // The thirty draws of vanilla's ten attempts (2h+1 = 11, 2v+1 = 9).
            let mut expected = m.random.clone();
            for _ in 0..10 {
                let _ = expected.next_int_bound(11);
                let _ = expected.next_int_bound(9);
                let _ = expected.next_int_bound(11);
            }
            let mut lr = LegacyRandom::new(seed);
            let mut events = Vec::new();
            let mut cx = Cx {
                w,
                lr: &mut lr,
                events: &mut events,
            };
            let mut goal = RunAroundLikeCrazy::new();
            if !goal.can_use(&mut m, &mut cx) {
                continue;
            }
            let probe = |r: &mut LegacyRandom| {
                (0..4)
                    .map(|_| r.next_int_bound(1 << 20).unwrap_or(0))
                    .collect::<Vec<_>>()
            };
            let draws = (probe(&mut m.random), probe(&mut expected));
            goal.start(&mut m, &mut cx);
            let target = m.ai.nav.target_pos.map(|p| {
                let cell = w.facts.get(w.world.block(p[0], p[1], p[2]));
                (
                    cell.name.to_string(),
                    w.solid(p[0], p[1], p[2]),
                    w.solid(p[0], p[1] - 1, p[2]),
                )
            });
            trials.push((seed, draws, target));
        }
        out = Some(trials);
    }) {
        return;
    }
    let trials = out.unwrap();
    assert!(
        trials.len() >= 10,
        "the horse only bolted {} times of {SEEDS}",
        trials.len()
    );
    for (seed, draws, target) in &trials {
        assert_eq!(
            draws.0, draws.1,
            "seed {seed}: the bolt's ten attempts must draw what vanilla's do"
        );
        match target {
            None => panic!("seed {seed}: bolts to a spot it cannot walk to"),
            Some((name, solid, floor)) => assert!(
                name != "minecraft:water" && !*solid && *floor,
                "seed {seed}: bolts into {name} (solid {solid}, floor {floor})"
            ),
        }
    }
}
