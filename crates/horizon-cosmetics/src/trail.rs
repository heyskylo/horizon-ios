//! A trail's particles: spawned only by moving, at most `PER_PLAYER` alive for one wearer and
//! `BUDGET` for everyone, each living its fixed lifetime. A particle's path is a function of its
//! age, so a frame only spawns, retires and draws: nothing is stepped per tick.

use crate::Trail;
use glam::{DVec3, Vec3};

/// Particles alive at once for one wearer.
pub const PER_PLAYER: usize = 24;
/// Particles alive at once for every wearer together.
pub const BUDGET: usize = 512;
/// A move longer than this between two frames (blocks) is a teleport: it leaves nothing.
pub const JUMP: f64 = 4.0;

/// One particle: where and when it started, which sprite, how big, and its sway's phase.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Particle {
    start: DVec3,
    born: f64,
    sprite: u8,
    size: f32,
    phase: f32,
}

/// The particles one wearer left, and what their next move adds.
#[derive(Clone, Debug, Default)]
pub struct Emitter {
    particles: Vec<Particle>,
    last: Option<DVec3>,
    /// Particles owed by the distance walked so far (the fraction left over).
    owed: f32,
    /// Particles spawned, for each one's look (no random source: a hash of this).
    count: u32,
}

/// Particles alive across every emitter, kept from frame to frame: `step` adds and retires,
/// `clear` gives an emitter's back.
#[derive(Clone, Copy, Debug, Default)]
pub struct Budget {
    pub alive: usize,
}

/// A particle's quad: corners relative to the camera (blocks), the sprite's UV corners in 0..1
/// of the item's texture, and its opacity.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Quad {
    pub corners: [Vec3; 4],
    pub uv: [[f32; 2]; 4],
    pub alpha: f32,
}

/// A hash of `n` to 0..1 (lowbias32).
fn unit(n: u32) -> f32 {
    let mut x = n;
    x ^= x >> 16;
    x = x.wrapping_mul(0x7feb_352d);
    x ^= x >> 15;
    x = x.wrapping_mul(0x846c_a68b);
    x ^= x >> 16;
    (x >> 8) as f32 / (1 << 24) as f32
}

impl Emitter {
    /// Particles alive.
    pub fn len(&self) -> usize {
        self.particles.len()
    }

    pub fn is_empty(&self) -> bool {
        self.particles.is_empty()
    }

    /// One frame at `now` (ticks) for a wearer whose feet are at `feet`: retires the particles
    /// past their lifetime, then, if the wearer `moves` (not sneaking, not riding...), spawns
    /// what the distance walked since the last frame owes, within `PER_PLAYER` and the `budget`.
    pub fn step(&mut self, trail: &Trail, feet: DVec3, moves: bool, now: f64, budget: &mut Budget) {
        let life = f64::from(trail.lifetime);
        let before = self.particles.len();
        self.particles.retain(|p| now - p.born < life);
        budget.alive -= (before - self.particles.len()).min(budget.alive);
        let last = self.last.replace(feet);
        let walked = last.map_or(0.0, |l| l.distance(feet));
        if !moves || walked > JUMP {
            self.owed = 0.0;
        } else {
            self.owed += walked as f32 * trail.rate;
        }
        let height = match trail.from {
            crate::From::Feet => 0.1,
            crate::From::Body => 0.9,
        };
        while self.owed >= 1.0 && self.particles.len() < PER_PLAYER && budget.alive < BUDGET {
            self.owed -= 1.0;
            budget.alive += 1;
            let n = self.count;
            self.count = self.count.wrapping_add(1);
            let seed = n.wrapping_mul(4);
            // Spread along the way walked this frame, and a little around it.
            let along = f64::from(unit(seed));
            let from = last.unwrap_or(feet);
            let jitter = DVec3::new(
                f64::from(unit(seed + 1) - 0.5) * 0.4,
                height,
                f64::from(unit(seed + 2) - 0.5) * 0.4,
            );
            self.particles.push(Particle {
                start: from.lerp(feet, along) + jitter,
                born: now,
                sprite: (unit(seed + 3) * trail.sprites as f32) as u8 % trail.sprites as u8,
                size: trail.size[0] + (trail.size[1] - trail.size[0]) * unit(seed + 1),
                phase: unit(seed + 2) * std::f32::consts::TAU,
            });
        }
        // What can't be spawned now isn't owed later: the trail thins, it never bursts.
        self.owed = self.owed.min(1.0);
    }

    /// Drops every particle (the wearer left the view, or took the trail off).
    pub fn clear(&mut self, budget: &mut Budget) {
        budget.alive -= self.particles.len().min(budget.alive);
        *self = Self::default();
    }

    /// The quads of the particles alive at `now`, facing a camera at `camera` whose right and up
    /// are `right` and `up` (unit).
    pub fn quads<'a>(
        &'a self,
        trail: &'a Trail,
        now: f64,
        camera: DVec3,
        right: Vec3,
        up: Vec3,
    ) -> impl Iterator<Item = Quad> + 'a {
        let life = trail.lifetime as f32;
        self.particles.iter().filter_map(move |p| {
            let age = (now - p.born) as f32;
            if !(0.0..life).contains(&age) {
                return None;
            }
            let sway = trail.drift / 16.0 * (age * 0.2 + p.phase).sin();
            let fall = 0.5 * trail.gravity * age * age;
            let at = (p.start - camera).as_vec3()
                + Vec3::new(sway * p.phase.cos(), -fall, sway * p.phase.sin());
            let spin = (trail.spin * age).to_radians() + p.phase;
            let (s, c) = spin.sin_cos();
            let half = p.size / 32.0;
            let (r, u) = ((right * c + up * s) * half, (up * c - right * s) * half);
            let w = 1.0 / trail.sprites as f32;
            let (u0, u1) = (p.sprite as f32 * w, (p.sprite as f32 + 1.0) * w);
            let alpha = match trail.fade {
                true => (3.0 * (1.0 - age / life)).min(1.0),
                false => 1.0,
            };
            Some(Quad {
                corners: [at - r - u, at + r - u, at + r + u, at - r + u],
                uv: [[u0, 1.0], [u1, 1.0], [u1, 0.0], [u0, 0.0]],
                alpha,
            })
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::From;

    fn leaves(rate: f32, lifetime: u32) -> Trail {
        Trail {
            sprites: 4,
            rate,
            lifetime,
            size: [2.0, 3.0],
            gravity: 0.01,
            drift: 1.5,
            spin: 8.0,
            from: From::Feet,
            fade: true,
            glow: false,
        }
    }

    /// Walks `blocks` along x in `frames` frames of a tick each from tick `t0`.
    fn walk(e: &mut Emitter, t: &Trail, blocks: f64, frames: u32, t0: f64, budget: &mut Budget) {
        for i in 0..=frames {
            let x = blocks * f64::from(i) / f64::from(frames);
            e.step(t, DVec3::new(x, 64.0, 0.0), true, t0 + f64::from(i), budget);
        }
    }

    #[test]
    fn spawns_by_the_distance_walked_and_nothing_standing_or_sneaking() {
        let t = leaves(2.0, 40);
        let mut e = Emitter::default();
        // 5 blocks at 2 a block: 10 particles.
        walk(&mut e, &t, 5.0, 20, 0.0, &mut Budget::default());
        assert_eq!(e.len(), 10);
        // Standing still for the rest of their lives: none more, then none at all.
        for i in 21..=60 {
            e.step(
                &t,
                DVec3::new(5.0, 64.0, 0.0),
                true,
                f64::from(i),
                &mut Budget::default(),
            );
        }
        assert!(e.is_empty());
        // Sneaking along: nothing.
        let mut sneaking = Emitter::default();
        for i in 0..=20 {
            let feet = DVec3::new(f64::from(i) * 0.25, 64.0, 0.0);
            sneaking.step(&t, feet, false, f64::from(i), &mut Budget::default());
        }
        assert!(sneaking.is_empty());
    }

    #[test]
    fn a_teleport_leaves_nothing_and_particles_live_their_lifetime() {
        let t = leaves(4.0, 10);
        let mut e = Emitter::default();
        e.step(&t, DVec3::ZERO, true, 0.0, &mut Budget::default());
        e.step(
            &t,
            DVec3::new(100.0, 0.0, 0.0),
            true,
            1.0,
            &mut Budget::default(),
        );
        assert!(e.is_empty());
        e.step(
            &t,
            DVec3::new(101.0, 0.0, 0.0),
            true,
            2.0,
            &mut Budget::default(),
        );
        assert_eq!(e.len(), 4);
        // Born at tick 2, alive through 11, gone at 12.
        let quads = |e: &Emitter, now| e.quads(&t, now, DVec3::ZERO, Vec3::X, Vec3::Y).count();
        assert_eq!(quads(&e, 11.5), 4);
        e.step(
            &t,
            DVec3::new(101.0, 0.0, 0.0),
            true,
            12.0,
            &mut Budget::default(),
        );
        assert!(e.is_empty());
    }

    #[test]
    fn the_per_player_and_global_budgets_hold() {
        let t = leaves(4.0, 40);
        // Sprinting for 2 s: 4 a block would make 44; one wearer keeps 24.
        let mut e = Emitter::default();
        walk(&mut e, &t, 11.0, 40, 0.0, &mut Budget::default());
        assert_eq!(e.len(), PER_PLAYER);
        // 64 wearers sprinting side by side, one frame each: never past the shared budget.
        let mut crowd = vec![Emitter::default(); 64];
        let mut alive = 0;
        let mut budget = Budget::default();
        for tick in 0..=40 {
            for (i, e) in crowd.iter_mut().enumerate() {
                let feet = DVec3::new(f64::from(tick) * 0.28, 64.0, i as f64 * 2.0);
                e.step(&t, feet, true, f64::from(tick), &mut budget);
            }
            alive = crowd.iter().map(Emitter::len).sum::<usize>();
            assert!(alive <= BUDGET, "{alive} alive at tick {tick}");
            assert_eq!(budget.alive, alive);
        }
        assert_eq!(alive, BUDGET);
        // One leaves the view: its share goes back.
        let gone = crowd[0].len();
        crowd[0].clear(&mut budget);
        assert_eq!(budget.alive, BUDGET - gone);
    }

    #[test]
    fn quads_face_the_camera_fall_and_fade() {
        let t = leaves(1.0, 30);
        let mut e = Emitter::default();
        e.step(&t, DVec3::ZERO, true, 0.0, &mut Budget::default());
        e.step(
            &t,
            DVec3::new(1.0, 0.0, 0.0),
            true,
            0.0,
            &mut Budget::default(),
        );
        let (right, up) = (Vec3::X, Vec3::Y);
        let at = |now| e.quads(&t, now, DVec3::ZERO, right, up).next().unwrap();
        let young = at(0.0);
        // In the camera's plane: no depth across the quad.
        let normal =
            (young.corners[1] - young.corners[0]).cross(young.corners[3] - young.corners[0]);
        assert!(normal.normalize().abs().z > 0.999);
        // 2 to 3 pixels across.
        let side = (young.corners[1] - young.corners[0]).length() * 16.0;
        assert!((2.0..=3.0).contains(&side), "{side}");
        // Falls by gravity: half of 0.01 * 20² = 2 blocks in 20 ticks.
        let centre = |q: Quad| q.corners.iter().copied().sum::<Vec3>() / 4.0;
        let fell = centre(young).y - centre(at(20.0)).y;
        assert!((fell - 2.0).abs() < 1e-3, "{fell}");
        // Opaque for its first two thirds, then fading out.
        assert_eq!((young.alpha, at(19.0).alpha), (1.0, 1.0));
        assert!((at(25.0).alpha - 0.5).abs() < 1e-5);
        // Each shows one of the 4 sprites: a quarter of the texture across.
        let [a, b, ..] = young.uv;
        assert!((b[0] - a[0] - 0.25).abs() < 1e-6);
    }
}
