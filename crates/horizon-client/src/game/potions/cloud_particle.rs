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
/// A number tag as `Codec.INT` reads it (`Number.intValue`).
fn tag_i32(tag: &Tag) -> Option<i32> {
    Some(match tag {
        Tag::Byte(v) => i32::from(*v),
        Tag::Short(v) => i32::from(*v),
        Tag::Int(v) => *v,
        Tag::Long(v) => *v as i32,
        Tag::Float(v) => f32::from_bits(*v) as i32,
        Tag::Double(v) => f64::from_bits(*v) as i32,
        _ => return None,
    })
}
/// `ExtraCodecs.RGB_COLOR_CODEC` (`floats` 3) or `ARGB_COLOR_CODEC` (4): an
/// int, or a list of exactly that many floats packed through
/// `ARGB.colorFromFloat`; the channels as `ARGB` reads them back. The RGB
/// codec keeps no alpha (opaque).
fn tag_color(tag: Option<&Tag>, floats: usize) -> Option<[f32; 4]> {
    let packed = match tag? {
        Tag::List(l) if l.len() == floats => {
            let mut c = [1.0f32; 4];
            for (slot, value) in c.iter_mut().zip(l) {
                *slot = tag_f32(value)?;
            }
            // ARGB.as8BitChannel, then ARGB.color's byte masks.
            let byte = |f: f32| ((f * 255.0).floor() as i32) & 255;
            byte(c[3]) << 24 | byte(c[0]) << 16 | byte(c[1]) << 8 | byte(c[2])
        }
        Tag::List(_) => return None,
        other => tag_i32(other)?,
    };
    let channel = |shift: i32| ((packed >> shift) & 255) as f32 / 255.0;
    let alpha = if floats == 4 { channel(24) } else { 1.0 };
    Some([channel(16), channel(8), channel(0), alpha])
}
fn tag_rgb(tag: Option<&Tag>) -> Option<[f32; 3]> {
    let [r, g, b, _] = tag_color(tag, 3)?;
    Some([r, g, b])
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
    // ScalableParticleOptionsBase.SCALE: required, within [0.01, 4].
    let scale = || {
        field("scale")
            .and_then(tag_f32)
            .filter(|s| (0.01..=4.0).contains(s))
    };
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
        // DustParticleOptions: color and scale, both required.
        ParticleType::Dust => ParticleOptions::dust(tag_rgb(field("color"))?, scale()?),
        ParticleType::DustColorTransition => ParticleOptions::dust_transition(
            tag_rgb(field("from_color"))?,
            tag_rgb(field("to_color"))?,
            scale()?,
        ),
        // ColorParticleOption: a required ARGB colour.
        ParticleType::EntityEffect | ParticleType::TintedLeaves => {
            ParticleOptions::colored(ty, tag_color(field("color"), 4)?)
        }
        // SpellParticleOption: color (default -1, white) and power.
        ParticleType::Effect | ParticleType::InstantEffect => {
            let [r, g, b] = match field("color") {
                Some(color) => tag_rgb(Some(color))?,
                None => [1.0; 3],
            };
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
        // SculkChargeParticleOptions.roll and ShriekParticleOption.delay are required.
        ParticleType::SculkCharge => {
            ParticleOptions::sculk_charge(field("roll").and_then(tag_f32)?)
        }
        ParticleType::Shriek => ParticleOptions::shriek(field("delay").and_then(tag_i32)?),
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

    /// The options codecs' rules: `ColorParticleOption` keeps the colour's
    /// alpha (an int's top byte, the fourth float), float lists have a fixed
    /// length and are packed to 8-bit channels, dust needs a scale in
    /// [0.01, 4], and a spell's colour defaults to white. What fails to decode
    /// gives no custom particle.
    #[test]
    fn custom_particle_follows_the_option_codecs() {
        let decode = |fields: Vec<(&str, Tag)>| custom_particle_options(&Tag::compound(fields));
        let s = |v: &str| Tag::String(v.into());
        // 0x80FF0000: alpha 128, red.
        let tinted = decode(vec![
            ("type", s("entity_effect")),
            ("color", Tag::Int(0x80FF_0000u32 as i32)),
        ])
        .unwrap();
        assert_eq!(tinted.color, [1.0, 0.0, 0.0, 128.0 / 255.0]);
        // [r, g, b, a] = [0, 1, 0, 0.5]: floor(0.5 * 255) = 127.
        let listed = decode(vec![
            ("type", s("entity_effect")),
            (
                "color",
                Tag::List(vec![flt(0.0), flt(1.0), flt(0.0), flt(0.5)]),
            ),
        ])
        .unwrap();
        assert_eq!(listed.color, [0.0, 1.0, 0.0, 127.0 / 255.0]);
        let three = vec![flt(1.0), flt(0.0), flt(0.0)];
        assert!(
            decode(vec![
                ("type", s("entity_effect")),
                ("color", Tag::List(three.clone()))
            ])
            .is_none()
        );
        // RGB_COLOR_CODEC: 0.3 * 255 = 76.5, floored to 76.
        let dust = decode(vec![
            ("type", s("dust")),
            ("color", Tag::List(vec![flt(0.3), flt(0.0), flt(1.0)])),
            ("scale", flt(4.0)),
        ])
        .unwrap();
        assert_eq!(dust.color[..3], [76.0 / 255.0, 0.0, 1.0]);
        assert!(
            decode(vec![
                ("type", s("dust")),
                ("color", Tag::List(three.clone()))
            ])
            .is_none()
        );
        assert!(
            decode(vec![
                ("type", s("dust")),
                ("color", Tag::List(three.clone())),
                ("scale", flt(4.5))
            ])
            .is_none()
        );
        assert!(
            decode(vec![
                ("type", s("dust")),
                ("color", Tag::List(vec![flt(1.0); 4])),
                ("scale", flt(1.0))
            ])
            .is_none()
        );
        let spell = decode(vec![("type", s("effect"))]).unwrap();
        assert_eq!(spell.color[..3], [1.0, 1.0, 1.0]);
        assert!(decode(vec![("type", s("sculk_charge"))]).is_none());
        assert!(decode(vec![("type", s("shriek"))]).is_none());
        assert_eq!(
            decode(vec![("type", s("shriek")), ("delay", Tag::Int(7))])
                .unwrap()
                .ty,
            ParticleType::Shriek
        );
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
