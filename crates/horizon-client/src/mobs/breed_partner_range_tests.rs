use super::*;

/// A cow in love at (0.5, 64, 0.5) and single in-love cows at `mates`, on
/// a flat grass shelf, one tick of its goals: whether its `Breed` goal
/// took a mate, and the block it walks to. `None` without the vanilla
/// data.
fn courtship(mates: &[[f64; 3]]) -> Option<(bool, Option<[i32; 3]>)> {
    // A mate's `Seen` needs its kind, so two loads.
    let mut kind = None;
    crate::mobs::world::with_built_test_world(&[], shelf, |_, types| {
        kind = Some(Arc::new(types.types["minecraft:cow"].clone()));
    });
    let kind = kind?;
    let seen: Vec<Seen> = mates
        .iter()
        .enumerate()
        .map(|(i, [x, y, z])| {
            let mut p = Mob::new(i as u32 + 2, kind.clone(), Vec3::new(*x, *y, *z), 0.0);
            p.extra.in_love = 600;
            p.seen(None)
        })
        .collect();
    let mut out = None;
    crate::mobs::world::with_built_test_world(&seen, shelf, |w, _| {
        let mut m = Mob::new(1, kind.clone(), Vec3::new(0.5, 64.0, 0.5), 0.0);
        m.extra.in_love = 600;
        m.on_ground = true;
        let mut lr = LegacyRandom::new(0);
        let mut events = Vec::new();
        let mut cx = Cx {
            w,
            lr: &mut lr,
            events: &mut events,
        };
        let mut goals = std::mem::take(&mut m.ai.goals);
        goals.tick(&mut m, &mut cx);
        m.ai.goals = goals;
        out = Some((m.ai.goals.is_running("Breed"), m.ai.nav.target_pos));
    });
    out
}

/// A flat grass shelf the cows stand and walk on.
fn shelf(world: &mut crate::world::World, registry: &horizon_world::registry::BlockRegistry) {
    let grass = registry.default_state("minecraft:grass_block").unwrap();
    for cx in -3..3 {
        for cz in -3..3 {
            world
                .chunks
                .insert((cx, cz), Arc::new(crate::chunk::Chunk::empty(cx, cz)));
        }
    }
    for x in -16..=16 {
        for z in -16..=16 {
            world.set(x, 63, z, grass).unwrap();
        }
    }
}

/// `BreedGoal.getFreePartner` searches the box `getBoundingBox().inflate(8.0)`
/// but keeps only what `PARTNER_TARGETING` (`forNonCombat().range(8.0)`)
/// passes, and `TargetingConditions.test` rejects a target whose
/// `distanceToSqr` is above 8 squared.
#[test]
fn a_mate_outside_the_eight_block_sphere_is_ignored() {
    // 7,0,5 apart: squared 74, inside the box, outside the sphere.
    let Some((running, target)) = courtship(&[[7.5, 64.0, 5.5]]) else {
        return;
    };
    assert!(!running, "a mate 8.6 blocks away is out of range");
    assert_eq!(target, None, "it does not walk to it");
    // Just outside: 8 on x and 0.6 on z, squared 64.36.
    let Some((running, target)) = courtship(&[[8.5, 64.0, 1.1]]) else {
        return;
    };
    assert!(!running, "a mate just past eight blocks is out of range");
    assert_eq!(target, None, "it does not walk to it");
}

/// The other half of the same predicate: the sphere keeps a diagonal mate
/// and one exactly eight blocks away, and the nearest of two still wins.
#[test]
fn a_mate_inside_the_eight_block_sphere_is_courted() {
    // 5,0,5 apart: squared 50.
    let Some((running, target)) = courtship(&[[5.5, 64.0, 5.5]]) else {
        return;
    };
    assert!(running, "a diagonal mate 7.1 blocks away is in range");
    let [x, _, z] = target.expect("it walks toward its mate");
    assert!((x, z) == (5, 5), "it heads for its mate, got {x} {z}");
    // Exactly eight: squared 64 is not above the limit.
    let Some((running, target)) = courtship(&[[8.5, 64.0, 0.5]]) else {
        return;
    };
    assert!(running, "a mate exactly eight blocks away is in range");
    let [x, _, z] = target.expect("it walks toward its mate");
    assert!((x, z) == (8, 0), "it heads for its mate, got {x} {z}");
    // Two mates: the nearer (5,5) wins over the one at exactly eight.
    let Some((running, target)) = courtship(&[[8.5, 64.0, 0.5], [5.5, 64.0, 5.5]]) else {
        return;
    };
    assert!(running);
    let [x, _, z] = target.expect("it walks toward its mate");
    assert!((x, z) == (5, 5), "the nearest mate, got {x} {z}");
}
