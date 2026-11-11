//! `AreaEffectCloud` save data: reading a cloud from NBT and writing it back.

use super::{CloudSettings, PotionCloud, brewing};
use crate::game::dragon::Breath;
use horizon_client::text::Text;
use horizon_core::geometry::Vec3;
use horizon_world::nbt::Tag;

/// `AreaEffectCloud.readAdditionalSaveData` for the fields a client cloud keeps:
/// its position, lifetime, radius and use settings, and the potion it carries.
/// The owner reference is not modelled on a client cloud, and the entity id
/// is the caller's (`load_potion_cloud`).
pub(in crate::game) fn potion_cloud_from_nbt(tag: &Tag) -> Option<(Vec3, Breath)> {
    use horizon_client::save::{float, number, text};
    let at = horizon_client::entity_nbt::position(tag)?;
    let motion = horizon_client::entity_nbt::loaded_motion(tag);
    let int = |key, default| number(tag.get(key)).map_or(default, |v| v as i32);
    let real = |key, default| float(tag.get(key)).map_or(default, |v| v as f32);
    let contents = tag.get("potion_contents");
    let mut effects = contents
        .and_then(|c| text(c.get("potion")))
        .and_then(|id| brewing::potion(&id))
        .map_or_else(Vec::new, |p| p.effects.to_vec());
    let (mut hidden, mut ambient) = (Vec::new(), Vec::new());
    if let Some(Tag::List(custom)) = contents.and_then(|c| c.get("custom_effects")) {
        for effect in custom {
            let Some(def) = text(effect.get("id")).and_then(|id| brewing::effect(&id)) else {
                continue;
            };
            let duration = number(effect.get("duration")).unwrap_or(0) as i32;
            let amp = number(effect.get("amplifier")).unwrap_or(0).clamp(0, 255) as i32;
            effects.push((def.id, duration, amp));
            if number(effect.get("show_particles")) == Some(0) {
                hidden.push(def.id);
            }
            if number(effect.get("ambient")).unwrap_or(0) != 0 {
                ambient.push(def.id);
            }
        }
    }
    let color = contents
        .and_then(|c| number(c.get("custom_color")))
        .map_or_else(
            || {
                horizon_client_core::potions::effects_color(
                    effects
                        .iter()
                        .filter(|(id, ..)| !hidden.contains(id))
                        .map(|(id, _, amp)| (*id, *amp)),
                )
            },
            |color| color as u32,
        );
    Some((
        at,
        Breath {
            id: 0,
            at: [at.x, at.y, at.z],
            motion: [motion.x, motion.y, motion.z],
            radius: real("Radius", 3.0).clamp(0.0, 32.0),
            age: int("Age", 0),
            duration: int("Duration", -1),
            from_dragon: false,
            victims: Vec::new(),
            potion: Some(PotionCloud {
                settings: CloudSettings {
                    wait_time: int("WaitTime", 20),
                    reapplication_delay: int("ReapplicationDelay", 20),
                    radius_on_use: real("RadiusOnUse", 0.0),
                    duration_on_use: int("DurationOnUse", 0),
                    duration_scale: real("potion_duration_scale", 1.0),
                },
                effects,
                hidden,
                ambient,
                radius_per_tick: real("RadiusPerTick", 0.0),
                color,
                custom_particle: tag.get("custom_particle").cloned(),
                victims: Vec::new(),
                source: Text::translate("entity.minecraft.area_effect_cloud", &[]),
                owner: None,
                guest: None,
            }),
        },
    ))
}

/// `AreaEffectCloud.addAdditionalSaveData`: the lifetime, radius and use
/// settings with the effects the cloud applies, as `custom_effects` with the
/// colour they were drawn in. The client keeps no potion id, so a cloud that
/// came from a named potion is written effect by effect; the owner has no
/// counterpart here.
pub(in crate::game) fn potion_cloud_to_nbt(c: &Breath, uuid: [i32; 4], raw: Option<&Tag>) -> Tag {
    use horizon_world::nbt::Compound;
    let Some(potion) = &c.potion else {
        return Tag::End;
    };
    let at = Vec3::new(c.at[0], c.at[1], c.at[2]);
    let motion = Vec3::new(c.motion[0], c.motion[1], c.motion[2]);
    let mut tag = horizon_client::entity_nbt::entity_base_tag(
        "minecraft:area_effect_cloud",
        at,
        motion,
        [0.0, 0.0],
        uuid,
        raw,
    );
    let custom: Vec<Tag> = potion
        .effects
        .iter()
        .map(|(id, duration, amplifier)| {
            let mut effect = Compound::new();
            effect.insert("id".into(), Tag::String(brewing::namespaced(id).into()));
            effect.insert("duration".into(), Tag::Int(*duration));
            effect.insert("amplifier".into(), Tag::Int(*amplifier));
            if potion.hidden.contains(id) {
                effect.insert("show_particles".into(), Tag::Byte(0));
            }
            if potion.ambient.contains(id) {
                effect.insert("ambient".into(), Tag::Byte(1));
            }
            Tag::Compound(effect)
        })
        .collect();
    let mut contents = Compound::new();
    contents.insert("custom_effects".into(), Tag::List(custom));
    contents.insert("custom_color".into(), Tag::Int(potion.color as i32));
    let Tag::Compound(map) = &mut tag else {
        return tag;
    };
    map.insert("Age".into(), Tag::Int(c.age));
    map.insert("Duration".into(), Tag::Int(c.duration));
    map.insert("Radius".into(), Tag::Float(c.radius.to_bits()));
    map.insert(
        "RadiusPerTick".into(),
        Tag::Float(potion.radius_per_tick.to_bits()),
    );
    if let Some(particle) = &potion.custom_particle {
        map.insert("custom_particle".into(), particle.clone());
    }
    map.insert("WaitTime".into(), Tag::Int(potion.settings.wait_time));
    map.insert(
        "ReapplicationDelay".into(),
        Tag::Int(potion.settings.reapplication_delay),
    );
    map.insert(
        "RadiusOnUse".into(),
        Tag::Float(potion.settings.radius_on_use.to_bits()),
    );
    map.insert(
        "DurationOnUse".into(),
        Tag::Int(potion.settings.duration_on_use),
    );
    map.insert(
        "potion_duration_scale".into(),
        Tag::Float(potion.settings.duration_scale.to_bits()),
    );
    // Vanilla writes the key only while the contents differ from
    // PotionContents.EMPTY: no effects and the default water colour.
    if !(potion.effects.is_empty()
        && potion.color == horizon_client_core::potions::BASE_POTION_COLOR)
    {
        map.insert("potion_contents".into(), Tag::Compound(contents));
    }
    tag
}
