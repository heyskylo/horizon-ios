//! Block-attached hanging entities: paintings and item frames.
use super::*;

/// The pitch `Entity.load` leaves on a hanging entity: `setXRot` ignores a
/// non-finite value and otherwise clamps `xRot % 360` to +-90. `Painting` and
/// `HangingEntity.setDirection` only write the yaw, so the loaded pitch is
/// kept and `Entity.saveWithoutId` writes it back.
fn loaded_pitch(raw: Option<&Tag>) -> f32 {
    let pitch = raw.map_or(0.0, |tag| rotation(tag)[1]);
    if pitch.is_finite() {
        (pitch % 360.0).clamp(-90.0, 90.0)
    } else {
        0.0
    }
}
/// Painting.addAdditionalSaveData: `facing`, `variant` and `block_pos`.
pub fn painting_to_nbt(p: &Painting, uuid: [i32; 4], raw: Option<&Tag>) -> Tag {
    let (lo, hi) = p.bounds();
    let base = Base {
        id: "minecraft:painting",
        uuid,
        position: Vec3::new(
            (lo[0] + hi[0]) / 2.0,
            (lo[1] + hi[1]) / 2.0,
            (lo[2] + hi[2]) / 2.0,
        ),
        motion: Vec3::ZERO,
        rotation: [f32::from(p.facing) * 90.0, loaded_pitch(raw)],
        on_ground: false,
        fall_distance: 0.0,
    };
    let mut tag = base.tag(raw);
    put(&mut tag, "facing", Tag::Byte(p.facing as i8));
    put(&mut tag, "variant", string(&p.variant));
    put(&mut tag, "block_pos", Tag::IntArray(p.pos.to_vec()));
    tag
}
/// `BlockAttachedEntity.readAdditionalSaveData`: `block_pos` when saved; a
/// summoned one has none, and `setPos` puts its block at the block holding
/// the summon position.
pub(super) fn attached_block_pos(tag: &Tag) -> Option<[i32; 3]> {
    match tag.get("block_pos") {
        Some(Tag::IntArray(v)) if v.len() == 3 => Some([v[0], v[1], v[2]]),
        _ => {
            let at = position(tag)?;
            Some([
                at.x.floor() as i32,
                at.y.floor() as i32,
                at.z.floor() as i32,
            ])
        }
    }
}
/// `Direction.from3DDataValue` / `from2DDataValue`: the byte read by
/// `LEGACY_ID_CODEC` indexes the table as `abs(data % modulus)`, never clamped.
fn wrap_data_value(raw: impl TryInto<i64> + Copy, modulus: i64) -> u8 {
    let byte = raw.try_into().unwrap_or(0) as i8;
    (i64::from(byte) % modulus).unsigned_abs() as u8
}
/// The block position, facing and variant (if saved) of a painting.
pub fn painting_from_nbt(tag: &Tag) -> Option<([i32; 3], u8, Option<String>)> {
    let pos = attached_block_pos(tag)?;
    let facing = wrap_data_value(number(tag.get("facing")).unwrap_or(0), 4);
    Some((pos, facing, text(tag.get("variant"))))
}
/// ItemFrame.addAdditionalSaveData.
pub fn item_frame_to_nbt(f: &ItemFrame, uuid: [i32; 4], raw: Option<&Tag>) -> Tag {
    let (lo, hi) = f.bounds();
    let base = Base {
        id: f.kind(),
        uuid,
        position: Vec3::new(
            (lo[0] + hi[0]) / 2.0,
            (lo[1] + hi[1]) / 2.0,
            (lo[2] + hi[2]) / 2.0,
        ),
        motion: Vec3::ZERO,
        rotation: [0.0, 0.0],
        on_ground: false,
        fall_distance: 0.0,
    };
    let mut tag = base.tag(raw);
    // `Entity.saveWithoutId`: `CustomName` before the frame's own fields.
    if let Some(name) = &f.custom_name {
        put(&mut tag, "CustomName", name.clone());
    } else if let Tag::Compound(map) = &mut tag {
        map.remove(&"CustomName".into());
    }
    put(&mut tag, "Facing", Tag::Byte(f.facing as i8));
    put(&mut tag, "block_pos", Tag::IntArray(f.pos.to_vec()));
    put(&mut tag, "ItemRotation", Tag::Byte(f.rotation));
    put(
        &mut tag,
        "ItemDropChance",
        Tag::Float(f.drop_chance.to_bits()),
    );
    put(&mut tag, "Invisible", Tag::Byte(i8::from(f.invisible)));
    put(&mut tag, "Fixed", Tag::Byte(i8::from(f.fixed)));
    if let Tag::Compound(map) = &mut tag {
        map.remove(&horizon_world::nbt::JavaString::from("Item"));
    }
    if let Some(item) = &f.item {
        put(&mut tag, "Item", item_to_nbt(item));
    }
    tag
}
/// `ItemFrame.readAdditionalSaveData`: an absent Facing defaults to Direction.DOWN.
pub fn item_frame_from_nbt(tag: &Tag, id: u32) -> Option<ItemFrame> {
    let pos = attached_block_pos(tag)?;
    Some(ItemFrame {
        id,
        pos,
        facing: wrap_data_value(number(tag.get("Facing")).unwrap_or(0), 6),
        glow: kind(tag).as_deref() == Some("minecraft:glow_item_frame"),
        item: tag
            .get("Item")
            .and_then(item_from_nbt)
            .map(|s| crate::items::ItemStack { count: 1, ..s }),
        // `TagValueInput.getByteOr` narrows before `ItemFrame.setRotation`'s remainder.
        rotation: (tag.get("ItemRotation").and_then(int_value).unwrap_or(0) as i8) % 8,
        drop_chance: float(tag.get("ItemDropChance")).map_or(1.0, |v| v as f32),
        fixed: flag(tag, "Fixed"),
        invisible: flag(tag, "Invisible"),
        check_interval: 0,
        custom_name: tag.get("CustomName").cloned(),
        random: crate::random_source::entity(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn negative_facing_wraps_like_vanilla_direction_codecs() {
        // from3DDataValue: abs(-1 % 6) = 1 (UP); 7 -> 1; -6 -> 0.
        for (raw, want) in [(-1i8, 1u8), (7, 1), (-6, 0), (5, 5), (0, 0)] {
            let tag = Tag::compound([
                ("Facing", Tag::Byte(raw)),
                ("block_pos", Tag::IntArray(vec![0, 0, 0])),
            ]);
            assert_eq!(item_frame_from_nbt(&tag, 1).unwrap().facing, want);
        }
        // from2DDataValue: abs(-1 % 4) = 1.
        let tag = Tag::compound([
            ("facing", Tag::Byte(-1)),
            ("block_pos", Tag::IntArray(vec![0, 0, 0])),
        ]);
        assert_eq!(painting_from_nbt(&tag).unwrap().1, 1);
    }
}
