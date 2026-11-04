//! Entity mesh occlusion and view-distance checks.
use super::{
    Game, Vertex,
    entity::RenderEntityBounds,
    occlusion_culling::{QuerySource, occluder_ready},
};
use horizon_client::graphics::EntityCullingMode;
use horizon_core::geometry::{Aabb, Vec3};

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

    pub(super) fn cull_entity_vertices(&self, vertices: &mut Vec<Vertex>, start: usize) {
        if self.occlusion_hidden(Some(vertices), start, None)
            && !(self.pack_shadow_casters.0 && self.hide_spans_from(start, vertices.len()))
        {
            if self.settings.horizon.entity_culling == EntityCullingMode::Asynchronous
                && let Some(culler) = &self.async_occlusion_culler
            {
                culler.record_entity_vertices_culled(vertices.len().saturating_sub(start));
            }
            vertices.truncate(start);
        }
    }

    /// Cull from the shared entity AABB before generating its model vertices.
    pub(super) fn entity_cull_decision<T: RenderEntityBounds>(
        &self,
        entity: &T,
        alpha: f64,
    ) -> EntityCullDecision {
        if self.settings.horizon.entity_culling == EntityCullingMode::Off
            || self.pack_shadow_casters.0
            || entity.render_glowing(self)
        {
            return EntityCullDecision::Bypass;
        }
        let margin = Vec3::new(0.5, 0.5, 0.5);
        let bounds = entity.render_bounds(alpha).inflate(margin);
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
            let opaque = |pos| {
                occluder_ready(
                    &self.world,
                    self.res.facts(),
                    self.renderer.as_ref(),
                    &self.streaming,
                    &self.pending_uploads,
                    pos,
                )
            };
            match bounds {
                Some(bounds) => culler.hidden_bounds(bounds, opaque),
                None => vertices.is_some_and(|vertices| culler.hidden(vertices, start, opaque)),
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
mod tests {
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
}
