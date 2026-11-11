//! `LivingEntity.FallFlying` on a mob: read and written by the entity tag,
//! and checked by `updateFallFlying` in every `aiStep`, with or without AI.
//! The expected flags are vanilla 26.3's, measured on loaded husks summoned
//! with `FallFlying:1b` (tools/test_mob_glide_state_parity.py).
use super::*;
use horizon_world::nbt::Tag;
use std::sync::Arc;

fn husk(types: &crate::mobs::MobTypes, no_ai: bool, chest_damage: Option<u32>) -> Mob {
    let kind = Arc::new(types.types["minecraft:husk"].clone());
    let mut m = Mob::new(1, kind, Vec3::new(8.5, 300.0, 8.5), 0.0);
    m.no_ai = no_ai;
    m.persistent = true;
    m.random.set_seed(1);
    m.extra.fall_flying = true;
    if let Some(damage) = chest_damage {
        let mut stack = crate::items::ItemStack::new("minecraft:elytra", 1);
        stack.damage = damage;
        crate::mobs::finalize::set_item_stack(&mut m, crate::mobs::finalize::CHEST, stack);
    }
    m
}

fn tick_once(m: &mut Mob, w: &MobWorld) {
    let mut level_random = LegacyRandom::new(2);
    let mut events = Vec::new();
    assert!(
        tick(m, w, &mut level_random, &mut events),
        "still in the level"
    );
}

/// `LivingEntity.readAdditionalSaveData`: `setSharedFlag(7, FallFlying)`.
#[test]
#[ignore = "needs prepared resources in .local/horizon-client"]
fn a_loaded_mob_keeps_its_saved_glide_flag() {
    let ran = crate::mobs::world::with_built_test_world(
        &[],
        |_, _| {},
        |_, types| {
            let kinds: std::collections::HashMap<String, Arc<crate::mobs::MobType>> = types
                .types
                .iter()
                .map(|(id, kind)| (id.clone(), Arc::new(kind.clone())))
                .collect();
            for (saved, gliding) in [(Some(1), true), (Some(0), false), (None, false)] {
                let mut entries = vec![
                    ("id", Tag::String("minecraft:husk".into())),
                    (
                        "Pos",
                        Tag::List(vec![
                            Tag::Double(8.5f64.to_bits()),
                            Tag::Double(300f64.to_bits()),
                            Tag::Double(8.5f64.to_bits()),
                        ]),
                    ),
                ];
                if let Some(flag) = saved {
                    entries.push(("FallFlying", Tag::Byte(flag)));
                }
                let m = crate::entity_nbt::mob_from_nbt(&Tag::compound(entries), &kinds, 1, None)
                    .expect("a husk");
                assert_eq!(m.extra.fall_flying, gliding, "FallFlying {saved:?}");
            }
        },
    );
    assert!(ran, "the prepared resources were not found");
}

/// `LivingEntity.addAdditionalSaveData`: `putBoolean(FallFlying, isFallFlying())`,
/// whatever the tag the mob was loaded from said.
#[test]
#[ignore = "needs prepared resources in .local/horizon-client"]
fn a_saved_mob_writes_its_current_glide_flag() {
    let ran = crate::mobs::world::with_built_test_world(
        &[],
        |_, _| {},
        |_, types| {
            let flag = |tag: &Tag| tag.get("FallFlying").cloned();
            let raw = Tag::compound([("FallFlying", Tag::Byte(1))]);
            for (gliding, raw, expected) in [
                (true, None, 1),
                (false, Some(&raw), 0),
                (true, Some(&raw), 1),
            ] {
                let mut m = husk(types, true, None);
                m.extra.fall_flying = gliding;
                let tag = crate::entity_nbt::mob_to_nbt(&m, [168, 0, 0, 1], raw, None, |_| None);
                assert_eq!(
                    flag(&tag),
                    Some(Tag::Byte(expected)),
                    "gliding {gliding}, raw {raw:?}"
                );
            }
        },
    );
    assert!(ran, "the prepared resources were not found");
}

/// `LivingEntity.updateFallFlying`: in the air, a mob with no glider, or one
/// whose next damage breaks it, stops gliding on its first tick, NoAI or not;
/// a whole elytra keeps the flag.
#[test]
#[ignore = "needs prepared resources in .local/horizon-client"]
fn a_mob_that_cannot_glide_stops_gliding_on_its_first_tick() {
    let ran = crate::mobs::world::with_built_test_world(
        &[],
        |_, _| {},
        |w, types| {
            for no_ai in [false, true] {
                for (chest, gliding) in [(None, false), (Some(431), false), (Some(0), true)] {
                    let mut m = husk(types, no_ai, chest);
                    tick_once(&mut m, w);
                    assert!(!m.on_ground, "the husk is in the air");
                    assert_eq!(
                        m.extra.fall_flying, gliding,
                        "NoAI {no_ai}, elytra damage {chest:?}"
                    );
                }
            }
        },
    );
    assert!(ran, "the prepared resources were not found");
}

/// `LivingEntity.canGlide`: on the ground, riding, or levitating, a mob with
/// a whole elytra stops gliding too, with or without AI.
#[test]
#[ignore = "needs prepared resources in .local/horizon-client"]
fn a_grounded_riding_or_levitating_mob_stops_gliding() {
    let ran = crate::mobs::world::with_built_test_world(
        &[],
        |_, _| {},
        |w, types| {
            for no_ai in [false, true] {
                let mut grounded = husk(types, no_ai, Some(0));
                grounded.on_ground = true;
                let mut riding = husk(types, no_ai, Some(0));
                riding.extra.mount = Some(99);
                let mut levitating = husk(types, no_ai, Some(0));
                levitating.add_effect("minecraft:levitation", 200, 0);
                for (what, mut m) in [
                    ("grounded", grounded),
                    ("riding", riding),
                    ("levitating", levitating),
                ] {
                    let mut events = Vec::new();
                    update_fall_flying(&mut m, w, &mut events);
                    assert!(!m.extra.fall_flying, "{what}, NoAI {no_ai}");
                    assert!(events.is_empty(), "{what}: no wear");
                }
            }
        },
    );
    assert!(ran, "the prepared resources were not found");
}

/// `updateFallFlying` wears the glider on the 20th and 40th glide ticks of
/// the mob's own tick and on no other; a mob that stops gliding counts from
/// 0 again.
#[test]
#[ignore = "needs prepared resources in .local/horizon-client"]
fn a_glider_wears_on_the_twentieth_and_fortieth_glide_ticks() {
    use horizon_world::player_inventory::EquipmentSlot;
    let ran = crate::mobs::world::with_built_test_world(
        &[],
        |_, _| {},
        |w, types| {
            let mut m = husk(types, true, Some(0));
            let mut worn_at = Vec::new();
            for t in 1..=40 {
                let mut events = Vec::new();
                assert!(tick(&mut m, w, &mut LegacyRandom::new(2), &mut events));
                for e in &events {
                    if let MobEvent::GliderWear { mob, slot } = e {
                        assert_eq!((*mob, *slot), (1, EquipmentSlot::Chest));
                        worn_at.push(t);
                    }
                }
                assert!(
                    m.extra.fall_flying,
                    "tick {t}: a whole elytra keeps gliding"
                );
                assert_eq!(m.extra.fall_fly_ticks, t, "tick {t}");
            }
            assert_eq!(worn_at, [20, 40]);
            m.on_ground = true;
            tick_once(&mut m, w);
            assert!(!m.extra.fall_flying);
            assert_eq!(m.extra.fall_fly_ticks, 0, "the count starts again");
        },
    );
    assert!(ran, "the prepared resources were not found");
}

/// `Util.getRandom(slotsWithGliders, this.random)`: a wear tick takes exactly
/// one `nextInt(size)` from the mob's own random and picks that slot; the
/// other glide ticks, and a refused glide, draw nothing.
#[test]
#[ignore = "needs prepared resources in .local/horizon-client"]
fn the_wear_draws_once_from_the_mobs_own_random() {
    use horizon_world::player_inventory::EquipmentSlot;
    let ran = crate::mobs::world::with_built_test_world(
        &[],
        |_, _| {},
        |w, types| {
            let mut two = husk(types, true, Some(0));
            crate::mobs::finalize::set_item_stack(
                &mut two,
                crate::mobs::finalize::HEAD,
                crate::items::ItemStack::new("minecraft:elytra", 1),
            );
            // A head elytra is not in its `equippable` slot: only the chest glides.
            for (ticks, seed, wears) in [
                (19, 7, true),
                (39, 7, true),
                (9, 7, false),
                (18, 7, false),
                (20, 7, false),
            ] {
                let mut m = husk(types, true, Some(0));
                m.extra.fall_fly_ticks = ticks;
                m.random.set_seed(seed);
                let mut expected = m.random.clone();
                let mut events = Vec::new();
                update_fall_flying(&mut m, w, &mut events);
                if wears {
                    assert_eq!(expected.next_int_bound(1), Ok(0));
                    assert_eq!(events.len(), 1, "count {ticks}");
                } else {
                    assert!(events.is_empty(), "count {ticks}");
                }
                assert_eq!(m.random.next_long(), expected.next_long(), "count {ticks}");
            }
            two.extra.fall_fly_ticks = 19;
            let mut expected = two.random.clone();
            let mut events = Vec::new();
            update_fall_flying(&mut two, w, &mut events);
            assert!(matches!(
                events[..],
                [MobEvent::GliderWear {
                    slot: EquipmentSlot::Chest,
                    ..
                }]
            ));
            let _ = expected.next_int_bound(1);
            assert_eq!(two.random.next_long(), expected.next_long());
            let mut refused = husk(types, true, None);
            refused.extra.fall_fly_ticks = 19;
            let mut expected = refused.random.clone();
            let mut events = Vec::new();
            update_fall_flying(&mut refused, w, &mut events);
            assert!(events.is_empty() && !refused.extra.fall_flying);
            assert_eq!(
                refused.random.next_long(),
                expected.next_long(),
                "no draw when refused"
            );
        },
    );
    assert!(ran, "the prepared resources were not found");
}

/// `canGlideUsing` refuses a glider whose next damage breaks it, so the one
/// point `updateFallFlying` takes (Unbreaking can only lower it) never breaks
/// the glider it picked: checked for every damage of the elytra's own
/// maximum.
#[test]
#[ignore = "needs prepared resources in .local/horizon-client"]
fn the_wear_of_a_glide_never_breaks_the_glider() {
    let ran = crate::mobs::world::with_built_test_world(
        &[],
        |_, _| {},
        |w, _| {
            let items = w.items.expect("the item data");
            let max = items.get("minecraft:elytra").expect("elytra").max_damage;
            assert_eq!(max, 432);
            let mut eligible = 0;
            for damage in 0..max {
                let mut stack = crate::items::ItemStack::new("minecraft:elytra", 1);
                stack.damage = damage;
                if crate::item_sync::can_glide_using(&stack, "chest", items) {
                    eligible += 1;
                    assert!(damage + 1 < max, "damage {damage} would break");
                }
            }
            assert_eq!(eligible, max - 1, "0 to 430 glide, 431 does not");
        },
    );
    assert!(ran, "the prepared resources were not found");
}
