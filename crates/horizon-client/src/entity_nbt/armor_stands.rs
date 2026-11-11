//! Armor stand save and load: `ArmorStand.addAdditionalSaveData` and `armor_stand_from_nbt`.
use super::*;
pub use crate::entities::ArmorStand;

/// ArmorStand.addAdditionalSaveData with LivingEntity's `equipment`.
pub fn armor_stand_to_nbt(a: &ArmorStand, uuid: [i32; 4], raw: Option<&Tag>) -> Tag {
    let base = Base {
        id: "minecraft:armor_stand",
        uuid,
        position: a.position,
        motion: a.velocity,
        rotation: [a.yaw, a.pitch],
        on_ground: a.on_ground,
        fall_distance: a.enchantment_fall_distance,
    };
    let mut tag = base.non_mob_tag(raw);
    let equipment: Vec<(&str, Tag)> = ArmorStand::SLOTS
        .iter()
        .zip(&a.equipment)
        .filter_map(|(slot, stack)| Some((*slot, item_to_nbt(stack.as_ref()?))))
        .collect();
    // `equipment` is saved only when something is worn, `Marker` only when set.
    if equipment.is_empty() {
        if let Tag::Compound(map) = &mut tag {
            map.remove(&"equipment".into());
        }
    } else {
        put(&mut tag, "equipment", Tag::compound(equipment));
    }
    put(&mut tag, "DisabledSlots", Tag::Int(a.disabled_slots));
    for (key, value) in [
        ("Small", a.small),
        ("ShowArms", a.show_arms),
        ("NoBasePlate", a.no_base_plate),
        ("Invisible", a.invisible),
    ] {
        put(&mut tag, key, Tag::Byte(i8::from(value)));
    }
    if a.marker {
        put(&mut tag, "Marker", Tag::Byte(1));
    } else if let Tag::Compound(map) = &mut tag {
        map.remove(&"Marker".into());
    }
    // Entity.saveWithoutId writes NoGravity only when set.
    if a.no_gravity {
        put(&mut tag, "NoGravity", Tag::Byte(1));
    } else if let Tag::Compound(map) = &mut tag {
        map.remove(&"NoGravity".into());
    }
    if a.name_visible {
        put(&mut tag, "CustomNameVisible", Tag::Byte(1));
    } else if let Tag::Compound(map) = &mut tag {
        map.remove(&"CustomNameVisible".into());
    }
    // A name from the save keeps its own shape unless it changed.
    if let Some(name) = &a.custom_name
        && !tag.get("CustomName").is_some_and(|t| name_text(t) == *name)
    {
        put(
            &mut tag,
            "CustomName",
            crate::scoreboard::json_to_tag(&name.component()),
        );
    }
    put(&mut tag, "Health", Tag::Float(a.health.to_bits()));
    put(&mut tag, "Fire", Tag::Short(a.fire_ticks as i16));
    // ArmorStand.addAdditionalSaveData: the `Pose` parts that are not default.
    let pose: Vec<(&str, Tag)> = ArmorStand::POSE_KEYS
        .into_iter()
        .zip(a.pose)
        .zip(ArmorStand::DEFAULT_POSE)
        .filter(|((_, rotation), default)| rotation != default)
        .map(|((key, rotation), _)| (key, floats(&rotation)))
        .collect();
    put(&mut tag, "Pose", Tag::compound(pose));
    // LivingEntity.addAdditionalSaveData and ArmorStand's own fields.
    if let Tag::Compound(map) = &mut tag {
        for (key, value) in [
            ("HurtTime", Tag::Short(0)),
            ("DeathTime", Tag::Short(0)),
            ("AbsorptionAmount", Tag::Float(0)),
            ("FallFlying", Tag::Byte(0)),
            ("current_impulse_context_reset_grace_time", Tag::Int(0)),
            ("attributes", Tag::List(Vec::new())),
            (
                "Brain",
                Tag::compound([("memories", Tag::Compound(Compound::new()))]),
            ),
            ("DisabledSlots", Tag::Int(0)),
            ("Pose", Tag::Compound(Compound::new())),
        ] {
            map.entry(key.into()).or_insert(value);
        }
        // `removeFrost`'s first-tick movement_speed, over a saved list without it.
        if a.speed_instance
            && let Some(Tag::List(list)) = map.get_mut(&"attributes".into())
            && !list
                .iter()
                .any(|t| text(t.get("id")).as_deref() == Some(SPEED_ATTRIBUTE))
        {
            list.push(Tag::compound([
                ("id", string(SPEED_ATTRIBUTE)),
                ("base", Tag::Double(0.7f64.to_bits())),
            ]));
        }
    }
    tag
}

/// The attribute every living entity's first tick makes (`removeFrost`).
const SPEED_ATTRIBUTE: &str = "minecraft:movement_speed";

/// Vanilla 26.3 `Entity.setXRot`: finite pitch is clamped after modulo 360,
/// non-finite pitch resets to 0.0.
fn normalize_pitch(pitch: f32) -> f32 {
    if pitch.is_finite() {
        (pitch % 360.0).clamp(-90.0, 90.0)
    } else {
        0.0
    }
}

pub fn armor_stand_from_nbt(tag: &Tag, id: u32) -> Option<ArmorStand> {
    let rot = rotation(tag);
    let mut a = ArmorStand::new(id, position(tag)?, rot[0]);
    a.pitch = normalize_pitch(rot[1]);
    a.enchantment_fall_distance = float(tag.get("fall_distance")).unwrap_or(0.0);
    // AttributeMap.load makes an instance for each saved attribute.
    a.speed_instance = matches!(tag.get("attributes"), Some(Tag::List(list))
        if list.iter().any(|attr| text(attr.get("id")).as_deref() == Some(SPEED_ATTRIBUTE)));
    a.velocity = loaded_motion(tag);
    a.health = float(tag.get("Health")).unwrap_or(20.0) as f32;
    a.fire_ticks = i32::from(number(tag.get("Fire")).unwrap_or(0) as i16);
    a.custom_name = tag.get("CustomName").map(name_text);
    a.name_visible = flag(tag, "CustomNameVisible");
    if let Some(eq) = tag.get("equipment") {
        for (i, slot) in ArmorStand::SLOTS.iter().enumerate() {
            a.equipment[i] = eq.get(slot).and_then(item_from_nbt);
        }
    }
    a.small = flag(tag, "Small");
    a.show_arms = flag(tag, "ShowArms");
    a.disabled_slots = number(tag.get("DisabledSlots")).unwrap_or(0) as i32;
    a.no_base_plate = flag(tag, "NoBasePlate");
    a.invisible = flag(tag, "Invisible");
    a.marker = flag(tag, "Marker");
    a.no_gravity = flag(tag, "NoGravity");
    a.on_ground = flag(tag, "OnGround");
    // ArmorStand.readAdditionalSaveData: each part of `Pose` that is a list of
    // three numbers, the others staying at their defaults.
    if let Some(pose) = tag.get("Pose") {
        for (rotation, key) in a.pose.iter_mut().zip(ArmorStand::POSE_KEYS) {
            if let Some(Tag::List(values)) = pose.get(key)
                && values.len() == 3
            {
                for (r, v) in rotation.iter_mut().zip(values) {
                    *r = float(Some(v)).unwrap_or(0.0) as f32;
                }
            }
        }
    }
    Some(a)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn armor_stand_loads_and_saves_pitch() {
        let tag = Tag::compound([
            ("id", string("minecraft:armor_stand")),
            (
                "Pos",
                Tag::List(vec![
                    Tag::Double(4103.5f64.to_bits()),
                    Tag::Double(300.0f64.to_bits()),
                    Tag::Double(4103.5f64.to_bits()),
                ]),
            ),
            (
                "Rotation",
                Tag::List(vec![
                    Tag::Float((-170.5f32).to_bits()),
                    Tag::Float((-90.0f32).to_bits()),
                ]),
            ),
        ]);
        let stand = armor_stand_from_nbt(&tag, 1).unwrap();
        assert_eq!(stand.yaw, -170.5);
        assert_eq!(stand.pitch, -90.0);

        let saved = armor_stand_to_nbt(&stand, [1, 2, 3, 4], None);
        let rot = rotation(&saved);
        assert_eq!(rot[0], -170.5);
        assert_eq!(rot[1], -90.0);

        // Out-of-range pitch (e.g. 450.0f) normalizes to 90.0f matching Entity.setXRot.
        let out_of_range = Tag::compound([
            ("id", string("minecraft:armor_stand")),
            (
                "Pos",
                Tag::List(vec![
                    Tag::Double(0.0f64.to_bits()),
                    Tag::Double(0.0f64.to_bits()),
                    Tag::Double(0.0f64.to_bits()),
                ]),
            ),
            (
                "Rotation",
                Tag::List(vec![
                    Tag::Float(0.0f32.to_bits()),
                    Tag::Float(450.0f32.to_bits()),
                ]),
            ),
        ]);
        let normalized = armor_stand_from_nbt(&out_of_range, 2).unwrap();
        assert_eq!(normalized.pitch, 90.0);

        // Nonfinite pitch resets to 0.0f.
        let nonfinite = Tag::compound([
            ("id", string("minecraft:armor_stand")),
            (
                "Pos",
                Tag::List(vec![
                    Tag::Double(0.0f64.to_bits()),
                    Tag::Double(0.0f64.to_bits()),
                    Tag::Double(0.0f64.to_bits()),
                ]),
            ),
            (
                "Rotation",
                Tag::List(vec![
                    Tag::Float(0.0f32.to_bits()),
                    Tag::Float(f32::NAN.to_bits()),
                ]),
            ),
        ]);
        let nan_stand = armor_stand_from_nbt(&nonfinite, 3).unwrap();
        assert_eq!(nan_stand.pitch, 0.0);
    }
}
