//! Companions, trails and auras ([docs/cosmetics.md](../../../../../docs/cosmetics.md)): worn
//! like the other cosmetics but on no bone. A companion floats beside its wearer and follows with
//! a lag (`horizon_cosmetics::follow`), drawn with the wearer; a trail's particles
//! (`horizon_cosmetics::trail`) and an aura blend, so they are drawn see-through after every
//! player (`PlayerDraw::particles`, `Look::see_through`). Nothing of them is an entity, collides
//! or is sent to a server.

use super::Seat;
use glam::{DVec3, Mat4, Vec3};
use horizon_client::fast_hash::FxHashMap;
use horizon_cosmetics::follow::{self, Follow};
use horizon_cosmetics::trail::{Budget, Emitter};
use horizon_cosmetics::{Item, Slot};
use horizon_hooks::players::{Kind, Light, Look, Particle, Pass, PlayerDraw, PlayerModel};
use std::sync::Arc;
use std::time::Instant;

/// How far from the camera (blocks) a wearer's companion, trail and aura are drawn.
const FAR: f32 = 48.0;

/// The key of the player's own model in `Fx`'s maps (others go by entity id).
const OWN: u32 = u32::MAX;

/// What companions and trails keep between frames, per wearer (marked with the frame they were
/// last drawn in; the others are dropped).
#[derive(Default)]
pub(super) struct Fx {
    companions: FxHashMap<u32, (Follow, u32)>,
    trails: FxHashMap<u32, (Emitter, u32)>,
    budget: Budget,
    clock: Option<(Instant, Instant)>,
    /// Ticks since the first frame, and the last frame's length in seconds.
    now: f64,
    dt: f32,
    frame: u32,
    /// Kept so a frame of trails allocates none.
    particles: Vec<Particle>,
}

impl Fx {
    /// A new frame: the clock moves on and what no frame drew since the last one goes.
    pub(super) fn frame(&mut self) {
        self.frame = self.frame.wrapping_add(1);
        let frame = self.frame;
        let now = Instant::now();
        let (start, last) = self.clock.get_or_insert((now, now));
        self.dt = now.duration_since(*last).as_secs_f32().min(0.25);
        self.now = now.duration_since(*start).as_secs_f64() * 20.0;
        *last = now;
        let seen = |mark: u32| frame.wrapping_sub(mark) <= 1;
        self.companions.retain(|_, (_, mark)| seen(*mark));
        let budget = &mut self.budget;
        self.trails.retain(|_, (e, mark)| {
            let keep = seen(*mark);
            if !keep {
                e.clear(budget);
            }
            keep
        });
    }

    /// The companion of wearer `key` moved toward `target` (once a frame, however many times it
    /// is drawn).
    fn follow(&mut self, key: u32, target: DVec3, yaw: f32, lag: f32) -> Follow {
        let (frame, dt) = (self.frame, self.dt);
        let (f, mark) = self
            .companions
            .entry(key)
            .or_insert((Follow::at(target, yaw), frame));
        if *mark != frame {
            f.step(target, yaw, lag, dt);
            *mark = frame;
        }
        *f
    }
}

/// Where a companion floats: `at` (relative to the camera) facing `yaw`, at the wearer's scale,
/// in the model space the item's file uses (y down).
fn companion_matrix(at: Vec3, yaw: f32, scale: f32) -> Mat4 {
    Mat4::from_translation(at)
        * Mat4::from_rotation_y((180.0 - yaw).to_radians())
        * Mat4::from_scale(Vec3::new(-scale, -scale, scale))
}

/// Full light for what glows, else `otherwise`.
fn glowing(glow: bool, otherwise: Light) -> Light {
    if glow { Light::Full } else { otherwise }
}

impl Seat {
    /// A companion, trail or aura `m` wears, around its model as `draw` draws it. Other players'
    /// trails and auras stay hidden with "Hide other players' trails and auras".
    pub(super) fn fx(&mut self, m: &PlayerModel, item: &Arc<Item>, draw: &mut dyn PlayerDraw) {
        let own = m.kind == Kind::Own;
        if !own && item.slot.is_effect() && self.said.hide_fx {
            return;
        }
        // Past `FAR` a companion, trail or aura is a few pixels across for the cost of a model:
        // not drawn, and a trail that isn't drawn doesn't spawn (its particles go back to the
        // budget).
        let base = Mat4::from_cols_array_2d(&draw.base());
        if base.w_axis.truncate().length_squared() > FAR * FAR {
            return;
        }
        let Some(uv) = self.uv(item, |id| draw.sprite(id)) else {
            return;
        };
        let key = if own { OWN } else { m.id };
        match item.slot {
            Slot::Companion => self.companion(key, item, uv, base, draw),
            Slot::Trail => self.trail(key, item, uv, draw),
            Slot::Aura => self.aura(item, uv, base, draw),
            _ => {}
        }
    }

    /// The companion where it floats, its follow moved on once a frame. In first person it keeps
    /// out of the middle of the view, lit where it floats.
    fn companion(
        &mut self,
        key: u32,
        item: &Arc<Item>,
        uv: [f32; 4],
        base: Mat4,
        draw: &mut dyn PlayerDraw,
    ) {
        let Some(c) = item.companion.as_ref() else {
            return;
        };
        let Some((_, mesh)) = self.kept(item).meshes.first().cloned() else {
            return;
        };
        let first_person = draw.pass() == Pass::FirstPerson;
        let (motion, view) = (draw.motion(), draw.view());
        let camera = DVec3::from(view.camera);
        let scale = base.x_axis.truncate().length();
        let target = base.transform_point3(Vec3::from(c.anchor) / 16.0);
        let head = base.transform_point3(Vec3::from(horizon_cosmetics::HEAD) / 16.0);
        let follow = self
            .fx
            .follow(key, camera + target.as_dvec3(), motion.head_yaw, c.lag);
        let clearance = (horizon_cosmetics::HEAD_CLEARANCE + c.reach) / 16.0 * scale;
        let mut rel = (follow.pos - camera).as_vec3();
        if first_person {
            rel = follow::out_of_view(rel, Vec3::from(view.forward), Vec3::from(view.left));
        }
        rel = follow::clear_of(rel, head, clearance);
        let matrix = companion_matrix(rel, follow.yaw, scale).to_cols_array_2d();
        let near = if first_person {
            Light::AtPlace
        } else {
            Light::Model
        };
        let look = Look::new(uv).lit(glowing(c.glow, near));
        let ticks = draw.ticks();
        // Its animations, and its `look` part turned with the wearer's pitch.
        let pitch = (motion.head_pitch * 0.6).clamp(-30.0, 30.0).to_radians();
        draw.draw(&mesh, matrix, look, &mut |p| {
            item.animate(ticks, |name, pose| *p.pose(name) = pose);
            if let Some(part) = c.look.as_deref() {
                p.pose(part)[3] += pitch;
            }
        });
    }

    /// The trail's particles: spawned once a frame by the distance walked (not while sneaking or
    /// asleep), facing the camera.
    fn trail(&mut self, key: u32, item: &Arc<Item>, uv: [f32; 4], draw: &mut dyn PlayerDraw) {
        let Some(t) = &item.trail else {
            return;
        };
        let (motion, view) = (draw.motion(), draw.view());
        let moves = !motion.crouching && !motion.asleep;
        let Fx {
            trails,
            budget,
            now,
            frame,
            particles,
            ..
        } = &mut self.fx;
        let (e, mark) = trails
            .entry(key)
            .or_insert_with(|| (Emitter::default(), frame.wrapping_sub(1)));
        if *mark != *frame {
            e.step(t, DVec3::from(motion.feet), moves, *now, budget);
            *mark = *frame;
        }
        let (camera, right, up) = (
            DVec3::from(view.camera),
            Vec3::from(view.right),
            Vec3::from(view.up),
        );
        particles.clear();
        particles.extend(
            e.quads(t, *now, camera, right, up)
                .map(|q| Particle::new(q.corners.map(Vec3::to_array), q.uv, q.alpha)),
        );
        draw.particles(uv, particles, glowing(t.glow, Light::Model));
    }

    /// The aura at the wearer's feet, see-through. None while they fly, swim or sleep: it would
    /// tip over with the body.
    fn aura(&mut self, item: &Arc<Item>, uv: [f32; 4], base: Mat4, draw: &mut dyn PlayerDraw) {
        let Some(a) = &item.aura else {
            return;
        };
        let motion = draw.motion();
        if motion.flying || motion.asleep || motion.swimming {
            return;
        }
        let Some((_, mesh)) = self.kept(item).meshes.first().cloned() else {
            return;
        };
        // The model's origin is the middle of the feet: 24 pixels under the neck.
        let matrix = base * Mat4::from_translation(Vec3::new(0.0, 24.0 / 16.0, 0.0));
        let look = Look::new(uv)
            .lit(glowing(a.glow, Light::Model))
            .see_through(a.alpha);
        let ticks = draw.ticks();
        draw.draw(&mesh, matrix.to_cols_array_2d(), look, &mut |p| {
            item.animate(ticks, |name, pose| *p.pose(name) = pose)
        });
    }
}
