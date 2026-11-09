//! Still-fluid geometry under the native block entity pose, with an ordinary translucent material.
use super::{Game, native_block_entity_draw::BlockEntityDraw};
use crate::{
    fluid_render::{fluid_box, fluid_light},
    gfx::FLUID_OVERLAY,
    mesher::Vertex,
    mod_api::{NativeFluidRenderOptions, NativeFluidStack},
};
use glam::Vec3;

#[allow(dead_code)]
impl Game<'_> {
    /// Draw a still-fluid box under `draw`'s current pose. Supports water and lava with an empty
    /// component patch; unknown fluid/component payloads return false before emitting geometry.
    /// Empty accepted stacks return true without validating bounds. Nonempty stacks reject
    /// invalid/oversized bounds before emitting geometry. The tint is stack ARGB (plain water
    /// 0xff3f76e4, lava white), never biome water color; `options.argb` overrides it explicitly.
    /// The actual animated block sprite is sampled with mipmaps. Faces are sorted, alpha blended,
    /// back culled and write ordinary color/depth, with sky light retained and block luminosity
    /// raised to the fluid's (lava 15). No model diffuse shade, geometry inset or alpha cutoff.
    /// API @API:fluid_render@.
    pub(super) fn native_draw_fluid_box(
        &self,
        draw: &mut BlockEntityDraw,
        stack: &NativeFluidStack,
        bounds: [f32; 6],
        options: NativeFluidRenderOptions,
    ) -> bool {
        let Some((sprite, tint, luminosity)) = fluid_visual(stack) else {
            return false;
        };
        if stack.amount <= 0 {
            return true;
        }
        let layers = &self.pack.render.sprite_layers;
        let Some(&layer) = layers
            .get(sprite)
            .or_else(|| layers.get(sprite.strip_prefix("minecraft:").unwrap_or(sprite)))
        else {
            return false;
        };
        let pose = draw.pose();
        let light = fluid_light(draw.light, luminosity);
        let color = options.argb.unwrap_or(tint);
        let color = (color & 0xff00_ff00) | ((color >> 16) & 0xff) | ((color & 0xff) << 16);
        fluid_box(bounds, options, |v| {
            draw.vertices_mut().push(Vertex {
                pos: pose.transform_point3(Vec3::from_array(v.pos)).to_array(),
                uv: v.uv,
                layer: FLUID_OVERLAY,
                color,
                light,
                // The source TRANSLUCENT shader does not use normals for lighting.
                normal: [0; 4],
                glint_uv: [layer as f32, 0.0],
            });
        })
    }
}

fn fluid_visual(stack: &NativeFluidStack) -> Option<(&'static str, u32, u8)> {
    if !stack.components.is_empty() {
        return None;
    }
    match stack.fluid.as_str() {
        "minecraft:water" => Some(("minecraft:block/water_still", 0xff3f76e4, 0)),
        "minecraft:lava" => Some(("minecraft:block/lava_still", 0xffffffff, 15)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn component_payloads_and_unknown_fluids_are_not_guessed() {
        let mut stack = NativeFluidStack {
            fluid: "minecraft:water".into(),
            amount: 1000,
            components: Default::default(),
        };
        assert_eq!(
            fluid_visual(&stack),
            Some(("minecraft:block/water_still", 0xff3f76e4, 0))
        );
        stack.components.insert(
            "!minecraft:custom_data".into(),
            horizon_world::nbt::Tag::Compound(Default::default()),
        );
        assert!(fluid_visual(&stack).is_none());
        stack.components.clear();
        stack.fluid = "test:unknown".into();
        assert!(fluid_visual(&stack).is_none());
    }
}
