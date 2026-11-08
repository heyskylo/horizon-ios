//! How resource packs' Polytone particles are drawn (#11486): the rotation modes of custom
//! particles, and GPU particles (wiki "GPU Particles"). A GPU particle never ticks: its spawn is a
//! record (position, velocity, spawn time, lifetime, seed, colour, size, roll), and where it is,
//! how big, what colour and which frame are a function of that record and the time. Here the
//! function runs when the frame's particle quads are built, beside the engine's own particles.
use super::{ParticleLevel, ParticleLightCache};
use crate::hud::Rect;
use crate::mesher::Vertex;
use glam::{DVec3, Mat3, Quat, Vec3 as FVec3};
use horizon_polytone::particles::{CustomParticle, Gpu, RenderType};
use std::collections::HashMap;
use std::sync::Arc;

/// How a particle quad turns (`rotation_mode`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum Orientation {
    /// `look_at_xyz`: faces the camera (what every other mode falls back to).
    #[default]
    Camera,
    /// `look_at_y`: turns around Y only.
    AroundY,
    /// `look_up`: lies flat, facing up.
    Flat,
    /// `movement_aligned`: stands along its velocity, facing the camera around it.
    Movement,
}

impl Orientation {
    pub(super) fn parse(mode: &str) -> Self {
        match mode {
            "look_at_y" => Self::AroundY,
            "look_up" => Self::Flat,
            "movement_aligned" => Self::Movement,
            _ => Self::Camera,
        }
    }
}

/// The rotation of a quad (corners in its XY plane, facing +Z) at `center` (relative to the
/// camera) moving along `velocity`, rolled by `roll`; `camera` is the camera-facing rotation.
pub(super) fn orient(
    mode: Orientation,
    center: FVec3,
    velocity: FVec3,
    roll: f32,
    camera: Quat,
) -> Quat {
    let rolled = |q: Quat| q * Quat::from_rotation_z(roll);
    match mode {
        Orientation::Camera => rolled(camera),
        Orientation::AroundY => rolled(Quat::from_rotation_y((-center.x).atan2(-center.z))),
        Orientation::Flat => rolled(Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2)),
        Orientation::Movement => {
            let up = velocity.normalize_or_zero();
            let right = up.cross((-center).normalize_or_zero()).normalize_or_zero();
            if right == FVec3::ZERO {
                return rolled(camera);
            }
            Quat::from_mat3(&Mat3::from_cols(right, up, right.cross(up)))
        }
    }
}

/// One GPU particle spawn.
#[derive(Clone, Copy, Debug)]
struct Record {
    position: [f64; 3],
    velocity: [f64; 3],
    /// The engine's tick it spawned on, and its life in ticks.
    spawn: u64,
    lifetime: f32,
    seed: u64,
    rgba: [f32; 4],
    size: f32,
    roll: f32,
}

/// A GPU particle type's records: a ring of `limit` slots, the oldest overwritten.
pub(super) struct Store {
    def: Arc<CustomParticle>,
    gpu: Gpu,
    orientation: Orientation,
    /// The texture's sprite name in the particle atlas (`ns:particle/path`).
    sprite: String,
    records: Vec<Record>,
    next: usize,
    limit: usize,
}

/// The atlas name of a full texture path: `ns:textures/particle/a/b.png` is `ns:particle/a/b`
/// (`particle/a/b` in `minecraft`).
fn sprite_name(texture: &str) -> String {
    let (namespace, path) = texture.split_once(':').unwrap_or(("minecraft", texture));
    let path = path
        .trim_start_matches("textures/")
        .trim_end_matches(".png");
    if namespace == "minecraft" {
        path.to_owned()
    } else {
        format!("{namespace}:{path}")
    }
}

/// SplitMix64: a well-mixed number from `seed` and `i`, for each quad's own randoms.
fn mix(seed: u64, i: u64) -> u64 {
    let mut z = seed ^ i.wrapping_mul(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// A number in 0..1 from `mix`.
fn unit(seed: u64, i: u64) -> f64 {
    (mix(seed, i) >> 11) as f64 / (1u64 << 53) as f64
}

/// Where a record's particle is after `t` ticks, and how fast it moves: gravity pulls down,
/// friction (`friction` kept per tick, as a continuous drag) slows it toward a terminal velocity.
fn motion(gpu: &Gpu, position: [f64; 3], velocity: [f64; 3], t: f64) -> ([f64; 3], [f64; 3]) {
    let drag = if gpu.friction > 0.0 && gpu.friction < 1.0 {
        -gpu.friction.ln()
    } else {
        0.0
    };
    let gravity = [0.0, -gpu.gravity, 0.0];
    let mut at = [0.0; 3];
    let mut moving = [0.0; 3];
    for i in 0..3 {
        let (v, g) = (velocity[i], gravity[i]);
        if drag > 1.0e-9 {
            let e = (-drag * t).exp();
            at[i] = position[i] + v * (1.0 - e) / drag + g * (t / drag - (1.0 - e) / (drag * drag));
            moving[i] = v * e + g * (1.0 - e) / drag;
        } else {
            at[i] = position[i] + v * t + 0.5 * g * t * t;
            moving[i] = v + g * t;
        }
    }
    (at, moving)
}

impl Store {
    pub(super) fn new(def: &Arc<CustomParticle>) -> Option<Self> {
        let gpu = def.gpu.clone()?;
        Some(Self {
            def: def.clone(),
            orientation: Orientation::parse(&def.rotation_mode),
            sprite: sprite_name(&gpu.texture),
            gpu,
            records: Vec::new(),
            next: 0,
            // The wiki's default ring capacity.
            limit: def.limit.map_or(16_384, |l| l.max(1) as usize),
        })
    }

    pub(super) fn is(&self, def: &Arc<CustomParticle>) -> bool {
        Arc::ptr_eq(&self.def, def)
    }

    /// Records a spawn from a particle its initializer set up (position, motion, life, look).
    pub(super) fn spawn(&mut self, p: &super::Particle, now: u64, seed: u64) {
        if p.lifetime <= 0 {
            return;
        }
        let record = Record {
            position: p.position,
            velocity: p.velocity,
            spawn: now,
            lifetime: p.lifetime as f32,
            seed,
            rgba: p.rgba,
            size: p.size,
            roll: p.roll,
        };
        if self.records.len() < self.limit {
            self.records.push(record);
        } else {
            self.records[self.next] = record;
            self.next = (self.next + 1) % self.limit;
        }
    }

    /// Forgets the records whose life is over.
    pub(super) fn expire(&mut self, now: u64) {
        let before = self.records.len();
        self.records
            .retain(|r| (now.saturating_sub(r.spawn) as f32) < r.lifetime);
        if self.records.len() != before {
            self.next = 0;
        }
    }

    pub(super) fn len(&self) -> usize {
        self.records.len()
    }

    /// The quads of every live record at `now + partial` ticks.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn render(
        &self,
        atlas: &HashMap<String, (Rect, bool)>,
        camera: DVec3,
        facing: Quat,
        now: u64,
        partial: f32,
        light_cache: &mut ParticleLightCache,
        light_at: &dyn Fn([i32; 3]) -> u8,
        out: &mut Vec<Vertex>,
        additive: &mut Vec<Vertex>,
    ) {
        if self.def.render_type == RenderType::Invisible {
            return;
        }
        let Some(&(rect, _)) = atlas.get(&self.sprite) else {
            return;
        };
        let gpu = &self.gpu;
        let translucent = matches!(
            self.def.render_type,
            RenderType::Translucent | RenderType::AdditiveTranslucent
        );
        let blend_add = self.def.render_type == RenderType::AdditiveTranslucent;
        let target = if blend_add { additive } else { out };
        let layer = if translucent && !blend_add {
            crate::gfx::TRANSLUCENT_PARTICLE_OVERLAY
        } else {
            0
        };
        let (count, area) = gpu.area.map_or((1, [0.0; 3]), |(size, n)| (n, size));
        let frames = gpu.frames.max(1);
        for r in &self.records {
            let t = (now.saturating_sub(r.spawn)) as f32 + partial;
            if t >= r.lifetime {
                continue;
            }
            let life = (t / r.lifetime).clamp(0.0, 1.0);
            let (at, moving) = motion(gpu, r.position, r.velocity, f64::from(t));
            // Fading in and out, and the colour and size toward their ends.
            let [fade_in, fade_out] = gpu.fade.map(|f| f as f32);
            let mut alpha = r.rgba[3];
            if fade_in > 0.0 {
                alpha *= (life / fade_in).min(1.0);
            }
            if fade_out > 0.0 {
                alpha *= ((1.0 - life) / fade_out).min(1.0);
            }
            let mut rgb = [r.rgba[0], r.rgba[1], r.rgba[2]];
            if let Some(end) = gpu.color_end {
                let channel = |shift: u32| ((end >> shift) & 0xff) as f32 / 255.0;
                let to = [channel(16), channel(8), channel(0)];
                for i in 0..3 {
                    rgb[i] += (to[i] - rgb[i]) * life;
                }
                alpha += (alpha * channel(24) - alpha) * life;
            }
            let size = gpu
                .size_end
                .map_or(r.size, |end| r.size + (end as f32 - r.size) * life);
            let c = |v: f32| (v.clamp(0.0, 1.0) * 255.0) as u32;
            let color = c(alpha) << 24 | c(rgb[2]) << 16 | c(rgb[1]) << 8 | c(rgb[0]);
            let frame = if self.def.random_sprite {
                (mix(r.seed, 0) % u64::from(frames)) as u32
            } else {
                ((life * frames as f32) as u32).min(frames - 1)
            };
            let (dv, v0) = ((rect.v1 - rect.v0) / frames as f32, rect.v0);
            let (u0, u1) = (rect.u0, rect.u1);
            let (v0, v1) = (v0 + dv * frame as f32, v0 + dv * (frame + 1) as f32);
            let velocity = FVec3::new(moving[0] as f32, moving[1] as f32, moving[2] as f32);
            for quad in 0..u64::from(count) {
                // Each quad of an area has its own place in the box, sway phase and roll.
                let offset: [f64; 3] = if count > 1 {
                    std::array::from_fn(|i| (unit(r.seed, quad * 8 + i as u64 + 1) - 0.5) * area[i])
                } else {
                    [0.0; 3]
                };
                let phase = unit(r.seed, quad * 8 + 4) * std::f64::consts::TAU;
                let sway = gpu.sway * (f64::from(t) * 0.1 + phase).sin();
                let sway_z = gpu.sway * (f64::from(t) * 0.07 + phase).cos();
                let world = [
                    at[0] + offset[0] + sway,
                    at[1] + offset[1],
                    at[2] + offset[2] + sway_z,
                ];
                let block = ParticleLevel::block_at(world);
                let packed = light_cache.get(block, light_at);
                // kill_below_heightmap: under the surface, where the sky doesn't reach.
                if gpu.kill_below_heightmap && packed >> 4 < 15 {
                    continue;
                }
                let block_light = u32::from((packed & 15).max(self.def.light_level));
                let light = (u32::from(packed >> 4) * 16) | ((block_light * 16) << 8);
                let center = (DVec3::from_array(world) - camera).as_vec3();
                let roll = r.roll
                    + gpu.spin as f32 * t
                    + if count > 1 {
                        (unit(r.seed, quad * 8 + 5) * std::f64::consts::TAU) as f32
                    } else {
                        0.0
                    };
                let rotation = orient(self.orientation, center, velocity, roll, facing);
                let half = FVec3::new(size, size * gpu.aspect as f32, 0.0);
                for (n, uv) in [
                    ((1.0, -1.0), [u1, v1]),
                    ((1.0, 1.0), [u1, v0]),
                    ((-1.0, 1.0), [u0, v0]),
                    ((-1.0, -1.0), [u0, v1]),
                ] {
                    let corner = FVec3::new(n.0 * half.x, n.1 * half.y, 0.0);
                    target.push(Vertex {
                        normal: [0; 4],
                        pos: (center + rotation * corner).to_array(),
                        uv,
                        layer,
                        color,
                        light,
                        glint_uv: [0.0, 0.0],
                    });
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gpu(gravity: f64, friction: f64) -> Gpu {
        Gpu {
            texture: String::new(),
            frames: 1,
            gravity,
            friction,
            size_end: None,
            color_end: None,
            fade: [0.0; 2],
            sway: 0.0,
            spin: 0.0,
            aspect: 1.0,
            area: None,
            kill_below_heightmap: false,
        }
    }

    #[test]
    fn texture_paths_name_their_atlas_sprites() {
        assert_eq!(
            sprite_name("davidparticles:textures/particle/weather/rain.png"),
            "davidparticles:particle/weather/rain"
        );
        assert_eq!(
            sprite_name("minecraft:textures/particle/generic_7.png"),
            "particle/generic_7"
        );
    }

    #[test]
    fn motion_follows_gravity_and_drag() {
        // No drag: a thrown body, x = v t, y = -g t^2 / 2.
        let (at, v) = motion(&gpu(0.1, 1.0), [0.0; 3], [1.0, 0.0, 0.0], 10.0);
        assert_eq!(at, [10.0, -5.0, 0.0]);
        assert!((v[1] + 1.0).abs() < 1e-12);
        // Drag: the fall speed tends to g / drag (terminal velocity).
        let friction = 0.9;
        let (_, v) = motion(&gpu(0.1, friction), [0.0; 3], [0.0; 3], 1000.0);
        assert!((v[1] + 0.1 / -f64::ln(friction)).abs() < 1e-9);
    }

    #[test]
    fn orientations_turn_the_quad_normal() {
        let normal = |q: Quat| q * FVec3::Z;
        let close = |a: FVec3, b: FVec3| (a - b).length() < 1e-5;
        // Lying flat: facing up.
        assert!(close(
            normal(orient(
                Orientation::Flat,
                FVec3::ONE,
                FVec3::ZERO,
                0.0,
                Quat::IDENTITY
            )),
            FVec3::Y
        ));
        // Around Y: facing the camera horizontally (the camera at the origin, the quad east).
        assert!(close(
            normal(orient(
                Orientation::AroundY,
                FVec3::new(5.0, 3.0, 0.0),
                FVec3::ZERO,
                0.0,
                Quat::IDENTITY
            )),
            -FVec3::X
        ));
        // Along its motion: its up is the velocity, its face toward the camera.
        let q = orient(
            Orientation::Movement,
            FVec3::new(0.0, 0.0, -4.0),
            FVec3::new(0.0, -2.0, 0.0),
            0.0,
            Quat::IDENTITY,
        );
        assert!(close(q * FVec3::Y, -FVec3::Y));
        assert!(close(normal(q), FVec3::Z));
    }
}
