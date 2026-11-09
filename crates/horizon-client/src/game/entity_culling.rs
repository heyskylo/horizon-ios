//! Entity mesh occlusion and view-distance checks.
use super::{
    Game, Vertex,
    entity::RenderEntityBounds,
    occlusion_culling::{QuerySource, SyncOccluder},
};
use horizon_client::graphics::EntityCullingMode;
use horizon_client::mobs::Species;
use horizon_core::geometry::{Aabb, Vec3};

const MAX_ENTITY_SECTION_COVERAGE: i64 = 50;

/// The orb quad is asymmetric about its translated centre: its farthest corner
/// is sqrt(0.15² + 0.225²) blocks away. A sphere covers every billboard rotation
/// and pickup-animation position; include rounding of camera-relative vertices.
fn orb_billboard_radius(origin: glam::Vec3) -> f32 {
    0.3 + origin.abs().max_element() * f32::EPSILON * 4.0
}

pub(super) fn orb_in_view(view: &horizon_client::gfx::Frustum, origin: glam::Vec3) -> bool {
    view.intersects_sphere(origin, orb_billboard_radius(origin))
}

/// The inclusive range of sections an entity's padded render bounds can touch,
/// in the layout whose section 0 starts at `bottom_y`. `None` when the bounds
/// are not finite, hold block coordinates outside `i32`, or span more sections
/// than `MAX_ENTITY_SECTION_COVERAGE`: those go to the per-entity path.
fn entity_section_span(bounds: Aabb, bottom_y: i32) -> Option<([i32; 3], [i32; 3])> {
    // A half-block border keeps models near section edges checked against
    // every section their geometry could touch.
    let lo = bounds.min_position();
    let hi = bounds.max_position();
    let min_block = [lo.x - 0.5, lo.y - 0.5, lo.z - 0.5];
    let max_block = [hi.x + 0.5, hi.y + 0.5, hi.z + 0.5];
    if min_block
        .into_iter()
        .chain(max_block)
        .any(|v| !v.is_finite() || v < f64::from(i32::MIN) || v > f64::from(i32::MAX))
    {
        return None;
    }
    let min_block = min_block.map(|v| v.floor() as i32);
    let max_block = max_block.map(|v| v.floor() as i32);
    let min_y = min_block[1].checked_sub(bottom_y)?;
    let max_y = max_block[1].checked_sub(bottom_y)?;
    let min = [min_block[0] >> 4, min_y >> 4, min_block[2] >> 4];
    let max = [max_block[0] >> 4, max_y >> 4, max_block[2] >> 4];
    let sections = (0..3).try_fold(1_i64, |count, axis| {
        count.checked_mul(i64::from(max[axis]) - i64::from(min[axis]) + 1)
    });
    // Very large render envelopes (for example, beams) are cheaper and safer to
    // leave to the existing per-entity path.
    sections
        .is_some_and(|count| count <= MAX_ENTITY_SECTION_COVERAGE)
        .then_some((min, max))
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum EntityCullDecision {
    Bypass,
    Hidden,
    CheckVertices,
    Pending,
}

impl EntityCullDecision {
    pub(super) const fn hides_entity(self) -> bool {
        matches!(self, Self::Hidden)
    }

    pub(super) const fn checks_vertices(self) -> bool {
        matches!(self, Self::CheckVertices)
    }
}

impl Game<'_> {
    pub(super) fn prepare_entity_culling(&mut self) {
        // Spans tell the pack's passes which entities are hidden; without them, nothing changes.
        self.pack_shadow_casters = match (&self.renderer, &self.pack_ids) {
            (Some(r), Some(_)) => r.pack_shadow_casters(),
            // A native pack's shadow pass has no spans to hide entities with: it only needs
            // the first-person model.
            (Some(r), None) => (false, r.native_shadow()),
            _ => (false, false),
        };
    }

    pub(super) fn cull_entity_vertices(&self, vertices: &mut Vec<Vertex>, start: usize) -> bool {
        let hidden = self.occlusion_hidden(Some(vertices), start, None);
        if hidden && !(self.pack_shadow_casters.0 && self.hide_spans_from(start, vertices.len())) {
            if self.settings.horizon.entity_culling == EntityCullingMode::Asynchronous
                && let Some(culler) = &self.async_occlusion_culler
            {
                culler.record_entity_vertices_culled(vertices.len().saturating_sub(start));
            }
            vertices.truncate(start);
        }
        hidden
    }

    pub(super) fn cull_entity_camera_bounds(&self, lo: glam::Vec3, hi: glam::Vec3) -> bool {
        if self.occlusion_hidden_camera_bounds(lo, hi) {
            if self.settings.horizon.entity_culling == EntityCullingMode::Asynchronous
                && let Some(culler) = &self.async_occlusion_culler
            {
                culler.record_entity_vertices_culled(2);
            }
            return true;
        }
        false
    }

    fn occlusion_hidden_camera_bounds(&self, lo: glam::Vec3, hi: glam::Vec3) -> bool {
        match self.settings.horizon.entity_culling {
            EntityCullingMode::Off => false,
            EntityCullingMode::Asynchronous => {
                if let Some(culler) = &self.async_occlusion_culler {
                    if !culler.camera_is_stationary() {
                        return false;
                    }
                    let Some(query) = culler.query_camera_bounds(lo, hi) else {
                        return false;
                    };
                    match culler.query_result(&query, QuerySource::EntityMesh) {
                        Some((false, _)) => false,
                        Some((true, Some(proof)))
                            if culler.proof_is_current(
                                &self.world,
                                self.renderer.as_ref(),
                                &self.streaming,
                                &self.pending_uploads,
                                proof,
                            ) =>
                        {
                            true
                        }
                        Some(_) | None => {
                            culler.queue(query);
                            false
                        }
                    }
                } else {
                    self.entity_hidden_sync_camera_bounds(lo, hi)
                }
            }
            EntityCullingMode::Synchronous => self.entity_hidden_sync_camera_bounds(lo, hi),
        }
    }

    fn entity_hidden_sync_camera_bounds(&self, lo: glam::Vec3, hi: glam::Vec3) -> bool {
        self.sync_occlusion_culler.as_ref().is_some_and(|culler| {
            let mut occluder = SyncOccluder::new(
                &self.world,
                self.res.facts(),
                self.renderer.as_ref(),
                &self.streaming,
                &self.pending_uploads,
            );
            culler.hidden_camera_bounds(lo.to_array(), hi.to_array(), |pos| occluder.ready(pos))
        })
    }

    /// Pre-model visibility hook for dynamic block-entity geometry.
    pub(super) fn block_entity_occluded(&self, bounds: Aabb) -> bool {
        self.occlusion_hidden(None, 0, Some(bounds))
    }

    /// Cull from the shared entity AABB before generating its model vertices.
    pub(super) fn entity_cull_decision<T: RenderEntityBounds>(
        &self,
        entity: &T,
        alpha: f64,
    ) -> EntityCullDecision {
        self.entity_cull_decision_in_bounds(entity, entity.render_bounds(alpha))
    }

    /// Cull from a renderer-computed envelope when the rendered transform has
    /// extra offsets or model geometry that the entity type alone cannot see.
    pub(super) fn entity_cull_decision_in_bounds<T: RenderEntityBounds>(
        &self,
        entity: &T,
        render_bounds: Aabb,
    ) -> EntityCullDecision {
        if self.settings.horizon.entity_culling == EntityCullingMode::Off
            || self.pack_shadow_casters.0
            || entity.render_glowing(self)
        {
            return EntityCullDecision::Bypass;
        }
        // `render_bounds` is already the conservative model and animation
        // envelope. Only cover f32 vertex rounding here; another half-block
        // would make coarse walls falsely visible and force mesh generation.
        let margin = Vec3::new(1.0e-4, 1.0e-4, 1.0e-4);
        let bounds = render_bounds.inflate(margin);
        if self.terrain_sections_occlude(bounds) {
            if let Some(culler) = &self.async_occlusion_culler {
                culler.record_entity_culled();
            }
            return EntityCullDecision::Hidden;
        }
        match self.settings.horizon.entity_culling {
            EntityCullingMode::Off => EntityCullDecision::Bypass,
            EntityCullingMode::Synchronous => {
                if self.entity_hidden_sync(None, 0, Some(bounds)) {
                    EntityCullDecision::Hidden
                } else {
                    EntityCullDecision::CheckVertices
                }
            }
            EntityCullingMode::Asynchronous => {
                let Some(culler) = &self.async_occlusion_culler else {
                    return if self.entity_hidden_sync(None, 0, Some(bounds)) {
                        EntityCullDecision::Hidden
                    } else {
                        EntityCullDecision::CheckVertices
                    };
                };
                if !culler.camera_is_stationary() {
                    return EntityCullDecision::CheckVertices;
                }
                let Some(query) = culler.query_entity_bounds(bounds) else {
                    return EntityCullDecision::CheckVertices;
                };
                culler.sample_entity_aabb(entity.render_id(), &query);
                match culler.query_result(&query, QuerySource::EntityAabb) {
                    Some((false, _)) => EntityCullDecision::CheckVertices,
                    Some((true, Some(proof)))
                        if culler.proof_is_current(
                            &self.world,
                            self.renderer.as_ref(),
                            &self.streaming,
                            &self.pending_uploads,
                            proof,
                        ) =>
                    {
                        culler.record_entity_culled();
                        EntityCullDecision::Hidden
                    }
                    Some(_) | None => {
                        culler.queue(query);
                        EntityCullDecision::Pending
                    }
                }
            }
        }
    }

    /// Reject entities whose padded render bounds lie entirely in sections the
    /// terrain visibility graph cannot reach. Unknown graph cells fail open.
    fn terrain_sections_occlude(&self, bounds: Aabb) -> bool {
        if !self.entity_section_visibility_valid
            || (self.sync_occlusion_culler.is_none() && self.async_occlusion_culler.is_none())
        {
            return false;
        }
        let Some(renderer) = &self.renderer else {
            return false;
        };
        let Some(camera_section) = self.entity_section_camera else {
            return false;
        };
        let Some((min, max)) = entity_section_span(bounds, self.world.bottom_y()) else {
            return false;
        };

        for x in min[0]..=max[0] {
            for y in min[1]..=max[1] {
                for z in min[2]..=max[2] {
                    let Some(visible) =
                        renderer.section_occlusion_visible((x, y, z), camera_section)
                    else {
                        return false;
                    };
                    if visible {
                        return false;
                    }
                }
            }
        }
        true
    }

    /// Refine an AABB-visible result with the exact bounds of the rendered mesh.
    pub(super) fn finish_entity_occlusion<T: RenderEntityBounds>(
        &self,
        decision: EntityCullDecision,
        entity: &T,
        vertices: &mut Vec<Vertex>,
        start: usize,
    ) {
        if decision.checks_vertices() && !self.pack_shadow_casters.0 && !entity.render_glowing(self)
        {
            self.cull_entity_vertices(vertices, start);
        }
    }

    fn occlusion_hidden(
        &self,
        vertices: Option<&[Vertex]>,
        start: usize,
        bounds: Option<Aabb>,
    ) -> bool {
        match self.settings.horizon.entity_culling {
            EntityCullingMode::Off => false,
            EntityCullingMode::Synchronous => self.entity_hidden_sync(vertices, start, bounds),
            EntityCullingMode::Asynchronous => {
                if let Some(culler) = &self.async_occlusion_culler {
                    if !culler.camera_is_stationary() {
                        return false;
                    }
                    let Some(query) = (match bounds {
                        Some(bounds) => culler.query_entity_bounds(bounds),
                        None => vertices.and_then(|vertices| culler.query(vertices, start)),
                    }) else {
                        return false;
                    };
                    match culler.query_result(&query, QuerySource::EntityMesh) {
                        Some((false, _)) => false,
                        Some((true, Some(proof)))
                            if culler.proof_is_current(
                                &self.world,
                                self.renderer.as_ref(),
                                &self.streaming,
                                &self.pending_uploads,
                                proof,
                            ) =>
                        {
                            true
                        }
                        Some(_) | None => {
                            culler.queue(query);
                            false
                        }
                    }
                } else {
                    self.entity_hidden_sync(vertices, start, bounds)
                }
            }
        }
    }

    fn entity_hidden_sync(
        &self,
        vertices: Option<&[Vertex]>,
        start: usize,
        bounds: Option<Aabb>,
    ) -> bool {
        self.sync_occlusion_culler.as_ref().is_some_and(|culler| {
            let mut occluder = SyncOccluder::new(
                &self.world,
                self.res.facts(),
                self.renderer.as_ref(),
                &self.streaming,
                &self.pending_uploads,
            );
            match bounds {
                Some(bounds) => culler.hidden_bounds(bounds, |pos| occluder.ready(pos)),
                None => vertices.is_some_and(|vertices| {
                    culler.hidden(vertices, start, |pos| occluder.ready(pos))
                }),
            }
        })
    }
}

impl Game<'_> {
    /// Entity.setViewScale (LevelExtractor.extractVisibleEntities): the entity
    /// distance option times the render distance's boost — 1 at 8 chunks, 2.5
    /// at 20 or more. The render distance is the client setting, capped by
    /// the server's view distance when connected
    /// (Options.getEffectiveRenderDistance).
    pub(super) fn view_scale(&self) -> f64 {
        let distance = match &self.remote {
            Some(r) => self.settings.render_distance.min(r.view_distance),
            None => self.settings.render_distance,
        };
        entity_view_scale(distance, self.settings.entity_distance)
    }
}

/// Entity.setViewScale: `Mth.clamp(renderDistance / 8, 1, 2.5) *
/// entityDistanceScaling`. Every entity's draw distance is scaled by it, so
/// a small mob is drawn nearer than a big one and a longer render distance
/// draws both further away.
pub(super) fn entity_view_scale(render_distance: i32, entity_distance: f32) -> f64 {
    (f64::from(render_distance) / 8.0).clamp(1.0, 2.5) * f64::from(entity_distance)
}

/// Entity.shouldRenderAtSqrDistance's default range: 64 times the bounding
/// box's mean edge, scaled by the view scale.
pub(super) fn default_range(size: glam::DVec3, view_scale: f64) -> f64 {
    let edge = (size.x + size.y + size.z) / 3.0;
    let edge = if edge.is_nan() { 1.0 } else { edge };
    edge * 64.0 * view_scale
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct CullGrow {
    side: f64,
    all: f64,
    down: f64,
}

impl CullGrow {
    fn of(species: Species, height: f64, dragon_head: bool) -> Self {
        let mut grow = Self::default();
        if dragon_head {
            grow.all += 0.5;
        }
        match species {
            Species::Illusioner => grow.side += 3.0,
            Species::Sniffer => grow.all += f64::from(0.6_f32), // inflate(0.6F)
            Species::HappyGhast => grow.down += height / 2.0,
            _ => {}
        }
        grow
    }
}

fn grown_box(
    pos: glam::DVec3,
    width: f64,
    height: f64,
    grow: CullGrow,
) -> (glam::DVec3, glam::DVec3) {
    let side = width / 2.0 + grow.side + grow.all;
    (
        pos - glam::DVec3::new(side, grow.all + grow.down, side),
        pos + glam::DVec3::new(side, height + grow.all, side),
    )
}

/// `LivingEntityRenderer.getBoundingBoxForCulling` for the mob's interpolated
/// position, including renderer-specific growth for mirrors, riders and heads.
pub(super) fn cull_box(mob: &horizon_client::mobs::Mob, alpha: f64) -> (glam::DVec3, glam::DVec3) {
    let (width, height, _) = mob.dims();
    let grow = CullGrow::of(
        mob.species(),
        height,
        mob.extra.head.as_deref() == Some("minecraft:dragon_head"),
    );
    let pos = glam::DVec3::new(
        mob.previous.x + (mob.position.x - mob.previous.x) * alpha,
        mob.previous.y + (mob.position.y - mob.previous.y) * alpha,
        mob.previous.z + (mob.position.z - mob.previous.z) * alpha,
    );
    grown_box(pos, width, height, grow)
}

impl Game<'_> {
    /// EntityRenderer.shouldRender: the entity within `range` blocks of the
    /// camera, measured from its current `position`, and its culling box
    /// (`lo`..`hi`, interpolated, in world space) grown by 0.5 in the view.
    pub(super) fn entity_in_view(
        &self,
        camera: glam::DVec3,
        position: glam::DVec3,
        range: f64,
        lo: glam::DVec3,
        hi: glam::DVec3,
    ) -> bool {
        if (position - camera).length_squared() >= range * range {
            return false;
        }
        self.view_frustum.as_ref().is_none_or(|view| {
            view.intersects(
                (lo - camera - glam::DVec3::splat(0.5)).as_vec3(),
                (hi - camera + glam::DVec3::splat(0.5)).as_vec3(),
            )
        })
    }
}

/// An entity's position as a `DVec3`.
pub(super) fn at(p: horizon_core::geometry::Vec3) -> glam::DVec3 {
    glam::DVec3::new(p.x, p.y, p.z)
}

impl Game<'_> {
    /// `entity_in_view` for an entity whose box stands on its position,
    /// `width` wide and `height` tall, interpolated by `alpha` and grown by
    /// `grow` (a renderer's getBoundingBoxForCulling).
    #[allow(clippy::too_many_arguments)]
    pub(super) fn standing_entity_in_view(
        &self,
        camera: glam::DVec3,
        previous: glam::DVec3,
        position: glam::DVec3,
        alpha: f64,
        width: f64,
        height: f64,
        range: f64,
        grow: f64,
    ) -> bool {
        let p = previous.lerp(position, alpha);
        let half = glam::DVec3::new(width / 2.0 + grow, grow, width / 2.0 + grow);
        let lo = p - half;
        let hi = p + glam::DVec3::new(half.x, height + grow, half.z);
        self.entity_in_view(camera, position, range, lo, hi)
    }
}

#[cfg(test)]
mod vanilla_entity_tests {
    use super::*;

    /// `Entity.setViewScale`: the entity distance option times the render
    /// distance's boost — 1 at eight chunks, 1.5 at twelve, 2.5 at twenty.
    #[test]
    fn the_view_scale_follows_the_render_distance() {
        assert_eq!(entity_view_scale(4, 1.0), 1.0);
        assert_eq!(entity_view_scale(8, 1.0), 1.0);
        assert_eq!(entity_view_scale(12, 1.0), 1.5);
        assert_eq!(entity_view_scale(20, 1.0), 2.5);
        assert_eq!(entity_view_scale(32, 1.0), 2.5);
        // The entity distance option scales it down.
        assert_eq!(entity_view_scale(12, 0.5), 0.75);
    }

    /// `Entity.shouldRenderAtSqrDistance`: a chicken's box (0.4 by 0.7 by
    /// 0.4) is drawn up to 48 blocks away at the vanilla defaults, not the
    /// 32 blocks a missing view scale leaves (#2273).
    #[test]
    fn a_chicken_is_drawn_forty_eight_blocks_away() {
        let range = default_range(glam::DVec3::new(0.4, 0.7, 0.4), entity_view_scale(12, 1.0));
        assert_eq!(range, 48.0);
    }

    /// `Entity.shouldRenderAtSqrDistance` for a dropped item (`ItemEntity`
    /// keeps the default and its box is 0.25 on each side): 24 blocks at
    /// render distance 12, 32 at 16, 40 from 20 on; never 64.
    #[test]
    fn a_dropped_item_is_drawn_sixteen_blocks_times_the_view_scale() {
        let item = glam::DVec3::splat(0.25);
        assert_eq!(default_range(item, entity_view_scale(12, 1.0)), 24.0);
        assert_eq!(default_range(item, entity_view_scale(16, 1.0)), 32.0);
        assert_eq!(default_range(item, entity_view_scale(32, 1.0)), 40.0);
    }

    /// `getBoundingBoxForCulling`: only four living renderers grow the box.
    /// An illusioner asks for three blocks either side (room for its mirror
    /// images, which are drawn outside its own box), a sniffer for 0.6 all
    /// round, a happy ghast for half its height below (its riders hang under
    /// it) and anyone wearing a dragon head for 0.5 all round.
    #[test]
    fn only_the_four_living_renderers_grow_the_culling_box() {
        assert_eq!(
            CullGrow::of(Species::Villager, 1.95, false),
            CullGrow::default()
        );
        let illusioner = CullGrow::of(Species::Illusioner, 1.95, false);
        assert_eq!(
            (illusioner.side, illusioner.all, illusioner.down),
            (3.0, 0.0, 0.0)
        );
        let sniffer = CullGrow::of(Species::Sniffer, 1.75, false);
        assert!((sniffer.all - 0.6).abs() < 1e-7 && sniffer.side == 0.0);
        let ghast = CullGrow::of(Species::HappyGhast, 4.0, false);
        assert_eq!((ghast.down, ghast.all), (2.0, 0.0));
        // A dragon head grows every axis, on any living entity.
        let headed = CullGrow::of(Species::Villager, 1.95, true);
        assert_eq!((headed.all, headed.side, headed.down), (0.5, 0.0, 0.0));
        // The growths add up: an illusioner in a dragon head gets both.
        let both = CullGrow::of(Species::Illusioner, 1.95, true);
        assert_eq!((both.all, both.side), (0.5, 3.0));
    }

    /// The box the frustum test sees: an illusioner's own box is 0.6 wide, so
    /// its culling box reaches 3.3 blocks either side of its position and
    /// 0.5 below its feet, where a villager's reaches 0.8.
    #[test]
    fn an_illusioner_is_culled_three_blocks_wider_than_its_box() {
        let pos = glam::DVec3::new(10.0, 64.0, -4.0);
        let (lo, hi) = grown_box(
            pos,
            0.6,
            1.95,
            CullGrow::of(Species::Illusioner, 1.95, false),
        );
        assert_eq!(lo, glam::DVec3::new(10.0 - 3.3, 64.0, -4.0 - 3.3));
        assert_eq!(hi, glam::DVec3::new(10.0 + 3.3, 64.0 + 1.95, -4.0 + 3.3));
        let (lo, hi) = grown_box(pos, 0.6, 1.95, CullGrow::of(Species::Villager, 1.95, false));
        assert!((lo.x - (10.0 - 0.3)).abs() < 1e-9);
        assert!((hi.y - (64.0 + 1.95)).abs() < 1e-9);
        // A happy ghast's box drops by half its height below its feet.
        let (lo, hi) = grown_box(pos, 4.0, 4.0, CullGrow::of(Species::HappyGhast, 4.0, false));
        assert_eq!(lo.y, 62.0);
        assert_eq!(hi.y, 68.0);
    }

    /// `Entity.shouldRenderAtSqrDistance` on a broken box: the size falls
    /// back to one block.
    #[test]
    fn a_broken_bounding_box_falls_back_to_one_block() {
        let range = default_range(
            glam::DVec3::new(f64::NAN, 0.7, 0.4),
            entity_view_scale(12, 1.0),
        );
        assert_eq!(range, 96.0);
    }

    /// The sections an entity's padded bounds can touch: the half-block border
    /// a model near a section edge reaches into, the layout `bottom_y` sets,
    /// and the boxes that get no answer and stay on the per-entity path.
    #[test]
    fn entity_section_spans_cover_the_border_and_fail_open() {
        let span = |lo: [f64; 3], hi: [f64; 3], bottom_y: i32| {
            entity_section_span(
                Aabb::new(
                    Vec3::new(lo[0], lo[1], lo[2]),
                    Vec3::new(hi[0], hi[1], hi[2]),
                ),
                bottom_y,
            )
        };

        // A chicken well inside one section stays in it.
        assert_eq!(
            span([100.2, -63.4, 7.1], [100.6, -62.9, 7.5], -64),
            Some(([6, 0, 0], [6, 0, 0]))
        );
        // A box 0.8 wide inside the next section does not spill out of it.
        assert_eq!(
            span([16.6, -60.0, 100.6], [17.4, -59.4, 101.4], -64),
            Some(([1, 0, 6], [1, 0, 6]))
        );
        // On a section border the model reaches the section either side.
        assert_eq!(
            span([16.0, -60.0, 100.0], [16.0, -59.0, 100.0], -64),
            Some(([0, 0, 6], [1, 0, 6]))
        );
        // `bottom_y` places section 0: the same box in the Nether.
        assert_eq!(
            span([100.2, -63.4, 7.1], [100.6, -62.9, 7.5], 0),
            Some(([6, -4, 0], [6, -4, 0]))
        );
        // Not finite, beyond `i32`, or more sections than the coverage cap:
        // no answer.
        assert_eq!(span([f64::NAN, 0.0, 0.0], [f64::NAN, 1.0, 1.0], -64), None);
        assert_eq!(span([0.0; 3], [f64::INFINITY; 3], -64), None);
        assert_eq!(
            span([0.0; 3], [f64::from(i32::MAX) + 1.0, 1.0, 1.0], -64),
            None
        );
        // A beam across the world spans far more than the cap.
        assert_eq!(span([0.0, -63.0, 0.0], [1.0e6, -62.0, 1.0], -64), None);
    }
}
#[cfg(test)]
#[path = "entity_culling_tests.rs"]
mod tests;

#[cfg(test)]
mod orb_tests {
    use super::orb_billboard_radius;

    #[test]
    fn sphere_contains_the_asymmetric_billboard_including_vertex_rounding() {
        use glam::Vec3;
        use horizon_client::entities::{ORB_QUAD, ORB_QUAD_SCALE};

        for origin in [
            Vec3::ZERO,
            Vec3::new(-47.25, 12.5, 81.0),
            Vec3::new(30_000_000.0, -30_000_000.0, 256.0),
        ] {
            let radius = orb_billboard_radius(origin);
            for yaw in (0..360).step_by(15) {
                for pitch in [-90.0_f32, -89.99, -45.0, 0.0, 45.0, 89.99, 90.0] {
                    let (yaw, pitch) = ((yaw as f32).to_radians(), pitch.to_radians());
                    let forward = Vec3::new(
                        -yaw.sin() * pitch.cos(),
                        -pitch.sin(),
                        yaw.cos() * pitch.cos(),
                    );
                    let right = forward.cross(Vec3::Y).normalize_or_zero();
                    let up = right.cross(forward).normalize_or_zero();
                    for corner in ORB_QUAD {
                        let vertex = origin
                            + right * (corner[0] * ORB_QUAD_SCALE)
                            + up * (corner[1] * ORB_QUAD_SCALE);
                        assert!(
                            vertex.distance(origin) <= radius,
                            "{origin:?}, yaw {yaw}, pitch {pitch}: {vertex:?} outside {radius}"
                        );
                    }
                }
            }
        }
    }
}
