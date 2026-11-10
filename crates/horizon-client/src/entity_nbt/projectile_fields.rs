//! The `Entity` fields a projectile keeps beside `Projectile`'s own: fire,
//! air, the visual fire flag and the fall distance.
use super::{flag, float, int_value, put};
use crate::projectiles::Projectile;
use horizon_world::nbt::Tag;

/// `Entity.load`: fire is read as a short, air as an int and the visual
/// fire flag as a byte-valued boolean. Portal state lives on the level.
pub(super) fn load_projectile_entity_fields(p: &mut Projectile, tag: &Tag) {
    p.fire_ticks = i32::from(tag.get("Fire").and_then(int_value).unwrap_or(0) as i16);
    p.air_supply = tag.get("Air").and_then(int_value).unwrap_or(p.air_supply);
    p.has_visual_fire = flag(tag, "HasVisualFire");
    // Entity.load: getDoubleOr, any number widened.
    p.fall_distance = float(tag.get("fall_distance")).unwrap_or(0.0);
}

/// `Entity.saveWithoutId`: save the current fire and air as shorts, and
/// only write the optional visual fire flag when it is true.
pub(super) fn put_projectile_entity_fields(tag: &mut Tag, p: &Projectile) {
    put(tag, "Fire", Tag::Short(p.fire_ticks as i16));
    put(tag, "Air", Tag::Short(p.air_supply as i16));
    if let Tag::Compound(map) = tag {
        map.remove(&"HasVisualFire".into());
    }
    if p.has_visual_fire {
        put(tag, "HasVisualFire", Tag::Byte(1));
    }
}

/// `Entity.load` for an arrow: fire and fall distance live on its arrow state,
/// air on the projectile and defaults to the 300 of `getMaxAirSupply`.
pub(super) fn load_arrow_entity_fields(p: &mut Projectile, tag: &Tag) {
    p.air_supply = tag.get("Air").and_then(int_value).unwrap_or(300);
    if let Some(a) = p.arrow.as_mut() {
        a.fire_ticks = i32::from(tag.get("Fire").and_then(int_value).unwrap_or(0) as i16);
        a.fall_distance = float(tag.get("fall_distance")).unwrap_or(0.0);
    }
}

#[cfg(test)]
#[path = "projectile_fields_tests.rs"]
mod tests;
