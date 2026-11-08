//! What a client's copy of a mob makes on its own, in the
//! `level().isClientSide()` branches of `aiStep`: nothing of it reaches the
//! server or goes over the network, so the window's game runs it as it follows
//! a server.
use super::{Mob, MobEvent, Species, bee};
use horizon_core::geometry::trig::Mth;
use horizon_core::random::RandomSource;
use std::f32::consts::PI;

/// `Enderman.aiStep`, `Endermite.aiStep` and `Blaze.aiStep` on a client:
/// two portal particles a tick around endermen and endermites; two large
/// smoke particles a tick around blazes and, one tick in 24, their burn.
/// Also `Dolphin.tick`'s trail on a client.
pub fn client_ai_step(m: &mut Mob, out: &mut Vec<MobEvent>) {
    let species = m.species();
    if species == Species::Dolphin {
        dolphin_trail(m, out);
        return;
    }
    if !matches!(
        species,
        Species::Enderman | Species::Endermite | Species::Blaze
    ) {
        return;
    }
    let (w, h) = (f64::from(m.width()), f64::from(m.height()));
    let p = m.position;
    if species == Species::Blaze && m.random.next_int_bound(24).unwrap_or(1) == 0 && !m.silent {
        let volume = 1.0 + m.random.next_float();
        let pitch = m.random.next_float() * 0.7 + 0.3;
        out.push(MobEvent::LocalSound(
            "entity.blaze.burn",
            [p.x + 0.5, p.y + 0.5, p.z + 0.5],
            volume,
            pitch,
        ));
    }
    for _ in 0..2 {
        // Entity.getRandomX(0.5), getRandomY(), getRandomZ(0.5).
        let r = &mut m.random;
        let x = p.x + w * (2.0 * r.next_double() - 1.0) * 0.5;
        let y = p.y + h * r.next_double();
        let z = p.z + w * (2.0 * r.next_double() - 1.0) * 0.5;
        let (kind, at, velocity) = if species == Species::Blaze {
            ("large_smoke", [x, y, z], [0.0; 3])
        } else {
            // The enderman's are a quarter block lower.
            let y = if species == Species::Enderman {
                y - 0.25
            } else {
                y
            };
            let velocity = [
                (r.next_double() - 0.5) * 2.0,
                -r.next_double(),
                (r.next_double() - 0.5) * 2.0,
            ];
            ("portal", [x, y, z], velocity)
        };
        out.push(MobEvent::ParticleAt { kind, at, velocity });
    }
}

/// `Dolphin.tick` on a client: a dolphin that swims fast in water (squared
/// speed over 0.03) leaves four dolphin particles a tick behind it, two on
/// each side, set back along its view by a distance from its own random.
fn dolphin_trail(m: &mut Mob, out: &mut Vec<MobEvent>) {
    if m.no_ai || !m.fluids.in_water || m.velocity.length_squared() <= 0.03 {
        return;
    }
    // Entity.getViewVector(0).
    let view = m.view_vector();
    let yaw = m.yaw * (PI / 180.0);
    let (c, s) = (f64::from(yaw.mcos() * 0.3), f64::from(yaw.msin() * 0.3));
    let back = f64::from(1.2 - m.random.next_float() * 0.7);
    let p = m.position;
    let (x, y, z) = (p.x - view[0] * back, p.y - view[1], p.z - view[2] * back);
    for _ in 0..2 {
        for (dx, dz) in [(c, s), (-c, -s)] {
            out.push(MobEvent::ParticleAt {
                kind: "dolphin",
                at: [x + dx, y, z + dz],
                velocity: [0.0; 3],
            });
        }
    }
}

/// `Phantom.tick` and `Bee.tick` on a client, which draw from the client
/// level's random (`random`) and never from the mob's own, so a window that
/// simulates the world keeps the mob's draws as the server has them: a
/// phantom's wings leave mycelium dust, a bee carrying nectar now and then
/// drips it.
pub fn client_particles(m: &Mob, random: &mut impl RandomSource, out: &mut Vec<MobEvent>) {
    match m.species() {
        Species::Phantom => {
            // The wings flap every phantom in a different phase
            // (`getUniqueFlapTickOffset`).
            let flap = ((m.id.wrapping_mul(3).wrapping_add(m.age)) as i32 as f32
                * 7.448451
                * (PI / 180.0)
                + PI)
                .mcos();
            let width = m.width() * 1.48;
            let yaw = m.yaw * (PI / 180.0);
            let (c, s) = (yaw.mcos() * width, yaw.msin() * width);
            let h = (0.3 + flap * 0.45) * m.height() * 2.5;
            let p = m.position;
            for (dx, dz) in [(c, s), (-c, -s)] {
                out.push(MobEvent::ParticleAt {
                    kind: "mycelium",
                    at: [p.x + f64::from(dx), p.y + f64::from(h), p.z + f64::from(dz)],
                    velocity: [0.0; 3],
                });
            }
        }
        Species::Bee => {
            if !bee::has_nectar(m)
                || !bee::get(m).is_some_and(|b| b.crops_grown < 10)
                || random.next_float() >= 0.05
            {
                return;
            }
            // `for (i = 0; i < nextInt(2) + 1; i++)` rolls the bound each time.
            let mut i = 0;
            while i < random.next_int_bound(2).unwrap_or(0) + 1 {
                // Mth.lerp(level.random.nextDouble(), x - 0.3F, x + 0.3F).
                let spread = f64::from(0.3_f32);
                let lerp = |delta: f64, from: f64, to: f64| from + delta * (to - from);
                let x = lerp(
                    random.next_double(),
                    m.position.x - spread,
                    m.position.x + spread,
                );
                let z = lerp(
                    random.next_double(),
                    m.position.z - spread,
                    m.position.z + spread,
                );
                out.push(MobEvent::ParticleAt {
                    kind: "falling_nectar",
                    // getY(0.5).
                    at: [x, m.position.y + f64::from(m.height()) * 0.5, z],
                    velocity: [0.0; 3],
                });
                i += 1;
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mobs::sensor_scan_tests::kind;
    use horizon_core::geometry::Vec3;

    fn dolphin() -> Mob {
        let mut kind = kind();
        let k = std::sync::Arc::make_mut(&mut kind);
        k.id = "minecraft:dolphin".into();
        k.species = Species::Dolphin;
        let mut m = Mob::new(1, kind, Vec3::new(10.0, 60.0, 20.0), 0.0);
        m.fluids.in_water = true;
        m.velocity = Vec3::new(0.0, 0.0, 0.2);
        m
    }

    fn trail(m: &mut Mob) -> Vec<[f64; 3]> {
        let mut out = Vec::new();
        client_ai_step(m, &mut out);
        out.into_iter()
            .map(|e| match e {
                MobEvent::ParticleAt { kind, at, velocity } => {
                    assert_eq!((kind, velocity), ("dolphin", [0.0; 3]));
                    at
                }
                _ => panic!("only particles"),
            })
            .collect()
    }

    /// `Dolphin.tick` on a client: facing south (yaw 0, level), a swimming
    /// dolphin leaves two pairs of particles 0.3 to either side, set back
    /// 0.5 to 1.2 behind it on the same height.
    #[test]
    fn a_swimming_dolphin_leaves_four_trail_particles_behind_it() {
        let mut m = dolphin();
        let at = trail(&mut m);
        assert_eq!(at.len(), 4);
        let back = 20.0 - at[0][2];
        assert!((0.5..=1.2).contains(&back), "set back {back}");
        for (i, p) in at.iter().enumerate() {
            let side = if i % 2 == 0 { 0.3 } else { -0.3 };
            assert!((p[0] - (10.0 + side)).abs() < 1e-6, "{p:?}");
            assert_eq!((p[1], p[2]), (60.0, at[0][2]));
        }
    }

    /// No trail out of water, at a squared speed of 0.03 or less, or with NoAI.
    #[test]
    fn a_slow_dry_or_no_ai_dolphin_leaves_no_trail() {
        let mut m = dolphin();
        m.fluids.in_water = false;
        assert!(trail(&mut m).is_empty());
        let mut m = dolphin();
        m.velocity = Vec3::new(0.0, 0.0, 0.17);
        assert!(trail(&mut m).is_empty());
        let mut m = dolphin();
        m.no_ai = true;
        assert!(trail(&mut m).is_empty());
    }
}
