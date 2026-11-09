//! The particle an `AreaEffectCloud` draws from its saved `custom_particle`.

use horizon_client::particles::{ParticleOptions, ParticleType};
use horizon_world::nbt::Tag;

/// A number tag as a float.
fn tag_f32(tag: &Tag) -> Option<f32> {
    Some(match tag {
        Tag::Byte(v) => f32::from(*v),
        Tag::Short(v) => f32::from(*v),
        Tag::Int(v) => *v as f32,
        Tag::Long(v) => *v as f32,
        Tag::Float(v) => f32::from_bits(*v),
        Tag::Double(v) => f64::from_bits(*v) as f32,
        _ => return None,
    })
}
/// `ExtraCodecs.RGB_COLOR_CODEC` / `ARGB_COLOR_CODEC`: an int (alpha dropped)
/// or a list of three or four floats.
fn tag_rgb(tag: Option<&Tag>) -> Option<[f32; 3]> {
    match tag? {
        Tag::Int(v) => Some([
            ((v >> 16) & 255) as f32 / 255.0,
            ((v >> 8) & 255) as f32 / 255.0,
            (v & 255) as f32 / 255.0,
        ]),
        Tag::List(l) if l.len() == 3 || l.len() == 4 => {
            Some([tag_f32(&l[0])?, tag_f32(&l[1])?, tag_f32(&l[2])?])
        }
        _ => None,
    }
}
/// The particle a saved `custom_particle` names (`ParticleTypes.CODEC`), for
/// the types that need no world data. A type that carries a block, an item or a
/// position, or one that does not decode, gives `None`.
pub(super) fn custom_particle_options(tag: &Tag) -> Option<ParticleOptions> {
    let Tag::Compound(map) = tag else {
        return None;
    };
    let Some(Tag::String(name)) = map.get(&"type".into()) else {
        return None;
    };
    let ty = ParticleType::from_name(&name.to_string_lossy())?;
    let field = |key: &str| map.get(&key.into());
    let float = |key: &str, default: f32| field(key).and_then(tag_f32).unwrap_or(default);
    Some(match ty {
        ParticleType::Block
        | ParticleType::BlockCrumble
        | ParticleType::BlockMarker
        | ParticleType::FallingDust
        | ParticleType::DustPillar
        | ParticleType::Item
        | ParticleType::Vibration
        | ParticleType::Trail
        | ParticleType::Geyser
        | ParticleType::GeyserPlume
        | ParticleType::GeyserBase
        | ParticleType::GeyserPoof => return None,
        // DustParticleOptions: color and scale (0.01..=4).
        ParticleType::Dust => ParticleOptions::dust(
            tag_rgb(field("color"))?,
            float("scale", 1.0).clamp(0.01, 4.0),
        ),
        ParticleType::DustColorTransition => ParticleOptions::dust_transition(
            tag_rgb(field("from_color"))?,
            tag_rgb(field("to_color"))?,
            float("scale", 1.0),
        ),
        // ColorParticleOption.
        ParticleType::EntityEffect | ParticleType::TintedLeaves => {
            let [r, g, b] = tag_rgb(field("color"))?;
            ParticleOptions::colored(ty, [r, g, b, 1.0])
        }
        // SpellParticleOption: color and power.
        ParticleType::Effect | ParticleType::InstantEffect => {
            let [r, g, b] = tag_rgb(field("color"))?;
            ParticleOptions {
                scale: float("power", 1.0),
                ..ParticleOptions::colored(ty, [r, g, b, 1.0])
            }
        }
        // PowerParticleOption.
        ParticleType::DragonBreath => ParticleOptions {
            scale: float("power", 1.0),
            ..ParticleOptions::simple(ty)
        },
        ParticleType::SculkCharge => ParticleOptions::sculk_charge(float("roll", 0.0)),
        ParticleType::Shriek => {
            ParticleOptions::shriek(field("delay").and_then(tag_f32).unwrap_or(0.0) as i32)
        }
        _ => ParticleOptions::simple(ty),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flt(v: f32) -> Tag {
        Tag::Float(v.to_bits())
    }

    /// `AreaEffectCloud.updateParticle` with a `custom_particle`: the codec's
    /// type and payload win over the potion colour; none falls back to the
    /// entity effect in that colour.
    #[test]
    fn cloud_custom_particle_overrides_potion_colour() {
        let cloud = |custom: Option<Tag>| {
            let mut tag = Tag::compound([
                ("Pos", Tag::List(vec![Tag::Double(0f64.to_bits()); 3])),
                ("Radius", flt(3.0)),
                ("Duration", Tag::Int(200)),
            ]);
            if let (Tag::Compound(map), Some(custom)) = (&mut tag, custom) {
                map.insert("custom_particle".into(), custom);
            }
            crate::game::potions::potion_cloud_from_nbt(&tag)
                .unwrap()
                .1
                .potion
                .unwrap()
        };
        let none = cloud(None).particle();
        assert_eq!(none.ty, ParticleType::EntityEffect);
        let flame = cloud(Some(Tag::compound([("type", Tag::String("flame".into()))])));
        assert_eq!(flame.particle().ty, ParticleType::Flame);
        let dust = cloud(Some(Tag::compound([
            ("type", Tag::String("minecraft:dust".into())),
            ("color", Tag::List(vec![flt(1.0), flt(0.0), flt(0.0)])),
            ("scale", flt(2.0)),
        ])))
        .particle();
        assert_eq!(dust.ty, ParticleType::Dust);
        assert_eq!(dust.color[..3], [1.0, 0.0, 0.0]);
        assert_eq!(dust.scale, 2.0);
        let effect = cloud(Some(Tag::compound([
            ("type", Tag::String("entity_effect".into())),
            ("color", Tag::Int(0x00FF_8000u32 as i32)),
        ])))
        .particle();
        assert_eq!(effect.ty, ParticleType::EntityEffect);
        assert_eq!(effect.color[..3], [1.0, 128.0 / 255.0, 0.0]);
    }

    /// `AreaEffectCloud.addAdditionalSaveData` writes `custom_particle` back.
    #[test]
    fn cloud_custom_particle_survives_a_save() {
        let particle = Tag::compound([("type", Tag::String("flame".into()))]);
        let tag = Tag::compound([
            ("Pos", Tag::List(vec![Tag::Double(0f64.to_bits()); 3])),
            ("Radius", flt(3.0)),
            ("Duration", Tag::Int(200)),
            ("custom_particle", particle.clone()),
        ]);
        let (at, cloud) = crate::game::potions::potion_cloud_from_nbt(&tag).unwrap();
        let _ = at;
        let saved = crate::game::potions::potion_cloud_to_nbt(&cloud, [1, 2, 3, 4], None);
        let Tag::Compound(map) = saved else {
            panic!("not a compound");
        };
        assert_eq!(map.get(&"custom_particle".into()), Some(&particle));
    }
}
