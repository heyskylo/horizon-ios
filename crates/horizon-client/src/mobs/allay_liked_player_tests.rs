//! An Allay's `minecraft:liked_player` memory and its time to live, as
//! `Brain` reads, writes and ticks it (`MemorySlot`): vanilla 26.3 counts a
//! finite memory down on each brain tick and clears it on the tick after 0;
//! `NoAI` runs no brain tick; a memory without a time to live never expires
//! (tools/test_allay_liked_player_parity.py measures the same).
use super::*;
use horizon_core::random::LegacyRandom;
use horizon_world::nbt::Tag;
use std::sync::Arc;

const OWNER: [i32; 4] = [95, 0, 0, 99];

/// The Allay as `/summon` would load it, with the memory absent (`None`),
/// permanent (`Some(None)`) or with a time to live.
fn saved_tag(memory: Option<Option<i64>>, no_ai: bool) -> Tag {
    let d = |v: f64| Tag::Double(v.to_bits());
    let mut entries = vec![
        ("id", Tag::String("minecraft:allay".into())),
        ("Pos", Tag::List(vec![d(8.5), d(200.0), d(8.5)])),
        ("NoAI", Tag::Byte(i8::from(no_ai))),
        ("NoGravity", Tag::Byte(1)),
    ];
    if let Some(ttl) = memory {
        let mut liked = vec![("value", Tag::IntArray(OWNER.to_vec()))];
        if let Some(ttl) = ttl {
            liked.push(("ttl", Tag::Long(ttl)));
        }
        entries.push((
            "Brain",
            Tag::compound([(
                "memories",
                Tag::compound([("minecraft:liked_player", Tag::compound(liked))]),
            )]),
        ));
    }
    Tag::compound(entries)
}

fn load(types: &crate::mobs::MobTypes, tag: &Tag) -> Mob {
    let kinds: std::collections::HashMap<String, Arc<crate::mobs::MobType>> = types
        .types
        .iter()
        .map(|(id, kind)| (id.clone(), Arc::new(kind.clone())))
        .collect();
    crate::entity_nbt::mob_from_nbt(tag, &kinds, 1, None).expect("an allay")
}

/// The saved memory: None when absent, else its value and time to live.
fn saved_memory(m: &Mob, raw: Option<&Tag>) -> Option<(Option<Tag>, Option<i64>)> {
    let tag = crate::entity_nbt::mob_to_nbt(m, [95, 0, 0, 1], raw, None, |_| None);
    let liked = tag
        .get("Brain")
        .and_then(|b| b.get("memories"))
        .and_then(|b| b.get("minecraft:liked_player"))?;
    let ttl = match liked.get("ttl") {
        Some(Tag::Long(t)) => Some(*t),
        None => None,
        other => panic!("ttl: {other:?}"),
    };
    Some((liked.get("value").cloned(), ttl))
}

fn value() -> Option<Tag> {
    Some(Tag::IntArray(OWNER.to_vec()))
}

/// `Brain.readAdditionalSaveData` keeps the time to live, and the save writes
/// the mob's own memory: with no raw tag at all, and over a raw tag whose
/// time to live the mob has since counted down.
#[test]
#[ignore = "needs prepared resources in .local/horizon-client"]
fn a_loaded_finite_liked_player_saves_its_own_time_to_live() {
    let ran = crate::mobs::world::with_built_test_world(
        &[],
        |_, _| {},
        |w, types| {
            for (memory, expected) in [
                (Some(Some(2)), Some((value(), Some(2)))),
                (Some(Some(0)), Some((value(), Some(0)))),
                (Some(None), Some((value(), None))),
                (None, None),
            ] {
                let tag = saved_tag(memory, false);
                let m = load(types, &tag);
                assert_eq!(saved_memory(&m, None), expected, "{memory:?}, no raw tag");
            }
            let tag = saved_tag(Some(Some(2)), false);
            let mut m = load(types, &tag);
            crate::mobs::ai::tick(&mut m, w, &mut LegacyRandom::new(1), &mut Vec::new());
            assert_eq!(
                saved_memory(&m, Some(&tag)),
                Some((value(), Some(1))),
                "stale raw ttl 2"
            );
        },
    );
    assert!(ran, "the prepared resources were not found");
}

/// `MemorySlot.tick` in the Allay's brain tick: 2, 1, 0, then gone; a memory
/// at 0 goes on the first tick; `NoAI` keeps it as it was; a permanent one
/// stays. A gone memory no longer makes the Allay like its player.
#[test]
#[ignore = "needs prepared resources in .local/horizon-client"]
fn a_finite_liked_player_expires_only_under_ai() {
    let ran = crate::mobs::world::with_built_test_world(
        &[],
        |_, _| {},
        |w, types| {
            let ttl_at = |memory, no_ai| {
                let tag = saved_tag(memory, no_ai);
                let mut m = load(types, &tag);
                let mut seen = vec![saved_memory(&m, None).map(|(_, t)| t)];
                for _ in 0..3 {
                    crate::mobs::ai::tick(&mut m, w, &mut LegacyRandom::new(1), &mut Vec::new());
                    seen.push(saved_memory(&m, None).map(|(_, t)| t));
                }
                let liked = is_liked_player(&m, Some(crate::mobs::Who::Player), OWNER);
                (seen, liked)
            };
            assert_eq!(
                ttl_at(Some(Some(2)), false),
                (
                    vec![Some(Some(2)), Some(Some(1)), Some(Some(0)), None],
                    false
                )
            );
            assert_eq!(
                ttl_at(Some(Some(0)), false),
                (vec![Some(Some(0)), None, None, None], false)
            );
            assert_eq!(ttl_at(Some(Some(2)), true), (vec![Some(Some(2)); 4], true));
            assert_eq!(ttl_at(Some(Some(0)), true), (vec![Some(Some(0)); 4], true));
            assert_eq!(ttl_at(Some(None), false), (vec![Some(None); 4], true));
            assert_eq!(ttl_at(Some(None), true), (vec![Some(None); 4], true));
            assert_eq!(ttl_at(None, false), (vec![None; 4], false));
        },
    );
    assert!(ran, "the prepared resources were not found");
}

/// `Allay.mobInteract` sets `LIKED_PLAYER` with `Brain.setMemory`, which
/// never expires, over a loaded finite memory; taking the item back erases
/// it.
#[test]
#[ignore = "needs prepared resources in .local/horizon-client"]
fn a_given_item_makes_the_liked_player_permanent_again() {
    let ran = crate::mobs::world::with_built_test_world(
        &[],
        |_, _| {},
        |w, types| {
            let tag = saved_tag(Some(Some(2)), false);
            let mut m = load(types, &tag);
            let actor = [7, 8, 9, 10];
            let stick = crate::items::ItemStack::new("minecraft:stick", 1);
            assert!(interaction::predict(&mut m, Some(&stick), true, false, actor).is_some());
            let mine = Some(Tag::IntArray(actor.to_vec()));
            assert_eq!(saved_memory(&m, Some(&tag)), Some((mine.clone(), None)));
            for _ in 0..4 {
                crate::mobs::ai::tick(&mut m, w, &mut LegacyRandom::new(1), &mut Vec::new());
            }
            assert_eq!(
                saved_memory(&m, Some(&tag)),
                Some((mine, None)),
                "never expires"
            );
            assert!(interaction::predict(&mut m, None, true, false, actor).is_some());
            assert_eq!(saved_memory(&m, Some(&tag)), None, "erased");
        },
    );
    assert!(ran, "the prepared resources were not found");
}

/// Added controls. `MemorySlot.canExpire` is `ttl != Long.MAX_VALUE`: a
/// memory saved with that time to live never expires and is saved without
/// one. A memory that expired and is given again is permanent. A new owner
/// replaces the old one with no time to live carried over.
#[test]
#[ignore = "needs prepared resources in .local/horizon-client"]
fn never_expiring_readded_and_replaced_liked_players() {
    let ran = crate::mobs::world::with_built_test_world(
        &[],
        |_, _| {},
        |w, types| {
            let step = |m: &mut Mob| {
                crate::mobs::ai::tick(m, w, &mut LegacyRandom::new(1), &mut Vec::new());
            };
            // Long.MAX_VALUE: permanent.
            let tag = saved_tag(Some(Some(i64::MAX)), false);
            let mut m = load(types, &tag);
            for _ in 0..3 {
                step(&mut m);
            }
            assert_eq!(
                saved_memory(&m, Some(&tag)),
                Some((value(), None)),
                "MAX never expires"
            );
            // Expired, then given again.
            let tag = saved_tag(Some(Some(0)), false);
            let mut m = load(types, &tag);
            step(&mut m);
            assert_eq!(saved_memory(&m, Some(&tag)), None, "expired");
            let stick = crate::items::ItemStack::new("minecraft:stick", 1);
            let readded = [1, 2, 3, 4];
            assert!(interaction::predict(&mut m, Some(&stick), true, false, readded).is_some());
            for _ in 0..3 {
                step(&mut m);
            }
            assert_eq!(
                saved_memory(&m, Some(&tag)),
                Some((Some(Tag::IntArray(readded.to_vec())), None)),
                "re-added, permanent"
            );
            // A finite owner, replaced by another player through take and give.
            let tag = saved_tag(Some(Some(2)), false);
            let mut m = load(types, &tag);
            assert!(interaction::predict(&mut m, Some(&stick), true, false, OWNER).is_some());
            assert!(interaction::predict(&mut m, None, true, false, OWNER).is_some());
            let other = [5, 6, 7, 8];
            assert!(interaction::predict(&mut m, Some(&stick), true, false, other).is_some());
            for _ in 0..3 {
                step(&mut m);
            }
            assert_eq!(
                saved_memory(&m, None),
                Some((Some(Tag::IntArray(other.to_vec())), None))
            );
            assert!(!is_liked_player(&m, Some(crate::mobs::Who::Player), OWNER));
            assert!(is_liked_player(&m, Some(crate::mobs::Who::Player), other));
        },
    );
    assert!(ran, "the prepared resources were not found");
}
