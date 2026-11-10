//! `LivingEntity.baseTick`'s air supply and drowning: `WaterAnimal`'s handler for
//! the aquatic mobs, the axolotl's and the nautilus's own, and the `Breath`/`drown`
//! path the rest take, with `canBreatheUnderWater`.
use super::{Mob, MobWorld, axolotl_air_supply, hurt, nautilus_air_supply};
use crate::breathing::{Breath, breathe, drown};
use crate::mobs::{Damage, DamageKind, MobEvent, Species};

/// `LivingEntity.canBreatheUnderWater`: the `can_breathe_under_water` tag, which
/// `MobType.flags.breathes_water` holds, and the two overrides — a baby happy ghast
/// and a sulfur cube with a block in its body breathe under water too.
fn can_breathe_under_water(m: &Mob) -> bool {
    m.kind.flags.breathes_water
        || m.species() == Species::HappyGhast && m.is_baby()
        || m.species() == Species::SulfurCube && crate::mobs::sulfur_cube::has_body(m)
}

pub(super) fn tick_air(m: &mut Mob, w: &MobWorld, events: &mut Vec<MobEvent>) {
    // Air supply and drowning (WaterAnimal.handleAirSupply for aquatic mobs).
    // Nautilus.baseTick: NoAI skips only the custom handler; the
    // superclass Breath/drown path must still process its air.
    if m.species() == Species::Axolotl {
        axolotl_air_supply(m, w);
    } else if m.species() == Species::Nautilus {
        nautilus_air_supply(m);
    } else if m.kind.flags.needs_water {
        if !m.fluids.in_water {
            m.air -= 1;
            if m.air == -20 {
                m.air = 0;
                hurt(m, w, events, 2.0, Damage::of(DamageKind::Drown));
            }
        } else {
            m.air = 300;
        }
    } else {
        // LivingEntity.baseTick / MobEffectUtil: bubble columns and breathing
        // effects protect submerged mobs. Nautilus alone holds air rather
        // than refilling it; Water Breathing or Conduit Power still refill.
        let submerged = (m.fluids.eye_in_water || m.eye_in_drowning_fluid())
            && w.block_name(
                m.position.x.floor() as i32,
                m.eye_y().floor() as i32,
                m.position.z.floor() as i32,
            ) != "minecraft:bubble_column";
        let water_breathing =
            m.has_effect("minecraft:water_breathing") || m.has_effect("minecraft:conduit_power");
        let nautilus = m.has_effect("minecraft:breath_of_the_nautilus");
        let drowns = submerged && !can_breathe_under_water(m) && !water_breathing && !nautilus;
        let held = submerged && nautilus && !water_breathing;
        let vanilla = Breath {
            can_breathe: !drowns,
            consume: 1,
            refill: if drowns || held { 0 } else { 4 },
        };
        let breath = breathe(m.id, &m.kind.id, vanilla);
        if let Some(drowning) = drown(m.id, &m.kind.id, &mut m.air, breath, true) {
            events.push(MobEvent::Drown {
                mob: m.id,
                at: [m.position.x, m.position.y, m.position.z],
            });
            if let Some(damage) = drowning.damage {
                hurt(m, w, events, damage, Damage::of(DamageKind::Drown));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use horizon_core::geometry::Vec3;
    use std::sync::Arc;

    /// `LivingEntity.canBreatheUnderWater` on the drown path: the
    /// `can_breathe_under_water` tag (an iron golem is not in it, a guardian is),
    /// with the baby happy ghast and the sulfur cube holding a block on top of it.
    /// 300 ticks take the air from 300 to 0 and 20 more deal the drown's 2.
    #[test]
    fn the_types_the_jar_lists_breathe_under_water() {
        let ran = crate::mobs::world::with_test_world(&[], |w, types| {
            let drowned = |id: &str, baby: bool, body: Option<&str>| {
                let kind = Arc::new(types.types[id].clone());
                let mut m = Mob::new(1, kind, Vec3::new(0.5, 64.0, 0.5), 0.0);
                m.fluids.eye_in_water = true;
                m.fluids.in_water = true;
                if baby {
                    m.extra.age = -24000;
                }
                m.extra.body = body.map(Into::into);
                let start = m.health;
                let mut events = Vec::new();
                for _ in 0..320 {
                    tick_air(&mut m, w, &mut events);
                }
                start - m.health
            };
            assert_eq!(drowned("minecraft:guardian", false, None), 0.0);
            assert_eq!(drowned("minecraft:iron_golem", false, None), 2.0);
            assert_eq!(drowned("minecraft:happy_ghast", false, None), 2.0);
            assert_eq!(drowned("minecraft:happy_ghast", true, None), 0.0);
            assert_eq!(drowned("minecraft:sulfur_cube", false, None), 2.0);
            assert_eq!(
                drowned("minecraft:sulfur_cube", false, Some("minecraft:stone")),
                0.0
            );
        });
        let _ = ran;
    }
}
