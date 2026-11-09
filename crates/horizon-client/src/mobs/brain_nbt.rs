//! `Brain.Packed`'s saved memories for the mobs whose state Horizon models.
//!
//! `LivingEntity.addAdditionalSaveData` stores every memory the mob's brain
//! keeps under `Brain.memories`, an `ExpirableValue` each: a value and an
//! optional long TTL. These functions merge those memories into the compound a
//! mob was loaded with, and drop the ones the live state no longer has.
use super::Mob;
use horizon_world::nbt::{Compound, Tag};

/// `finalizeSpawn`'s memories (`m.extra.memories`) as `Brain.Packed` writes
/// them: a `Unit` empty compound, a boolean, an integer, or a global position
/// for `home`, with the remaining TTL when it is finite.
pub fn put_brain(map: &mut Compound, m: &Mob, dimension: &str) {
    if m.extra.memories.is_empty() {
        return;
    }
    let mut memories = match map.get(&"Brain".into()).and_then(|b| b.get("memories")) {
        Some(Tag::Compound(c)) => c.clone(),
        _ => Compound::new(),
    };
    for &(name, value, ttl) in &m.extra.memories {
        let value = match name {
            "hunted_recently" => Tag::Byte(value as i8),
            // `Unit` memories save an empty compound.
            "is_pregnant" => Tag::Compound(Compound::new()),
            "home" => match m.extra.home {
                Some(p) => Tag::compound([
                    ("dimension", Tag::String(dimension.into())),
                    ("pos", Tag::IntArray(p.to_vec())),
                ]),
                None => continue,
            },
            _ => Tag::Int(value as i32),
        };
        let mut memory = vec![("value", value)];
        if ttl != i64::MAX {
            memory.push(("ttl", Tag::Long(ttl)));
        }
        memories.insert(
            format!("minecraft:{name}").as_str().into(),
            Tag::compound(memory),
        );
    }
    map.insert(
        "Brain".into(),
        Tag::compound([("memories", Tag::Compound(memories))]),
    );
}

/// `Frog`'s brain memory `IS_PREGNANT`, which `Brain.Packed` writes beside the
/// other memories and `mob_from_nbt` reads back into the frog's memories. The
/// memory is a `Unit`: its value is an empty compound, with no TTL.
pub fn write_frog(m: &Mob, map: &mut Compound) {
    let brain = map
        .entry("Brain".into())
        .or_insert_with(|| Tag::Compound(Compound::new()));
    if !matches!(brain, Tag::Compound(_)) {
        *brain = Tag::Compound(Compound::new());
    }
    let Tag::Compound(brain) = brain else { return };
    let memories = brain
        .entry("memories".into())
        .or_insert_with(|| Tag::Compound(Compound::new()));
    if !matches!(memories, Tag::Compound(_)) {
        *memories = Tag::Compound(Compound::new());
    }
    let Tag::Compound(memories) = memories else {
        return;
    };
    if super::frog::is_pregnant(m) {
        memories.insert(
            "minecraft:is_pregnant".into(),
            Tag::compound([("value", Tag::Compound(Compound::new()))]),
        );
    } else {
        memories.remove(&"minecraft:is_pregnant".into());
    }
}
