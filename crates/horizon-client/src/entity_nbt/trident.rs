//! Thrown tridents: `AbstractArrow`, `Projectile` and `ThrownTrident` save data.
use super::{
    Base, Pickup, Tag, Vec3, flag, item_from_nbt, item_to_nbt, motion, number, owner_uuid,
    position, put, rotation, string, text,
};

/// A thrown trident as `AbstractArrow` + `ThrownTrident` save it.
pub struct TridentSave<'a> {
    pub position: Vec3,
    pub motion: Vec3,
    pub yaw: f32,
    pub pitch: f32,
    pub in_ground: bool,
    pub life: i32,
    pub shake: i32,
    pub dealt_damage: bool,
    pub pickup: Pickup,
    pub left_owner: bool,
    pub has_been_shot: bool,
    pub owner: Option<[i32; 4]>,
    pub no_gravity: bool,
    /// `Entity.onGround`: a trident moves by `setPos`, so it keeps the loaded one.
    pub on_ground: bool,
    pub fire_ticks: i32,
    /// `AbstractArrow.soundEvent`: the sound of its next block hit.
    pub sound_event: &'a str,
    pub item: &'a crate::items::ItemStack,
}
/// `ThrownTrident.getDefaultHitGroundSoundEvent`, a new or loaded trident's
/// `soundEvent` until its first block hit.
pub const TRIDENT_HIT_GROUND: &str = "minecraft:item.trident.hit_ground";
/// `ThrownTrident.addAdditionalSaveData`, including its arrow/projectile state.
pub fn trident_to_nbt(t: &TridentSave, uuid: [i32; 4], raw: Option<&Tag>) -> Tag {
    let mut tag = Base {
        id: "minecraft:trident",
        uuid,
        position: t.position,
        motion: t.motion,
        rotation: [t.yaw, t.pitch],
        on_ground: t.on_ground,
        fall_distance: 0.0,
    }
    .tag(raw);
    // Projectile.addAdditionalSaveData omits LeftOwner until it is true,
    // including when raw NBT explicitly supplied false.
    if let Tag::Compound(map) = &mut tag {
        map.remove(&"LeftOwner".into());
        map.remove(&"Owner".into());
    }
    if t.left_owner {
        put(&mut tag, "LeftOwner", Tag::Byte(1));
    }
    if let Some(owner) = t.owner {
        put(&mut tag, "Owner", Tag::IntArray(owner.to_vec()));
    }
    for (key, value) in [
        ("life", Tag::Short(t.life as i16)),
        ("shake", Tag::Byte(t.shake as i8)),
        ("inGround", Tag::Byte(i8::from(t.in_ground))),
        // AbstractArrow.Pickup: 0 disallowed, 1 allowed, 2 creative only.
        ("pickup", Tag::Byte(t.pickup.byte())),
        ("damage", Tag::Double(2.0_f64.to_bits())),
        ("HasBeenShot", Tag::Byte(i8::from(t.has_been_shot))),
        ("crit", Tag::Byte(0)),
        ("PierceLevel", Tag::Byte(0)),
        ("SoundEvent", string(t.sound_event)),
        ("item", item_to_nbt(t.item)),
        ("DealtDamage", Tag::Byte(i8::from(t.dealt_damage))),
        ("Fire", Tag::Short(t.fire_ticks as i16)),
    ] {
        put(&mut tag, key, value);
    }
    // Entity.saveWithoutId writes NoGravity only when set.
    if t.no_gravity {
        put(&mut tag, "NoGravity", Tag::Byte(1));
    } else if let Tag::Compound(map) = &mut tag {
        map.remove(&"NoGravity".into());
    }
    tag
}
/// `AbstractArrow` and `Projectile.readAdditionalSaveData` for a trident.
pub struct TridentNbt {
    pub position: Vec3,
    pub motion: Vec3,
    pub yaw: f32,
    pub pitch: f32,
    pub in_ground: bool,
    pub life: i32,
    pub shake: i32,
    pub dealt_damage: bool,
    pub pickup: Pickup,
    pub left_owner: bool,
    pub has_been_shot: bool,
    pub owner: Option<[i32; 4]>,
    pub no_gravity: bool,
    pub on_ground: bool,
    pub fire_ticks: i32,
    /// `SoundEvent`, or the trident's default when missing or no identifier.
    pub sound_event: String,
    pub item: crate::items::ItemStack,
}
/// `ThrownTrident.readAdditionalSaveData`, including its arrow/projectile defaults.
pub fn trident_from_nbt(tag: &Tag) -> Option<TridentNbt> {
    let [yaw, pitch] = rotation(tag);
    Some(TridentNbt {
        position: position(tag)?,
        motion: motion(tag),
        yaw,
        pitch,
        in_ground: flag(tag, "inGround"),
        // AbstractArrow.readAdditionalSaveData: `life` is the short as saved,
        // negative too, and `shakeTime` is `getByteOr("shake", 0) & 255`.
        life: number(tag.get("life")).unwrap_or(0) as i16 as i32,
        shake: (number(tag.get("shake")).unwrap_or(0) as i8 as u8).into(),
        dealt_damage: flag(tag, "DealtDamage"),
        pickup: Pickup::from_byte(number(tag.get("pickup")).unwrap_or(0)),
        left_owner: flag(tag, "LeftOwner"),
        has_been_shot: flag(tag, "HasBeenShot"),
        owner: owner_uuid(tag),
        no_gravity: flag(tag, "NoGravity"),
        on_ground: flag(tag, "OnGround"),
        fire_ticks: number(tag.get("Fire")).unwrap_or(0) as i32,
        sound_event: text(tag.get("SoundEvent"))
            .and_then(|s| horizon_client_core::identifier::normalize_id(&s))
            .unwrap_or_else(|| TRIDENT_HIT_GROUND.to_owned()),
        item: tag
            .get("item")
            .and_then(item_from_nbt)
            .unwrap_or_else(|| crate::items::ItemStack::new("minecraft:trident", 1)),
    })
}
