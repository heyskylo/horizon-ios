//! Section meshing on worker threads. Faces follow the baked blockstate models
//! with vanilla's neighbour culling; vertex light and colour follow
//! `ModelBlockRenderer`/`BlockModelLighter` (smooth light, ambient occlusion,
//! cardinal shading, tint sources, block offsets; see `lighting`). Fluids
//! follow `FluidRenderer` (see `fluid`).
mod fluid;
mod leaf_culling;
mod lighting;
pub mod packed;
mod sides;
#[cfg(test)]
#[path = "mesher/source_tests.rs"]
mod source_tests;
use crate::connected::{Pick, TileLayer};
use crate::{
    assets::{Assets, RenderAssets},
    biome::{Biomes, GrassModifier},
    blocks::{BlockFacts, RenderShape},
    chunk::{Chunk, MIN_Y},
    graphics::LeafCullingMode,
    models::catalog::BakedFace,
};
use horizon_assets::model::Direction as ModelDirection;
use horizon_core::{
    biome::BiomeZoom, geometry::Direction, position::BlockPos, random::position_seed,
};
use horizon_optifine::ctm::connect::{Axes as CtmAxes, Face as CtmFace};
use horizon_world::{
    block_render::FaceVisibility,
    fluid::{FluidKind, FluidState},
    registry::BlockStateId,
};
#[cfg(test)]
pub(crate) use sides::sort_tinted;
pub use sides::{Part, Sides, group_parts};
use std::sync::Arc;

/// An entity, item and effect vertex (the entity pipelines), with a face
/// normal for `entity.wgsl`'s directional light.
#[repr(C)]
#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Vertex {
    pub pos: [f32; 3],
    pub uv: [f32; 2],
    pub layer: u32,
    /// RGBA8: tint multiplied by directional shade and ambient occlusion.
    pub color: u32,
    /// Smooth light, 16x fixed point: sky in bits 0..8, block in bits 8..16.
    pub light: u32,
    /// RGBA8_SNORM face normal; w = -127 for two-sided lighting, 0 to keep baked/unlit colour,
    /// [`CULL_BACK`] to keep it and also drop back-facing pixels.
    pub normal: [i8; 4],
    /// The `GLINT_TEXTURING` atlas UV for a foil item quad; zero elsewhere.
    pub glint_uv: [f32; 2],
}
/// `Vertex::normal` of a baked-colour quad of an `entitySolid` model: the render type culls
/// back faces, which the entity pipelines here do not, so `entity.wgsl` discards them.
pub const CULL_BACK: [i8; 4] = [0, 0, 0, 1];
/// `Vertex::normal.w` of a shaded quad of an `entityCutoutCull` model (the bat): shaded by the
/// normal like -127, and `entity.wgsl` also discards its back-facing pixels.
pub const SHADED_CULL_W: i8 = -126;
/// A section vertex: vanilla's `DefaultVertexFormat.BLOCK` (position, colour,
/// UV0, UV2) with the sprite's array layer beside the light, in 24 bytes
/// rather than vanilla's 28: the UV is fixed point, which holds model UVs
/// (multiples of 1/16 and their halves) exactly. The position stays a float:
/// fixed point would move the edges of offset plants and rotated elements by
/// a fraction of a pixel. Terrain has no normal: `ModelBlockRenderer` bakes
/// the face shade into the colour and `terrain.vsh` only multiplies it by the
/// lightmap.
#[repr(C)]
#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct TerrainVertex {
    /// Section-relative position.
    pub pos: [f32; 3],
    /// UV within the sprite, `UV_STEPS` per unit (`TerrainVertex::new`,
    /// `terrain.wgsl`'s `terrain_uv`).
    pub(crate) uv: [u16; 2],
    /// Sprite array layer, with `OPAQUE_LAYER` for opaque leaves and quads over opaque texels.
    pub layer: u16,
    /// Smooth light, 16x fixed point: sky in the low byte, block in the high one.
    pub light: u16,
    /// RGBA8: tint multiplied by directional shade and ambient occlusion.
    pub color: u32,
}
/// `v.round().clamp(0.0, 65535.0) as u16`, without the `roundf` call x86-64's
/// baseline instruction set makes of `f32::round`: clamped first (the same
/// result), the fraction is exact below 2^23, and every float above is whole.
#[inline]
fn round_to_u16(v: f32) -> u16 {
    let v = v.clamp(0.0, f32::from(u16::MAX));
    let whole = v as u32;
    (whole + u32::from(v - whole as f32 >= 0.5)) as u16
}
/// Fixed-point steps per unit of `TerrainVertex` UVs, which span `[0, 2)`.
pub const UV_STEPS: f32 = 32768.0;
/// A model quad's own UVs in `TerrainVertex` fixed point, the packing
/// `TerrainVertex::new` does, baked once per face (`BakedFace::uv_fixed`).
pub(crate) fn baked_uv(uv: [[f32; 2]; 4]) -> [[u16; 2]; 4] {
    uv.map(|uv| uv.map(|v| round_to_u16(v * UV_STEPS)))
}
/// Fixed-point steps per block of distant-terrain UVs (`TerrainVertex::tiled`),
/// which span `[0, 2048)`: merged faces repeat their sprite once per block.
pub const TILED_UV_STEPS: f32 = 32.0;
impl TerrainVertex {
    pub fn new(pos: [f32; 3], uv: [f32; 2], layer: u16, color: u32, light: u16) -> Self {
        Self {
            pos,
            uv: uv.map(|v| round_to_u16(v * UV_STEPS)),
            layer,
            light,
            color,
        }
    }
    /// A vertex whose UV is already in `UV_STEPS` fixed point (a model face's
    /// own UVs, baked in `BakedFace::uv_fixed`).
    pub(crate) fn packed_uv(
        pos: [f32; 3],
        uv: [u16; 2],
        layer: u16,
        color: u32,
        light: u16,
    ) -> Self {
        Self {
            pos,
            uv,
            layer,
            light,
            color,
        }
    }
    /// A vertex of a merged distant-terrain face, its UV in blocks from the
    /// face's corner. Drawn with an instance UV scale of `1 / TILED_UV_STEPS`.
    pub fn tiled(pos: [f32; 3], uv: [f32; 2], layer: u16, color: u32, light: u16) -> Self {
        Self {
            pos,
            uv: uv.map(|v| round_to_u16(v * TILED_UV_STEPS)),
            layer,
            light,
            color,
        }
    }
    /// The UV the GPU samples.
    pub fn uv(&self) -> [f32; 2] {
        self.uv.map(|v| f32::from(v) / UV_STEPS)
    }
}
#[cfg(test)]
mod vertex_tests {
    use super::TerrainVertex;

    #[test]
    fn baked_face_keeps_shape_bounds_and_fixed_point_uvs() {
        use crate::models::catalog::BakedFace;
        use horizon_assets::{
            bake::ModelQuad,
            model::{Direction, Material},
        };

        let face = BakedFace::new(
            ModelQuad {
                positions: [
                    [-0.25, 0.5, 0.125],
                    [-0.25, 0.5, 0.875],
                    [0.625, 0.5, 0.875],
                    [0.625, 0.5, 0.125],
                ],
                uv: [
                    [-0.25, f32::NAN],
                    [1.0 / 65536.0, 3.0 / 65536.0],
                    [1.5, 2.0],
                    [f32::INFINITY, f32::NEG_INFINITY],
                ],
                facing: Direction::Up,
                cull: None,
                material: Material {
                    sprite: "test:face".into(),
                    force_translucent: false,
                },
                tint_index: -1,
                shade_direction_override: None,
                light_emission: 0,
                light: [0; 2],
                color: None,
                ambient_occlusion: None,
                render_type: None,
                shade: true,
            },
            0,
            true,
            false,
            None,
        );
        // Literal expectations: axis-ordered extrema and complements, plus
        // clamping and half-step rounding in 1/32768 UV units.
        let bounds: [f32; 12] = [
            0.5, 0.5, 0.125, 0.875, -0.25, 0.625, 0.5, 0.5, 0.875, 0.125, 1.25, 0.375,
        ];
        for shape in face.shapes {
            assert_eq!(shape.bounds.map(f32::to_bits), bounds.map(f32::to_bits));
            assert!(shape.partial);
        }
        assert!(!face.shapes[0].cubic);
        assert!(face.shapes[1].cubic);
        assert_eq!(face.uv_fixed, [[0, 0], [1, 2], [49152, 65535], [65535, 0]]);
    }

    #[test]
    fn model_uvs_pack_exactly() {
        for i in 0..=32 {
            let u = i as f32 / 32.0;
            let v = TerrainVertex::new([0.0; 3], [u, 1.0 - u], 0, 0, 0);
            assert_eq!(v.uv(), [u, 1.0 - u]);
        }
    }
    #[test]
    fn uv_rounding_matches_f32_round() {
        let reference = |v: f32| v.round().clamp(0.0, f32::from(u16::MAX)) as u16;
        let mut values = vec![
            f32::NAN,
            f32::INFINITY,
            f32::NEG_INFINITY,
            -0.0,
            0.5,
            1.5,
            65534.5,
            1e9,
        ];
        for bits in (0..u32::MAX).step_by(9973) {
            values.push(f32::from_bits(bits));
        }
        for i in 0..=140_000 {
            values.push(i as f32 * 0.5 - 2.0);
            values.push(i as f32 * 0.5 - 2.0 + f32::EPSILON);
        }
        for v in values {
            assert_eq!(super::round_to_u16(v), reference(v), "{v:e}");
        }
    }
    #[test]
    fn other_uvs_round_within_a_step() {
        // A flowing fluid's rotated UV.
        let uv = [0.5 + 0.25 * 0.3f32.sin(), 0.5 - 0.25 * 0.3f32.cos()];
        let packed = TerrainVertex::new([0.0; 3], uv, 0, 0, 0).uv();
        for (a, b) in packed.iter().zip(uv) {
            assert!((a - b).abs() <= 0.5 / 32768.0, "{a} {b}");
        }
    }
    #[test]
    fn emission_keeps_the_colour_and_the_free_bits() {
        let rgb = super::pack_rgb([0x12, 0x34, 0x56]);
        assert_eq!(super::emitting(rgb, 0), rgb);
        assert_eq!(super::emitting(rgb, super::INCANDESCENT), 0xFD56_3412);
        assert_eq!(super::emitting(rgb, super::VERY_BRIGHT), 0xFB56_3412);
        assert_eq!(super::emitting(rgb, super::LOW | super::WHOLE), 0xFA56_3412);
        assert_eq!(super::emitting(rgb, super::WHOLE), rgb);
        assert_eq!(
            super::emitting(rgb, super::VERY_BRIGHT) | super::EMISSION_BITS,
            rgb
        );
    }
    #[test]
    fn light_sources_shine_by_kind_not_light_level() {
        use super::{BRIGHT, INCANDESCENT, LOW, WHOLE, hdr_tier};
        assert_eq!(hdr_tier("glowstone", 15), BRIGHT);
        // A candle's flame is a particle: the block keeps only its wick.
        assert_eq!(hdr_tier("torch", 14), INCANDESCENT);
        assert_eq!(hdr_tier("red_candle", 3), LOW);
        assert_eq!(hdr_tier("glow_lichen", 7), 0);
        assert_eq!(hdr_tier("stone", 0), 0);
        assert_eq!(hdr_tier("ochre_froglight", 15), LOW | WHOLE);
        assert_eq!(hdr_tier("lava", 15), INCANDESCENT | WHOLE);
        assert_eq!(hdr_tier("red_candle_cake", 3), 0);
        assert_eq!(hdr_tier("respawn_anchor", 3), INCANDESCENT);
        assert_eq!(hdr_tier("waxed_copper_bulb", 15), LOW);
        assert_eq!(hdr_tier("sea_pickle", 15), 0);
        // Unlisted ones by their light level.
        assert_eq!(hdr_tier("vault", 12), INCANDESCENT);
        assert_eq!(hdr_tier("sculk_sensor", 1), LOW);
    }
    #[test]
    fn fluid_faces_keep_the_colour_and_the_emission() {
        let lit = super::emitting(super::pack_rgb([0x12, 0x34, 0x56]), super::INCANDESCENT);
        assert_eq!(super::fluid_face(lit, 0), 0x8D56_3412);
        assert_eq!(super::fluid_face(lit, 5), 0xDD56_3412);
        // Another face replaces the first.
        assert_eq!(super::fluid_face(super::fluid_face(lit, 5), 1), 0x9D56_3412);
    }
    /// What `gfx/native_material.rs` reads back from the colour's alpha byte: the face of a
    /// fluid quad, and the HDR tier.
    fn decoded(color: u32) -> (Option<u32>, u32) {
        let flags = color >> 24;
        let high = flags >> 4;
        let fluid = high >= 8 && (high & 7) < 6;
        (fluid.then_some(high & 7), 15 - (flags & 15))
    }
    #[test]
    fn the_material_stage_reads_back_what_the_mesher_encodes() {
        let rgb = super::pack_rgb([0x12, 0x34, 0x56]);
        for tier in 0..=super::VERY_BRIGHT {
            let lit = super::emitting(rgb, tier);
            assert_eq!(decoded(lit), (None, u32::from(tier)));
            for face in 0..6 {
                assert_eq!(
                    decoded(super::fluid_face(lit, face)),
                    (Some(face as u32), u32::from(tier))
                );
            }
        }
    }
}

#[cfg(test)]
mod section_mesh_tests {
    use super::*;

    #[test]
    fn compute_bounds_empty_solid_and_translucent() {
        let mut mesh = SectionMesh::default();
        assert_eq!(mesh.compute_bounds(), None);

        let v1 = TerrainVertex::new([1.0, 2.0, 3.0], [0.0, 0.0], 0, 0, 0);
        let v2 = TerrainVertex::new([4.0, 5.0, 6.0], [0.0, 0.0], 0, 0, 0);
        mesh.solid.push(v1);
        mesh.solid.push(v2);

        let pad = glam::Vec3::splat(SectionMesh::BOUNDS_PAD);
        assert_eq!(
            mesh.compute_bounds(),
            Some([
                glam::Vec3::new(1.0, 2.0, 3.0) - pad,
                glam::Vec3::new(4.0, 5.0, 6.0) + pad,
            ])
        );

        let v3 = TerrainVertex::new([0.5, 7.0, 8.0], [0.0, 0.0], 0, 0, 0);
        mesh.translucent.push(v3);
        assert_eq!(
            mesh.compute_bounds(),
            Some([
                glam::Vec3::new(0.5, 2.0, 3.0) - pad,
                glam::Vec3::new(4.0, 7.0, 8.0) + pad,
            ])
        );
    }
}
/// A shader terrain quad's unmodified source state and owning section cell. Two words
/// per quad, kept beside (not in) the native 24-byte vertex format. Word 1 has cell XYZ
/// in three nibbles, a fluid flag, a valid flag, then the emitted light (0..15).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct TerrainSource {
    state: u32,
    cell: u32,
}
impl TerrainSource {
    pub(crate) fn new(state: u32, pos: [i32; 3], emission: u8, fluid: bool) -> Self {
        debug_assert!(pos.iter().all(|&v| (0..16).contains(&v)));
        Self {
            state,
            cell: pos[0] as u32
                | (pos[1] as u32) << 4
                | (pos[2] as u32) << 8
                | u32::from(fluid) << 12
                | 1 << 13
                | u32::from(emission.min(15)) << 14,
        }
    }
}

pub struct SectionMesh {
    pub solid: Vec<TerrainVertex>,
    /// One source per quad when a shader pack meshes terrain; independent of its sprite
    /// and geometry. Empty on native/default meshes and on distant terrain.
    pub solid_sources: Vec<TerrainSource>,
    /// The buckets `solid` is sorted in by the side its quads face: empty
    /// until `sort_solid`, which the meshing workers call; `solid` is in
    /// model order before.
    pub sides: Sides,
    pub translucent: Vec<TerrainVertex>,
    pub translucent_sources: Vec<TerrainSource>,
    /// Whether `packed` holds far tile records (`lod::packed`) rather than
    /// section records (`mesher::packed`): the two are read apart, by the base
    /// vertex their draws use.
    pub packed_far: bool,
    /// `solid` in the compact layout (`packed`), when the renderer reads it and it is
    /// smaller; empty otherwise. Made by `sort_solid`.
    pub packed: Vec<[u32; 6]>,
    /// The tint of each solid quad as the mesher multiplied it (`packed::UNKNOWN_TINT` where
    /// it does not say), for `packed`; dropped once that is made.
    pub(crate) solid_tints: Vec<u32>,
    /// VisGraph: bit `a * 6 + b` is set when faces `a` and `b` (Direction
    /// order) are connected through non-opaque blocks.
    pub visibility: u64,
    /// Blocks with a block-entity renderer (world position, state id), drawn
    /// each frame by `block_entity_render` like vanilla's per-section
    /// `SectionCompiler.Results.blockEntities`.
    pub block_entities: Vec<([i32; 3], u32)>,
    /// Blocks that give off light (world position, state id), for the colour of block light
    /// Horizon's shader packs read (`gfx::block_light`).
    pub lights: Vec<([i32; 3], u32)>,
    /// Sprite layers emitted while building this mesh. Kept distinct before
    /// upload so the renderer does not rescan every terrain vertex.
    pub sprite_layers: Vec<u16>,
    /// Axis-aligned bounding box [min, max] of actual terrain mesh vertices in section-local space [0, 16].
    pub bounds: Option<[glam::Vec3; 2]>,
    /// A far mesh was clipped around finer tiles or resident chunks. Keep
    /// the previous GPU mesh while its replacements fade in.
    pub keep: bool,
}
impl Default for SectionMesh {
    fn default() -> Self {
        Self {
            solid: Vec::new(),
            solid_sources: Vec::new(),
            sides: Sides::default(),
            translucent: Vec::new(),
            translucent_sources: Vec::new(),
            packed: Vec::new(),
            packed_far: false,
            solid_tints: Vec::new(),
            visibility: ALL_VISIBLE,
            block_entities: Vec::new(),
            lights: Vec::new(),
            sprite_layers: Vec::new(),
            bounds: None,
            keep: false,
        }
    }
}
impl SectionMesh {
    /// Record a sprite layer used by a quad. Opaque is a vertex-only flag and
    /// is removed here so the renderer can compare the actual atlas layer.
    pub fn note_sprite_layer(&mut self, layer: u16) {
        let layer = layer & !crate::mesher::OPAQUE_LAYER;
        if !self.sprite_layers.contains(&layer) {
            self.sprite_layers.push(layer);
        }
    }
    /// Finish the compact layer list after meshing has emitted all faces.
    pub fn finish_sprite_layers(&mut self) {
        self.sprite_layers.sort_unstable();
        self.sprite_layers.dedup();
    }

    /// Padding applied to mesh vertex bounds to prevent clipping at the frustum
    /// boundary when shader packs with vertex displacement (e.g. waving foliage, water) are active.
    pub const BOUNDS_PAD: f32 = 0.5;

    /// Retain geometry capacity after an upload, but none of its old contents.
    pub fn clear(&mut self) {
        self.solid.clear();
        self.solid_sources.clear();
        self.translucent.clear();
        self.translucent_sources.clear();
        self.packed.clear();
        self.packed_far = false;
        self.solid_tints.clear();
        self.block_entities.clear();
        self.lights.clear();
        self.sprite_layers.clear();
        self.sides = Sides::default();
        self.visibility = ALL_VISIBLE;
        self.bounds = None;
        self.keep = false;
    }
    pub(crate) fn capacity_bytes(&self) -> usize {
        self.solid.capacity() * std::mem::size_of::<TerrainVertex>()
            + self.translucent.capacity() * std::mem::size_of::<TerrainVertex>()
            + (self.solid_sources.capacity() + self.translucent_sources.capacity())
                * std::mem::size_of::<TerrainSource>()
            + self.packed.capacity() * std::mem::size_of::<[u32; 6]>()
            + self.solid_tints.capacity() * std::mem::size_of::<u32>()
            + self.block_entities.capacity() * std::mem::size_of::<([i32; 3], u32)>()
            + self.lights.capacity() * std::mem::size_of::<([i32; 3], u32)>()
            + self.sprite_layers.capacity() * std::mem::size_of::<u16>()
    }
    /// Sorts the solid quads by the side they face, for the renderer to
    /// leave out those facing away from the eye (`sides`).
    /// Then lays them out compactly (`packed`) while the renderer reads that.
    pub fn sort_solid(&mut self) {
        let quads = self.solid.len() / 4;
        if !self.solid_tints.is_empty() {
            self.solid_tints.resize(quads, packed::UNKNOWN_TINT);
        }
        self.sides = sides::sort_sourced(
            &mut self.solid,
            &mut self.solid_tints,
            &mut self.solid_sources,
        );
        self.bounds = self.compute_bounds();
        self.packed.clear();
        self.packed_far = false;
        if packed::packing() && quads > 0 && self.solid_tints.len() == quads {
            packed::layout_into(&self.solid, &self.solid_tints, &mut self.packed);
        }
        self.solid_tints.clear();
    }

    /// Every face emitted since `start` belongs to this block, including multipart,
    /// connected-texture pieces, overlays and duplicated/back-facing fluid faces.
    fn record_source(&mut self, start: [usize; 2], source: TerrainSource) {
        for (vertices, sources, start) in [
            (&self.solid, &mut self.solid_sources, start[0]),
            (&self.translucent, &mut self.translucent_sources, start[1]),
        ] {
            if vertices.len() > start {
                sources.resize(start / 4, TerrainSource::default());
                sources.resize(vertices.len() / 4, source);
            }
        }
    }

    /// Computes the tight axis-aligned bounding box [min, max] of actual terrain geometry vertices.
    pub fn compute_bounds(&self) -> Option<[glam::Vec3; 2]> {
        if self.solid.is_empty() && self.translucent.is_empty() {
            return None;
        }
        let mut min = glam::Vec3::splat(f32::INFINITY);
        let mut max = glam::Vec3::splat(f32::NEG_INFINITY);
        for v in &self.solid {
            let p = glam::Vec3::from(v.pos);
            min = min.min(p);
            max = max.max(p);
        }
        for v in &self.translucent {
            let p = glam::Vec3::from(v.pos);
            min = min.min(p);
            max = max.max(p);
        }
        let pad = glam::Vec3::splat(Self::BOUNDS_PAD);
        Some([min - pad, max + pad])
    }
}
pub const ALL_VISIBLE: u64 = (1 << 36) - 1;
/// VisGraph.resolve: flood fill non-opaque cells and connect the faces each region touches.
#[cfg(test)]
fn visibility(pad: &Padded, info: &[Info]) -> u64 {
    let solid: Vec<bool> = info.iter().map(|i| i.solid).collect();
    let mut rows = Vec::new();
    solid_rows(pad, &solid, &mut rows);
    visibility_rows(&open_rows(&rows))
}
/// The section's cells that are not `isSolidRender`, by row (`y << 4 | z`, bit
/// x), from the padded rows of `MeshScratch::solid_rows`.
#[cfg(test)]
fn open_rows(solid: &[u32]) -> [u16; 256] {
    std::array::from_fn(|r| {
        let (y, z) = ((r >> 4) as i32, (r & 15) as i32);
        !(solid[((y + B) as usize) * P + (z + B) as usize] >> B) as u16
    })
}
/// The cells of `open` connected to `seeds` (a subset) within the row: an
/// occluded fill each way, doubling its reach at each step.
#[inline]
fn fill_row(seeds: u16, open: u16) -> u16 {
    let (mut up, mut through_up) = (seeds, open);
    let (mut down, mut through_down) = (seeds, open);
    for step in [1, 2, 4, 8] {
        up |= through_up & up << step;
        through_up &= through_up << step;
        down |= through_down & down >> step;
        through_down &= through_down >> step;
    }
    up | down
}
/// `visibility` on bit rows, one region at a time: a region grows a row at
/// a time (each row filled along its open runs at once) by sweeps over the
/// rows it can reach, until a sweep adds nothing.
fn visibility_rows(open: &[u16; 256]) -> u64 {
    let open_count: u32 = open.iter().map(|r| r.count_ones()).sum();
    if open_count == 4096 {
        return ALL_VISIBLE;
    }
    if open_count == 0 {
        return 0;
    }
    let mut unvisited = *open;
    let mut region = [0u16; 256];
    let mut result = 0u64;
    for start in 0..256 {
        while unvisited[start] != 0 {
            let seed = unvisited[start] & unvisited[start].wrapping_neg();
            region[start] = fill_row(seed, unvisited[start]);
            let (mut lo, mut hi) = (start, start);
            loop {
                let mut grown = false;
                let (first, last) = (lo.saturating_sub(16), (hi + 16).min(255));
                for r in (first..=last).chain((first..=last).rev()) {
                    let mut near = region[r];
                    if r >= 16 {
                        near |= region[r - 16];
                    }
                    if r + 16 < 256 {
                        near |= region[r + 16];
                    }
                    if r & 15 != 0 {
                        near |= region[r - 1];
                    }
                    if r & 15 != 15 {
                        near |= region[r + 1];
                    }
                    let reached = near & unvisited[r];
                    if reached & !region[r] != 0 {
                        region[r] = fill_row(reached, unvisited[r]);
                        (lo, hi) = (lo.min(r), hi.max(r));
                        grown = true;
                    }
                }
                if !grown {
                    break;
                }
            }
            let mut faces = 0u8;
            for r in lo..=hi {
                let row = std::mem::take(&mut region[r]);
                if row == 0 {
                    continue;
                }
                unvisited[r] &= !row;
                let (y, z) = (r >> 4, r & 15);
                faces |= u8::from(y == 0)
                    | u8::from(y == 15) << 1
                    | u8::from(z == 0) << 2
                    | u8::from(z == 15) << 3
                    | u8::from(row & 1 != 0) << 4
                    | u8::from(row & 0x8000 != 0) << 5;
            }
            // A single connected region touching all six faces sets every
            // pair (including the diagonal); unvisited cavities cannot add more.
            if faces == 0b11_1111 {
                return ALL_VISIBLE;
            }
            for a in 0..6 {
                if faces & (1 << a) != 0 {
                    for b in 0..6 {
                        if faces & (1 << b) != 0 {
                            result |= 1 << (a * 6 + b);
                        }
                    }
                }
            }
            if result == ALL_VISIBLE {
                return result;
            }
        }
    }
    result
}
#[cfg(test)]
mod particle_tests;
#[cfg(test)]
mod visibility_tests;
/// A face's tint source as the game colours it (`BlockColors.createDefault`), for drawing a block
/// elsewhere ([`crate::block_sprites`]): the biome's grass, foliage, dry foliage or water colour
/// where the block is, or a fixed colour (0xRRGGBB).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TintSource {
    Grass,
    Foliage,
    DryFoliage,
    Water,
    Fixed(u32),
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum Tint {
    None,
    Grass,
    /// `BlockTintSources.grassBlock`: grass in the world, none on its particles.
    GrassBlock,
    TallGrassUpper,
    SugarCane,
    LilyPad,
    Foliage,
    DryFoliage,
    Water,
    Constant(u32),
    Redstone(u8),
    Stem(u8),
}
/// Per-state data the mesher needs on every block, flattened for speed.
#[derive(Clone, Copy)]
struct Info {
    /// `isSolidRender`.
    solid: bool,
    /// `isCollisionShapeFullBlock`: AO faces flush with a full block sample outside it.
    full: bool,
    /// `isLightPermeable`: diagonal AO samples are taken through such side blocks.
    permeable: bool,
    /// `getShadeBrightness` (ambient occlusion contribution).
    shade: f32,
    emission: u8,
    /// `hdr_tier`.
    tier: u8,
    /// `emissiveRendering`: drawn and sampled at full brightness.
    emissive: bool,
    fluid: FluidState,
    model: bool,
    /// The state has a block entity (candidates for block-entity renderers).
    block_entity: bool,
    tints: [Tint; 2],
    tint_count: u8,
    /// A resource pack's Polytone colormap over its tints (an index in `MeshTables::colormaps`).
    colormap: Option<u16>,
    /// `OffsetType` as (max horizontal, max vertical): vertical 0 for XZ.
    offset: Option<(f32, f32)>,
    /// `isSolid()` (legacy), for fluid corner heights.
    legacy_solid: bool,
    /// `HalfTransparentBlock`/`LeavesBlock`: water sides show the overlay sprite.
    overlay: bool,
    /// `LeavesBlock`: with opaque leaves the faces between two leaf blocks are skipped.
    leaves: bool,
    /// A glass pane: connected-texture tiles take the cube's UVs on it (OptiFine's pane models
    /// drop vanilla's mirrored faces).
    pane: bool,
    /// A full opaque cube: `RenderShape::Model`, `can_occlude`, `solid_render`
    /// and a full collision shape. The only block leaf culling treats as solid.
    opaque_cube: bool,
    blocks_flow: bool,
    /// `instanceof IceBlock` (frosted ice too): fluids read it.
    ice: bool,
    /// Regular ice, the block the Opaque Ice option draws opaque; frosted ice
    /// keeps its see-through, cracking look.
    opaque_ice: bool,
    sturdy: u8,
    /// Occlusion face per direction: 0 empty, 1 the full-block singleton, 2 partial.
    face: [u8; 6],
    /// Height from the bottom over which face `d`, flush with the `d` side, covers
    /// the full width (vertical faces: 1 when the whole square is covered).
    cover: [f32; 6],
}
/// Derived per-state facts, exposed for the section-mesh parity gate.
#[derive(Clone, Copy, Debug)]
pub struct MeshFacts {
    pub shade: f32,
    pub emissive: bool,
    pub legacy_solid: bool,
    pub light_permeable: bool,
    pub solid_render: bool,
    pub full_collision: bool,
    pub emission: u8,
    pub overlay: bool,
    pub tint_sources: u8,
}
pub struct MeshTables {
    info: Vec<Info>,
    /// `Info::solid` by state, packed apart: the opacity mask of a section
    /// reads 8,000 states, and a few bytes each stay in cache where `Info`
    /// does not.
    solid: Vec<bool>,
    /// By state, bit `d` set where its occlusion face toward direction `d` is the
    /// full block (`Info::face` 1): a face looking at it never shows.
    full_faces: Vec<u8>,
    pub visibility: FaceVisibility,
    water_still: u16,
    water_flow: u16,
    water_overlay: u16,
    lava_still: u16,
    lava_flow: u16,
    animated_layers: Vec<bool>,
    /// Resource packs' Polytone block and fluid colormaps (#11486), and the water's.
    colormaps: Vec<Arc<crate::environment::Colormap>>,
    water_colormap: Option<u16>,
}
fn stem_color(age: u8) -> u32 {
    let a = u32::from(age);
    (a * 32) << 16 | (255 - a * 8) << 8 | (a * 4)
}
fn redstone_color(power: u8) -> u32 {
    // RedstoneWireBlock.COLORS
    let f = f32::from(power) / 15.0;
    let r = f * 0.6 + if f > 0.0 { 0.4 } else { 0.3 };
    let g = (f * f * 0.7 - 0.5).clamp(0.0, 1.0);
    let b = (f * f * 0.6 - 0.7).clamp(0.0, 1.0);
    ((r * 255.0) as u32) << 16 | ((g * 255.0) as u32) << 8 | (b * 255.0) as u32
}
impl MeshTables {
    pub fn new(
        assets: &Assets,
        render: &RenderAssets,
        facts: &BlockFacts,
        visibility: FaceVisibility,
    ) -> crate::Result<Self> {
        // Terrain vertices keep the sprite layer below `OPAQUE_LAYER`.
        let layers = render.sprite_layers.values().max().map_or(0, |l| l + 1);
        if layers > u32::from(OPAQUE_LAYER) {
            return Err(
                format!("{layers} block sprites, terrain vertices index {OPAQUE_LAYER}").into(),
            );
        }
        let layer = |name: &str| -> crate::Result<u16> {
            render
                .sprite_layers
                .get(name)
                .map(|&l| l as u16)
                .ok_or_else(|| format!("missing fluid sprite {name}").into())
        };
        let max_animated = render.animations.iter().map(|a| a.layer).max().unwrap_or(0);
        let mut animated_layers = vec![false; max_animated as usize + 1];
        for animation in &render.animations {
            animated_layers[animation.layer as usize] = true;
        }
        let mut covers = std::collections::HashMap::new();
        let mut info = Vec::with_capacity(facts.states.len());
        for (id, s) in facts.states.iter().enumerate() {
            let state = assets
                .registry
                .state(BlockStateId(id as u32))
                .ok_or("missing registry state")?;
            let props = &state.properties;
            let prop = |k: &str| -> u8 { props.get(k).and_then(|v| v.parse().ok()).unwrap_or(0) };
            let name = s.name.strip_prefix("minecraft:").unwrap_or(&s.name);
            // BlockColors.createDefault: tint sources by block.
            let (tints, tint_count) = match name {
                "large_fern" | "tall_grass" => {
                    if props.get("half").map(String::as_str) == Some("upper") {
                        ([Tint::TallGrassUpper; 2], 1)
                    } else {
                        ([Tint::Grass; 2], 1)
                    }
                }
                "fern" | "short_grass" | "potted_fern" | "bush" => ([Tint::Grass; 2], 1),
                "grass_block" => ([Tint::GrassBlock; 2], 1),
                "sugar_cane" => ([Tint::SugarCane; 2], 1),
                "pink_petals" | "wildflowers" => ([Tint::None, Tint::Grass], 2),
                "spruce_leaves" => ([Tint::Constant(0x61_9961); 2], 1),
                "birch_leaves" => ([Tint::Constant(0x80_A755); 2], 1),
                "oak_leaves" | "jungle_leaves" | "acacia_leaves" | "dark_oak_leaves" | "vine"
                | "mangrove_leaves" => ([Tint::Foliage; 2], 1),
                "leaf_litter" => ([Tint::DryFoliage; 2], 1),
                "water_cauldron" => ([Tint::Water; 2], 1),
                // Their in-world colour is the particle-only source's -1.
                "water" | "bubble_column" => ([Tint::None; 2], 1),
                "redstone_wire" => ([Tint::Redstone(prop("power")); 2], 1),
                "attached_melon_stem" | "attached_pumpkin_stem" => {
                    ([Tint::Constant(0xE0_C71C); 2], 1)
                }
                "melon_stem" | "pumpkin_stem" => ([Tint::Stem(prop("age")); 2], 1),
                "lily_pad" => ([Tint::LilyPad; 2], 1),
                _ => ([Tint::None; 2], 0),
            };
            // getShadeBrightness overrides; the default is 0.2 for full collision shapes.
            let overrides = [
                "TransparentBlock",
                "BarrierBlock",
                "LightBlock",
                "StructureVoidBlock",
            ];
            let shade = if overrides.iter().any(|c| s.has_class(c)) {
                1.0
            } else if s.has_class("SoulSandBlock") || s.has_class("MudBlock") {
                0.2
            } else if s.has_class("SnowLayerBlock") {
                if prop("layers") == 8 { 0.2 } else { 1.0 }
            } else if s.full_collision {
                0.2
            } else {
                1.0
            };
            let emissive = name == "magma_block"
                || (matches!(name, "sculk_sensor" | "calibrated_sculk_sensor")
                    && props.get("sculk_sensor_phase").map(String::as_str) == Some("active"));
            // OffsetType.XZ/XYZ with getMaxHorizontalOffset/getMaxVerticalOffset.
            let offset = s.random_offset.then(|| {
                let horizontal = if s.has_class("SpeleothemBlock") {
                    0.125
                } else {
                    0.25
                };
                let vertical = match name {
                    "small_dripleaf" => 0.1,
                    "short_grass" | "fern" | "short_dry_grass" | "tall_dry_grass" => 0.2,
                    _ => 0.0,
                };
                (horizontal, vertical)
            });
            let mut face = [0u8; 6];
            let mut cover = [0f32; 6];
            // A server's visibility knows no state (`FaceVisibility::empty`):
            // it meshes no fluid, so these stay unset.
            for d in Direction::ALL
                .into_iter()
                .filter(|_| !visibility.is_empty())
            {
                let (face_id, shape) = visibility
                    .occlusion_face(BlockStateId(id as u32), d)
                    .ok_or("missing occlusion face")?;
                face[d as usize] = face_id.min(2) as u8;
                cover[d as usize] = *covers
                    .entry((face_id, d as u8))
                    .or_insert_with(|| fluid::coverage(shape, d));
            }
            info.push(Info {
                solid: s.solid_render,
                full: s.full_collision,
                permeable: !s.solid_render || s.dampening == 0,
                shade,
                emission: s.emission,
                tier: hdr_tier(name, s.emission),
                emissive,
                fluid: FluidState::from_block(state)?,
                model: s.render_shape == RenderShape::Model,
                block_entity: s.block_entity,
                tints,
                tint_count,
                colormap: None,
                offset,
                legacy_solid: s.legacy_solid,
                overlay: s.has_class("HalfTransparentBlock") || s.has_class("LeavesBlock"),
                leaves: s.has_class("LeavesBlock"),
                pane: s.has_class("IronBarsBlock") && name.ends_with("glass_pane"),
                opaque_cube: leaf_culling::opaque_cube(
                    s.render_shape,
                    s.can_occlude,
                    s.solid_render,
                    s.full_collision,
                ),
                blocks_flow: assets.blocks_fluid_flow.contains(&*s.name),
                ice: s.has_class("IceBlock"),
                opaque_ice: s.has_class("IceBlock") && !s.has_class("FrostedIceBlock"),
                sturdy: s.sturdy_full,
                face,
                cover,
            });
        }
        Ok(Self {
            solid: info.iter().map(|i| i.solid).collect(),
            full_faces: info
                .iter()
                .map(|i| (0..6).fold(0, |bits, d| bits | u8::from(i.face[d] == 1) << d))
                .collect(),
            info,
            visibility,
            water_still: layer("minecraft:block/water_still")?,
            water_flow: layer("minecraft:block/water_flow")?,
            water_overlay: layer("minecraft:block/water_overlay")?,
            lava_still: layer("minecraft:block/lava_still")?,
            lava_flow: layer("minecraft:block/lava_flow")?,
            animated_layers,
            colormaps: Vec::new(),
            water_colormap: None,
        })
    }
    /// These tables with the colormaps of the Polytone block and fluid modifiers of the packs
    /// merged at `pack_root` (wiki "Colormaps"): a tinted block they target takes its colour
    /// from the colormap, and its own tint where the colormap's pixel is transparent.
    #[must_use]
    pub fn with_polytone_colormaps(
        mut self,
        pack_root: &std::path::Path,
        registry: &horizon_world::registry::BlockRegistry,
    ) -> Self {
        let modifiers = crate::environment::polytone_colormap_modifiers(pack_root);
        if modifiers.is_empty() {
            return self;
        }
        let mut loaded: std::collections::HashMap<String, Option<u16>> = Default::default();
        let mut index = |id: &str, colormaps: &mut Vec<Arc<crate::environment::Colormap>>| {
            *loaded.entry(id.to_owned()).or_insert_with(|| {
                let colormap = crate::environment::Colormap::load(pack_root, id)?;
                colormaps.push(Arc::new(colormap));
                Some((colormaps.len() - 1) as u16)
            })
        };
        for (fluid, targets, id) in &modifiers {
            if *fluid {
                if targets.matches("minecraft:water", |_| false) {
                    self.water_colormap = index(id, &mut self.colormaps);
                }
                continue;
            }
            let Some(i) = index(id, &mut self.colormaps) else {
                continue;
            };
            for (state, info) in self.info.iter_mut().enumerate() {
                let Some(block) = registry.state(BlockStateId(state as u32)) else {
                    continue;
                };
                if info.tint_count > 0 && targets.matches(&block.name, |_| false) {
                    info.colormap = Some(i);
                }
            }
        }
        self
    }
    /// Whether a terrain sprite layer has an animated clip.
    pub fn is_animated(&self, layer: u16) -> bool {
        self.animated_layers
            .get(layer as usize)
            .copied()
            .unwrap_or(false)
    }
    /// HDR output's tier of a state (`hdr_tier`), for what meshes it elsewhere (the LOD tiles).
    pub fn hdr_tier(&self, state: BlockStateId) -> u8 {
        self.info.get(state.0 as usize).map_or(0, |i| i.tier)
    }
    pub fn facts(&self, state: BlockStateId) -> Option<MeshFacts> {
        let i = self.info.get(state.0 as usize)?;
        Some(MeshFacts {
            shade: i.shade,
            emissive: i.emissive,
            legacy_solid: i.legacy_solid,
            light_permeable: i.permeable,
            solid_render: i.solid,
            full_collision: i.full,
            emission: i.emission,
            overlay: i.overlay,
            tint_sources: i.tint_count,
        })
    }
    /// Whether `state` is a full opaque cube: the only block leaf culling
    /// treats as a solid neighbour. Glass, fluids, slabs, stairs and models
    /// that only partly fill their block are not.
    pub fn opaque_cube(&self, state: BlockStateId) -> bool {
        self.info
            .get(state.0 as usize)
            .is_some_and(|i| i.opaque_cube)
    }
    /// `BlockTintSource.color(state)`: block-model colour without a level.
    /// `grass` is `GrassColor.getDefaultColor()` from the active pack's colormap.
    /// Untinted faces, blank sources and invalid tint indices return `None`.
    pub fn default_tint(&self, state: BlockStateId, index: i32, grass: u32) -> Option<u32> {
        let info = self.info.get(state.0 as usize)?;
        if index < 0 || index >= i32::from(info.tint_count) {
            return None;
        }
        Some(match info.tints[index as usize] {
            Tint::None | Tint::SugarCane | Tint::Water => return None,
            Tint::Grass | Tint::GrassBlock | Tint::TallGrassUpper => grass & 0xFF_FFFF,
            Tint::Foliage => 0x48_B518,
            Tint::DryFoliage => 0x5C_3C32,
            Tint::LilyPad => 0x71_C35C,
            Tint::Constant(c) => c,
            Tint::Redstone(p) => redstone_color(p),
            Tint::Stem(age) => stem_color(age),
        })
    }
    /// What tint source `index` (a face's `tintindex`) of `state` colours a face with in the
    /// world, for drawing the block elsewhere ([`crate::block_sprites`]): `None` for an untinted
    /// face. Grass blocks, tall grass and sugar cane are `Grass`; redstone and stems are their
    /// colour at the state's power and age.
    pub fn tint_source(&self, state: BlockStateId, index: i32) -> Option<TintSource> {
        let info = self.info.get(state.0 as usize)?;
        if index < 0 || index >= i32::from(info.tint_count) {
            return None;
        }
        Some(match info.tints[index as usize] {
            Tint::None => return None,
            Tint::Grass | Tint::GrassBlock | Tint::TallGrassUpper | Tint::SugarCane => {
                TintSource::Grass
            }
            Tint::Foliage => TintSource::Foliage,
            Tint::DryFoliage => TintSource::DryFoliage,
            Tint::Water => TintSource::Water,
            Tint::LilyPad => TintSource::Fixed(0x20_8030),
            Tint::Constant(c) => TintSource::Fixed(c),
            Tint::Redstone(p) => TintSource::Fixed(redstone_color(p)),
            Tint::Stem(age) => TintSource::Fixed(stem_color(age)),
        })
    }
    /// `TerrainParticle`'s colour: tint source 0's `colorAsTerrainParticle`,
    /// which is the in-world (biome-blended) colour except on grass blocks,
    /// whose dirt crumbs stay untinted. `None` when the state has no source.
    /// `region[dz][dx]` are the chunks around `pos`'s chunk (centre 1,1).
    pub fn terrain_particle_tint(
        &self,
        state: BlockStateId,
        pos: [i32; 3],
        region: &[[Option<&Chunk>; 3]; 3],
        biomes: &Biomes,
        zoom: BiomeZoom,
        blend_radius: i32,
    ) -> Option<u32> {
        let info = self.info.get(state.0 as usize)?;
        if info.tints[0] == Tint::GrassBlock {
            return None;
        }
        let [x, y, z] = pos;
        let origin = [x >> 4, z >> 4, y & !15];
        let mut tints = Tints::new(region, biomes, zoom, blend_radius, origin).with_colormaps(self);
        Mesher::tint(info, 0, &mut tints, [x & 15, y & 15, z & 15])
    }
    /// `BlockColors` for each of the state's tint indices at `pos`, as the
    /// chunk mesher tints them (grass blocks included): `None` for an
    /// untinted (-1) layer. For block models drawn outside the chunk mesh.
    pub fn block_tints(
        &self,
        state: BlockStateId,
        pos: [i32; 3],
        region: &[[Option<&Chunk>; 3]; 3],
        biomes: &Biomes,
        zoom: BiomeZoom,
        blend_radius: i32,
    ) -> impl Fn(i32) -> Option<u32> + use<> {
        let sources = self.info.get(state.0 as usize).map(|info| {
            let [x, y, z] = pos;
            let origin = [x >> 4, z >> 4, y & !15];
            let mut tints =
                Tints::new(region, biomes, zoom, blend_radius, origin).with_colormaps(self);
            let local = [x & 15, y & 15, z & 15];
            let colors = [0, 1].map(|i| Mesher::tint(info, i, &mut tints, local));
            (colors, i32::from(info.tint_count))
        });
        move |index| {
            let (colors, count) = sources?;
            if index < 0 || index >= count {
                return None;
            }
            colors[(index as usize).min(1)]
        }
    }
    /// `ModelBlockRenderer.tesselateFlat`'s light coords (`LightCoordsUtil`
    /// packing, the quad's own emission included) for `quad` of `state`
    /// drawn as the only block at `pos`, as `MovingBlockRenderState` gives
    /// it: `light` is the world's stored light (sky << 4 | block) at a
    /// position. For block models drawn outside the chunk mesh.
    pub fn flat_light(
        &self,
        state: BlockStateId,
        pos: [i32; 3],
        quad: &horizon_assets::bake::ModelQuad,
        light: impl Fn([i32; 3]) -> u8,
    ) -> u32 {
        let own = self.info.get(state.0 as usize);
        let full = own.is_some_and(|i| i.full);
        let at = lighting::flat_position(pos, &quad.positions, quad.facing, quad.cull, full);
        let packed = light(at);
        let coords = match own {
            Some(own) => lighting::packed_coords(own, packed),
            None => u32::from(packed & 15) << 4 | u32::from(packed >> 4) << 20,
        };
        lighting::with_min(coords, quad.light)
    }
    /// `BlockState.getOffset` for blocks with a random model offset.
    pub fn offset(&self, state: BlockStateId, x: i32, z: i32) -> Option<[f64; 3]> {
        offset(self.info.get(state.0 as usize)?, x, z)
    }
}
/// `OffsetType.XZ`/`XYZ`: a per-column jitter from `Mth.getSeed(x, 0, z)`.
fn offset(info: &Info, x: i32, z: i32) -> Option<[f64; 3]> {
    let (horizontal, vertical) = info.offset?;
    let seed = position_seed(BlockPos::new(x, 0, z));
    let nibble = |shift: u32| ((seed >> shift) & 15) as f32 / 15.0;
    let h = f64::from(horizontal);
    Some([
        ((f64::from(nibble(0)) - 0.5) * 0.5).clamp(-h, h),
        (f64::from(nibble(4)) - 1.0) * f64::from(vertical),
        ((f64::from(nibble(8)) - 0.5) * 0.5).clamp(-h, h),
    ])
}
/// Border width: smooth lighting samples up to two blocks out along a face normal.
const B: i32 = 2;
const P: usize = 16 + 2 * B as usize;
#[inline]
fn pi(x: i32, y: i32, z: i32) -> usize {
    ((y + B) as usize * P + (z + B) as usize) * P + (x + B) as usize
}
/// `pi`'s index offset of a step `n`: `pi(p + n) - pi(p)`, so the samples
/// around a cell are `base + step_index(n)` rather than another `pi`.
#[inline]
const fn step_index(n: [i32; 3]) -> isize {
    (n[0] + P as i32 * n[2] + (P * P) as i32 * n[1]) as isize
}
/// Blocks and light of one section plus a `B`-block border.
#[derive(Default)]
struct Padded {
    blocks: Vec<u32>,
    light: Vec<u8>,
}
impl Padded {
    #[cfg(test)]
    fn new(region: &[[Option<&Chunk>; 3]; 3], sy: i32) -> Self {
        let mut pad = Self::default();
        pad.fill(region, sy, &mut Box::new([0; 4096]));
        pad
    }
    fn fill(&mut self, region: &[[Option<&Chunk>; 3]; 3], sy: i32, own: &mut [u32; 4096]) {
        self.blocks.resize(P * P * P, 0);
        self.light.resize(P * P * P, 0);
        let (blocks, light) = (&mut self.blocks, &mut self.light);
        let base_y = region[1][1].map_or(MIN_Y, Chunk::min_y) + sy * 16;
        // The section itself is unpacked in one pass and copied row by row;
        // the border cells of the neighbouring sections are read in runs, from
        // sections found once per layer.
        let center = region[1][1];
        if let Some(c) = center {
            c.sections[sy as usize].unpack(own);
        } else {
            own.fill(0);
        }
        for y in -B..16 + B {
            let wy = base_y + y;
            // Where each chunk column's cells of this layer come from, found once.
            let sources: [[Source; 3]; 3] = std::array::from_fn(|rz| {
                std::array::from_fn(|rx| match region[rz][rx] {
                    None => Source::Missing,
                    Some(c) => match c.section_at(wy) {
                        Ok(section) => Source::Section(section),
                        Err(light) => Source::Outside(light),
                    },
                })
            });
            for z in -B..16 + B {
                let row = pi(-B, y, z);
                let rz = ((z + 16) >> 4) as usize;
                let (west, rest) = blocks[row..row + P].split_at_mut(B as usize);
                let (middle, east) = rest.split_at_mut(16);
                let (west_light, rest) = light[row..row + P].split_at_mut(B as usize);
                let (middle_light, east_light) = rest.split_at_mut(16);
                let own_row = (rz == 1 && (0..16).contains(&y))
                    .then(|| &own[(y << 8 | z << 4) as usize..][..16]);
                let at = |x: i32| crate::chunk::index(x, wy, z);
                let [w, m, e] = &sources[rz];
                let edge = (center, wy, z);
                fill_run(west, west_light, w, None, at(-B), -B, edge);
                fill_run(middle, middle_light, m, own_row, at(0), 0, edge);
                fill_run(east, east_light, e, None, at(16), 16, edge);
            }
        }
    }
}
/// Where the cells of one chunk column at one height come from (`Padded::fill`).
enum Source<'a> {
    /// A neighbour not loaded.
    Missing,
    /// Above or below the column's sections: air with this light.
    Outside(u8),
    Section(&'a crate::chunk::Section),
}
/// Fills one run of a padded row, `blocks.len()` cells from x = `x0`, starting
/// at section index `at`; `own` is the centre section's row, already unpacked.
#[inline(always)]
fn fill_run(
    blocks: &mut [u32],
    light: &mut [u8],
    source: &Source,
    own: Option<&[u32]>,
    at: usize,
    x0: i32,
    (center, wy, z): (Option<&Chunk>, i32, i32),
) {
    match source {
        Source::Section(section) => {
            match own {
                Some(own) => blocks.copy_from_slice(own),
                None => section.blocks.get_run(at, blocks),
            }
            section.light.copy_run(at, light);
        }
        Source::Outside(v) => {
            blocks.fill(0);
            light.fill(*v);
        }
        // A neighbour not loaded continues this chunk's edge column. As lit
        // air, the outermost chunks' water drew a wall toward it (fluids show
        // both sides), a light sheet in the water where far terrain carries
        // on. Vanilla draws no chunk without its neighbours.
        Source::Missing => match center {
            Some(c) => {
                let ez = z.clamp(0, 15);
                for (k, (block, light)) in blocks.iter_mut().zip(light.iter_mut()).enumerate() {
                    let ex = (x0 + k as i32).clamp(0, 15);
                    *block = c.get(ex, wy, ez);
                    *light = c.light(ex, wy, ez);
                }
            }
            None => {
                blocks.fill(0);
                light.fill(0xF0);
            }
        },
    }
}
// Direction order is the same as Direction::ALL; all values are evaluated at compile time.
const DIRECTIONS: [(Direction, [i32; 3], u8); 6] = [
    direction_info(Direction::Down),
    direction_info(Direction::Up),
    direction_info(Direction::North),
    direction_info(Direction::South),
    direction_info(Direction::West),
    direction_info(Direction::East),
];
const fn direction_info(d: Direction) -> (Direction, [i32; 3], u8) {
    (d, d.normal(), d.opposite() as u8)
}
/// `step_index` of each direction's normal, indexed like `DIRECTIONS`: the
/// padded index of a block's neighbour is its own plus one of these.
const OFFSETS: [isize; 6] = {
    let mut out = [0isize; 6];
    let mut i = 0;
    while i < 6 {
        out[i] = step_index(DIRECTIONS[i].1);
        i += 1;
    }
    out
};
/// Per-worker temporary arrays. They never hold resource-pack references or
/// geometry still owned by the renderer, and total about 150 KiB per worker
/// (the leaf mask adds 8,000 cells, depths and a queue).
#[derive(Default)]
pub struct MeshScratch {
    padded: Padded,
    own: Option<Box<[u32; 4096]>>,
    leaf: leaf_culling::LeafMask,
    /// Per padded row (`pi(-B, y, z) / P`), bit `x + B` set where the block
    /// is `isSolidRender` (`MeshTables::solid`).
    solid_rows: Vec<u32>,
}
/// Fills `rows` with the padded section's `isSolidRender` blocks, one bit each (`MeshScratch::solid_rows`).
fn solid_rows(pad: &Padded, solid: &[bool], rows: &mut Vec<u32>) {
    rows.clear();
    for row in pad.blocks.as_chunks::<P>().0 {
        let mut bits = 0;
        for (x, &id) in row.iter().enumerate() {
            bits |= u32::from(solid[id as usize]) << x;
        }
        rows.push(bits);
    }
}
/// Open and fully surrounded cells in each inner row.
fn row_masks(rows: &[u32]) -> ([u16; 256], [u16; 256]) {
    let mut open = [0; 256];
    let mut hidden = [0; 256];
    for y in 0..16 {
        for z in 0..16 {
            let i = (y + B as usize) * P + z + B as usize;
            let r = rows[i];
            let around =
                r & r << 1 & r >> 1 & rows[i - P] & rows[i + P] & rows[i - 1] & rows[i + 1];
            open[y * 16 + z] = !(r >> B) as u16;
            hidden[y * 16 + z] = (around >> B) as u16;
        }
    }
    (open, hidden)
}
#[cfg(test)]
#[test]
fn row_masks_match_cell_definition() {
    let mut seed = 42u32;
    for _ in 0..100 {
        let rows: Vec<u32> = (0..P * P)
            .map(|_| {
                seed ^= seed << 13;
                seed ^= seed >> 17;
                seed ^= seed << 5;
                seed & ((1 << P) - 1)
            })
            .collect();
        let (open, hidden) = row_masks(&rows);
        for y in 0..16 {
            for z in 0..16 {
                let cell = |x: usize, y: usize, z: usize| (rows[y * P + z] >> x) & 1 != 0;
                for x in 0..16 {
                    let (x, py, pz) = (x + B as usize, y + B as usize, z + B as usize);
                    let occupied = cell(x, py, pz);
                    let enclosed = occupied
                        && cell(x - 1, py, pz)
                        && cell(x + 1, py, pz)
                        && cell(x, py - 1, pz)
                        && cell(x, py + 1, pz)
                        && cell(x, py, pz - 1)
                        && cell(x, py, pz + 1);
                    assert_eq!((open[y * 16 + z] >> (x - B as usize)) & 1 != 0, !occupied);
                    assert_eq!((hidden[y * 16 + z] >> (x - B as usize)) & 1 != 0, enclosed);
                }
            }
        }
    }
}
#[cfg(test)]
mod leaf_culling_tests;
#[cfg(test)]
mod opaque_ice_tests;
#[cfg(test)]
mod reuse_tests;
#[cfg(test)]
mod tint_tests;
/// Set on a terrain vertex's sprite layer when its texture alpha must be ignored.
pub const OPAQUE_LAYER: u16 = 1 << 15;
/// The bits of `TerrainVertex::color`'s alpha byte that hold how brightly its face shines
/// in HDR output (`gfx/hdr.rs`): 15 minus the tier, plus 4 for one that shines all over
/// (`WHOLE`), so the usual 0xFF means none. The byte's high nibble carries the fluid face
/// flags (`fluid_face`); the shaders ignore the vertex alpha, so both travel to the
/// material stage untouched.
pub const EMISSION_BITS: u32 = 0x0F00_0000;
/// HDR tiers, by the kind of light rather than its level: a candle's flame is as bright as
/// a torch's, it only lights less around it. 0 stays as in SDR.
pub const LOW: u8 = 1;
pub const INCANDESCENT: u8 = 2;
pub const BRIGHT: u8 = 3;
/// The sun and lightning.
pub const VERY_BRIGHT: u8 = 4;
/// Added to a tier: the light source shines all over (a froglight), not only where its
/// sprite is brightest (a torch's flame, not its handle). See `gfx/hdr.rs` `Masks`.
pub const WHOLE: u8 = 8;
/// `color` marked as shining at `tier` (0 to `VERY_BRIGHT`, plus `WHOLE`).
pub fn emitting(color: u32, tier: u8) -> u32 {
    let level = (tier & !WHOLE).min(VERY_BRIGHT);
    let code = if level > 0 && tier & WHOLE != 0 {
        level + 4
    } else {
        level
    };
    color & !EMISSION_BITS | u32::from(15 - code) << 24
}
/// The high nibble of `TerrainVertex::color`'s alpha byte: 8 plus the face (`Direction`
/// as usize) on a fluid quad, 0xF on every other. `terrain.wgsl` reads only the low nibble
/// (the emission), so only the material stage of Horizon's own packs reads it
/// (`gfx/native_material.rs`); whether the fluid is water or lava follows from the pass
/// that draws it.
pub const FLUID_FACE_BITS: u32 = 0xF000_0000;
/// `color` marked as a fluid quad of face `face` (`Direction` as usize, 0 to 5).
pub fn fluid_face(color: u32, face: usize) -> u32 {
    debug_assert!(face < 6);
    color & !FLUID_FACE_BITS | 0x8000_0000 | (face as u32) << 28
}
/// The HDR tier of block `name` (no namespace), which emits light `level`.
pub fn hdr_tier(name: &str, level: u8) -> u8 {
    match name {
        _ if level == 0 => 0,
        "glowstone"
        | "soul_lantern"
        | "soul_torch"
        | "soul_wall_torch"
        | "beacon"
        | "end_rod"
        | "furnace"
        | "redstone_lamp"
        | "brewing_stand"
        | "enchanting_table"
        | "medium_amethyst_bud"
        | "small_amethyst_bud" => BRIGHT,
        "campfire" | "soul_campfire" | "soul_fire" | "lava_cauldron" | "crying_obsidian"
        | "amethyst_cluster" | "large_amethyst_bud" => BRIGHT | WHOLE,
        "torch" | "wall_torch" | "lantern" | "sea_lantern" => INCANDESCENT,
        "fire" | "lava" => INCANDESCENT | WHOLE,
        // Glowing at every charge, not only the brighter ones.
        "respawn_anchor" => INCANDESCENT,
        "ochre_froglight" | "verdant_froglight" | "pearlescent_froglight" | "jack_o_lantern" => {
            LOW | WHOLE
        }
        "redstone_torch" | "redstone_wall_torch" | "magma_block" | "shroomlight" => LOW,
        // Its flame is a particle; the cake's frosting must not shine in daylight.
        _ if name.ends_with("candle_cake") => 0,
        _ if name.ends_with("candle") => LOW,
        // One tier at every oxidation stage, waxed or not.
        _ if name.ends_with("copper_bulb") => LOW,
        // A faint glow in the dark, as in SDR.
        "glow_lichen" | "sea_pickle" | "brown_mushroom" | "dragon_egg" | "end_portal_frame"
        | "ender_chest" => 0,
        _ if level >= 10 => INCANDESCENT,
        _ => LOW,
    }
}
const UNSET: u32 = u32::MAX;
const NO_BIOME: u16 = u16::MAX;
/// The `ColorResolver`s `ClientLevel` caches block tints for.
#[derive(Clone, Copy)]
enum Resolver {
    Grass,
    Foliage,
    Dry,
    Water,
}
/// Biome tints of one block layer: the zoomed biome of every block of the
/// layer plus the blend radius around it, and the blended colours per column.
struct Layer {
    zoomed: Vec<u16>,
    /// Each resolver's colour of every cell of `zoomed`, filled on first use:
    /// a cell is in the blend square of up to `(2r+1)^2` columns.
    cells: [Vec<u32>; 4],
    /// `BiomeZoom::corner_jitter` of the quart cells `zoomed` reaches, by
    /// their offset from the first: each is shared by up to 16 cells of the layer.
    jitter: Vec<Option<[[f64; 3]; 8]>>,
    colors: [[u32; 256]; 4],
}
/// `ClientLevel.calculateBlockTint` behind a per-layer `BlockTintCache`: each
/// block averages the colours of the biomes `BiomeManager.getBiome` picks (the
/// eight-corner zoom) over a `(2r+1)^2` square at the block's own height.
/// Layers and columns are only computed when a tinted block asks for them.
struct Tints<'a> {
    region: &'a [[Option<&'a Chunk>; 3]; 3],
    biomes: &'a Biomes,
    zoom: BiomeZoom,
    radius: i32,
    plains: u16,
    /// Chunk x, chunk z and the section's lowest block y.
    origin: [i32; 3],
    layers: [Option<Box<Layer>>; 16],
    /// `uniform_biome`, once computed.
    uniform: Option<Option<u16>>,
    /// The packs' colormaps (`MeshTables::colormaps`), the water's, and their colours so far
    /// by colormap and section-local block.
    colormaps: &'a [Arc<crate::environment::Colormap>],
    water_colormap: Option<u16>,
    sampled: std::collections::HashMap<(u16, [i32; 3]), Option<u32>>,
    /// Each colormap's own colour by block, before the blend averages them.
    cells: std::collections::HashMap<(u16, [i32; 3]), Option<u32>>,
}
impl<'a> Tints<'a> {
    fn new(
        region: &'a [[Option<&'a Chunk>; 3]; 3],
        biomes: &'a Biomes,
        zoom: BiomeZoom,
        radius: i32,
        origin: [i32; 3],
    ) -> Self {
        Self {
            region,
            biomes,
            zoom,
            radius: radius.clamp(0, 7),
            plains: biomes.id("minecraft:plains"),
            origin,
            layers: Default::default(),
            uniform: None,
            colormaps: &[],
            water_colormap: None,
            sampled: std::collections::HashMap::new(),
            cells: std::collections::HashMap::new(),
        }
    }
    /// With the packs' colormaps of `tables`.
    fn with_colormaps(mut self, tables: &'a MeshTables) -> Self {
        self.colormaps = &tables.colormaps;
        self.water_colormap = tables.water_colormap;
        self
    }
    /// Colormap `index`'s colour at block `(x, y, z)` (section-local), `None` where its pixel
    /// is transparent.
    fn colormap(&mut self, index: u16, x: i32, y: i32, z: i32) -> Option<u32> {
        if let Some(&known) = self.sampled.get(&(index, [x, y, z])) {
            return known;
        }
        let colormap = self.colormaps.get(usize::from(index))?.clone();
        // ClientLevel.calculateBlockTint: the mean of the (2r+1)^2 colours around the block, each
        // channel summed and divided. A transparent pixel in the square leaves the block's own.
        let r = if colormap.biome_blend { self.radius } else { 0 };
        let mut sum = [0u32; 3];
        let mut blended = true;
        'square: for dz in -r..=r {
            for dx in -r..=r {
                let Some(c) = self.colormap_cell(&colormap, index, [x + dx, y, z + dz]) else {
                    blended = false;
                    break 'square;
                };
                sum = [
                    sum[0] + (c >> 16 & 255),
                    sum[1] + (c >> 8 & 255),
                    sum[2] + (c & 255),
                ];
            }
        }
        let colour = if blended {
            let n = ((2 * r + 1) * (2 * r + 1)) as u32;
            Some((sum[0] / n) << 16 | (sum[1] / n) << 8 | (sum[2] / n))
        } else {
            self.colormap_cell(&colormap, index, [x, y, z])
        };
        self.sampled.insert((index, [x, y, z]), colour);
        colour
    }
    /// Colormap `index`'s own colour at block `at` (section-local, x and z may reach into the
    /// neighbours), before any blend.
    fn colormap_cell(
        &mut self,
        colormap: &crate::environment::Colormap,
        index: u16,
        at: [i32; 3],
    ) -> Option<u32> {
        if let Some(&known) = self.cells.get(&(index, at)) {
            return known;
        }
        let id = Self::biome_id(&self.zoom, self.region, self.plains, self.origin, at);
        let [cx, cz, base_y] = self.origin;
        let block = [cx * 16 + at[0], base_y + at[1], cz * 16 + at[2]];
        let mut host = crate::environment::BlockHost::new(block, self.biomes.get(id));
        let colour = colormap.sample(&mut host).map(|c| c & 0xFF_FFFF);
        self.cells.insert((index, at), colour);
        colour
    }
    /// The water's tint at block `(x, y, z)`: the packs' water colormap, else the biome's.
    fn water(&mut self, x: i32, y: i32, z: i32) -> u32 {
        if let Some(index) = self.water_colormap
            && let Some(colour) = self.colormap(index, x, y, z)
        {
            return colour;
        }
        self.get(Resolver::Water, x, y, z)
    }
    /// The biome of block `(x, y, z)` (section-local x/z, y in 0..16), as `BiomeManager.getBiome`
    /// picks it.
    fn biome(&self, x: i32, y: i32, z: i32) -> &'a str {
        let id = Self::biome_id(&self.zoom, self.region, self.plains, self.origin, [x, y, z]);
        &self.biomes.get(id).name
    }
    /// `BiomeManager.getBiome` over the loaded chunks (plains where there is none), for block
    /// `(x, y, z)` of the section at `origin`. Takes the fields it reads, so `get` can call it
    /// while it holds a layer.
    fn biome_id(
        zoom: &BiomeZoom,
        region: &[[Option<&Chunk>; 3]; 3],
        plains: u16,
        [cx, cz, base_y]: [i32; 3],
        [x, y, z]: [i32; 3],
    ) -> u16 {
        let q = zoom.quart_at_block(BlockPos::new(cx * 16 + x, base_y + y, cz * 16 + z));
        Self::quart_biome(region, plains, [cx, cz], q)
    }
    /// The biome of quart cell `q` in the loaded chunks around chunk `(cx, cz)`, plains where
    /// there is none.
    fn quart_biome(
        region: &[[Option<&Chunk>; 3]; 3],
        plains: u16,
        [cx, cz]: [i32; 2],
        q: [i32; 3],
    ) -> u16 {
        let (rx, rz) = ((q[0] >> 2) - cx + 1, (q[2] >> 2) - cz + 1);
        if (0..3).contains(&rx) && (0..3).contains(&rz) {
            region[rz as usize][rx as usize].map_or(plains, |c| {
                c.biome((q[0] & 3) * 4, q[1] * 4, (q[2] & 3) * 4)
            })
        } else {
            plains
        }
    }
    /// The biome every block of the section's blend squares zooms to, when they
    /// all do: `quart_at_block` picks one of the eight quart cells at its base
    /// cell plus 0 or 1 on each axis, so if every such cell the section reaches
    /// holds one biome, every zoomed cell is that biome.
    fn uniform_biome(&mut self) -> Option<u16> {
        if let Some(uniform) = self.uniform {
            return uniform;
        }
        let (r, [cx, cz, base_y]) = (self.radius, self.origin);
        // The base cells of blocks lo..=hi, and the cell past the last.
        let span = |lo: i32, hi: i32| (lo.wrapping_sub(2) >> 2)..=(hi.wrapping_sub(2) >> 2) + 1;
        let (xs, ys, zs) = (
            span(cx * 16 - r, cx * 16 + 15 + r),
            span(base_y, base_y + 15),
            span(cz * 16 - r, cz * 16 + 15 + r),
        );
        let first = Self::quart_biome(
            self.region,
            self.plains,
            [cx, cz],
            [*xs.start(), *ys.start(), *zs.start()],
        );
        let uniform = ys
            .flat_map(|y| zs.clone().map(move |z| (y, z)))
            .flat_map(|(y, z)| xs.clone().map(move |x| [x, y, z]))
            .all(|q| Self::quart_biome(self.region, self.plains, [cx, cz], q) == first)
            .then_some(first);
        self.uniform = Some(uniform);
        uniform
    }
    /// The blended tint of block `(x, y, z)` (section-local x/z, y in 0..16).
    fn get(&mut self, resolver: Resolver, x: i32, y: i32, z: i32) -> u32 {
        // One biome over every blend square: the mean of (2r+1)^2 equal
        // colours is that colour, unless it varies by position (swamp grass).
        if let Some(biome) = self.uniform_biome() {
            let info = self.biomes.get(biome);
            match resolver {
                Resolver::Grass if !matches!(info.modifier, GrassModifier::Swamp) => {
                    return self.biomes.grass(biome, 0.0, 0.0) & 0xFF_FFFF;
                }
                Resolver::Grass => {}
                Resolver::Foliage => return info.foliage & 0xFF_FFFF,
                Resolver::Dry => return info.dry_foliage & 0xFF_FFFF,
                Resolver::Water => return info.water & 0xFF_FFFF,
            }
        }
        let r = self.radius;
        let dim = (16 + 2 * r) as usize;
        let layer = self.layers[y as usize].get_or_insert_with(|| {
            Box::new(Layer {
                zoomed: vec![NO_BIOME; dim * dim],
                cells: Default::default(),
                jitter: Vec::new(),
                colors: [[UNSET; 256]; 4],
            })
        });
        let slot = (z * 16 + x) as usize;
        let cached = layer.colors[resolver as usize][slot];
        if cached != UNSET {
            return cached;
        }
        let [cx, cz, base_y] = self.origin;
        let (bx, by, bz) = (cx * 16, base_y + y, cz * 16);
        let (mut red, mut green, mut blue) = (0, 0, 0);
        let cells = &mut layer.cells[resolver as usize];
        if cells.is_empty() {
            cells.resize(dim * dim, UNSET);
        }
        for dz in -r..=r {
            for dx in -r..=r {
                let cell = ((z + dz + r) as usize) * dim + (x + dx + r) as usize;
                let known = cells[cell];
                if known != UNSET {
                    red += known >> 16 & 255;
                    green += known >> 8 & 255;
                    blue += known & 255;
                    continue;
                }
                let mut biome = layer.zoomed[cell];
                if biome == NO_BIOME {
                    // BiomeManager.getBiome over the loaded chunks (plains
                    // where there is none): `quart_at_block`, its cell's
                    // corner jitter mixed once per layer.
                    let block = BlockPos::new(bx + x + dx, by, bz + z + dz);
                    let base = [block.x, block.y, block.z].map(|v| v.wrapping_sub(2) >> 2);
                    let first = [bx - r, bz - r].map(|v| v.wrapping_sub(2) >> 2);
                    let span = dim / 4 + 2;
                    let at = (base[2] - first[1]) as usize * span + (base[0] - first[0]) as usize;
                    if layer.jitter.is_empty() {
                        layer.jitter.resize(span * span, None);
                    }
                    let jitter =
                        layer.jitter[at].get_or_insert_with(|| self.zoom.corner_jitter(base));
                    let corner = BiomeZoom::select_corner(block, jitter);
                    let q = [
                        base[0].wrapping_add((corner >> 2 & 1) as i32),
                        base[1].wrapping_add((corner >> 1 & 1) as i32),
                        base[2].wrapping_add((corner & 1) as i32),
                    ];
                    biome = Self::quart_biome(self.region, self.plains, [cx, cz], q);
                    layer.zoomed[cell] = biome;
                }
                let c = match resolver {
                    Resolver::Grass => {
                        self.biomes
                            .grass(biome, f64::from(bx + x + dx), f64::from(bz + z + dz))
                    }
                    Resolver::Foliage => self.biomes.get(biome).foliage,
                    Resolver::Dry => self.biomes.get(biome).dry_foliage,
                    Resolver::Water => self.biomes.get(biome).water,
                };
                cells[cell] = c;
                red += c >> 16 & 255;
                green += c >> 8 & 255;
                blue += c & 255;
            }
        }
        let n = ((2 * r + 1) * (2 * r + 1)) as u32;
        let color = (red / n) << 16 | (green / n) << 8 | (blue / n);
        layer.colors[resolver as usize][slot] = color;
        color
    }
}
/// Packs 8-bit RGB into the vertex's RGBA8 (red in the low byte), opaque.
fn pack_rgb([r, g, b]: [u32; 3]) -> u32 {
    0xFF00_0000 | b << 16 | g << 8 | r
}
/// Vanilla light coords (block*16 in the low half, sky*16 in the high half) to
/// the vertex layout (sky*16 in bits 0..8, block*16 in bits 8..16).
fn pack_coords(coords: u32) -> u16 {
    ((coords >> 16 & 0xFF) | (coords & 0xFF) << 8) as u16
}
pub struct Mesher<'a> {
    /// CardinalLighting.NETHER: brighter up/down faces.
    pub nether_shading: bool,
    /// A shader pack lights the faces (its `oldLighting` off): no face shading in the colours,
    /// only the dimension's up-face brightness (`lighting::pack_flat`).
    pub shader_pack_lighting: bool,
    /// OptiFine/Iris separate ambient occlusion: vertex RGB does not include AO, and AO factor is passed in vertex alpha.
    pub separate_ao: bool,
    /// A `shaders/` pack draws the terrain (with or without its `oldLighting`): fluid tops are
    /// split into triangles as Iris splits them (`fluid::splits_north_east`).
    pub shader_pack: bool,
    pub assets: &'a Assets,
    pub render: &'a RenderAssets,
    pub tables: &'a MeshTables,
    pub biomes: &'a Biomes,
    pub visibility: FaceVisibility,
    /// The client's biome zoom (the hashed world seed `BiomeManager` holds).
    pub zoom: BiomeZoom,
    /// The `biomeBlendRadius` option, 0 to 7.
    pub blend_radius: i32,
    /// The `ao` option: smooth lighting for models that allow it.
    pub smooth_lighting: bool,
    /// The `cutoutLeaves` option (fancy graphics); off makes leaves opaque.
    pub cutout_leaves: bool,
    /// The `opaqueIce` option: regular ice draws opaque in the solid pass.
    pub opaque_ice: bool,
    /// Sprite layers a shader pack moves into the translucent pass, by layer.
    pub translucent_layers: &'a [bool],
    /// What the worker does with leaf faces inside a canopy.
    pub leaf_culling: LeafCullingMode,
}
impl Mesher<'_> {
    /// `region[dz][dx]` are the chunks around the meshed column (centre 1,1).
    pub fn mesh(&mut self, region: &[[Option<&Chunk>; 3]; 3], sy: i32) -> SectionMesh {
        self.mesh_reusing(
            region,
            sy,
            SectionMesh::default(),
            &mut MeshScratch::default(),
        )
    }
    /// The meshing workers' path: `out`'s buffers and `scratch` are reused
    /// rather than allocated per section.
    pub fn mesh_reusing(
        &mut self,
        region: &[[Option<&Chunk>; 3]; 3],
        sy: i32,
        mut out: SectionMesh,
        scratch: &mut MeshScratch,
    ) -> SectionMesh {
        out.clear();
        let center = region[1][1].expect("centre chunk");
        let section = &center.sections[sy as usize];
        if section.is_uniform(0) {
            return out;
        }
        let own = scratch.own.get_or_insert_with(|| Box::new([0; 4096]));
        scratch.padded.fill(region, sy, own);
        let pad = &scratch.padded;
        solid_rows(pad, &self.tables.solid, &mut scratch.solid_rows);
        let opaque = &scratch.solid_rows;
        let (open, hidden) = row_masks(opaque);
        out.visibility = visibility_rows(&open);
        let base_y = center.min_y() + sy * 16;
        if self.leaf_culling.is_interior() {
            scratch
                .leaf
                .prepare(self.leaf_culling, pad, &self.tables.info, region, base_y);
        }
        let mut tints = Tints::new(
            region,
            self.biomes,
            self.zoom,
            self.blend_radius,
            [center.x, center.z, base_y],
        )
        .with_colormaps(self.tables);
        let info = &self.tables.info;
        let full_faces = &self.tables.full_faces;
        let catalog = &self.render.model_catalog;
        let mut models = Vec::new();
        for y in 0..16 {
            for z in 0..16 {
                // The padded index of the row's x = 0 cell: `pi(x, y, z)` is
                // `row + x` and a neighbour adds `OFFSETS[d]`.
                let row = pi(0, y, z);
                for x in 0..16 {
                    let cell = row + x as usize;
                    let id = pad.blocks[cell];
                    if id == 0 {
                        continue;
                    }
                    let state = &info[id as usize];
                    if state.block_entity {
                        out.block_entities
                            .push(([center.x * 16 + x, base_y + y, center.z * 16 + z], id));
                    }
                    if state.emission > 0 {
                        out.lights
                            .push(([center.x * 16 + x, base_y + y, center.z * 16 + z], id));
                    }
                    if hidden[(y * 16 + z) as usize] >> x & 1 != 0 {
                        continue;
                    }
                    if state.fluid.kind() != FluidKind::Empty {
                        let start = [out.solid.len(), out.translucent.len()];
                        let water = tints.water(x, y, z);
                        let shade = (
                            self.nether_shading,
                            self.shader_pack_lighting,
                            self.separate_ao,
                            self.shader_pack,
                        );
                        fluid::tesselate(self.tables, shade, pad, water, &mut out, [x, y, z]);
                        if self.shader_pack {
                            out.record_source(
                                start,
                                TerrainSource::new(id, [x, y, z], state.emission, true),
                            );
                        }
                    }
                    if !state.model {
                        continue;
                    }
                    let world = BlockPos::new(center.x * 16 + x, base_y + y, center.z * 16 + z);
                    if catalog
                        .models_at(BlockStateId(id), world, &mut models)
                        .is_err()
                    {
                        continue;
                    }
                    // ModelBlockRenderer: the first part decides smooth lighting
                    // for all; light-emitting blocks are always lit flat.
                    let Some(first) = models.first() else {
                        continue;
                    };
                    let ao = self.smooth_lighting && state.emission == 0 && first.ambient_occlusion;
                    let origin = match offset(state, world.x, world.z) {
                        Some(o) => [
                            x as f32 + o[0] as f32,
                            y as f32 + o[1] as f32,
                            z as f32 + o[2] as f32,
                        ],
                        None => [x as f32, y as f32, z as f32],
                    };
                    let mut visible = [false; 6];
                    let interior_leaf = state.leaves
                        && self.leaf_culling.is_interior()
                        && scratch.leaf.interior(self.leaf_culling, x, y, z);
                    for (d, [dx, dy, dz], opposite) in DIRECTIONS {
                        let neighbor = pad.blocks[(cell as isize + OFFSETS[d as usize]) as usize];
                        // `should_render_face`'s first test, without its lookups: a
                        // full occlusion face toward this block hides the face.
                        if full_faces[neighbor as usize] >> opposite & 1 != 0 {
                            continue;
                        }
                        let other = &info[neighbor as usize];
                        // LeavesBlock.skipRendering with opaque leaves.
                        let opaque_leaves = !self.cutout_leaves && state.leaves && other.leaves;
                        // Opaque Ice: faces between two ice blocks are hidden, as they are
                        // between two opaque blocks.
                        let opaque_ice = self.opaque_ice && state.opaque_ice && other.opaque_ice;
                        let culled_leaf = match self.leaf_culling {
                            LeafCullingMode::Hollow | LeafCullingMode::Hidden => {
                                state.leaves
                                    && other.leaves
                                    && leaf_culling::in_section(x + dx, y + dy, z + dz)
                                    && (self.leaf_culling == LeafCullingMode::Hollow
                                        || leaf_culling::backed(
                                            pad,
                                            info,
                                            region,
                                            base_y,
                                            [x + 2 * dx, y + 2 * dy, z + 2 * dz],
                                        ))
                            }
                            _ => false,
                        };
                        visible[d as usize] = !opaque_leaves
                            && !opaque_ice
                            && !culled_leaf
                            && self
                                .visibility
                                .should_render_face(
                                    BlockStateId(id),
                                    BlockStateId(neighbor),
                                    d,
                                    true,
                                )
                                .unwrap_or(true);
                    }
                    let connected = &self.render.connected;
                    let start = [out.solid.len(), out.translucent.len()];
                    for model in &models {
                        for face in &model.faces {
                            if face.quad.cull.is_some_and(|d| !visible[d as usize]) {
                                continue;
                            }
                            // A pane's top and bottom strips between stacked panes, with
                            // connected textures.
                            if state.pane
                                && !connected.is_empty()
                                && let Some(dy) = match face.quad.facing {
                                    ModelDirection::Up => Some(1),
                                    ModelDirection::Down => Some(-1),
                                    _ => None,
                                }
                            {
                                let p = &face.quad.positions;
                                // Only on the block's top or bottom plane: a pack's own pane
                                // model may have ledges between.
                                let plane = if dy > 0 { 1.0 } else { 0.0 };
                                let on_edge = p.iter().all(|v| (v[1] - plane).abs() < 1e-4);
                                let centre =
                                    [0, 2].map(|k| p.iter().map(|v| v[k]).sum::<f32>() / 4.0);
                                let other =
                                    pad.blocks[(cell as isize + step_index([0, dy, 0])) as usize];
                                if on_edge && connected.stacked_pane_hides(id, other, centre) {
                                    continue;
                                }
                            }
                            self.face(
                                pad,
                                &mut tints,
                                &mut out,
                                [x, y, z],
                                cell,
                                origin,
                                face,
                                ao,
                                (id, state),
                                world,
                                interior_leaf,
                            );
                        }
                    }
                    if self.shader_pack {
                        out.record_source(
                            start,
                            TerrainSource::new(id, [x, y, z], state.emission, false),
                        );
                    }
                }
            }
        }
        out.finish_sprite_layers();
        out
    }
    /// `BlockColors` tint source `index`, or `None` for an untinted (-1) layer.
    fn tint(state: &Info, index: i32, tints: &mut Tints, [x, y, z]: [i32; 3]) -> Option<u32> {
        if index < 0 || index >= i32::from(state.tint_count) {
            return None;
        }
        if let Some(colormap) = state.colormap
            && let Some(colour) = tints.colormap(colormap, x, y, z)
        {
            return Some(colour);
        }
        Some(match state.tints[(index as usize).min(1)] {
            Tint::None => return None,
            Tint::Grass | Tint::GrassBlock | Tint::SugarCane => tints.get(Resolver::Grass, x, y, z),
            Tint::TallGrassUpper => tints.get(Resolver::Grass, x, y, z),
            Tint::Foliage => tints.get(Resolver::Foliage, x, y, z),
            Tint::DryFoliage => tints.get(Resolver::Dry, x, y, z),
            Tint::Water => tints.water(x, y, z),
            Tint::Constant(c) => c,
            Tint::LilyPad => 0x20_8030,
            Tint::Redstone(p) => redstone_color(p),
            Tint::Stem(age) => stem_color(age),
        })
    }
    #[allow(clippy::too_many_arguments)]
    fn face(
        &self,
        pad: &Padded,
        tints: &mut Tints,
        out: &mut SectionMesh,
        pos: [i32; 3],
        cell: usize,
        origin: [f32; 3],
        face: &BakedFace,
        ao: bool,
        (id, state): (u32, &Info),
        world: BlockPos,
        interior_leaf: bool,
    ) {
        let q = &face.quad;
        // A quad's own light (a model's glowing part) may raise the tier; whether it
        // shines all over stays the block's.
        let tier = (state.tier & !WHOLE).max(hdr_tier("", q.light[0])) | state.tier & WHOLE;
        // A quad's `neoforge_data` ambient occlusion replaces the model's; glowing blocks stay flat.
        let ao = match q.ambient_occlusion {
            Some(on) => on && self.smooth_lighting && state.emission == 0,
            None => ao,
        };
        let shade_face = q.shade_direction_override.unwrap_or(q.facing);
        let directional = if !q.shade {
            1.0
        } else if self.shader_pack_lighting {
            lighting::pack_flat(self.nether_shading)
        } else {
            lighting::cardinal(self.nether_shading, shade_face)
        };
        // The quad's shape (bounds, partial, cubic), baked per face for full and
        // non-full blocks: a pure function of the quad and the block's `full`.
        let shape = &face.shapes[usize::from(state.full)];
        // Grey level per vertex before tinting, vanilla light coords, and ambient occlusion factor.
        let (gray, coords, ao_factor) = if ao {
            let info = &self.tables.info;
            let (brightness, coords) =
                lighting::ambient_occlusion(pad, info, state, pos, shape, q.facing);
            let gray = if self.separate_ao {
                [lighting::gray(directional); 4]
            } else {
                brightness.map(|b| lighting::scale(lighting::gray(b), directional))
            };
            (gray, coords, brightness)
        } else {
            let coords = match q.cull {
                Some(d) => lighting::coords(pad, state, lighting::step(pos, d)),
                None => lighting::flat_shaped(pad, state, pos, q.facing, shape),
            };
            ([lighting::gray(directional); 4], [coords; 4], [1.0f32; 4])
        };
        let tint = Self::tint(state, q.tint_index, tints, pos);
        let tint = match q.color {
            Some(c) => {
                let t = tint.unwrap_or(0xFF_FFFF);
                let channel = |s: u32| (t >> s & 255) * (c as u32 >> s & 255) / 255;
                Some(channel(16) << 16 | channel(8) << 8 | channel(0))
            }
            None => tint,
        };
        // Fusion's and then OptiFine's connected textures: another sprite, or one per quarter,
        // and the overlays drawn over it (a few per face at most; more are dropped).
        let (fusion, connected) = (&self.render.fusion, &self.render.connected);
        let mut overlays = [None; 8];
        let mut count = 0;
        let mut push = |overlay| {
            if let Some(slot) = overlays.get_mut(count) {
                *slot = Some(overlay);
                count += 1;
            }
        };
        let neighbour = |n: [i32; 3]| pad.blocks[(cell as isize + step_index(n)) as usize];
        let fused = face.fusion.and_then(|rule| {
            fusion.pick(
                rule,
                id,
                CtmFace::ALL[q.facing as usize],
                q.tint_index,
                neighbour,
                |s| {
                    self.tables
                        .info
                        .get(s as usize)
                        .is_some_and(|i| i.opaque_cube)
                },
                &mut push,
            )
        });
        let pick = if let Some(pick) = fused {
            pick
        } else if connected.is_empty() {
            Pick::Keep
        } else {
            let tints = &*tints;
            connected.pick(
                id,
                face.layer as u16,
                CtmFace::ALL[q.facing as usize],
                [world.x, world.y, world.z],
                neighbour,
                || Some(tints.biome(pos[0], pos[1], pos[2])),
                // A pane's tiles are drawn with the cube's UVs (below), so that is its texture.
                || {
                    if state.pane {
                        Some(CtmAxes::cube(CtmFace::ALL[q.facing as usize]))
                    } else {
                        CtmAxes::texture(&q.positions, &q.uv)
                    }
                },
                &mut push,
            )
        };
        let vertex = |tint: Option<u32>, i: usize, uv: [u16; 2]| {
            let g = gray[i] as u32;
            let rgb = match tint {
                Some(t) => [16, 8, 0].map(|shift| g * (t >> shift & 255) / 255),
                None => [g; 3],
            };
            let p = q.positions[i];
            let color = if self.separate_ao {
                let ao_byte = (ao_factor[i] * 255.0).round().clamp(0.0, 255.0) as u32;
                ao_byte << 24 | (pack_rgb(rgb) & 0x00FF_FFFF)
            } else {
                emitting(pack_rgb(rgb), tier)
            };
            TerrainVertex::packed_uv(
                [p[0] + origin[0], p[1] + origin[1], p[2] + origin[2]],
                uv,
                0,
                color,
                pack_coords(lighting::with_min(coords[i], q.light)),
            )
        };
        let vertices = |tint: Option<u32>, uv: [[f32; 2]; 4]| {
            let fixed = uv.map(|uv| uv.map(|v| round_to_u16(v * UV_STEPS)));
            std::array::from_fn::<_, 4, _>(|i| (vertex(tint, i, fixed[i]), uv[i]))
        };
        // Vanilla's pane models mirror some faces' textures; OptiFine's don't, so a tile's borders
        // land where the neighbours are: on a pane, tiles take the cube's own UVs.
        let standard = || q.positions.map(|p| standard_uv(q.facing, p));
        let tiled = !matches!(pick, Pick::Keep);
        let corners = if state.pane && tiled {
            vertices(tint, standard())
        } else {
            // The face's own UVs, already packed: no per-vertex rounding.
            std::array::from_fn(|i| (vertex(tint, i, face.uv_fixed[i]), q.uv[i]))
        };
        // Opaque leaves (forceOpaque), leaves a leaf-culling mode proved internal and Opaque
        // Ice draw in the solid layer whatever their sprite.
        let force_solid = !self.cutout_leaves && state.leaves
            || interior_leaf
            || self.opaque_ice && state.opaque_ice;
        let own = TileLayer::Layer {
            layer: face.layer as u16,
            translucent: face.translucent,
            opaque: face.opaque,
        };
        // A tile keeps the quad's forced translucency; `<skip>` and `<default>` are its own.
        let tile = |t: TileLayer| match t {
            TileLayer::Layer {
                layer,
                translucent,
                opaque,
            } => TileLayer::Layer {
                layer,
                translucent: translucent || q.render_type.is_none() && q.material.force_translucent,
                opaque,
            },
            TileLayer::Skip | TileLayer::Default => own,
        };
        match pick {
            Pick::Keep => self.quad(out, force_solid, own, corners.map(|c| c.0), tint),
            Pick::Tile(t) => self.quad(out, force_solid, tile(t), corners.map(|c| c.0), tint),
            Pick::Quarters(tiles) => {
                let cuts = cuts(&corners);
                for s in cuts.windows(2) {
                    for t in cuts.windows(2) {
                        let piece = [(s[0], t[0]), (s[1], t[0]), (s[1], t[1]), (s[0], t[1])]
                            .map(|(s, t)| bilinear(&corners, s, t));
                        let quarter = quarter_of(&piece);
                        let piece = piece.map(|c| c.0);
                        self.quad(out, force_solid, tile(tiles[quarter]), piece, tint);
                    }
                }
            }
            Pick::Nothing => {}
        }
        // Overlays: the face again, after it (equal depth passes), tinted as their rule says.
        // `layer=cutout` is an alpha cutout whatever the tile's alpha, but over a translucent
        // face it must be in the translucent pass too, or the face would draw over it.
        // Most faces have none: nothing below runs for them.
        if count == 0 {
            return;
        }
        let face_translucent = match pick {
            Pick::Keep => self.translucent(own),
            Pick::Tile(t) => self.translucent(tile(t)),
            Pick::Quarters(tiles) => tiles.into_iter().any(|t| self.translucent(tile(t))),
            Pick::Nothing => false,
        };
        // Overlay tiles face the standard way too (Continuity: "overlay textures always have the
        // standard orientation"), whatever turns the model's own UVs (dirt's and sand's random
        // rotations).
        // Over a `ctm_compact` face, the same pieces: the same depths and sort centres.
        let cuts = match pick {
            Pick::Quarters(_) => cuts(&corners),
            _ => Cuts([0.0, 1.0, 1.0, 1.0], 2),
        };
        for overlay in overlays[..count].iter().flatten().copied() {
            let tint = overlay.tint.and_then(|(index, block)| {
                let info = block
                    .and_then(|b| self.tables.info.get(b as usize))
                    .unwrap_or(state);
                Self::tint(info, index, tints, pos)
            });
            let tile = TileLayer::Layer {
                layer: overlay.layer,
                translucent: overlay.translucent || face_translucent,
                opaque: false,
            };
            let whole = vertices(tint, standard());
            for s in cuts.windows(2) {
                for t in cuts.windows(2) {
                    let piece = [(s[0], t[0]), (s[1], t[0]), (s[1], t[1]), (s[0], t[1])]
                        .map(|(s, t)| bilinear(&whole, s, t).0);
                    self.quad_as(out, false, tile, piece, false, tint);
                }
            }
        }
    }
    /// One terrain quad with sprite `tile`, in the pass its alpha needs (`force_solid`: no
    /// alpha cutout whatever the sprite).
    fn quad(
        &self,
        out: &mut SectionMesh,
        force_solid: bool,
        tile: TileLayer,
        v: [TerrainVertex; 4],
        tint: Option<u32>,
    ) {
        self.quad_as(out, force_solid, tile, v, true, tint);
    }
    /// Whether [`Mesher::quad`] draws `tile` in the translucent pass.
    fn translucent(&self, tile: TileLayer) -> bool {
        match tile {
            TileLayer::Layer {
                layer, translucent, ..
            } => {
                translucent
                    || self
                        .translucent_layers
                        .get(usize::from(layer))
                        .copied()
                        .unwrap_or(false)
            }
            TileLayer::Skip | TileLayer::Default => false,
        }
    }
    /// [`Mesher::quad`], with the sprite's own translucency (`translucent_layers`) counted only
    /// when `by_sprite`: an overlay's pass is its rule's. `tint`: what its grey levels were
    /// multiplied by, for the compact layout (`packed`).
    fn quad_as(
        &self,
        out: &mut SectionMesh,
        force_solid: bool,
        tile: TileLayer,
        mut v: [TerrainVertex; 4],
        by_sprite: bool,
        tint: Option<u32>,
    ) {
        let TileLayer::Layer {
            layer,
            translucent,
            opaque,
        } = tile
        else {
            return;
        };
        if self.tables.is_animated(layer) {
            out.note_sprite_layer(layer);
        }
        let translucent = if force_solid {
            false
        } else if by_sprite {
            self.translucent(tile)
        } else {
            translucent
        };
        // Quads over opaque texels draw in the solid layer: no alpha cutout (a dirt path's side
        // skips its clear top row). All four vertices share the layer.
        let layer = if opaque && !translucent || force_solid {
            layer | OPAQUE_LAYER
        } else {
            layer
        };
        let target = if translucent {
            &mut out.translucent
        } else {
            &mut out.solid
        };
        for vertex in &mut v {
            vertex.layer = layer;
        }
        target.extend(v);
        if !translucent {
            // Quads other code put in `solid` (lava) have no tint known.
            let quad = out.solid.len() / 4 - 1;
            out.solid_tints.resize(quad, packed::UNKNOWN_TINT);
            out.solid_tints.push(tint.unwrap_or(packed::WHITE));
        }
    }
}
/// The cuts of a `ctm_compact` face, in quad parameters, without allocating: the first `.1`
/// of `.0`.
struct Cuts([f32; 4], usize);
impl std::ops::Deref for Cuts {
    type Target = [f32];
    fn deref(&self) -> &[f32] {
        &self.0[..self.1]
    }
}
/// Where a quad's edge from corner 0 to corner `to` (1 for `s`, 3 for `t`) crosses the sprite's
/// middle line, the line its quarters meet on; `None` for an edge that stays on one side (a
/// slab's side).
fn crossing(c: &[(TerrainVertex, [f32; 2]); 4], to: usize) -> Option<f32> {
    let (a, b) = (c[0].1, c[to].1);
    // The UV the edge runs along.
    let k = usize::from((b[1] - a[1]).abs() > (b[0] - a[0]).abs());
    let at = (0.5 - a[k]) / (b[k] - a[k]);
    (at > 0.0 && at < 1.0).then_some(at)
}
/// The cuts of a `ctm_compact` face, the same along `s` and `t`: both edges' crossings. Pieces
/// on the quad's 0-2 diagonal are then square, so their own triangles lie on the quad's and a
/// split face shades exactly like the whole one; the others lie on one side of it.
fn cuts(c: &[(TerrainVertex, [f32; 2]); 4]) -> Cuts {
    let mut out = Cuts([0.0; 4], 1);
    let (mut s, mut t) = (crossing(c, 1), crossing(c, 3));
    if s > t {
        std::mem::swap(&mut s, &mut t);
    }
    for at in [s, t].into_iter().flatten().chain([1.0]) {
        if out.0[out.1 - 1] < at {
            out.0[out.1] = at;
            out.1 += 1;
        }
    }
    out
}
/// The UV a vanilla cube's face gives the block-local point `p` (`BlockElement`'s default UVs),
/// in sprite units: up is north on the top face, south on the bottom one, and up on the sides.
fn standard_uv(facing: horizon_assets::model::Direction, [x, y, z]: [f32; 3]) -> [f32; 2] {
    use horizon_assets::model::Direction as D;
    match facing {
        D::Down => [x, 1.0 - z],
        D::Up => [x, z],
        D::North => [1.0 - x, 1.0 - y],
        D::South => [x, 1.0 - y],
        D::West => [z, 1.0 - y],
        D::East => [1.0 - z, 1.0 - y],
    }
}
/// The quarter of the sprite (top-left, top-right, bottom-left, bottom-right) a piece of a
/// quad lies in, by its UV centre (v grows downward).
fn quarter_of(piece: &[(TerrainVertex, [f32; 2]); 4]) -> usize {
    let [u, v] = [0, 1].map(|k| piece.iter().map(|c| c.1[k]).sum::<f32>() / 4.0);
    usize::from(v >= 0.5) * 2 + usize::from(u >= 0.5)
}
/// The vertex at `(s, t)` across a quad's corners (`s` from corner 0 to 1, `t` from 0 to 3),
/// with its UV in sprite units: a quarter of a `ctm_compact` face. Blended within the quad's
/// two triangles (0-1-2 and 0-2-3), as the GPU blends the whole quad, so a split face shades
/// like an unsplit one.
fn bilinear(c: &[(TerrainVertex, [f32; 2]); 4], s: f32, t: f32) -> (TerrainVertex, [f32; 2]) {
    let w = if s >= t {
        [1.0 - s, s - t, t, 0.0]
    } else {
        [1.0 - t, 0.0, s, t - s]
    };
    let mix = |f: &dyn Fn(&(TerrainVertex, [f32; 2])) -> f32| {
        (0..4).map(|i| w[i] * f(&c[i])).sum::<f32>()
    };
    // Packed bytes (colour, light) blend one by one.
    let bytes = |field: fn(&TerrainVertex) -> u32, count: u32| {
        (0..count)
            .map(|k| (mix(&|c| (field(&c.0) >> (8 * k) & 255) as f32).round() as u32) << (8 * k))
            .sum::<u32>()
    };
    let color = bytes(|v| v.color, 4);
    let light = bytes(|v| u32::from(v.light), 2) as u16;
    let pos = [0, 1, 2].map(|k| mix(&|c| c.0.pos[k]));
    let uv = [0, 1].map(|k| mix(&|c| c.1[k]));
    (TerrainVertex::new(pos, uv, 0, color, light), uv)
}
/// Snapshot of the 3x3 chunk neighbourhood used by a worker job.
pub type Region = [[Option<Arc<Chunk>>; 3]; 3];
pub fn borrow(region: &Region) -> [[Option<&Chunk>; 3]; 3] {
    std::array::from_fn(|z| std::array::from_fn(|x| region[z][x].as_deref()))
}
#[cfg(test)]
mod quarter_tests {
    use super::*;

    fn corner(pos: [f32; 3], uv: [f32; 2], red: u32, light: u16) -> (TerrainVertex, [f32; 2]) {
        (TerrainVertex::new(pos, uv, 0, 0xFF00_0000 | red, light), uv)
    }

    #[test]
    fn quarters_blend_the_corners_by_triangle() {
        // Only corner 2 is lit: the GPU's triangles (0-1-2, 0-2-3) put half of it in the
        // middle, where a bilinear blend would put a quarter and the other diagonal none.
        let c = [
            corner([0.0, 0.0, 0.0], [0.0, 0.0], 0, 0),
            corner([1.0, 0.0, 0.0], [1.0, 0.0], 0, 0),
            corner([1.0, 1.0, 0.0], [1.0, 1.0], 200, 0x2010),
            corner([0.0, 1.0, 0.0], [0.0, 1.0], 0, 0),
        ];
        let (centre, uv) = bilinear(&c, 0.5, 0.5);
        assert_eq!(centre.pos, [0.5, 0.5, 0.0]);
        assert_eq!(uv, [0.5, 0.5]);
        assert_eq!(centre.color, 0xFF00_0000 | 100);
        assert_eq!(centre.light, 0x1008, "sky and block light blend apart");
        // Off the diagonal, on the 0-1-2 triangle: corner 2 weighs t.
        assert_eq!(bilinear(&c, 0.75, 0.25).0.color & 255, 50);
        assert_eq!(bilinear(&c, 0.5, 0.0).0.pos, [0.5, 0.0, 0.0]);
    }

    #[test]
    fn compact_faces_cut_where_the_sprite_does() {
        // A full face: cut in the middle both ways.
        let full = [
            corner([0.0, 1.0, 0.0], [0.0, 0.0], 0, 0),
            corner([1.0, 1.0, 0.0], [1.0, 0.0], 0, 0),
            corner([1.0, 0.0, 0.0], [1.0, 1.0], 0, 0),
            corner([0.0, 0.0, 0.0], [0.0, 1.0], 0, 0),
        ];
        assert_eq!(*cuts(&full), [0.0, 0.5, 1.0]);
        // A dirt path's side, v from 1/16: the sprite's middle is 7/15 of the way down; both
        // directions are cut there and at the middle of u.
        let mut path = full;
        path[0].1 = [0.0, 1.0 / 16.0];
        path[1].1 = [1.0, 1.0 / 16.0];
        assert_eq!(
            *cuts(&path),
            [0.0, (0.5 - 1.0 / 16.0) / (15.0 / 16.0), 0.5, 1.0]
        );
        // A slab's lower side (v 0.5..1) crosses the middle only along u.
        let mut slab = full;
        slab[0].1 = [0.0, 0.5];
        slab[1].1 = [1.0, 0.5];
        assert_eq!(crossing(&slab, 3), None);
        assert_eq!(*cuts(&slab), [0.0, 0.5, 1.0]);
        // Quarters by UV centre: top-left, top-right, bottom-left, bottom-right.
        let piece = |s0: f32, t0: f32| {
            [
                (s0, t0),
                (s0 + 0.5, t0),
                (s0 + 0.5, t0 + 0.5),
                (s0, t0 + 0.5),
            ]
            .map(|(s, t)| bilinear(&full, s, t))
        };
        assert_eq!(
            [
                piece(0.0, 0.0),
                piece(0.5, 0.0),
                piece(0.0, 0.5),
                piece(0.5, 0.5)
            ]
            .map(|p| quarter_of(&p)),
            [0, 1, 2, 3]
        );
    }

    #[test]
    fn overlays_take_the_cubes_own_uvs() {
        use horizon_assets::model::Direction as D;
        // A top face: the north-west corner is the sprite's top-left, so a tile's top edge is
        // north whatever the model's turn.
        assert_eq!(standard_uv(D::Up, [0.0, 1.0, 0.0]), [0.0, 0.0]);
        assert_eq!(standard_uv(D::Up, [1.0, 1.0, 1.0]), [1.0, 1.0]);
        // A north face seen from the north: its top-left is the east top corner.
        assert_eq!(standard_uv(D::North, [1.0, 1.0, 0.0]), [0.0, 0.0]);
        assert_eq!(standard_uv(D::East, [1.0, 0.0, 1.0]), [0.0, 1.0]);
        assert_eq!(standard_uv(D::Down, [0.0, 0.0, 1.0]), [0.0, 0.0]);
    }
}
