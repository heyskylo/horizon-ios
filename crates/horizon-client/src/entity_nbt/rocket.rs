//! Firework rockets: `Projectile` and `FireworkRocketEntity` save data.
use super::{
    Base, Tag, Vec3, Who, flag, item_from_nbt, item_to_nbt, motion, number, position, put,
    rotation, uuid_from_tag,
};

/// `FireworkRocketEntity.readAdditionalSaveData`: Life, LifeTime (0 when
/// absent: a bare `/summon` explodes at once), FireworksItem and
/// ShotAtAngle, with `Projectile`'s Owner and LeftOwner.
pub struct RocketNbt {
    pub position: Vec3,
    pub motion: Vec3,
    pub life: i32,
    pub lifetime: i32,
    pub item: Option<crate::items::ItemStack>,
    pub angled: bool,
    pub owner: Option<Who>,
    pub left_owner: bool,
    pub on_ground: bool,
    /// `Entity.load`'s Rotation.
    pub rotation: [f32; 2],
    /// `Projectile.hasBeenShot`: false until its first tick posts `projectile_shoot`.
    pub has_been_shot: bool,
    /// `Entity.isSilent`: no launch sound.
    pub silent: bool,
}
pub fn firework_rocket_from_nbt(tag: &Tag, player: Option<[i32; 4]>) -> Option<RocketNbt> {
    let owner = uuid_from_tag(tag.get("Owner"));
    Some(RocketNbt {
        position: position(tag)?,
        motion: motion(tag),
        life: number(tag.get("Life")).unwrap_or(0) as i32,
        lifetime: number(tag.get("LifeTime")).unwrap_or(0) as i32,
        item: tag.get("FireworksItem").and_then(item_from_nbt),
        angled: flag(tag, "ShotAtAngle"),
        owner: (player.is_some() && owner == player).then_some(Who::Player),
        left_owner: flag(tag, "LeftOwner"),
        on_ground: flag(tag, "OnGround"),
        rotation: rotation(tag),
        has_been_shot: flag(tag, "HasBeenShot"),
        silent: flag(tag, "Silent"),
    })
}

/// What `FireworkRocketEntity.addAdditionalSaveData` writes, over `Projectile`'s
/// Owner and LeftOwner.
pub struct RocketSave<'a> {
    pub rotation: [f32; 2],
    pub position: Vec3,
    pub motion: Vec3,
    pub life: i32,
    pub lifetime: i32,
    pub item: &'a crate::items::ItemStack,
    pub angled: bool,
    pub owner: Option<[i32; 4]>,
    pub left_owner: bool,
    /// `Entity.onGround`, from the rocket's last `Entity.move`.
    pub on_ground: bool,
    pub has_been_shot: bool,
}
pub fn firework_rocket_to_nbt(r: &RocketSave, uuid: [i32; 4], raw: Option<&Tag>) -> Tag {
    let mut tag = Base {
        id: "minecraft:firework_rocket",
        uuid,
        position: r.position,
        motion: r.motion,
        rotation: r.rotation,
        on_ground: r.on_ground,
        fall_distance: 0.0,
    }
    .tag(raw);
    for (key, value) in [
        ("Life", Tag::Int(r.life)),
        ("LifeTime", Tag::Int(r.lifetime)),
        ("FireworksItem", item_to_nbt(r.item)),
        ("ShotAtAngle", Tag::Byte(i8::from(r.angled))),
        ("HasBeenShot", Tag::Byte(i8::from(r.has_been_shot))),
    ] {
        put(&mut tag, key, value);
    }
    if r.left_owner {
        put(&mut tag, "LeftOwner", Tag::Byte(1));
    }
    if let Some(owner) = r.owner {
        put(&mut tag, "Owner", Tag::IntArray(owner.to_vec()));
    }
    tag
}
