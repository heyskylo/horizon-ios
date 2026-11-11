use super::*;

/// Two llamas in love, two blocks apart on a flat grass shelf, ticked by
/// the AI until the first breeds: its own events, with the `mob_drops`
/// game rule at `drops`. `None` without the vanilla data.
fn llama_events(drops: bool) -> Option<Vec<MobEvent>> {
    // The partner's `Seen` needs its kind, so two loads.
    let mut kind = None;
    crate::mobs::world::with_built_test_world_rules(&[], true, drops, shelf, |_, types| {
        kind = Some(Arc::new(types.types["minecraft:llama"].clone()));
    });
    let kind = kind?;
    let mut partner = Mob::new(2, kind.clone(), Vec3::new(2.5, 64.0, 0.5), 0.0);
    // AbstractHorse.canParent: only tame llamas breed.
    partner.extra.tame = true;
    partner.extra.in_love = 600;
    let seen = [partner.seen(None)];
    let mut out = None;
    crate::mobs::world::with_built_test_world_rules(&seen, true, drops, shelf, |w, _| {
        let mut m = Mob::new(1, kind, Vec3::new(0.5, 64.0, 0.5), 0.0);
        m.extra.in_love = 600;
        m.extra.tame = true;
        let mut lr = LegacyRandom::new(0);
        let mut events = Vec::new();
        for _ in 0..600 {
            crate::mobs::ai::tick(&mut m, w, &mut lr, &mut events);
            if events
                .iter()
                .any(|e| matches!(e, MobEvent::Breed { parent: 1, .. }))
            {
                break;
            }
        }
        out = Some(events);
    });
    out
}

/// A flat grass shelf two llamas can stand on.
fn shelf(world: &mut crate::world::World, registry: &horizon_world::registry::BlockRegistry) {
    let grass = registry.default_state("minecraft:grass_block").unwrap();
    for cx in -3..3 {
        for cz in -3..3 {
            world
                .chunks
                .insert((cx, cz), Arc::new(crate::chunk::Chunk::empty(cx, cz)));
        }
    }
    for x in -8..=8 {
        for z in -8..=8 {
            world.set(x, 63, z, grass).unwrap();
        }
    }
}

/// `finalizeSpawnChildFromBreeding` makes the experience draw only inside
/// the `mob_drops` check, from the breeding parent's own random: the
/// breed goal pushes one 1-7 orb with the rule on, none with it off, and
/// the pair breeds either way.
#[test]
fn the_breed_xp_follows_the_mob_drops_rule() {
    let Some(on) = llama_events(true) else {
        return;
    };
    let Some(off) = llama_events(false) else {
        return;
    };
    let orb = |events: &[MobEvent]| {
        events.iter().find_map(|e| match e {
            MobEvent::Experience { amount, .. } => Some(*amount),
            _ => None,
        })
    };
    assert!(
        orb(&on).is_some_and(|a| (1..=7).contains(&a)),
        "mob_drops on: one orb of 1-7, got {:?}",
        orb(&on)
    );
    assert!(orb(&off).is_none(), "mob_drops off: no experience draw");
    assert!(
        on.iter()
            .any(|e| matches!(e, MobEvent::Breed { parent: 1, .. })),
        "mob_drops on: the pair breeds"
    );
    assert!(
        off.iter()
            .any(|e| matches!(e, MobEvent::Breed { parent: 1, .. })),
        "mob_drops off: the pair still breeds"
    );
}
