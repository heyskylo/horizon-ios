//! Companions, trails and auras ([docs/cosmetics.md](../../../../../docs/cosmetics.md)): worn
//! like the other cosmetics but on no bone. A companion floats beside its wearer and follows with
//! a lag (`horizon_cosmetics::follow`), drawn with the wearer; a trail's particles
//! (`horizon_cosmetics::trail`) and an aura blend, so they go after every player in one
//! see-through span (`cosmetics_fx_geometry`). Nothing of them is an entity, collides or is sent
//! to a server.

use super::{FULL_LIGHT, OWN};
use crate::game::Game;
use glam::{DVec3, Mat4, Vec3};
use horizon_client::mesher::Vertex;
use horizon_client::mobs::anim::Poses;
use horizon_client::mobs::{Mob, Part};
use horizon_cosmetics::follow::{self, Follow};
use horizon_cosmetics::trail::Emitter;
use horizon_cosmetics::{Item, Slot};
use std::sync::Arc;

/// How far from the camera (blocks) a wearer's companion, trail and aura are drawn.
const FAR: f32 = 48.0;

/// Who wears it, as drawn this frame.
pub(super) struct Wearer<'a> {
    pub m: &'a Mob,
    /// The player's own model.
    pub own: bool,
    /// `OWN`, or the entity id.
    pub key: u32,
    /// The wearer's model matrix (`render::base_transform`), relative to `camera`.
    pub base: Mat4,
    pub camera: DVec3,
    pub alpha: f32,
    pub light: u32,
}

/// `m`'s head yaw this frame (degrees).
fn head_yaw(m: &Mob, alpha: f32) -> f32 {
    let turn = horizon_client::mobs::living::wrap(m.head_yaw - m.previous_head_yaw);
    m.previous_head_yaw + turn * alpha
}

/// Whether `m` lies in a bed.
fn asleep(m: &Mob) -> bool {
    m.extra.avatar_pose == horizon_client::pose::Pose::Sleeping
}

/// Where a companion floats: `at` (relative to the camera) facing `yaw`, at the wearer's scale,
/// in the model space the item's file uses (y down).
fn companion_matrix(at: Vec3, yaw: f32, scale: f32) -> Mat4 {
    Mat4::from_translation(at)
        * Mat4::from_rotation_y((180.0 - yaw).to_radians())
        * Mat4::from_scale(Vec3::new(-scale, -scale, scale))
}

impl Game<'_> {
    /// A companion, trail or aura `at` wears: a companion goes where the wearer's other items go
    /// (`gpu` and `out`, `bounds` grown), trails and auras to the frame's see-through vertices.
    /// Other players' trails and auras stay hidden with "Hide other players' trails and auras".
    pub(super) fn cosmetics_fx(
        &self,
        at: &Wearer,
        item: &Arc<Item>,
        gpu: bool,
        bounds: Option<(Vec3, Vec3)>,
        poses: &mut Poses,
        out: &mut Vec<Vertex>,
    ) -> Option<(Vec3, Vec3)> {
        if !at.own && item.slot.is_effect() && self.settings.extras.cosmetics_hide_fx {
            return bounds;
        }
        // Past `FAR` a companion, trail or aura is a few pixels across for the cost of a model:
        // not drawn, and a trail that isn't drawn doesn't spawn (its particles go back to the
        // budget).
        if at.base.w_axis.truncate().length_squared() > FAR * FAR {
            return bounds;
        }
        match item.slot {
            Slot::Companion => {
                let Some((model, uv, matrix)) = self.companion_at(at, item, None) else {
                    return bounds;
                };
                self.pose_companion(item, at.m, at.alpha, poses, &model);
                let glow = item.companion.as_ref().is_some_and(|c| c.glow);
                let light = if glow { FULL_LIGHT } else { at.light };
                self.cosmetics_model(&model, matrix, poses, uv, light, gpu, bounds, out)
            }
            Slot::Trail => {
                self.trail(at, item);
                bounds
            }
            Slot::Aura => {
                self.aura(at, item, poses);
                bounds
            }
            _ => bounds,
        }
    }

    /// The companion's model, texture and matrix this frame, its follow moved on once a frame.
    /// `first_person`: the camera's forward and left, for the player's own in first person: the
    /// companion keeps out of the middle of the view.
    fn companion_at(
        &self,
        at: &Wearer,
        item: &Arc<Item>,
        first_person: Option<(Vec3, Vec3)>,
    ) -> Option<(Arc<Part>, [f32; 4], Mat4)> {
        let c = item.companion.as_ref()?;
        let (models, uv) = self.cosmetics_baked(item)?;
        let model = models.first()?.1.clone();
        let scale = at.base.x_axis.truncate().length();
        let target = at.base.transform_point3(Vec3::from(c.anchor) / 16.0);
        let head = at
            .base
            .transform_point3(Vec3::from(horizon_cosmetics::HEAD) / 16.0);
        let yaw = head_yaw(at.m, at.alpha);
        let follow = self.companion_follow(at.key, at.camera + target.as_dvec3(), yaw, c.lag);
        let clearance = (horizon_cosmetics::HEAD_CLEARANCE + c.reach) / 16.0 * scale;
        let mut rel = (follow.pos - at.camera).as_vec3();
        if let Some((forward, left)) = first_person {
            rel = follow::out_of_view(rel, forward, left);
        }
        rel = follow::clear_of(rel, head, clearance);
        Some((model, uv, companion_matrix(rel, follow.yaw, scale)))
    }

    /// The companion of wearer `key` moved toward `target` (once a frame, however many times it
    /// is drawn).
    fn companion_follow(&self, key: u32, target: DVec3, yaw: f32, lag: f32) -> Follow {
        let frame = self.cosmetics.frame.get();
        let mut fx = self.cosmetics.fx.borrow_mut();
        let dt = fx.dt;
        let (f, mark) = fx
            .companions
            .entry(key)
            .or_insert((Follow::at(target, yaw), frame));
        if *mark != frame {
            f.step(target, yaw, lag, dt);
            *mark = frame;
        }
        *f
    }

    /// The companion's animations, and its `look` part turned with the wearer's pitch.
    fn pose_companion(&self, item: &Item, m: &Mob, alpha: f32, poses: &mut Poses, model: &Part) {
        poses.reset(model);
        item.animate(m.age as f32 + alpha, |name, pose| *poses.get(name) = pose);
        if let Some(look) = item.companion.as_ref().and_then(|c| c.look.as_deref()) {
            let pitch = m.previous_head_pitch + (m.head_pitch - m.previous_head_pitch) * alpha;
            poses.get(look)[3] += (pitch * 0.6).clamp(-30.0, 30.0).to_radians();
        }
    }

    /// The trail's particles: spawned once a frame by the distance walked (not while sneaking or
    /// asleep), facing the camera.
    fn trail(&self, at: &Wearer, item: &Arc<Item>) {
        let Some(t) = &item.trail else {
            return;
        };
        let Some((_, uv)) = self.cosmetics_baked(item) else {
            return;
        };
        let m = at.m;
        let a = f64::from(at.alpha);
        let feet = DVec3::new(
            m.previous.x + (m.position.x - m.previous.x) * a,
            m.previous.y + (m.position.y - m.previous.y) * a,
            m.previous.z + (m.position.z - m.previous.z) * a,
        );
        let moves = !m.extra.crouching && !asleep(m);
        let q = self.camera_orientation();
        let (right, up) = (q * Vec3::X, q * Vec3::Y);
        let light = if t.glow { FULL_LIGHT } else { at.light };
        let frame = self.cosmetics.frame.get();
        let mut fx = self.cosmetics.fx.borrow_mut();
        let super::Fx {
            trails,
            budget,
            now,
            clear,
            ..
        } = &mut *fx;
        let (e, mark) = trails
            .entry(at.key)
            .or_insert_with(|| (Emitter::default(), frame.wrapping_sub(1)));
        if *mark != frame {
            e.step(t, feet, moves, *now, budget);
            *mark = frame;
        }
        for quad in e.quads(t, *now, at.camera, right, up) {
            let color = ((quad.alpha.clamp(0.0, 1.0) * 255.0) as u32) << 24 | 0x00FF_FFFF;
            for (corner, [u, v]) in quad.corners.iter().zip(quad.uv) {
                clear.push(Vertex {
                    pos: corner.to_array(),
                    uv: [uv[0] + (uv[2] - uv[0]) * u, uv[1] + (uv[3] - uv[1]) * v],
                    layer: 0,
                    color,
                    light,
                    // Unlit: a particle keeps its colour whichever way it faces.
                    normal: [0, 0, 0, 0],
                    glint_uv: [0.0, 0.0],
                });
            }
        }
    }

    /// The aura at the wearer's feet, see-through. None while they fly, swim or sleep: it would
    /// tip over with the body.
    fn aura(&self, at: &Wearer, item: &Arc<Item>, poses: &mut Poses) {
        let (m, Some(a)) = (at.m, &item.aura) else {
            return;
        };
        if m.extra.fall_flying || asleep(m) || m.extra.swim.amount(at.alpha) > 0.0 {
            return;
        }
        let Some((models, uv)) = self.cosmetics_baked(item) else {
            return;
        };
        let Some((_, model)) = models.first() else {
            return;
        };
        // The model's origin is the middle of the feet: 24 pixels under the neck.
        let matrix = at.base * Mat4::from_translation(Vec3::new(0.0, 24.0 / 16.0, 0.0));
        poses.reset(model);
        item.animate(m.age as f32 + at.alpha, |name, pose| {
            *poses.get(name) = pose
        });
        let light = if a.glow { FULL_LIGHT } else { at.light };
        let mut fx = self.cosmetics.fx.borrow_mut();
        let start = fx.clear.len();
        horizon_client::mobs::render_model(
            model,
            matrix,
            &|p: &Part| poses.pose(p),
            uv,
            light,
            0,
            &mut fx.clear,
        );
        let color = ((a.alpha * 255.0) as u32) << 24 | 0x00FF_FFFF;
        for v in &mut fx.clear[start..] {
            v.color = color;
            // Seen through, a face turned away would add to the near one (and a card's two
            // faces would double it): `entity.wgsl` drops back faces, so it is `alpha` at most.
            v.normal[3] = horizon_client::mesher::SHADED_CULL_W;
        }
    }

    /// After every player: the player's own companion in first person (out of the middle of the
    /// view, after the first-person model went to the shadow pass only), then the frame's trails
    /// and auras in one see-through span.
    pub(in crate::game) fn cosmetics_fx_geometry(
        &self,
        camera: DVec3,
        alpha: f64,
        out: &mut Vec<Vertex>,
    ) {
        if !horizon_cosmetics::enabled() {
            return;
        }
        if !self.own_body_in_view()
            && let Some(m) = self.player_model.as_ref()
        {
            self.mark_entity(out.len(), "minecraft:player");
            self.own_companion_first_person(m, camera, alpha as f32, out);
        }
        let fx = self.cosmetics.fx.borrow();
        if !fx.clear.is_empty() {
            self.mark_translucent_entity(out.len(), "minecraft:player");
            out.extend_from_slice(&fx.clear);
        }
    }

    fn own_companion_first_person(
        &self,
        m: &Mob,
        camera: DVec3,
        alpha: f32,
        out: &mut Vec<Vertex>,
    ) {
        if horizon_client::mobs::trader::is_invisible(m) || self.own_invisible() {
            return;
        }
        let Some(worn) = self.worn(m, true) else {
            return;
        };
        let Some(item) = worn.iter().find(|i| i.slot == Slot::Companion) else {
            return;
        };
        let at = Wearer {
            m,
            own: true,
            key: OWN,
            base: horizon_client::mobs::render::base_transform(m, alpha, camera),
            camera,
            alpha,
            light: FULL_LIGHT,
        };
        let (yaw, pitch) = (self.yaw.to_radians(), self.pitch.to_radians());
        let forward = Vec3::new(
            -yaw.sin() * pitch.cos(),
            -pitch.sin(),
            yaw.cos() * pitch.cos(),
        );
        let left = Vec3::new(yaw.cos(), 0.0, yaw.sin());
        let Some((model, uv, matrix)) = self.companion_at(&at, item, Some((forward, left))) else {
            return;
        };
        // Lit where it floats.
        let p = (camera + matrix.w_axis.truncate().as_dvec3()).floor();
        let light = self.world.light(p.x as i32, p.y as i32, p.z as i32);
        let mut light = (u32::from(light >> 4) * 16) | ((u32::from(light & 15) * 16) << 8);
        if item.companion.as_ref().is_some_and(|c| c.glow) {
            light = FULL_LIGHT;
        }
        let mut poses = self.cosmetics.poses.borrow_mut();
        let poses = &mut poses.1;
        self.pose_companion(item, m, alpha, poses, &model);
        self.cosmetics_model(&model, matrix, poses, uv, light, false, None, out);
    }
}
