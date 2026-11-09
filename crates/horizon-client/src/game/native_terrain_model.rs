//! Explicit terrain-sheet material/shading/UV policy for a native standalone model.
use super::{Game, native_block_entity_draw::BlockEntityDraw};
use crate::mod_api::NativeModelRenderOptions;

#[allow(dead_code)]
impl Game<'_> {
    /// Draw a named standalone model using block sprite arrays and normal cull/depth writes.
    /// Model materials accept solid, cutout (.1 alpha cutoff, no mipmaps) and cutout-mipped
    /// (.5 cutoff, mipmaps); unknown materials/sprites or invalid UV/pose data fail before
    /// any geometry. An explicit material overrides the model's selection. Directional
    /// shading remains independent of light: fullbright shaded models remain shaded.
    /// UV shifts affect only the exact source sprite; a missing source match fails the
    /// whole call. Optional shrink is caller-specified, never inferred from an atlas guess.
    /// API @API:terrain_model@.
    pub(super) fn native_draw_terrain_model(
        &self,
        draw: &mut BlockEntityDraw,
        model: &str,
        options: NativeModelRenderOptions<'_>,
    ) -> bool {
        let Some(model) = self.pack.render.model_catalog.standalone(model) else {
            return false;
        };
        let pose = draw.pose();
        let light = draw.light;
        let layers = &self.pack.render.sprite_layers;
        crate::terrain_model::draw_model(
            &model.faces,
            pose,
            light,
            options,
            |name| {
                layers
                    .get(name)
                    .or_else(|| layers.get(name.strip_prefix("minecraft:").unwrap_or(name)))
                    .copied()
            },
            |vertex| draw.vertices_mut().push(vertex),
        )
    }
}
