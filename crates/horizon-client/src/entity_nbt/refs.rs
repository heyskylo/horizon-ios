//! `EntityReference`s a mob's compound holds: a UUID, resolved against what is in
//! the level.
use super::{Who, number, uuid_from_tag};
use crate::mobs::Mob;
use horizon_world::nbt::Tag;
use std::collections::HashMap;

/// `LivingEntity.readAdditionalSaveData`: `last_hurt_by_mob` is a lazy
/// `EntityReference` (the UUID, which the game resolves once the entity is in
/// the level) and its timestamp is `tickCount - ticks_since_last_hurt_by_mob`.
pub(super) fn read_last_hurt_by(m: &mut Mob, tag: &Tag) {
    m.ai.last_hurt_by_ref = uuid_from_tag(tag.get("last_hurt_by_mob"));
    m.ai.last_hurt_by_time = (m.age as i32)
        .wrapping_sub(number(tag.get("ticks_since_last_hurt_by_mob")).unwrap_or(0) as i32);
}

/// The entity a stored UUID names, while it is in the level: `player` (the
/// player's own UUID, when one is in the level), or the mob `uuids` gives that
/// UUID to. `uuids` is the game layer's id -> UUID map, so this is the read half
/// of the pair [`who_uuid`] and `mob_to_nbt`'s `uuid_of` form. A UUID nothing
/// loaded has, or one naming a kind `Who` cannot carry (an item, a projectile, a
/// guest), is `None` - never some other entity as a stand-in.
pub fn who_by_uuid(
    uuids: &HashMap<u32, [i32; 4]>,
    mobs: &[Mob],
    player: Option<[i32; 4]>,
    uuid: [i32; 4],
) -> Option<Who> {
    if player == Some(uuid) {
        return Some(Who::Player);
    }
    // Only a loaded mob: the map also holds projectiles, vehicles and items,
    // whose ids mean nothing to `Who`.
    uuids
        .iter()
        .find(|(id, u)| **u == uuid && mobs.iter().any(|m| m.id == **id))
        .map(|(id, _)| Who::Mob(*id))
}

#[cfg(test)]
mod tests {
    use super::super::*;
    use crate::mobs::{Species, sensor_scan_tests::kind};

    fn types() -> HashMap<String, std::sync::Arc<crate::mobs::MobType>> {
        let mut pig = (*kind()).clone();
        pig.id = "minecraft:pig".into();
        pig.species = Species::Pig;
        HashMap::from([("minecraft:pig".to_owned(), std::sync::Arc::new(pig))])
    }

    fn load(snbt: &str) -> Mob {
        let tag = horizon_world::nbt::parse_snbt(snbt).unwrap().0;
        mob_from_nbt(&tag, &types(), 7, None).unwrap()
    }

    /// `LivingEntity.readAdditionalSaveData`: `lastHurtByMob` is the UUID,
    /// `lastHurtByMobTimestamp = tickCount - ticks_since_last_hurt_by_mob`, and a
    /// freshly loaded entity has `tickCount` 0, so 30 saved ticks make -30.
    #[test]
    fn a_saved_hurt_by_mob_is_read_back_as_a_reference_and_a_timestamp() {
        let m = load(
            r#"{id:"minecraft:pig",UUID:[I;5,0,0,1],Pos:[2.5d,200d,4.5d],Health:10f,last_hurt_by_mob:[I;5,0,0,2],ticks_since_last_hurt_by_mob:30}"#,
        );
        assert_eq!(m.age, 0);
        assert_eq!(m.ai.last_hurt_by_ref, Some([5, 0, 0, 2]));
        assert_eq!(m.ai.last_hurt_by_time, -30);
        // Not resolved yet: `getLastHurtByMob` answers nothing.
        assert_eq!(m.ai.last_hurt_by, None);
    }

    /// Missing keys: no reference, and `getIntOr("ticks_since...", 0)` stamps the
    /// timestamp at `tickCount`.
    #[test]
    fn a_save_without_a_hurt_by_mob_has_none() {
        let m = load(r#"{id:"minecraft:pig",UUID:[I;5,0,0,1],Pos:[2.5d,200d,4.5d],Health:10f}"#);
        assert_eq!(m.ai.last_hurt_by_ref, None);
        assert_eq!(m.ai.last_hurt_by_time, 0);
        // A key that is not a UUID int array is no reference (the codec fails).
        let m = load(
            r#"{id:"minecraft:pig",UUID:[I;5,0,0,1],Pos:[2.5d,200d,4.5d],Health:10f,last_hurt_by_mob:"x"}"#,
        );
        assert_eq!(m.ai.last_hurt_by_ref, None);
    }
}
