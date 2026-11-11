//! The hook an external shader pipeline fills (#14035, layer plan M6). A shader pack's runtime
//! (a platform crate, later) implements [`ExternalPipeline`]; the renderer calls it at fixed
//! points of a world frame and draws vanilla's way wherever it doesn't answer. With none set
//! (`Renderer::set_external_pipeline`), each point is one `Option` test.
//!
//! Workspace-internal: the trait names the engine's frame, its mesh format and its pass order,
//! has one implementer in this workspace, carries no stability promise and is never exposed to
//! mods. A mesher or pass change updates its implementer in the same PR. HDR output
//! (`output.rs`, `frame_policy`) is not an implementer: it stays the one output stage after the
//! frame, and a pipeline draws into the frame it gives.
//!
//! Shadows are an engine frame point, on request: a pipeline that wants a shadow pass answers
//! [`ShadowView`], the engine culls the sections from it ([`ShadowCull`]) and hands back the
//! terrain seen from the light. Vanilla's path never asks, so it never computes shadow
//! visibility.

use glam::{Mat4, Vec3, Vec4};

use super::{Cull, EffectiveOutput, Frame, Frustum, Sides, facing_runs, frustum};

/// What a world frame gives an external pipeline at its points: the frame's own values, and
/// the display's from the renderer's one output snapshot (`EffectiveOutput`: whether HDR is on,
/// paper white, peak), never worked out again, so each split-screen window keeps its own.
pub struct FrameState<'a> {
    pub frame: &'a Frame<'a>,
    pub output: &'a EffectiveOutput,
}

/// An external shader pipeline, called by the renderer at fixed points of a world frame (not
/// title frames). Every method has a default that leaves vanilla's way in place.
pub trait ExternalPipeline: Send {
    /// A world frame starts, before culling.
    fn begin_frame(&mut self, _state: &FrameState<'_>) {}
    /// The shadow pass this frame wants; `None` draws none.
    fn shadow_request(&self, _state: &FrameState<'_>) -> Option<ShadowView> {
        None
    }
    /// The window's drawable size changed.
    fn resize(&mut self, _size: [u32; 2]) {}
    /// Forget what was kept from earlier frames: the next is drawn as a fresh pipeline's first.
    fn reset_history(&mut self) {}
}

/// What the shadow camera sees of the terrain, for the game's draw lists: its view and
/// projection in wgpu's clip space, the direction toward the light, and how far from the camera
/// terrain is drawn (`shadowDistanceRenderMul`).
pub struct ShadowView {
    pub cull: glam::Mat4,
    pub light: glam::Vec3,
    pub distance: Option<f32>,
    /// Draw everything, every face, within this many blocks (`shadow.culling=false|reversed`):
    /// packs that voxelize the world in their shadow pass need all of it.
    pub unculled: Option<f32>,
    /// Every face of the drawn sections, not only those that can face the light. Iris 1.11.7's
    /// shadow map holds the faces turned away from the light too (#9033): under a hill at low sun
    /// a cave's ceiling, which faces away, is often all that shadows the cave.
    pub every_face: bool,
    /// Only the sections whose shadow can fall into the camera's view: those the camera's
    /// frustum, stretched toward the light, takes in (`shadow_casters`). Our probes' Iris 1.11.7
    /// maps fit keeping only those with `shadow.culling` unset (#8464): under a noon sun, only
    /// the terrain above the ground the camera sees; with a low sun ahead, none behind the camera.
    pub view_casters: bool,
    /// Whether the projection's near and far planes in `cull` drop sections too. Our probes'
    /// Iris 1.11.7 maps hold the terrain at every depth we put it along the light (from 535 blocks
    /// beyond the shadow camera to about 120 before it), whatever `shadowNearPlane` and
    /// `shadowFarPlane` the pack sets, or none (#10403): only its sides cull.
    pub depth_planes: bool,
}

/// A shader pack's shadow camera's box, and with `casters` only what can shade the camera's view
/// (`shadow_casters`): a box must meet all of them.
#[derive(Clone, Copy, Debug)]
pub struct ShadowCull {
    pub light: Frustum,
    pub casters: Option<[Frustum; 2]>,
    /// `shadowDistance × shadowDistanceRenderMul`: only what lies within this many blocks of the
    /// camera along each axis, a cube around it. Our probe's Iris 1.11.7 maps keep the sections
    /// whose box, grown by `SHADOW_CASTER_MARGIN` as for the casters, meets that cube (#8464):
    /// out to its corners, and a section 1 block past its side but not one 1.5 blocks past.
    pub reach: Option<f32>,
}
impl ShadowCull {
    /// Whether the camera-relative box `min..max`, grown by the margin, meets (`whole`: lies
    /// within) `reach`'s cube.
    fn within_reach(&self, min: Vec3, max: Vec3, whole: bool) -> bool {
        let (min, max) = (min - SHADOW_CASTER_MARGIN, max + SHADOW_CASTER_MARGIN);
        self.reach.is_none_or(|d| {
            if whole {
                min.cmpge(Vec3::splat(-d)).all() && max.cmple(Vec3::splat(d)).all()
            } else {
                max.cmpge(Vec3::splat(-d)).all() && min.cmple(Vec3::splat(d)).all()
            }
        })
    }
}
/// How far past a section's 16 blocks the shadow pass looks for what can shade the view: our
/// probes' Iris 1.11.7 maps keep the sections whose box, grown by this much on every side, meets
/// `shadow_casters`' shape. Over 76 scenes at fields of view 30 to 110, Iris keeps every section
/// that needs up to 1.1224 blocks and drops every one that needs 1.1252 or more (#8464): one
/// growth, whatever the field of view, of 9/8 (a fit, not Iris's code). Without it the edges of
/// the wedge lose a column of terrain.
const SHADOW_CASTER_MARGIN: f32 = 1.125;
impl Cull for ShadowCull {
    fn intersects(&self, min: Vec3, max: Vec3) -> bool {
        let (grown_min, grown_max) = (min - SHADOW_CASTER_MARGIN, max + SHADOW_CASTER_MARGIN);
        self.within_reach(min, max, false)
            && self.light.intersects(min, max)
            && self
                .casters
                .is_none_or(|c| c.iter().all(|f| f.intersects(grown_min, grown_max)))
    }
    fn contains(&self, min: Vec3, max: Vec3) -> bool {
        let (grown_min, grown_max) = (min - SHADOW_CASTER_MARGIN, max + SHADOW_CASTER_MARGIN);
        self.within_reach(min, max, true)
            && self.light.contains(min, max)
            && self
                .casters
                .is_none_or(|c| c.iter().all(|f| f.contains(grown_min, grown_max)))
    }
    /// Our probe's Iris 1.11.7 maps fit testing the casters' whole sections (a guess that matched).
    fn tight(&self) -> bool {
        self.casters.is_none()
    }
}
/// The shadow camera's box that the shadow pass draws terrain in: the sides of `view.cull`, and
/// its near and far planes only with `depth_planes`.
pub(super) fn shadow_box(view: &ShadowView) -> Frustum {
    let mut planes = frustum(view.cull);
    if !view.depth_planes {
        // `frustum`'s faces at depth 0 and 1 take in everything.
        planes[4] = Vec4::W;
        planes[5] = Vec4::W;
    }
    Frustum { planes }
}

/// The runs of a section's solid quads the shadow pass draws: every face within the unculled
/// distance, or everywhere for `every_face`; else the quads that can face the light.
pub(super) fn shadow_runs(
    sides: &Sides,
    view: &ShadowView,
    origin: [f32; 3],
    draw: impl FnMut(u32, u32),
) {
    let unculled = view.unculled.is_some_and(|d| {
        let o = Vec3::from(origin) + 8.0;
        o.x.abs().max(o.z.abs()) <= d + 8.0
    });
    let toward = (!unculled && !view.every_face).then_some(view.light * 1.0e5);
    facing_runs(sides, toward, origin, draw);
}

/// The camera's view `view_proj` (wgpu depth, reversed or not) stretched without end toward
/// `light` (a direction): the boxes whose shadow, cast away from the light, can fall into the
/// view. Its planes are the view's faces turned toward the light, and through each edge between
/// such a face and one turned away, the plane along the light; unused ones take in everything.
pub(super) fn shadow_casters(view_proj: Mat4, light: Vec3) -> [Frustum; 2] {
    let faces = frustum(view_proj);
    let toward = faces.map(|p| p.truncate().dot(light) >= 0.0);
    let inverse = view_proj.inverse();
    // Corner (x, y, z) of clip space's box; `frustum`'s faces at x -1 and 1 are 0 and 1, at
    // y -1 and 1 are 2 and 3, at depth 0 and 1 are 4 and 5.
    let corner = |c: [usize; 3]| {
        inverse.project_point3(Vec3::new(
            c[0] as f32 * 2.0 - 1.0,
            c[1] as f32 * 2.0 - 1.0,
            c[2] as f32,
        ))
    };
    let corners: Vec<[usize; 3]> = (0..8).map(|i| [i & 1, (i >> 1) & 1, i >> 2]).collect();
    let centre = corners.iter().map(|&c| corner(c)).sum::<Vec3>() / 8.0;
    let mut planes = [Vec4::W; 12];
    let mut n = 0;
    for (k, &face) in faces.iter().enumerate() {
        if toward[k] {
            planes[n] = face;
            n += 1;
        }
    }
    for a in &corners {
        for axis in 0..3 {
            if a[axis] == 1 {
                continue;
            }
            let mut b = *a;
            b[axis] = 1;
            // The edge from a to b lies on the faces of the two other axes.
            let [f, g] = [(axis + 1) % 3, (axis + 2) % 3].map(|o| o * 2 + a[o]);
            if toward[f] == toward[g] {
                continue;
            }
            let (p, q) = (corner(*a), corner(b));
            let normal = (q - p).cross(light);
            if normal.length_squared() < 1e-12 {
                continue;
            }
            let normal = normal.normalize();
            let side = if normal.dot(centre - p) < 0.0 {
                -1.0
            } else {
                1.0
            };
            // At most six such edges with at most four faces toward the light: 12 planes.
            if let Some(plane) = planes.get_mut(n) {
                *plane = (normal * side).extend(-normal.dot(p) * side);
                n += 1;
            }
        }
    }
    let [a, b] = [0, 6].map(|i| Frustum {
        planes: planes[i..i + 6].try_into().expect("six planes"),
    });
    [a, b]
}
