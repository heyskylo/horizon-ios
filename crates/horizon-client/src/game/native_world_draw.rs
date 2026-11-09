//! A native mod's world-space text and atlas quads, through the normal renderer.
use super::{Game, native_block_entity_draw::BlockEntityDraw};
use glam::{DVec3, Mat4, Vec3};
use horizon_client::{
    font::Patch,
    gfx::{WORLD_TEXT_OVERLAY, WORLD_TEXT_POLYGON_OFFSET_OVERLAY},
    hud::{HudBatch, Rect, rgba},
    mesher::Vertex,
    text::Text,
};
use std::ops::{Deref, DerefMut};

/// One world frame, independently of block entities or source blocks. Its pose
/// starts camera-relative and shares `BlockEntityDraw`'s push/pop, translate,
/// rotate and scale operations, partial tick, game time and packed light.
/// `keep` does not cache a world canvas. API @API:world_canvas@.
pub struct NativeWorldDraw {
    pub camera: [f64; 3],
    draw: BlockEntityDraw,
    patches: Vec<Patch>,
    profiles: Vec<serde_json::Value>,
}
impl Deref for NativeWorldDraw {
    type Target = BlockEntityDraw;
    fn deref(&self) -> &Self::Target {
        &self.draw
    }
}
impl DerefMut for NativeWorldDraw {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.draw
    }
}
impl NativeWorldDraw {
    fn new(camera: [f64; 3], partial: f32, time: i64) -> Self {
        Self {
            camera,
            draw: BlockEntityDraw::posed(
                partial,
                time,
                240 | 240 << 8,
                [0; 3],
                Mat4::IDENTITY,
                (Vec3::Y, Vec3::Y),
                Vec::new(),
            ),
            patches: Vec::new(),
            profiles: Vec::new(),
        }
    }
    /// Translate to a world position, subtracting the camera in f64 first.
    /// Returns false without changing the pose for a non-finite offset.
    pub fn translate_world(&mut self, position: [f64; 3]) -> bool {
        let offset = (DVec3::from_array(position) - DVec3::from_array(self.camera)).as_vec3();
        if !offset.is_finite() {
            return false;
        }
        self.draw.translate(offset.to_array());
        true
    }
    fn quad(&mut self, sprite: Rect, corners: [[f32; 3]; 4], uv: [f32; 4], color: [u8; 4]) -> bool {
        let pose = self.draw.pose();
        if !pose.is_finite()
            || !corners.iter().flatten().all(|v| v.is_finite())
            || !uv.iter().all(|v| v.is_finite())
        {
            return false;
        }
        let [u0, v0, u1, v1] = uv.map(|v| v / 16.0);
        let points = corners.map(|point| pose.transform_point3(Vec3::from_array(point)));
        if points.iter().any(|point| !point.is_finite()) {
            return false;
        }
        let light = self.draw.light;
        for (point, [u, v]) in points
            .into_iter()
            .zip([[u0, v0], [u0, v1], [u1, v1], [u1, v0]])
        {
            self.draw.vertices_mut().push(Vertex {
                pos: point.to_array(),
                uv: [
                    sprite.u0 + (sprite.u1 - sprite.u0) * u,
                    sprite.v0 + (sprite.v1 - sprite.v0) * v,
                ],
                layer: WORLD_TEXT_OVERLAY,
                color: u32::from_le_bytes(color),
                light,
                normal: [0; 4],
                glint_uv: [0.0; 2],
            });
        }
        true
    }
    fn text(&mut self, batch: HudBatch, front: Option<usize>) {
        let pose = self.draw.pose();
        let light = self.draw.light;
        if pose.is_finite() {
            for (index, quad) in batch.vertices.as_chunks::<6>().0.iter().enumerate() {
                for i in horizon_client::text_display::CORNERS {
                    let vertex = quad[i];
                    self.draw.vertices_mut().push(Vertex {
                        pos: pose
                            .transform_point3(Vec3::new(vertex.pos[0], vertex.pos[1], vertex.depth))
                            .to_array(),
                        uv: vertex.uv,
                        color: vertex.color,
                        light,
                        layer: if front.is_some_and(|start| index * 6 >= start) {
                            WORLD_TEXT_POLYGON_OFFSET_OVERLAY
                        } else {
                            WORLD_TEXT_OVERLAY
                        },
                        normal: [0; 4],
                        glint_uv: [0.0; 2],
                    });
                }
            }
        }
        self.patches.extend(batch.patches);
        self.profiles.extend(batch.inline_profiles);
    }
}
impl Game<'_> {
    pub(super) fn native_world_geometry(
        &mut self,
        camera: DVec3,
        partial: f32,
        out: &mut Vec<Vertex>,
    ) {
        if !horizon_client::gfx::native_render::world_drawn_hooked()
            && crate::mod_api::mods().is_empty()
        {
            return;
        }
        let mut draw = NativeWorldDraw::new(camera.to_array(), partial, self.world.game_time);
        // NONE keeps these quads out of shader-pack entity programs.
        self.mark_stage(out.len(), "NONE");
        self.native_world_submit(&mut draw);
        self.resolve_inline_profiles(&draw.profiles);
        self.map_patches.extend(draw.patches);
        out.extend(draw.draw.into_vertices());
    }
    /// Normal, depth-tested atlas quad under the current pose. UVs use 16
    /// texels per sprite side; color is [red, green, blue, alpha]. No model
    /// diffuse lighting is applied; `draw.light` still selects lightmap light.
    /// False for an unknown sprite or non-finite geometry. API @API:world_canvas@.
    pub(super) fn native_world_quad(
        &self,
        draw: &mut NativeWorldDraw,
        sprite: &str,
        corners: [[f32; 3]; 4],
        uv: [f32; 4],
        color: [u8; 4],
    ) -> bool {
        self.gui
            .atlas
            .sprites
            .get(sprite)
            .copied()
            .is_some_and(|sprite| draw.quad(sprite, corners, uv, color))
    }
    /// Actual styled world font at (0,0), with ARGB default color and optional
    /// eight-offset ARGB outline. Returns the text advance in font pixels.
    /// The front of outlined text uses POLYGON_OFFSET. API @API:world_canvas@.
    pub(super) fn native_world_text(
        &self,
        draw: &mut NativeWorldDraw,
        text: &Text,
        color: u32,
        outline: Option<u32>,
    ) -> f32 {
        let packed = |c: u32| rgba((c >> 16) as u8, (c >> 8) as u8, c as u8, (c >> 24) as u8);
        let mut batch = HudBatch::new([1, 1]);
        let (advance, front) = if let Some(outline) = outline {
            let (advance, front) = batch.text_styled_outline_world(
                &self.gui.atlas,
                text,
                0.0,
                0.0,
                packed(color),
                packed(outline),
            );
            (advance, Some(front))
        } else {
            (
                batch.text_styled_world(&self.gui.atlas, text, 0.0, 0.0, packed(color), false),
                None,
            )
        };
        draw.text(batch, front);
        advance
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn world_quad_preserves_sub_block_precision_rgba_uv_and_pose() {
        let mut frame = NativeWorldDraw::new([30_000_000.125, 64.0, -30_000_000.5], 0.5, 123);
        assert!(frame.translate_world([30_000_000.625, 65.0, -30_000_000.25]));
        frame.scale([2.0, 3.0, 4.0]);
        frame.light = 0x12_34;
        assert!(frame.quad(
            Rect {
                w: 16,
                h: 16,
                u0: 0.25,
                v0: 0.5,
                u1: 0.75,
                v1: 1.0
            },
            [
                [0.0, 0.0, 0.0],
                [0.0, 1.0, 0.0],
                [1.0, 1.0, 0.0],
                [1.0, 0.0, 0.0]
            ],
            [4.0, 0.0, 12.0, 8.0],
            [12, 34, 56, 78]
        ));
        let vertices = frame.draw.into_vertices();
        assert_eq!(
            vertices.iter().map(|v| v.pos).collect::<Vec<_>>(),
            [
                [0.5, 1.0, 0.25],
                [0.5, 4.0, 0.25],
                [2.5, 4.0, 0.25],
                [2.5, 1.0, 0.25]
            ]
        );
        assert_eq!(
            vertices.iter().map(|v| v.uv).collect::<Vec<_>>(),
            [[0.375, 0.5], [0.375, 0.75], [0.625, 0.75], [0.625, 0.5]]
        );
        assert!(
            vertices
                .iter()
                .all(|v| v.color.to_le_bytes() == [12, 34, 56, 78]
                    && v.light == 0x12_34
                    && v.normal == [0; 4])
        );
    }
    #[test]
    fn invalid_world_offsets_leave_the_pose_usable() {
        let mut frame = NativeWorldDraw::new([0.0; 3], 0.0, 0);
        assert!(!frame.translate_world([f64::NAN, 0.0, 0.0]));
        assert!(!frame.translate_world([f64::MAX, 0.0, 0.0]));
        assert!(frame.translate_world([1.0, 2.0, 3.0]));
        assert_eq!(
            frame.draw.pose().transform_point3(Vec3::ZERO),
            Vec3::new(1.0, 2.0, 3.0)
        );
    }
}
