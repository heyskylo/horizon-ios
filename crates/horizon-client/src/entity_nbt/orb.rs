//! Experience orbs: `ExperienceOrb` save data.
use super::{Base, ExperienceOrb, Tag, flag, int_value, motion, number, position, put, rotation};

/// `ExperienceOrb`: Health, Age, Value, Count.
pub fn orb_to_nbt(o: &ExperienceOrb, uuid: [i32; 4]) -> Tag {
    let base = Base {
        id: "minecraft:experience_orb",
        uuid,
        position: o.position,
        motion: o.velocity,
        rotation: o.rotation,
        on_ground: o.on_ground,
        fall_distance: 0.0,
    };
    let mut tag = base.tag(None);
    for (key, value) in [
        ("Health", Tag::Short(o.health as i16)),
        ("Air", Tag::Short(o.air as i16)),
        ("Age", Tag::Short(o.age.min(i16::MAX as u32) as i16)),
        ("Value", Tag::Short(o.value as i16)),
        ("Count", Tag::Int(o.count.min(i32::MAX as u32) as i32)),
    ] {
        put(&mut tag, key, value);
    }
    // Entity.saveWithoutId writes NoGravity only when set.
    if o.no_gravity {
        put(&mut tag, "NoGravity", Tag::Byte(1));
    }
    tag
}
/// One orb with its merged `Count`.
pub fn orbs_from_nbt(tag: &Tag) -> Vec<ExperienceOrb> {
    let Some(position) = position(tag) else {
        return Vec::new();
    };
    let count = number(tag.get("Count")).unwrap_or(1).max(1) as u32;
    let orb = ExperienceOrb {
        rotation: rotation(tag),
        join: Default::default(),
        no_gravity: flag(tag, "NoGravity"),
        id: 0,
        position,
        previous: position,
        velocity: motion(tag),
        // `getShortOr("Value", 0)`, unclamped.
        value: tag.get("Value").and_then(int_value).unwrap_or(0) as i16 as i32,
        age: number(tag.get("Age")).unwrap_or(0).max(0) as u32,
        on_ground: flag(tag, "OnGround"),
        count,
        ticks: 0,
        health: number(tag.get("Health")).unwrap_or(5) as i32,
        air: number(tag.get("Air")).map_or(300, |air| air as i32), // getIntOr("Air", max)
        // ExperienceOrb's followingPlayer is transient: never saved, so a
        // loaded orb starts without one.
        following: None,
    };
    vec![orb]
}
