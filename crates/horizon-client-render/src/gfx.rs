//! GPU renderer for streamed terrain sections, sky, block outline and HUD.
//! Section origins are supplied relative to the camera each frame so large
//! world coordinates keep full precision.
use crate::{
    Result,
    block_entity_render::{StaticCacheFrameStats, StaticSectionRevision, StaticSectionSnapshot},
    environment::Sky,
    fast_hash::FxHashMap,
    graphics::GraphicsApi,
    hud::{HudBatch, HudRenderer},
    mesher::{Part as Quads, SectionMesh, Sides, TerrainVertex, Vertex, packed},
    section_mask::SectionMask,
    sprites::SpriteAnimation,
};
use glam::{DVec3, Mat4, Vec3, Vec4};
use std::{collections::HashMap, path::PathBuf, sync::Arc, time::Instant};
use wgpu::util::DeviceExt;
use winit::window::Window;

// RenderPipelines.CRUMBLING: dst * src + src * dst, alpha replaced.
const CRUMBLING_BLEND: wgpu::BlendState = wgpu::BlendState {
    color: wgpu::BlendComponent {
        src_factor: wgpu::BlendFactor::Dst,
        dst_factor: wgpu::BlendFactor::Src,
        operation: wgpu::BlendOperation::Add,
    },
    alpha: wgpu::BlendComponent::REPLACE,
};
/// `TerrainVertex` for `vs_terrain`: position, UV, layer, light, colour.
const TERRAIN_ATTRIBUTES: [wgpu::VertexAttribute; 5] =
    wgpu::vertex_attr_array![0=>Float32x3,1=>Uint16x2,2=>Uint16,4=>Uint8x2,3=>Unorm8x4];

/// entity.wgsl after the OverlayTexture constants it reads.
fn entity_shader() -> String {
    crate::overlay::wgsl_constants() + include_str!("hdr_light.wgsl") + include_str!("entity.wgsl")
}

fn no_pack_hdr_gain(layered: bool, pack: bool, world: bool) -> bool {
    layered && !pack && world
}

fn light_marks(format: wgpu::TextureFormat, hdr: bool) -> bool {
    hdr && format != LINEAR_FRAME
}

fn terrain_shader() -> String {
    include_str!("hdr_light.wgsl").to_owned() + include_str!("terrain.wgsl")
}

fn border_shader() -> String {
    include_str!("hdr_light.wgsl").to_owned() + include_str!("border.wgsl")
}

/// The static block-entity vertex entry point shares entity shading but lives
/// in its own source file and pipeline.
fn static_entity_shader() -> String {
    entity_shader() + include_str!("static_entity.wgsl")
}

#[cfg(test)]
mod crumbling_tests;
mod entity_batch;
mod entity_outline;
mod fluid;
#[cfg(test)]
mod inter_stage_tests;
mod terrain_model;
#[cfg(test)]
pub(crate) mod test_gpu;
mod weather;
mod world_text;
use entity_batch::EntityBatch;
pub use entity_batch::{
    ADDITIVE_PARTICLE_OVERLAY, DISSOLVE_OVERLAY, DRAGON_RAYS_OVERLAY, ENERGY_SWIRL_OVERLAY,
    FLUID_OVERLAY, OIT_ENTITY, SHADOW_OVERLAY, TERRAIN_MODEL_CUTOUT_MIPPED_OVERLAY,
    TERRAIN_MODEL_CUTOUT_OVERLAY, TERRAIN_MODEL_SOLID_OVERLAY, TEXT_POLYGON_OFFSET_OVERLAY,
    TRANSLUCENT_GLINT_ITEM_OVERLAY, TRANSLUCENT_ITEM_OVERLAY, TRANSLUCENT_PARTICLE_OVERLAY,
    WATER_MASK_OVERLAY, WORLD_TEXT_OVERLAY, WORLD_TEXT_POLYGON_OFFSET_OVERLAY, is_native_terrain,
    mark_oit,
};
pub use view::{View, ViewSet};

/// Window.getAppropriateLineWidth, in framebuffer pixels.
pub fn outline_width(width: u32) -> f32 {
    2.5f32.max(width as f32 / 1920.0 * 2.5)
}

/// ScreenSize, stroke width and ProjectionType.PERSPECTIVE layering.
pub fn outline_params(width: u32, height: u32) -> [f32; 4] {
    [
        width as f32,
        height.max(1) as f32,
        outline_width(width),
        1.0 - 1.0 / 4096.0,
    ]
}

/// Camera-relative segment endpoints; no shape expansion in vanilla 26.3.
pub fn outline_vertices(edges: &[[f64; 6]], camera: glam::DVec3) -> Vec<[f32; 6]> {
    let mut out = Vec::with_capacity(edges.len());
    outline_vertices_into(edges, camera, &mut out);
    out
}

fn outline_vertices_into(edges: &[[f64; 6]], camera: glam::DVec3, out: &mut Vec<[f32; 6]>) {
    let camera = camera.to_array();
    out.clear();
    out.extend(
        edges
            .iter()
            .map(|e| std::array::from_fn(|i| (e[i] - camera[i % 3]) as f32)),
    );
}

/// Whether the renderer should initialize its occlusion culling pass (`cull`).
///
/// Software adapters spend more time running the culling pass than drawing
/// the sections it removes. Keep the pass available for explicit A/B checks,
/// while leaving hardware adapters on the existing default.
fn occlusion_culling_requested(info: &wgpu::AdapterInfo, env: Option<&str>) -> bool {
    env != Some("0") && (env.is_some() || info.device_type != wgpu::DeviceType::Cpu)
}

/// `DrawableGizmoPrimitives.getGroup`: a line whose ARGB alpha is 255 joins the
/// opaque group, drawn with `RenderTypes.lines` (depth written); the rest
/// with `RenderTypes.linesTranslucentNoDepthWrite`.
fn debug_line_opaque(line: &[f32; 8]) -> bool {
    line[6].to_bits() >> 24 == 0xFF
}

/// The depth-tested gizmo lines in their upload order: the opaque group first,
/// then the translucent one, each in submission order.
fn debug_lines_opaque_first(lines: &[[f32; 8]]) -> impl Iterator<Item = &[f32; 8]> {
    let opaque = lines.iter().filter(|l| debug_line_opaque(l));
    opaque.chain(lines.iter().filter(|l| !debug_line_opaque(l)))
}

/// Whether Translucent Batching is offered (`graphics::offer_translucent_batching`): where
/// indirect multi-draws (`multi_draw`) take its batches, but not OpenGL's, which loop over
/// their entries on the CPU (and without them every section is a call of its own anyway).
/// Frames drawn flat (Metal, Intel Gen11/Gen12's Vulkan driver) take it too: their
/// translucent calls then come one per page of its arena rather than about one per section.
fn translucent_batchable(info: &wgpu::AdapterInfo, multi_draw: bool) -> bool {
    multi_draw && info.backend != wgpu::Backend::Gl
}

/// The vertices flat draws bind for a call's page key: a page of the section arena, or of
/// the translucent arena up to its last range in use (`flat::TRANSLUCENT_KEYS`).
fn flat_vertices<'a>(
    arena: &'a arena::Arena,
    translucent: Option<&'a arena::Arena>,
) -> impl Fn(u32) -> wgpu::BufferSlice<'a> + Copy {
    move |key| match (translucent, key.checked_sub(flat::TRANSLUCENT_KEYS)) {
        (Some(translucent), Some(page)) => translucent.binding(page),
        _ => arena.buffer(key).slice(..),
    }
}

/// Bytes of `texture` with every mip level of every layer.
pub(crate) fn texture_bytes(texture: &wgpu::Texture) -> u64 {
    let size = texture.size();
    let block = u64::from(texture.format().block_copy_size(None).unwrap_or(4));
    (0..texture.mip_level_count())
        .map(|level| {
            let side = |v: u32| u64::from((v >> level).max(1));
            side(size.width) * side(size.height) * block
        })
        .sum::<u64>()
        * u64::from(size.depth_or_array_layers)
}

/// Whether translucent quads have an arena of their own: with Translucent Batching
/// (`batching`), unless a shader pack's frames can draw terrain flat (as for `flat_frame`),
/// which reads them in the section arena through its tables: where flat draws exist and
/// are on (`flat_draws`), for a `shaders/` pack's terrain programs (`terrain`) or a shadow
/// pass (`shadow`: a `shaders/` pack's, or the `[shadow]` of one of Horizon's own).
fn translucent_split(batching: bool, flat_draws: bool, terrain: bool, shadow: bool) -> bool {
    batching && !(flat_draws && (terrain || shadow))
}

/// The compact layout of solid quads, unless `HORIZON_PACKED_QUADS=0` (A/B checks).
fn packed_wanted() -> bool {
    std::env::var("HORIZON_PACKED_QUADS").as_deref() != Ok("0")
}

mod animate;
mod arena;
pub mod capture;
pub mod compositor;
pub(crate) mod core_lines;
pub(crate) mod core_sky;
mod cull;
mod dx12_compiler;
mod end_sky;
pub mod entity_skin;
mod flat;
mod frame_gen;
mod gpu_culling;
mod hdr;
mod hdr_icc;
mod hdr_start;
mod hdr_stats;
pub(crate) mod hud_coverage;
pub use hdr::{Calibration as HdrCalibration, Output as HdrOutput};
mod hold;
pub use hold::WorldHold;
pub use output::{DisplayReading, EffectiveOutput, FrameConfiguration, FrameRequest};
pub mod block_light;
mod custom_sky;
mod displace;
pub mod lightning;
mod linear_blend;
mod lod;
mod native_material;
mod native_pack;
mod native_post;
pub mod native_render;
mod native_shadow;
mod native_sky;
mod occlusion;
mod oit;
mod oit_features;
pub(crate) mod output;
mod output_apply;
mod pack_atlas;
mod pack_center_depth;
pub mod pack_draw;
mod pack_entities;
mod pack_hand;
mod pack_images;
mod pack_lines;
mod pack_post;
mod pack_shadow;
mod pack_sky;
mod pack_textures;
pub(crate) mod pack_timing;
pub mod pack_uniforms;
mod pack_weather;
mod page_runs;
pub mod panorama;
mod readback;
mod resident;
mod reuse;
mod stars;
mod surface_setup;
use surface_setup::{AdapterChoice, AdapterFailure, open_adapter, pick_adapter, set_up_surface};
pub mod submit;
mod sunrise_fan;
mod timing;
pub mod translucency;
pub(crate) mod upload;
mod view;
mod visbuffer;
mod wgsl_prefix;
mod world_post;
pub mod world_primitives;
mod world_warp;

/// A section's instance: its camera-relative origin and, in `w`, the step of
/// its fixed-point UVs (`terrain.wgsl`'s `terrain_uv`).
/// The world pass again after occlusion culling's test: it loads both
/// targets (phase 2 draws over phase 1, reading the test's results).
/// `LevelRenderer.addSkyPass`: fluids and camera effects block the sky pass.
fn sky_open(frame: &Frame) -> bool {
    !frame.sky_blocked
}

/// The Overworld-style sky (disc, sunset, stars, void) draws this frame.
fn sky_visible(frame: &Frame) -> bool {
    !frame.title && frame.skybox && sky_open(frame)
}

/// The End during the dragon fight, whose world fog leaves out vanilla's sky: a shader pack's
/// `gbuffers_skybasic` still draws a fan around the horizon (`pack_sky`), as our probe packs read
/// it in Iris 1.11.7.
fn end_fog_sky(frame: &Frame) -> bool {
    !frame.title && frame.end_sky && frame.boss_world_fog
}

/// The world pass's part that draws a shader pack's solid terrain: the world's colour and
/// depth loaded, and its `surface` target, cleared on the first of these passes.
fn surface_pass<'e>(
    encoder: &'e mut wgpu::CommandEncoder,
    view: &wgpu::TextureView,
    surface: &wgpu::TextureView,
    depth: &wgpu::TextureView,
    clear: bool,
) -> wgpu::RenderPass<'e> {
    let keep = |load| wgpu::Operations {
        load,
        store: wgpu::StoreOp::Store,
    };
    encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("Horizon world, pack terrain"),
        color_attachments: &[
            Some(wgpu::RenderPassColorAttachment {
                view,
                resolve_target: None,
                depth_slice: None,
                ops: keep(wgpu::LoadOp::Load),
            }),
            Some(wgpu::RenderPassColorAttachment {
                view: surface,
                resolve_target: None,
                depth_slice: None,
                ops: keep(if clear {
                    wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT)
                } else {
                    wgpu::LoadOp::Load
                }),
            }),
        ],
        depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
            view: depth,
            depth_ops: Some(wgpu::Operations {
                load: wgpu::LoadOp::Load,
                store: wgpu::StoreOp::Store,
            }),
            stencil_ops: None,
        }),
        ..Default::default()
    })
}
fn world_pass_after_culling<'e>(
    encoder: &'e mut wgpu::CommandEncoder,
    view: &wgpu::TextureView,
    depth: &wgpu::TextureView,
    keep_depth: bool,
    timing: Option<&mut pack_timing::PassTiming>,
    name: &str,
) -> wgpu::RenderPass<'e> {
    encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("Horizon world, after culling"),
        timestamp_writes: timing.and_then(|t| t.writes(name)),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view,
            resolve_target: None,
            depth_slice: None,
            ops: wgpu::Operations {
                load: wgpu::LoadOp::Load,
                store: wgpu::StoreOp::Store,
            },
        })],
        depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
            view: depth,
            depth_ops: Some(wgpu::Operations {
                load: wgpu::LoadOp::Load,
                // A shader pack reads it as depthtex0.
                store: if keep_depth {
                    wgpu::StoreOp::Store
                } else {
                    wgpu::StoreOp::Discard
                },
            }),
            stencil_ops: None,
        }),
        ..Default::default()
    })
}
/// The point a `shaders/` pack's terrain is drawn relative to: the camera with the fraction of
/// each coordinate rounded to the nearest 1/65536 block. Read back with a probe pack of ours,
/// Iris 1.11.7's terrain `gl_Vertex` is the block's position minus this, for cameras from 13
/// to 40000 blocks out (#8464); `cameraPosition` and the matrices keep the exact camera. Off by
/// up to 2^-17 blocks, a fragment on a texel boundary of the shadow map reads the other texel.
fn pack_terrain_camera(camera: DVec3) -> DVec3 {
    let whole = camera.floor();
    whole + ((camera - whole) * 65536.0).round() / 65536.0
}
/// A section's instance: its camera-relative origin and its UV step, negated
/// past its fade code while it fades in over far terrain (`lod::instance`).
fn section_instance(origin: [f32; 3], fade: u32, visibility: f32) -> [f32; 4] {
    let step = 1.0 / crate::mesher::UV_STEPS;
    [
        origin[0],
        origin[1],
        origin[2],
        if fade == 0 {
            // The step is a power of two, so its mantissa is free: it holds the vanilla
            // chunk fade-in's `1 - visibility` in 23 bits (terrain.wgsl's `instance_step` and
            // `instance_visibility` read it back). A whole section is the bare step.
            let faded = ((1.0 - visibility) * 8_388_608.0)
                .round()
                .clamp(0.0, 8_388_607.0) as u32;
            f32::from_bits(step.to_bits() | faded)
        } else {
            -(fade as f32 + step)
        },
    ]
}
/// `SectionRenderDispatcher.RenderSection.getVisibility`: how far through its fade-in a section
/// uploaded `elapsed_ms` ago is.
fn chunk_visibility(elapsed_ms: u64, fade_ms: u64) -> f32 {
    if elapsed_ms >= fade_ms {
        1.0
    } else {
        elapsed_ms as f32 / fade_ms as f32
    }
}
/// `Util.toMillis`: `Mth.floor(seconds * 1000.0)`.
fn fade_millis(seconds: f32) -> u64 {
    (f64::from(seconds) * 1000.0).floor().max(0.0) as u64
}
/// `chunk_visibility` at `now` of a section first uploaded at `uploaded`
/// (`Renderer::section_uploaded`; none: one that fades in over far terrain, or
/// whose column left), with the chunk fade-in `fade_ms`.
fn section_visibility(uploaded: Option<&Instant>, fade_ms: u64, now: Instant) -> f32 {
    uploaded.map_or(1.0, |&t| {
        let elapsed = now.saturating_duration_since(t).as_millis() as u64;
        chunk_visibility(elapsed, fade_ms)
    })
}
/// What a section's fade depends on besides the section itself
/// (`SectionGpu::drawn_faded`): the time, the chunk fade-in time, the sections'
/// first uploads (`Renderer::section_uploaded`) and those held while far
/// terrain comes in (`Renderer::lod_retiring`).
#[derive(Clone, Copy)]
struct FadeInputs<'a> {
    now: Instant,
    fade_ms: u64,
    uploaded: &'a FxHashMap<SectionKey, Instant>,
    retiring: &'a crate::fast_hash::FxHashSet<SectionKey>,
}
/// `LevelRenderer.CHUNK_VISIBILITY_THRESHOLD`: a section's block entities wait for it.
const CHUNK_VISIBILITY_THRESHOLD: f32 = 0.3;
/// A new value for `Renderer::block_entity_revision`.
fn next_block_entity_revision() -> u64 {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Globals {
    view_proj: [[f32; 4]; 4],
    fog_color: [f32; 4],
    fog: [f32; 4],
    sky_color: [f32; 4],
    misc: [f32; 4],
    inverse_view: [[f32; 4]; 4],
    sunrise: [f32; 4],
    sun: [f32; 4],
    /// x: sky fog end, y: cloud fog end, z: dark disc (camera below the horizon),
    /// w: the glint strength (`GlintAlpha`).
    fog_ends: [f32; 4],
    /// Atlas rects (u0, v0, width, height) of end_sky.png and end_portal.png.
    end_portal_sprites: [[f32; 4]; 2],
    /// x: vanilla's `GameTime` uniform; y: nether cardinal lighting; z: how many
    /// `dynamic_lights` are set; w: the glint scroll clock (`Glint::scroll_ticks`).
    time: [f32; 4],
    /// Atlas rect of a destroy stage (entity.wgsl takes only its size: every stage's is the same,
    /// and each crack carries its own stage's origin).
    crumbling_sprite: [f32; 4],
    /// Framebuffer size, selection stroke width, perspective layering scale.
    outline_params: [f32; 4],
    /// Horizon Extras' dynamic lights: camera-relative position and level (`dynamic_light`).
    dynamic_lights: [[f32; 4]; crate::dynamic_light::MAX],
    colored_lights: [crate::dynamic_light::Colored; crate::dynamic_light::MAX],
    colored_count: [u32; 4],
    emissive_layers: [[u32; 4]; crate::native_emissive::MAX],
    emissive_count: [u32; 4],
    /// The sunrise fan's triangles as the sky shader tests them (`sunrise_fan::normals`).
    sunrise_fan: [[f32; 4]; 2 * sunrise_fan::TRIANGLES],
    /// x: `Options.textureFiltering` (0 none, 1 RGSS, 2 anisotropic); y: the sprite arrays' width;
    /// z: the mip level at which a sprite is one texel, when the views reach it, else 0.
    filtering: [f32; 4],
    /// HDR output's tier windows, low to very bright: where the lift starts, where it is
    /// full (`hdr::Hdr::windows`); zero without HDR.
    hdr_windows: [[f32; 4]; 2],
    hdr_light: [f32; 4],
    /// Headroom, effective scene gain, world/look and debug, as HDR output's pass has them
    /// (`Hdr::draw_params`).
    hdr_settings: [f32; 4],
    hdr_shares: [f32; 4],
}
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct StaticEntityUniform {
    camera_block: [i32; 4],
    camera_fraction: [f32; 4],
}
struct GpuStaticEntitySection {
    buffer: wgpu::Buffer,
    origin_buffer: wgpu::Buffer,
    origin: [i32; 3],
    capacity: u64,
    revision: u64,
    vertices: u32,
}
pub type SectionKey = (i32, i32, i32);
/// What a section's mesh brings with it besides its quads (`Renderer::upload`): placed with
/// the mesh, now or when a hold lets go of it.
struct MeshFacts<'a> {
    visibility: u64,
    block_entities: &'a [([i32; 3], u32)],
    lights: &'a [([i32; 3], u32)],
    bounds: Option<[glam::Vec3; 2]>,
    /// It has translucent quads.
    translucent: bool,
    /// It has no quads at all: that is its mesh.
    empty: bool,
}
impl<'a> MeshFacts<'a> {
    fn of(mesh: &'a SectionMesh) -> Self {
        Self {
            visibility: mesh.visibility,
            block_entities: &mesh.block_entities,
            lights: &mesh.lights,
            bounds: mesh.bounds,
            translucent: !mesh.translucent.is_empty(),
            empty: mesh.solid.is_empty() && mesh.translucent.is_empty(),
        }
    }
}
/// A section's new mesh waiting behind a hold (gfx/hold.rs): uploaded, with what goes with it.
struct HeldMesh {
    gpu: Option<SectionGpu>,
    visibility: u64,
    block_entities: Vec<([i32; 3], u32)>,
    lights: Vec<([i32; 3], u32)>,
    bounds: Option<[glam::Vec3; 2]>,
    translucent: bool,
    empty: bool,
}
impl HeldMesh {
    fn facts(&self) -> MeshFacts<'_> {
        MeshFacts {
            visibility: self.visibility,
            block_entities: &self.block_entities,
            lights: &self.lights,
            bounds: self.bounds,
            translucent: self.translucent,
            empty: self.empty,
        }
    }
}
/// The water mirror's targets: the world as a camera mirrored about the water plane sees
/// it, at half the world's resolution (the material stage's `reflection`).
struct MirrorTarget {
    size: [u32; 2],
    color: wgpu::TextureView,
    depth: wgpu::TextureView,
}
struct SectionGpu {
    /// Arena page and first vertex of its solid quads, and after them its
    /// translucent quads unless those have an arena of their own; (0, 0),
    /// unused, when that leaves it no quads in `arena`.
    page: u32,
    start: u64,
    /// The page and first vertex of its translucent quads: in
    /// `translucent_arena` when there is one, else after the solid quads.
    /// Unused without translucent quads.
    translucent_at: (u32, u64),
    solid: u32,
    /// Slots its solid quads take in `arena`: four vertices each, or in the compact layout
    /// (`packed`, `mesher::packed`) a record each and the vertices of those without one.
    solid_slots: u64,
    packed: bool,
    /// Its records are a far tile's (`lod::packed`), not a section's: the two
    /// layouts are told apart by the base vertex its draws use.
    packed_far: bool,
    /// Where the solid quads' buckets by facing side end (`mesher::Sides`).
    sides: Sides,
    translucent: u32,
    sort: translucency::SortState,
    sorted_indices: Option<(u32, u64)>,
    /// Its id for flat draws (`flat::Flat::add`), when they are on.
    flat_id: Option<u32>,
    /// Its record's id in GPU culling's table (`gpu_culling::Records`), while
    /// that is on and it has solid quads.
    gpu_id: Option<u32>,
    /// Its record is `FADING` (GPU culling on): the CPU draws its solid quads
    /// while its instance carries a fade (`drawn_faded`), and GPU culling's
    /// pass leaves it out.
    gpu_fading: bool,
    /// When it came in over the far terrain it replaces (drawn whole at once,
    /// `fade`): a section new where far tiles are drawn, which skips vanilla's
    /// fade-in. Meshed again, it stays as it was.
    fading: Option<std::time::Instant>,
    /// When it began to fade out, its column gone over far terrain that
    /// stays behind it (`Renderer::leave_section`); released once it has.
    leaving: Option<std::time::Instant>,
    /// Animated sprite layers present in this section.
    animated_layers: Box<[u16]>,
}
/// Extracts the animated sprite layers present in a section mesh. Meshing
/// records the distinct layers, so uploading does not rescan every vertex.
pub(crate) fn collect_animated_layers(
    mesh: &SectionMesh,
    is_animated: impl Fn(u16) -> bool,
) -> Box<[u16]> {
    mesh.sprite_layers
        .iter()
        .copied()
        .filter(|&layer| is_animated(layer))
        .collect()
}

impl SectionGpu {
    /// Its fade code this frame: 0, drawn whole, or `lod::HELD` (whole, in
    /// front of far terrain) while it leaves (`leaving`) or its column has left
    /// the render distance and far terrain is coming in under it (`retiring`).
    /// A near section never dithers over far terrain: the holes of its pattern
    /// showed its own inside (cave walls, the stone under the grass), nearer
    /// than the tile behind, an x-ray of each column coming in or going out.
    /// It comes in whole over the tile it replaces, which stays behind it until
    /// `fading` is `lod::FADE` old, and leaves whole after as long.
    fn fade(&self, retiring: bool) -> u32 {
        if self.leaving.is_some() || retiring {
            lod::HELD
        } else {
            0
        }
    }
    /// Its vertices in `arena`, and in `translucent_arena` when `split` (there
    /// is one): (page, first vertex, vertices) of each.
    fn vertex_ranges(&self, split: bool) -> (Option<(u32, u64, u64)>, Option<(u32, u64, u64)>) {
        if !split {
            let vertices = self.solid_slots + u64::from(self.translucent) * 4;
            return (Some((self.page, self.start, vertices)), None);
        }
        let (page, start) = self.translucent_at;
        (
            (self.solid > 0).then_some((self.page, self.start, self.solid_slots)),
            (self.translucent > 0).then(|| (page, start, u64::from(self.translucent) * 4)),
        )
    }
    /// Whether GPU culling keeps a record of it at `key` while it is on (`gpu_id`):
    /// it has solid quads, in one of the level's `sections`, which the pass's
    /// tables span.
    fn gpu_recorded(&self, key: SectionKey, sections: usize) -> bool {
        self.solid > 0 && (0..sections as i32).contains(&key.1)
    }
    /// The base vertex of its solid draws: quad `q`'s corners are `base + 4 * q` on (the
    /// quad indices). In a compact layout they name its record's slot past
    /// `PACKED_BASE`, or past `FAR_BASE` for a far tile's records, which is how
    /// terrain.wgsl's `section_corner` tells the two apart.
    fn solid_base(&self) -> u32 {
        if !self.packed {
            return self.start as u32;
        }
        let base = if self.packed_far {
            crate::lod::packed::FAR_BASE
        } else {
            packed::PACKED_BASE
        };
        base + 4 * self.start as u32
    }
    /// GPU culling's record of it at `key`, its quads within `bounds`
    /// (`SectionMesh::bounds`) and in their buckets by facing side (`sides`),
    /// `FADING` while it fades (`gpu_fading`).
    fn record(&self, key: SectionKey, bounds: Option<[Vec3; 2]>) -> gpu_culling::Record {
        let boxed = gpu_culling::rounded_box(bounds);
        gpu_culling::Record::new(
            key,
            self.page,
            u64::from(self.solid_base()),
            self.solid,
            boxed,
        )
        .facing(&self.sides)
        .fading(self.gpu_fading)
    }
    /// Whether GPU frames' walk lists it within the render distance
    /// (`Resident::walk`): its mark. Its translucent quads are the CPU's, or
    /// GPU culling can have no record of it, which holds whether GPU culling is
    /// on or not; or, while it is, its solid quads are the CPU's while it fades
    /// (`gpu_fading`).
    fn listed(&self, key: SectionKey, sections: usize) -> bool {
        self.translucent > 0 || !self.gpu_recorded(key, sections) || self.gpu_fading
    }
    /// Whether GPU culling draws its solid quads in a GPU frame, `near`: its
    /// column within the render distance.
    fn gpu_draws_solid(&self, near: bool) -> bool {
        near && self.gpu_id.is_some() && !self.gpu_fading
    }
    /// Whether frames draw it faded at `f.now`: its instance, as the instance
    /// loop makes it (`section_instance` with its `fade` and its vanilla
    /// visibility), is not a whole section's. As time goes on a fade only ends:
    /// once false, this stays false until the section is meshed again, leaves,
    /// is held (`lod_retiring`), or the chunk fade-in time changes.
    fn drawn_faded(&self, key: SectionKey, f: FadeInputs<'_>) -> bool {
        let fade = self.fade(f.retiring.contains(&key));
        let visibility = section_visibility(f.uploaded.get(&key), f.fade_ms, f.now);
        section_instance([0.0; 3], fade, visibility)[3].to_bits()
            != section_instance([0.0; 3], 0, 1.0)[3].to_bits()
    }
    /// When its fade is over at the latest (`drawn_faded`): the end of
    /// vanilla's fade-in (its first upload and `f.fade_ms`; one coming in over
    /// far terrain is whole at once, `fade`). None while it leaves or is held,
    /// which ends only when it goes or is meshed again, and without a fade-in.
    fn fade_end(&self, key: SectionKey, f: FadeInputs<'_>) -> Option<Instant> {
        if self.leaving.is_some() || f.retiring.contains(&key) {
            return None;
        }
        let vanilla = f.uploaded.get(&key).filter(|_| f.fade_ms > 0);
        vanilla.map(|&t| t + std::time::Duration::from_millis(f.fade_ms))
    }
}
/// Which of a mesh's ranges `move_off_page` moves: what it holds in the section
/// arena (`split`: its solid quads alone, `SectionGpu::vertex_ranges`), its
/// translucent quads in `translucent_arena`, or its sorted indices.
#[derive(Clone, Copy)]
enum Part {
    Section { split: bool },
    Translucent,
    Indices,
}
impl Part {
    /// (page, first vertex or index, count), as the release paths free it.
    fn of(self, s: &SectionGpu) -> Option<(u32, u64, u64)> {
        match self {
            Self::Section { split } => s.vertex_ranges(split).0,
            Self::Translucent => s.vertex_ranges(true).1,
            Self::Indices => s
                .sorted_indices
                .map(|(page, at)| (page, at, u64::from(s.translucent) * 6)),
        }
    }
    /// Points the mesh where its range moved.
    fn set(self, s: &mut SectionGpu, to: (u32, u64)) {
        match self {
            Self::Section { split } => {
                (s.page, s.start) = to;
                if !split {
                    // Its translucent quads came along, after the solid ones.
                    s.translucent_at = (to.0, to.1 + s.solid_slots);
                }
            }
            Self::Translucent => s.translucent_at = to,
            Self::Indices => s.sorted_indices = Some(to),
        }
    }
}
/// `Renderer::compact_arena`'s and `shrink_translucent`'s moves: what the meshes
/// hold on `page` of `arena` (`part`) goes to its other pages in video memory,
/// until the next range would pass `budget` bytes (it keeps what is left; the
/// first range moves whatever its size, so none keeps the page closed), the
/// copies into `encoder`, the ranges' flat draw ids (`flat`) and, in the
/// section arena, GPU culling's records (`records`) along. False when the other
/// pages have no room left.
fn move_off_page<'a>(
    arena: &mut arena::Arena,
    encoder: &mut wgpu::CommandEncoder,
    mut flat: Option<(&mut flat::Flat, &wgpu::Device, &mut upload::Uploads)>,
    mut records: Option<&mut gpu_culling::Records>,
    meshes: impl Iterator<Item = &'a mut SectionGpu>,
    page: u32,
    part: Part,
    budget: &mut u64,
) -> bool {
    debug_assert!(
        records.is_none() || matches!(part, Part::Section { .. }),
        "records follow the section arena's ranges"
    );
    let mut moved = false;
    for s in meshes {
        // Nothing of a mesh here: split, one without solid quads (its (0, 0) is no
        // range of page 0); in the other arenas, one without translucent quads.
        let Some((_, start, vertices)) = part.of(s).filter(|r| r.0 == page) else {
            continue;
        };
        let bytes = vertices * arena.stride();
        if moved && bytes > *budget {
            break;
        }
        let Some(to) = arena.alloc_moved(vertices) else {
            return false;
        };
        arena.copy(encoder, (page, start), to, vertices);
        arena.release(page, start, vertices);
        if let (Some((flat, device, uploads)), Some(id)) = (&mut flat, s.flat_id) {
            let key = match part {
                Part::Translucent => to.0 + flat::TRANSLUCENT_KEYS,
                _ => to.0,
            };
            flat.place(device, encoder, uploads, id, key, to.1, vertices);
        }
        part.set(s, to);
        // Its solid quads start the range in either layout.
        if let (Part::Section { .. }, Some(records), Some(id)) =
            (part, records.as_deref_mut(), s.gpu_id)
        {
            records.relocate(id, to.0, u64::from(s.solid_base()));
        }
        *budget = budget.saturating_sub(bytes);
        moved = true;
    }
    true
}
/// Starts moving a page of a large arena far larger than its ranges in use
/// (`Arena::oversized`) into a new page of the size they get (`Arena::open_page`),
/// for `Renderer::shrink_translucent`: the page closes and `move_off_page` empties
/// it. None: no page to shrink. False, and nothing changes: no video memory for
/// the new page, or a range there without its mesh in `meshes`, which would keep
/// the page from ever emptying.
fn begin_shrink<'a>(
    arena: &mut arena::Arena,
    device: &wgpu::Device,
    meshes: impl Iterator<Item = &'a mut SectionGpu>,
    part: Part,
) -> Option<bool> {
    let (page, capacity) = arena.oversized()?;
    let held: u64 = meshes
        .filter_map(|s| part.of(s).filter(|r| r.0 == page))
        .map(|(.., n)| arena::Arena::span(n))
        .sum();
    debug_assert_eq!(held, arena.used(page), "a range in use without its mesh");
    if held != arena.used(page) || !arena.open_page(device, capacity) {
        return Some(false);
    }
    arena.close(page);
    Some(true)
}
/// GPU culling's flag (`SectionGpu::gpu_fading`) for its recorded section `s`
/// at `key`, decided at `f.now`: whether it draws faded (`drawn_faded`), and if
/// time ends that, its end waited for in `fades` (`fade_end`).
fn decide_fade(
    s: &SectionGpu,
    key: SectionKey,
    fades: &mut gpu_culling::Fades,
    f: FadeInputs<'_>,
) -> bool {
    let fading = s.drawn_faded(key, f);
    if let Some(end) = s.fade_end(key, f).filter(|_| fading) {
        fades.push(end, key);
    }
    fading
}
/// GPU culling: the section at `key` began to fade out over far terrain
/// (`Renderer::leave_section`), or is held while far terrain comes in
/// (`Renderer::lod_retiring`). It draws faded until it goes or is meshed again,
/// so its record is flagged and the CPU draws it, with no end to wait for.
fn hold_fading(
    sections: &mut resident::Resident<SectionGpu>,
    records: &mut gpu_culling::Records,
    key: SectionKey,
) {
    let Some(s) = sections.find_mut(key).filter(|s| !s.gpu_fading) else {
        return;
    };
    let Some(id) = s.gpu_id else {
        return;
    };
    s.gpu_fading = true;
    records.set_fading(id, true);
    sections.set_marked(key, true);
}
/// GPU culling switched off (`Renderer::set_gpu_culling`): no section keeps a
/// record or a flag, and those it flagged get back the walk marks they have
/// while it is off (`SectionGpu::listed`). `rows`: the level's sections.
fn forget_records(sections: &mut resident::Resident<SectionGpu>, rows: usize) {
    let mut marks = Vec::new();
    for (key, s, _) in sections.iter_mut() {
        s.gpu_id = None;
        if std::mem::take(&mut s.gpu_fading) {
            marks.push((key, s.listed(key, rows)));
        }
    }
    for (key, marked) in marks {
        sections.set_marked(key, marked);
    }
}
/// GPU culling's fading sections brought up to `f.now`, before a frame's gate
/// and `prepare`: every recorded section anew when the flags were decided with
/// another chunk fade-in time (GPU culling just switched on, or Video Settings
/// changed it), else those whose fade was due to end, handed back to the GPU if
/// whole by then. A section's record (`FADING`) and its walk mark follow its
/// `gpu_fading`. `rows`: the level's sections.
fn mark_fades(
    sections: &mut resident::Resident<SectionGpu>,
    records: &mut gpu_culling::Records,
    fades: &mut gpu_culling::Fades,
    rows: usize,
    f: FadeInputs<'_>,
) {
    let mut marks = Vec::new();
    if fades.fade_ms != Some(f.fade_ms) {
        fades.clear();
        fades.fade_ms = Some(f.fade_ms);
        for (key, s, _) in sections.iter_mut() {
            let Some(id) = s.gpu_id else {
                continue;
            };
            let fading = decide_fade(s, key, fades, f);
            if fading != s.gpu_fading {
                s.gpu_fading = fading;
                records.set_fading(id, fading);
                marks.push((key, s.listed(key, rows)));
            }
        }
    } else {
        while let Some(key) = fades.pop_due(f.now) {
            // Gone since, or no longer flagged.
            let Some(s) = sections.find_mut(key).filter(|s| s.gpu_fading) else {
                continue;
            };
            // Still fading: meshed again since, with its own end waited for
            // (`decide_fade`), or leaving or held, with none.
            if s.drawn_faded(key, f) {
                continue;
            }
            s.gpu_fading = false;
            records.set_fading(s.gpu_id.expect("flagged sections have records"), false);
            marks.push((key, s.listed(key, rows)));
        }
    }
    for (key, marked) in marks {
        sections.set_marked(key, marked);
    }
}
/// Line segments the hit outline can hold.
const MAX_OUTLINE_EDGES: u64 = 256;
/// Staging buffer size for section meshes: a frame of uploads while
/// exploring fits in a few.
const SECTION_UPLOAD_CHUNK: u64 = 4 << 20;
/// Section copies are submitted early past this many staged bytes, which
/// bounds the staging memory frames that are not drawn can hold.
const SECTION_COPY_LIMIT: u64 = 64 << 20;
/// What compaction copies in a frame, `compact_arena` first and
/// `shrink_translucent` with what it left, and a range more where a call's
/// first is larger (`move_off_page`): a page empties over a few frames, with
/// no hitch.
const COMPACT_BUDGET: u64 = 32 << 20;
/// A large translucent page starts to shrink (`Renderer::shrink_translucent`)
/// this long after terrain last arrived, and `SHRINK_RETRY` after a try found
/// no room.
/// `Renderer::remove_columns` filters each store once rather than looking up every
/// section key from this many keys, or from as many as there are resident sections.
const BULK_REMOVAL_KEYS: usize = 256;
const SHRINK_SETTLE: std::time::Duration = std::time::Duration::from_secs(1);
const SHRINK_RETRY: std::time::Duration = std::time::Duration::from_secs(5);
/// Not before then may a large translucent page start to shrink.
#[derive(Default)]
struct ShrinkGate(Option<Instant>);
impl ShrinkGate {
    /// Not before `wait` from `now`, nor before an earlier wait is over.
    fn wait(&mut self, now: Instant, wait: std::time::Duration) {
        self.0 = self.0.max(Some(now + wait));
    }
    /// Whether one may start now: the waits are over, and `compact_arena` copied
    /// nothing this frame (it `left` the whole budget).
    fn may_start(&self, now: Instant, left: u64) -> bool {
        left == COMPACT_BUDGET && self.0.is_none_or(|at| now >= at)
    }
}
pub struct Frame<'a> {
    pub camera: DVec3,
    pub yaw: f32,
    pub pitch: f32,
    pub fov_degrees: f32,
    /// The camera entity's eye and view rotation (yaw, pitch): the camera's own except in a
    /// third-person view.
    pub eye: DVec3,
    pub eye_rotation: [f32; 2],
    /// The camera is the entity's eye: no third-person view.
    pub first_person: bool,
    /// The `GameRenderer.bobHurt`/`bobView` pose multiplied into the
    /// projection (`crate::camera::bob_matrix`).
    pub bob: Mat4,
    /// The spinning effect multiplied into the level's projection only
    /// (`crate::camera::spinning_effect_matrix`), not the hand's.
    pub spin: Mat4,
    pub sky: Sky,
    /// Block edits and chunk membership that can change the shader pack's shadow map.
    pub shadow_scene_revision: (u64, u64),
    /// Opt-in reuse of the shader pack's last shadow map when the key still matches
    /// (`horizon-graphics.txt` `shadowCache`; off by default).
    pub shadow_cache: bool,
    /// Render distance in blocks (fog end).
    pub distance: f32,
    /// Full-detail terrain distance in blocks; distant LODs bypass the cave graph.
    pub detail_distance: f32,
    /// `Camera.depthFar`: the far plane (`crate::camera::depth_far`).
    pub depth_far: f32,
    pub underwater: bool,
    /// The y of the horizontal water plane around the camera, in world blocks, while one
    /// of Horizon's own packs reads the water mirror (`Game::water_plane`,
    /// `Renderer::wants_water_plane`).
    pub water_plane: Option<f32>,
    /// Skip sections the camera cannot see through open faces (vanilla's
    /// occlusion graph; off in spectator-like views inside blocks).
    pub cave_culling: bool,
    /// Title screen: only the dedicated panorama cube map and the HUD.
    pub title: bool,
    /// Lava, powder snow, blindness, darkness or a boss bar's world fog leaves out the sky pass
    /// (`fog::sky_pass_skipped`).
    pub sky_blocked: bool,
    /// Dimensions without a skybox (the Nether) clear to the fog colour.
    pub skybox: bool,
    /// The End: clear to black and draw the resident sky box behind the world.
    pub end_sky: bool,
    /// End sky atlas bounds (u0, v0, u1, v1); absent when the sprite is missing.
    pub end_sky_sprite: Option<[f32; 4]>,
    /// A boss bar makes world fog (`BossHealthOverlay.shouldCreateWorldFog`: the Ender Dragon's).
    pub boss_world_fog: bool,
    /// Environmental fog start/end (Nether 10..96), if any.
    pub environment_fog: Option<(f32, f32)>,
    pub water_fog: [f32; 3],
    /// `FogRenderer.setupFog` result; replaces the underwater/environment fields when set.
    pub fog: Option<crate::fog::FogData>,
    /// Camera eye below the horizon height and not under water (`shouldRenderDarkDisc`).
    pub dark_disc: bool,
    /// The hit block outline as line segments (from, to) in world space.
    pub outline: Vec<[f64; 6]>,
    /// `BlockOutlineRenderState.isTranslucent`: draw after translucent terrain.
    pub outline_after_terrain: bool,
    /// `BlockOutlineRenderState.highContrast`: a thick black outline under a bright one.
    pub outline_high_contrast: bool,
    /// The PvP & HUD page's outline and hit colour (`pvp::Frame`; default: vanilla's).
    pub pvp: crate::pvp::Frame,
    /// `lines` with a colour and pixel width (`gizmos::camera_relative`):
    /// the fishing line and debug gizmos, depth tested, then gizmos drawn
    /// over everything.
    pub debug_lines: &'a [[f32; 8]],
    pub debug_lines_on_top: &'a [[f32; 8]],
    /// `DebugCrosshairRenderer`'s axes, camera relative like `debug_lines`:
    /// drawn after the hand at its projection, without the bob or fog.
    pub debug_crosshair: &'a [[f32; 8]],
    pub hud: &'a HudBatch,
    /// `GlintAlpha` (accessibility Glint Strength); the entity shader uses it
    /// for held/world items, independent of what the HUD batch last drew.
    pub glint_alpha: f32,
    /// The glint scroll clock (`Glint::scroll_ticks`): entity.wgsl's
    /// `ARMOR_ENTITY_GLINT_TEXTURING` phase.
    pub glint_ticks: f32,
    /// CardinalLighting.Type.NETHER, otherwise DEFAULT.
    pub nether_lighting: bool,
    /// Camera-relative entity geometry textured from the shared atlas.
    pub entities: &'a [Vertex],
    /// Per-section cacheable block-entity models. Positions are section-local.
    pub static_block_entities: &'a [StaticSectionSnapshot],
    /// Sections currently intersecting the coarse camera frustum.
    pub visible_static_block_entity_sections: &'a [[i32; 3]],
    /// Cache work for this frame, reported in bench summaries.
    pub static_block_entity_stats: StaticCacheFrameStats,
    /// Which entity `entities`' vertices belong to: (first vertex, entity id), each run up
    /// to the next; id 0 for none. Frame generation follows these entities' own motion.
    pub entity_motion: &'a [(u32, u64)],
    /// For each mark of `entity_motion`, the GPU-posed vertex count at it (`skin`'s
    /// `vertices`): where that entity's posed vertices start. A shader pack's `at_velocity`
    /// matches both parts of an entity with last frame's.
    pub entity_motion_posed: &'a [u32],
    /// The glowing entities' silhouettes in their team colours (`OutlineBufferSource`),
    /// outlined through walls after the hand.
    pub glowing: &'a [Vertex],
    /// Runs of `entities` by entity, while a shader pack draws them (`pack_draw::EntitySpan`).
    pub entity_spans: &'a [pack_draw::EntitySpan],
    /// Ordinary entity models posed on the GPU (`entity_skin`), drawn before `entities`'
    /// ordinary ones; only when `Renderer::skins_entities` said so.
    pub skin: Option<&'a entity_skin::SkinFrame>,
    pub weather: &'a [crate::weather::Vertex],
    pub weather_time: crate::weather::Time,
    /// Horizon Extras' dynamic lights, nearest first (`dynamic_light::select`); empty when off.
    pub dynamic_lights: &'a [[f32; 4]],
    pub colored_lights: &'a [crate::dynamic_light::Colored],
    pub emissive_layers: &'a [[u32; 4]],
    /// Cloud mesh inputs; geometry and GPU storage are retained by the renderer.
    pub clouds: Option<crate::clouds::Input<'a>>,
    /// Retained lightning bolts, drawn before the other additive entities.
    pub lightning: lightning::Input<'a>,
    /// Additive translucent geometry after lightning (dragon rays).
    pub additive: &'a [Vertex],
    /// Name tags over everything, without depth (`TEXT_SEE_THROUGH`).
    pub see_through: &'a [Vertex],
    /// Native mods' lines, boxes, beams and text (`native_world_primitives`), drawn at the end
    /// of the world pass, before the first-person hand.
    pub world_primitives: world_primitives::Batch<'a>,
    /// Opaque entity quads only a native pack's shadow pass draws (the local player in first
    /// person).
    pub shadow_only: &'a [Vertex],
    /// Camera-relative sun and moon, or the animated End flash (additive,
    /// behind everything and after the resident End sky box).
    pub celestial: &'a [Vertex],
    /// The stars, drawn from their static buffer after the sun and moon:
    /// the sky's rotation (camera-relative world from star space) and the
    /// star brightness.
    pub stars: Option<(Mat4, f32)>,
    /// First-person hand and held item, drawn after clearing depth.
    pub hand: &'a [Vertex],
    /// Where the arm and each held item start in `hand` (first vertex), with the
    /// `currentRenderedItemId` a shader pack's `gbuffers_hand` reads for them.
    pub hand_items: &'a [(u32, i32)],
    /// `CameraRenderState.hudFov`: the hand's own field of view
    /// (`GameRenderer.render3dHud`).
    pub hand_fov_degrees: f32,
    /// Full-screen camera effects drawn over the world and hand, under the HUD.
    pub screen: &'a [crate::screen_effects::ScreenQuad],
    /// What a shader pack's uniforms need from the game.
    pub pack: pack_uniforms::PackInputs,
    /// The world border wall, when near enough to be drawn.
    pub border: Option<BorderDraw>,
    pub game_time: f32,
    /// `GlobalSettingsUniform` GameTime: (game time % 24000 + partial tick) / 24000.
    pub shader_game_time: f32,
    /// Atlas rects (u0, v0, width, height) of end_sky.png and end_portal.png
    /// (`rendertype_end_portal`'s Sampler0 and Sampler1).
    pub end_portal_sprites: [[f32; 4]; 2],
    /// A destroy-stage sprite: projected UVs repeat inside a rect of this size, from the
    /// origin each crack vertex carries (`breaking::stage_light`).
    pub crumbling_sprite: [f32; 4],
    pub ticks: u64,
    /// The darkness effect on the lightmap: the brightness it takes off the
    /// gamma option and the pulsing scale subtracted from the light.
    pub darkness: (f32, f32),
    /// Night vision (or conduit water vision) intensity on the lightmap.
    pub night_vision: f32,
    /// Torch flicker added to the block light factor of 1.4.
    pub block_flicker: f32,
    /// `lightmap.fsh` BossOverlayWorldDarkeningFactor (a wither's bar).
    pub boss_darken: f32,
    /// `Screen.extractBlurredBackground`: blur the frame by this radius
    /// (`menuBackgroundBlurriness`) before the screen is drawn over it.
    pub menu_blur: Option<f32>,
    /// `Options.chunkSectionFadeInTime`, in seconds: how long a section takes to fade in once
    /// uploaded (0 draws it whole at once).
    pub chunk_fade: f32,
    /// `GameRenderer.requestedPostEffects`: the chains that run over the world.
    pub post_effects: crate::post_chain::Request<'a>,
}
/// One frame of the world border wall (`WorldBorderRenderer.render`).
#[derive(Clone, Debug)]
pub struct BorderDraw {
    /// The cached wall buffer; uploaded only when `rebuilt`.
    pub vertices: [crate::border_render::BorderVertex; 16],
    pub rebuilt: bool,
    pub transforms: crate::border_render::BorderTransforms,
    /// Walls to draw (indices of four-vertex quads), nearest first.
    pub sides: Vec<usize>,
}
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct BorderUniform {
    color: [f32; 4],
    offset: [f32; 4],
}
/// A frame to save as a PNG: F2's screenshot, an agent's `game_screenshot`,
/// or the icon the Select World list shows.
pub struct ScreenshotRequest {
    pub path: PathBuf,
    /// `GameRenderer.takeAutoScreenshot`: centre-crop the frame to a square
    /// and average it down to this size. `None` saves the frame as drawn.
    pub size: Option<[u32; 2]>,
    /// F2's: drawn at the game's resolution (Resolution Scale above 100%) times
    /// `Renderer::screenshot_quality`, not the window's.
    pub scaled: bool,
}
/// A solid terrain pipeline without HDR output's lighting (terrain.wgsl's `HDR_LIGHTING`),
/// for the frames HDR is off, and with it where the display offers HDR output: the
/// lighting's code, even never run, slows the shader on Apple GPUs (~15% of the solid
/// pass at Retina sizes).
pub struct SolidPipeline {
    plain: wgpu::RenderPipeline,
    hdr: Option<wgpu::RenderPipeline>,
}

impl SolidPipeline {
    /// The pipeline for a frame with HDR output on (`hdr`) or off.
    pub fn get(&self, hdr: bool) -> &wgpu::RenderPipeline {
        match &self.hdr {
            Some(lit) if hdr => lit,
            _ => &self.plain,
        }
    }
}

pub struct Renderer {
    instance: wgpu::Instance,
    adapter: wgpu::Adapter,
    /// The window's swapchain; `None` when rendering offscreen (`--offscreen`). Shared
    /// with frame generation's present thread while it runs.
    surface: Option<Arc<wgpu::Surface<'static>>>,
    /// The surface `release_surface` set aside for the renderer that replaces
    /// this one on the same device: reused, not created again, as a new
    /// surface for the window can come back lost (Wayland on Mesa).
    spare_surface: std::sync::Mutex<Option<wgpu::Surface<'static>>>,
    /// The offscreen colour target, sized like `config`.
    offscreen: Option<wgpu::Texture>,
    /// Offscreen submissions still on the GPU: at most two frames run ahead.
    in_flight: std::collections::VecDeque<wgpu::SubmissionIndex>,
    /// Splitscreen: the caller waits for the GPU once every player has submitted
    /// (`wait_in_flight`), so one player's work overlaps the GPU drawing another's.
    pub paced_elsewhere: bool,
    /// Which steps of the first frame on this device still go to the log
    /// (see `startup_step`).
    first_frame: FirstFrame,
    gpu_timer: Option<timing::GpuTimer>,
    /// GPU frame times (ms) read back since the caller last drained them.
    pub gpu_times: Vec<f32>,
    /// The last lightmap written, RGBA rows of sky level by block level
    /// (F3+4 shows it).
    pub lightmap_pixels: [u8; 16 * 16 * 4],
    /// Time the last `render` spent blocked on the GPU or the display
    /// (acquiring, presenting, offscreen pacing, the submit thread holding the queue),
    /// excluded from CPU time.
    pub last_wait: std::time::Duration,
    /// CPU time of the last `render`, waits excluded.
    pub last_encode: std::time::Duration,
    /// Watched only while HDR is On or Auto, including Auto in SDR;
    /// before `window`, so it's dropped first.
    display: Option<DisplayWatch>,
    /// What this renderer's display last reported (`DisplayWatch`), its own: another
    /// renderer's watch never changes it.
    reading: output::DisplayReading,
    /// The watch was just made: the settings' Auto values take what it reported.
    display_fresh: bool,
    window: Arc<Window>,
    pub device: wgpu::Device,
    /// The shader modules, layouts and pipelines kept for the next renderer on
    /// this device ([`reuse`]).
    reuse: Arc<reuse::Reuse>,
    /// Frames are finished and submitted on its submit thread ([`submit`]).
    pub queue: submit::Queue,
    /// The last frame went to the submit thread; whether it had no window surface.
    /// The next frame takes it in ([`Self::land_frame`]).
    deferred: Option<bool>,
    /// Set by the device's lost callback. wgpu reports nothing else when an
    /// operation fails on a lost device (no error scope sees it).
    device_lost: Arc<std::sync::atomic::AtomicBool>,
    pub config: wgpu::SurfaceConfiguration,
    solid: SolidPipeline,
    /// `solid` split in two passes for the world pass (terrain.wgsl's `opaque_quad`).
    solid_opaque: SolidPipeline,
    solid_cutout: SolidPipeline,
    translucent: wgpu::RenderPipeline,
    translucent_mark: wgpu::RenderPipeline,
    /// A native mod's world warp (`world_warp`): nothing made until a mod gives one.
    world_warps: world_warp::WorldWarps,
    /// Flat section draws (`flat`), where multi-draw is emulated.
    flat: Option<flat::Flat>,
    /// Sections upload their solid quads in the compact layout (`mesher::packed`): the solid
    /// passes read the arena themselves (`flat`) and no shader pack reads it as vertices.
    packed_quads: bool,
    /// The visibility buffer for flat frames' solid terrain (`visbuffer`,
    /// `HORIZON_VISBUFFER=1`).
    visbuf: Option<visbuffer::Visbuffer>,
    /// `flat` draws this frame's sections (off: the per-section draws).
    flat_on: bool,
    /// Occlusion culling of solid section draws, where the GPU's instance
    /// counts reach a multi-draw's entries (Vulkan, Metal).
    cull: Option<cull::Cull>,
    cull_on: bool,
    /// Solid quads facing away from the eye are left out (`mesher::Sides`).
    sides_on: bool,
    /// The near solid sections culled and drawn on the GPU (`gpu_culling`): its pass,
    /// records and fades while it is on, where it is offered (`gpu_culling_offer`, mirrored
    /// for the settings in `graphics::gpu_culling_offer`), and whether the last frame drew
    /// them so (`gpu_frame`).
    gpu_culling: Option<gpu_culling::GpuCulling>,
    gpu_culling_offer: crate::graphics::GpuCullingOffer,
    gpu_frame: bool,
    /// GPU frames since GPU culling was switched on: the first and every 64th after it
    /// check its records (`check_gpu_records`).
    #[cfg(debug_assertions)]
    gpu_checks: u32,
    sky: wgpu::RenderPipeline,
    /// The main sky without the sunrise fan, which `sunrise_pipeline` draws as
    /// geometry over it. The mirror keeps `sky`: its per-pixel fan is cheap at
    /// the reflection's size and needs no second projection.
    sky_base: wgpu::RenderPipeline,
    sunrise_pipeline: wgpu::RenderPipeline,
    sunrise_buffer: wgpu::Buffer,
    dark_disc: wgpu::RenderPipeline,
    /// Clears the world pass's depth to the far plane so the first-person hand draws in the
    /// same pass (`hand_in_world`), as the hand's own pass would clear it.
    depth_reset: wgpu::RenderPipeline,
    lines: wgpu::RenderPipeline,
    lines_secondary: wgpu::RenderPipeline,
    /// A resource pack's `core/rendertype_lines`, which draws the block outline in their place.
    core_lines: Option<core_lines::CoreLines>,
    /// The packs' `core/sky` and `core/stars`, drawn in place of the native sky and stars.
    core_sky: Option<core_sky::CoreSky>,
    /// Gizmo lines: opaque ones (`RenderTypes.lines`, which writes depth),
    /// translucent ones (`RenderTypes.linesTranslucentNoDepthWrite`) and the
    /// always-on-top ones.
    debug_lines_opaque: wgpu::RenderPipeline,
    debug_lines: wgpu::RenderPipeline,
    debug_lines_on_top: wgpu::RenderPipeline,
    debug_crosshair: Option<wgpu::RenderPipeline>,
    /// The debug crosshair and the first-person hand in `overlay_format`, where it isn't the
    /// frame's.
    overlay_crosshair: Option<wgpu::RenderPipeline>,
    overlay_hand: Option<wgpu::RenderPipeline>,
    debug_line_buffer: wgpu::Buffer,
    debug_line_capacity: u64,
    /// The terrain resources with the debug crosshair's projection
    /// (`crosshair_globals`).
    crosshair_group: wgpu::BindGroup,
    crosshair_globals: wgpu::Buffer,
    /// The water mirror: its solid terrain pipeline, its camera's globals and their group,
    /// and its targets once drawn.
    solid_mirror: wgpu::RenderPipeline,
    mirror_globals: wgpu::Buffer,
    mirror_group: wgpu::BindGroup,
    mirror: Option<MirrorTarget>,
    entity_pipeline: wgpu::RenderPipeline,
    static_entity_pipeline: wgpu::RenderPipeline,
    static_entity_group: wgpu::BindGroup,
    static_entity_transform: wgpu::Buffer,
    static_entity_camera: Option<DVec3>,
    static_entity_catalog_revision: u64,
    static_entity_frame_stats: StaticCacheFrameStats,
    hdr_mark_pipeline: wgpu::RenderPipeline,
    /// `hdr_mark_pipeline` at `entity_pipeline`'s depth bias, for what that drew.
    hdr_entity_mark_pipeline: wgpu::RenderPipeline,
    hdr_hand_unmark_pipeline: wgpu::RenderPipeline,
    hdr_mark_lightning_pipeline: wgpu::RenderPipeline,
    weather: weather::Weather,
    crumbling_pipeline: wgpu::RenderPipeline,
    shadow_blob_pipeline: wgpu::RenderPipeline,
    translucent_item_pipeline: wgpu::RenderPipeline,
    /// `RenderPipelines.WATER_MASK`: depth writes only, no colour.
    water_mask_pipeline: wgpu::RenderPipeline,
    cloud_pipeline: wgpu::RenderPipeline,
    hdr_cloud_mark_pipeline: wgpu::RenderPipeline,
    cloud_mesh: crate::clouds::Mesh,
    cloud_buffer: wgpu::Buffer,
    cloud_capacity: u64,
    cloud_uniform: wgpu::Buffer,
    cloud_group: wgpu::BindGroup,
    additive_pipeline: wgpu::RenderPipeline,
    dragon_rays_pipeline: wgpu::RenderPipeline,
    /// `RenderPipelines.ENERGY_SWIRL`: additive, unlit, the sprite repeated within itself.
    energy_swirl_pipeline: wgpu::RenderPipeline,
    lightning_pipeline: wgpu::RenderPipeline,
    lightning: lightning::Renderer,
    see_through_pipeline: wgpu::RenderPipeline,
    /// Native mods' world primitives (`Frame::world_primitives`).
    world_primitives: world_primitives::WorldPrimitives,
    text_polygon_offset_pipeline: wgpu::RenderPipeline,
    world_text_pipeline: wgpu::RenderPipeline,
    fluid_pipeline: wgpu::RenderPipeline,
    terrain_model_pipelines: [wgpu::RenderPipeline; 3],
    celestial_pipeline: wgpu::RenderPipeline,
    celestial_buffer: wgpu::Buffer,
    end_sky: end_sky::EndSky,
    stars: stars::Stars,
    /// OptiFine custom sky layers (`crate::custom_sky`), and the frame's from the game.
    custom_sky: custom_sky::CustomSky,
    custom_sky_draws: Vec<crate::custom_sky::Draw>,
    /// The glowing entities' outline (`LevelRenderer.entityOutlineTarget`).
    entity_outline: entity_outline::EntityOutline,
    panorama: panorama::Panorama,
    /// `RenderPipelines.WORLD_BORDER` and its buffers.
    border_pipeline: wgpu::RenderPipeline,
    border_group: wgpu::BindGroup,
    border_uniform: wgpu::Buffer,
    border_vertices: wgpu::Buffer,
    /// Screen effects: alpha-blended, then vignette (`dst * (1 - src)`)
    /// pipelines, each for every sampling (index `blend * 4 + sampling`).
    screen_pipelines: [wgpu::RenderPipeline; 16],
    screen_buffer: wgpu::Buffer,
    screen_capacity: u64,
    entity_group: wgpu::BindGroup,
    /// The layout of `entity_group`, for the native shadow pass.
    entity_bindings: wgpu::BindGroupLayout,
    /// The entity resources with the hand's projection (`hand_globals`).
    hand_group: wgpu::BindGroup,
    hand_globals: wgpu::Buffer,
    entity_buffer: wgpu::Buffer,
    entity_capacity: u64,
    static_entity_sections: HashMap<[i32; 3], GpuStaticEntitySection>,
    static_entity_indices: wgpu::Buffer,
    static_entity_index_quads: u32,
    entity_batch: EntityBatch,
    /// Ordinary entity models posed on the GPU; `None` without compute shaders.
    entity_skin: Option<entity_skin::EntitySkin>,
    group: wgpu::BindGroup,
    fluid_group: wgpu::BindGroup,
    fluid_sampler: wgpu::Sampler,
    terrain_model_group: wgpu::BindGroup,
    terrain_model_sampler: wgpu::Sampler,
    globals: wgpu::Buffer,
    /// Resource-pack normal/specular maps, split like the colour sprite arrays.
    pbr_maps: Option<Vec<[wgpu::Texture; 2]>>,
    lightmap: wgpu::Texture,
    /// The block sprites, `sprite_split` layers per array (the first is
    /// `sprites` in terrain.wgsl, the others `sprites_1`...).
    sprite_textures: Vec<wgpu::Texture>,
    /// Block sprite layers per array on this device ([`layer_split`]).
    sprite_split: u32,
    /// A 1-layer array bound where a pack needs fewer arrays.
    sprite_unused: wgpu::TextureView,
    /// The sprite arrays as the terrain samples them (`SPRITE_ARRAYS` views, `sprite_unused`
    /// past the ones in use, the Mipmap Levels option's levels), for native mods
    /// (`WorldDrawn::block_sprites`).
    sprite_views: Vec<wgpu::TextureView>,
    /// Nearest, repeating, nearest mip level: for drawing a sprite's texels sharp elsewhere.
    sprite_nearest: wgpu::Sampler,
    mip_levels: u32,
    /// The mipmap levels sampled (the Mipmap Levels option), at most `mip_levels - 1`.
    mipmaps: u32,
    terrain_layout: wgpu::BindGroupLayout,
    /// terrain.wgsl's `SPRITE_ARRAYS` and `SPRITE_SPLIT`, for a shader pack's terrain.
    terrain_sprites: [f64; 2],
    sprite_sampler: wgpu::Sampler,
    /// `Options.textureFiltering`: 0 none, 1 RGSS, 2 anisotropic.
    texture_filtering: u32,
    /// The anisotropic sampler's clamp (`Options.maxAnisotropyValue`); 1 unless anisotropic.
    max_anisotropy: u16,
    lightmap_view: wgpu::TextureView,
    lightmap_sampler: wgpu::Sampler,
    animations: animate::SpriteAnimator,
    material_animations: Vec<SpriteAnimation>,
    active_animated_layers: Vec<u64>,
    uploads: upload::Uploads,
    /// Section meshes are staged through their own belt into
    /// `section_copies`, which the next frame records its commands after:
    /// `Queue::write_buffer` would make a staging buffer per write.
    section_uploads: upload::Uploads,
    section_copies: Option<wgpu::CommandEncoder>,
    /// Bytes staged in `section_copies`.
    section_copy_bytes: u64,
    /// When a large translucent page may start to shrink (`shrink_translucent`).
    shrink_gate: ShrinkGate,
    /// Whether `lightmap` holds `lightmap_pixels` yet.
    lightmap_uploaded: bool,
    sprite_minimum_size: u32,
    /// Each sprite layer's own side, before the arrays scaled it up (`mipmap::Settings::native_size`).
    sprite_native: Vec<u32>,
    /// The Mipmap Levels option (`set_mipmap_levels`), which sets a shader pack atlas's padding.
    mipmap_option: u32,
    indices: wgpu::Buffer,
    index_quads: u32,
    instances: wgpu::Buffer,
    instance_capacity: u64,
    outline: wgpu::Buffer,
    depth: wgpu::TextureView,
    sections: resident::Resident<SectionGpu>,
    lods: FxHashMap<crate::lod::TileKey, lod::Tile>,
    /// Far tiles' occlusion culling slots (`lod::Tile::cull_slot`): those
    /// free, and the next never used.
    lod_cull_free: Vec<u32>,
    lod_cull_next: u32,
    /// The chunk columns the resident tiles leave out (`Lod::take_cutout`):
    /// their sections are drawn even past the render distance, so terrain
    /// leaving it never opens a hole before the tiles fill it in.
    lod_cutout: crate::fast_hash::FxHashSet<(i32, i32)>,
    /// Removed sections of `lod_cutout` columns, released once tiles cover them.
    lod_retiring: crate::fast_hash::FxHashSet<SectionKey>,
    /// Removed sections fading out over far terrain (`leave_section`).
    lod_leaving: Vec<(SectionKey, std::time::Instant)>,
    /// A mod's hold on the terrain's picture (`hold_world`, gfx/hold.rs) and the new meshes
    /// waiting behind it.
    hold: hold::Hold<HeldMesh>,
    /// Finished tile meshes `upload_lods` has yet to upload.
    lod_pending: Option<lod::Pending>,
    detail_columns: FxHashMap<(i32, i32), SectionMask>,
    /// Bumped whenever a column becomes or stops being complete.
    detail_revision: u64,
    arena: arena::Arena,
    /// Translucent quads' own arena, with large pages, while the layout is split
    /// (`translucent_split`): the back-to-front list then stays in one page (and
    /// one sorted-index page) and draws in a few multi-draws rather than one per
    /// few sections.
    translucent_arena: Option<arena::Arena>,
    /// Translucent Batching (`Graphics::translucent_batching`, read when a renderer
    /// is made), where it is offered (`graphics::offer_translucent_batching`).
    translucent_batching: bool,
    /// Every section and far tile was dropped to change the layout since the game
    /// last asked (`take_dropped_sections`).
    sections_dropped: bool,
    sorted_indices: arena::Arena,
    /// Buffers of released arena pages, freed a few frames later on a thread of their own.
    graveyard: arena::Graveyard<wgpu::Buffer>,
    translucent_sorter: translucency::Sorter,
    translucency_cursor: usize,
    /// `DrawIndexedIndirectArgs` for this frame's solid then translucent draws.
    indirect: wgpu::Buffer,
    indirect_capacity: u64,
    /// Indirect draws may set `first_instance` (the section origin).
    pub multi_draw: bool,
    /// Per-frame scratch, kept to avoid allocating every frame.
    visible: Vec<resident::Visible>,
    visible_lods: Vec<lod::Visible>,
    /// The sections a shader pack's shadow camera sees (`pack_post::ShadowView`).
    shadow_visible: Vec<resident::Visible>,
    translucent_draws: Vec<(u64, usize, [u32; 5], (u32, Option<u32>))>,
    origins: Vec<[f32; 4]>,
    /// `DrawIndexedIndirectArgs` words: index count, instance count, first
    /// index, base vertex, first instance.
    draws: Vec<[u32; 5]>,
    /// Vertex arena page and optional sorted index arena page of each draw:
    /// with sorted indices (a translucent draw), a page of `translucent_arena`
    /// when there is one.
    draw_pages: Vec<(u32, Option<u32>)>,
    /// Solid draws left to occlusion culling's phase 2, while the list is
    /// built, and their pages.
    phase2_draws: Vec<[u32; 5]>,
    phase2_pages: Vec<(u32, Option<u32>)>,
    /// Each section's solid draws, to group them by arena page.
    solid_runs: page_runs::PageRuns,
    phase2_runs: page_runs::PageRuns,
    /// The shadow camera's draws by page, in bands of distance from the light.
    shadow_runs: page_runs::PageRuns,
    /// Block-entity blocks per meshed section (world position, state id).
    block_entities: FxHashMap<SectionKey, Vec<([i32; 3], u32)>>,
    /// Changes on every mesh upload for that section; block-entity static
    /// geometry also depends on the section's packed light.
    block_entity_section_revisions: FxHashMap<SectionKey, u64>,
    /// Changes only when the block-entity positions/states in a section
    /// change. Empty-section tombstones remain until that section unloads so
    /// the static cache can remove deleted models without a global flush.
    block_entity_membership_revisions: FxHashMap<SectionKey, u64>,
    /// When each section was first uploaded (`RenderSection.uploadedTime`): kept across meshing
    /// it again, gone with its column. Sections that fade in over far terrain are not in it.
    section_uploaded: FxHashMap<SectionKey, std::time::Instant>,
    /// When `section_uploaded` last gained a section: once the fade time has passed since,
    /// every section is whole and frames skip the per-section lookups.
    newest_upload: Option<std::time::Instant>,
    /// When a section last began to fade in over far terrain (`SectionGpu::fading`), for the
    /// same shortcut.
    newest_lod_fade: Option<std::time::Instant>,
    /// Changes whenever `block_entities` does (unique across renderers).
    block_entity_revision: u64,
    /// The light sources of each section and their revision, for `block_light`.
    light_blocks: FxHashMap<SectionKey, Vec<([i32; 3], u32)>>,
    light_revision: u64,
    /// VisGraph face connectivity per meshed section (cave culling).
    occlusion: occlusion::Occlusion,
    /// The column `occlusion`, `sections` and `detail_columns` count in: the
    /// level's (`clear_sections`).
    layout: crate::chunk::ColumnLayout,
    /// Resolution scale, FXAA and TAA (Horizon settings).
    world_post: world_post::Post,
    /// Frame generation (Horizon setting).
    frame_gen: frame_gen::FrameGen,
    /// The entities' texture atlas (`entity_group`'s), for frame generation's motion.
    entity_atlas: wgpu::TextureView,
    /// HDR output (Horizon setting): `Some` while the swapchain is HDR.
    hdr: Option<hdr::Hdr>,
    /// Which pixels of each block sprite may shine in HDR output (terrain.wgsl's
    /// `emission_thresholds`), measured when HDR output turns on.
    emission_stats: hdr_stats::EmissionStats,
    /// What this renderer's destination offers and its device draws.
    output_caps: output::OutputCapabilities,
    /// An output that failed to apply, not tried again until something it needs changes.
    output_rejected: Option<output_apply::Rejected>,
    /// The output applied (`update_output`), with the frame every pipeline that draws it
    /// targets: the swapchain's 8-bit format, or `Rgba16Unorm` when the renderer was made with
    /// HDR output on and Smooth Gradients and the device draws and blends into it, or
    /// `LINEAR_FRAME` (`frame_format`). A 16-bit frame with HDR off since reaches the
    /// swapchain through `readback`.
    output: output::EffectiveOutput,
    /// Whether a pack picked or dropped since the renderer was made has been told it runs on
    /// the frame that exists (`set_native_shader_pack`).
    frame_kept_logged: bool,
    /// The marker file of an HDR output that has not presented a frame yet
    /// (`graphics::HDR_MARKER`).
    hdr_marker: Option<std::path::PathBuf>,
    /// The window's graphics driver (`hdr_start`); `None` offscreen.
    hdr_driver: Option<String>,
    /// The HDR Lighting setting, kept for the HDR output made when HDR turns on.
    hdr_lighting_setting: bool,
    /// The shader pack's post-processing, and the dimension (legacy id) it was loaded for.
    pack_post: Option<(i32, Option<pack_post::PackPost>)>,
    /// A `shaders/` pack whose pipelines build on a thread of their own (`load_shader_pack`).
    pack_loading: Option<PackLoading>,
    /// The post passes of one of Horizon's own shader packs; never beside `pack_post`'s.
    native: Option<native_pack::NativePack>,
    /// Time the shader pack's passes on the GPU (F3, or `HORIZON_SHADER_PACK_TIMING`).
    pass_timing: bool,
    /// Least time between timed frames (`set_pass_timing_gap`).
    pass_timing_gap: std::time::Duration,
    /// Times Horizon's own passes on the GPU, as `pass_timing` does (`own_pass_times`).
    own_timing: Option<pack_timing::PassTiming>,
    /// The entity vertices Horizon draws itself while a shader pack draws the others.
    pack_entity_rest: Vec<Vertex>,
    /// Set 1 of the cloud pipeline, for a shader pack's `gbuffers_clouds`.
    cloud_bindings: wgpu::BindGroupLayout,
    /// Rain and snow handed to a shader pack's `gbuffers_weather`, once a pack draws them.
    pack_weather: Option<pack_weather::PackWeather>,
    /// The block sprites as one atlas, while a shader pack is loaded.
    pack_atlas: Option<pack_atlas::PackAtlas>,
    pack_material_atlases: Option<[pack_atlas::PackAtlas; 2]>,
    /// The shared atlas's LabPBR normal and specular paged maps (`set_shared_material_maps`),
    /// which shader packs read for entities, items and the hand.
    shared_material_maps: [Option<wgpu::Texture>; 2],
    /// The pack's terrain program reads flat draws' vertices (`pack_terrain_inputs`).
    pack_terrain_flat: bool,
    /// OptiFine/Iris separate ambient occlusion: pass vertex alpha to hz_Color.a.
    pub separate_ao: bool,
    /// Shader-pack terrain pages used by post processing, retained between frames.
    pack_pages: Vec<pack_post::TerrainPage>,
    /// Camera-relative selection outline vertices, retained between frames.
    outline_edges_scratch: Vec<[f32; 6]>,
    /// Selection outline line vertices, retained between frames.
    outline_lines_scratch: Vec<[f32; 8]>,
    /// Debug line vertices, retained between frames.
    debug_lines_scratch: Vec<[f32; 8]>,
    /// Screen effect vertices, retained between frames.
    screen_vertices_scratch: Vec<crate::screen_effects::ScreenVertex>,
    /// LOD leaving chunk columns, retained between frames.
    lod_leaving_scratch: crate::fast_hash::FxHashSet<(i32, i32)>,
    /// Shadow visible boxed sections, retained between frames.
    shadow_boxed_scratch: Vec<resident::Visible>,
    /// Shadow visible seen section keys, retained between frames.
    shadow_seen_scratch: crate::fast_hash::FxHashSet<SectionKey>,
    /// Shadow translucent section draws, retained between frames.
    shadow_translucent_scratch: Vec<(u64, [u32; 5], (u32, Option<u32>))>,
    /// Lava's sprite layers, which a pack's solid terrain draws (`pack_atlas::terrain_parts`).
    pack_lava_layers: Vec<u16>,
    pub hud: HudRenderer,
    /// Vanilla's menu background blur, made on first use.
    menu_blur: Option<crate::menu_blur::MenuBlur>,
    /// `post_effect` chains, made on first use.
    post_effects: Option<crate::post_chain::PostEffects>,
    /// Where a screenshot's frame is drawn when the swapchain cannot be read back.
    readback: readback::Readback,
    pub info: wgpu::AdapterInfo,
    pub suspended: bool,
    /// Consecutive frames whose surface acquisition failed validation. The
    /// first gets the swapchain a fresh configuration; later ones reattach
    /// the window, as a lost surface does.
    acquire_validation_failures: u32,
    /// A surface from `recreate_surface` whose configuration failed. wgpu
    /// panics when such a surface is acquired from (it never had one that
    /// worked), so frames configure it again first and skip until it takes.
    surface_unconfigured: bool,
    /// The last frame's statistics, all of one frame (`show_stats`): its sections drawn
    /// (F3's `C:`, the world icon's check), section draws (face ranges and translucent
    /// sections) and the draw calls the backend issued for them. A CPU frame's are its own;
    /// in GPU frames (`gpu_culling`) they are the newest GPU frame's whose counts came back,
    /// a frame or a few earlier (`stats_lag`).
    pub drawn_sections: usize,
    pub section_draws: usize,
    pub section_draw_calls: usize,
    /// `draw_frame`'s frames past its early returns, and the one whose statistics are shown.
    frame_serial: u64,
    stats_frame: u64,
    pub uploaded_bytes: u64,
    started: Instant,
    /// The next frame to write to a PNG path (control socket, F2, world icon).
    pub screenshot: Option<ScreenshotRequest>,
    /// Outcome of the last screenshot: frame size, or the error.
    pub screenshot_result: Option<std::result::Result<[u32; 2], String>>,
    /// The HDR copy the last screenshot wrote beside it, if it wrote one.
    pub screenshot_hdr: Option<std::path::PathBuf>,
    /// Screenshot Quality (Horizon setting): a `scaled` screenshot's frame is this many
    /// times the game's resolution (1, 2, 4 or 8).
    pub screenshot_quality: u32,
    /// How many times the pending scaled screenshot ran out of video memory: each
    /// halves its size, down to the window's.
    screenshot_halvings: u32,
    /// Brightness option (lightmap gamma), 0..1.
    pub gamma: f32,
    /// Optional full-light lightmap (Horizon Extras).
    pub fullbright: bool,
    /// The Improved transparency option (`GameRenderer.useImprovedTransparency`).
    pub improved_transparency: bool,
    /// Its passes and targets, made when first drawn with.
    oit: Option<oit::Oit>,
    /// Advance animated block textures (Horizon setting; vanilla always does).
    pub animated_textures: bool,
    present_modes: Vec<wgpu::PresentMode>,
    /// CPU copies of the section and far tile meshes uploaded while `render-capture` is on
    /// (`keep_capture_copies`, or `HORIZON_RENDER_CAPTURE` at creation); `None` otherwise.
    capture_copies: Option<capture::Copies>,
    /// A scene to write from the next frame (`horizon-ctl render-capture DIR`).
    pub capture: Option<capture::Request>,
    /// Outcome of the last capture: its summary, or the error.
    pub capture_result: Option<std::result::Result<serde_json::Value, String>>,
}
/// Every layer's mip levels, layer major (`first` is the first layer's
/// index into `settings`), made by `MipmapGenerator`.
/// Block sprite layers per texture array at most: the
/// `max_texture_array_layers` of most GPUs. Mods add sprites past it into more
/// arrays (terrain.wgsl's `sprite`); a GPU with fewer layers per array gets a
/// smaller split ([`layer_split`]).
pub(crate) const SPRITE_SPLIT: u32 = 2048;
/// A smaller split from `HORIZON_SPRITE_SPLIT` (dev only: vanilla's sprites
/// drawn from several arrays must look the same).
fn requested_split() -> Option<u32> {
    std::env::var("HORIZON_SPRITE_SPLIT")
        .ok()
        .and_then(|v| v.parse().ok())
        .filter(|&n| (1..=SPRITE_SPLIT).contains(&n))
}
fn adapter_requested_split(info: &wgpu::AdapterInfo, requested: Option<u32>) -> Option<u32> {
    // Metal can fail to sample large mipmapped array textures despite advertising the layer limit.
    requested.or_else(|| (info.backend == wgpu::Backend::Metal).then_some(512))
}
/// The split the CPU mip chains are cut by before the adapter is known.
fn sprite_split() -> u32 {
    requested_split().unwrap_or(SPRITE_SPLIT)
}
/// The block sprite layers per texture array on an adapter that holds
/// `max_layers` layers per array: [`SPRITE_SPLIT`], `requested`
/// (`HORIZON_SPRITE_SPLIT`) or `max_layers`, whichever is smallest, with a full
/// array still fitting once [`array_layers`] pads it for OpenGL. Errors when
/// `layers` sprites need more than the [`SPRITE_ARRAYS`] arrays terrain.wgsl binds.
fn layer_split(
    max_layers: u32,
    gl: bool,
    requested: Option<u32>,
    layers: u32,
) -> std::result::Result<u32, String> {
    let mut split = requested.unwrap_or(SPRITE_SPLIT).min(max_layers).max(1);
    if split > 1 && array_layers(split, gl) > max_layers {
        split -= 1;
    }
    if layers > split * SPRITE_ARRAYS {
        return Err(format!(
            "GPU supports {max_layers} texture layers per array; {layers} block textures need \
             more than the {SPRITE_ARRAYS} arrays of {split} Horizon draws from"
        ));
    }
    Ok(split)
}
/// Sprite arrays terrain.wgsl binds (`sprites`, `sprites_1`...).
const SPRITE_ARRAYS: u32 = 8;
/// The layers to give a square texture of `count` layers viewed as a 2D
/// array. wgpu's OpenGL backend picks a texture's target from its layer
/// count when it is created: one layer makes a 2D texture and a multiple of
/// six a cube map, neither of which can be viewed as an array.
pub(crate) fn array_layers(count: u32, gl: bool) -> u32 {
    match count {
        _ if !gl => count,
        0 | 1 => 2,
        n if n % 6 == 0 => n + 1,
        n => n,
    }
}
/// The views of terrain.wgsl's sprite arrays, the unused ones a 1-layer
/// stand-in.
fn sprite_views(
    textures: &[wgpu::Texture],
    unused: &wgpu::TextureView,
    mips: Option<u32>,
) -> Vec<wgpu::TextureView> {
    (0..SPRITE_ARRAYS as usize)
        .map(|i| match textures.get(i) {
            Some(t) => t.create_view(&wgpu::TextureViewDescriptor {
                dimension: Some(wgpu::TextureViewDimension::D2Array),
                mip_level_count: mips,
                ..Default::default()
            }),
            None => unused.clone(),
        })
        .collect()
}
/// The block sprites sampled sharp, for native mods (`WorldDrawn::block_sprites`): nearest,
/// repeating, the nearest mip level.
const SPRITE_NEAREST: wgpu::SamplerDescriptor<'static> = wgpu::SamplerDescriptor {
    label: Some("Block sprites (nearest)"),
    address_mode_u: wgpu::AddressMode::Repeat,
    address_mode_v: wgpu::AddressMode::Repeat,
    address_mode_w: wgpu::AddressMode::ClampToEdge,
    mag_filter: wgpu::FilterMode::Nearest,
    min_filter: wgpu::FilterMode::Nearest,
    mipmap_filter: wgpu::MipmapFilterMode::Nearest,
    lod_min_clamp: 0.0,
    lod_max_clamp: 32.0,
    compare: None,
    anisotropy_clamp: 1,
    border_color: None,
};
/// The terrain sprites' sampler: LevelRenderer.chunkLayerSampler's linear one (clamped to the
/// edge) in every Texture Filtering mode, with `anisotropy` samples when it is above 1 (wgpu
/// then needs a linear mipmap filter too).
fn sprite_sampler_descriptor(anisotropy: u16) -> wgpu::SamplerDescriptor<'static> {
    wgpu::SamplerDescriptor {
        label: Some("Block sprites (filtered)"),
        address_mode_u: wgpu::AddressMode::ClampToEdge,
        address_mode_v: wgpu::AddressMode::ClampToEdge,
        mag_filter: wgpu::FilterMode::Linear,
        min_filter: wgpu::FilterMode::Linear,
        mipmap_filter: wgpu::MipmapFilterMode::Linear,
        anisotropy_clamp: anisotropy,
        ..Default::default()
    }
}
/// `Options.maxAnisotropyValue` for a `maxAnisotropyBit`: `1 << bit`, at most wgpu's 16.
fn sampler_anisotropy(bit: i32) -> u16 {
    1u16 << bit.clamp(0, 4)
}
/// terrain.wgsl's group 0.
#[allow(clippy::too_many_arguments)]
fn terrain_group(
    device: &wgpu::Device,
    label: &str,
    layout: &wgpu::BindGroupLayout,
    globals: &wgpu::Buffer,
    sprites: &[wgpu::TextureView],
    sprite_sampler: &wgpu::Sampler,
    lightmap: &wgpu::TextureView,
    lightmap_sampler: &wgpu::Sampler,
    emission_thresholds: &wgpu::TextureView,
) -> wgpu::BindGroup {
    let view = |binding, view| wgpu::BindGroupEntry {
        binding,
        resource: wgpu::BindingResource::TextureView(view),
    };
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some(label),
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: globals.as_entire_binding(),
            },
            view(1, &sprites[0]),
            wgpu::BindGroupEntry {
                binding: 2,
                resource: wgpu::BindingResource::Sampler(sprite_sampler),
            },
            view(3, lightmap),
            wgpu::BindGroupEntry {
                binding: 4,
                resource: wgpu::BindingResource::Sampler(lightmap_sampler),
            },
            view(5, &sprites[1]),
            view(6, &sprites[2]),
            view(7, &sprites[3]),
            view(8, &sprites[4]),
            view(9, &sprites[5]),
            view(10, &sprites[6]),
            view(11, &sprites[7]),
            view(12, emission_thresholds),
        ],
    })
}
/// CPU texture uploads prepared by the resource worker. Consumed by renderer
/// construction so the temporary mip chains are released after GPU upload.
pub struct PreparedTextures {
    sprites: Vec<(Vec<u8>, u32)>,
    /// The sprite layers per array `sprites` is cut by.
    split: u32,
    forcefield: (Vec<u8>, u32),
    /// The LabPBR maps' mip chains (normals, specular), their levels, layers and size.
    pbr: Option<(Vec<u8>, Vec<u8>, u32, u32, u32)>,
}
/// Every level of `layers` square RGBA layers of `size`, each the mean of four texels of the
/// one above, layer major: the LabPBR maps' mips (their channels are data, not colour).
fn box_mip_chain(pixels: &[u8], size: u32, layers: u32) -> Vec<u8> {
    let layer_bytes = (size * size * 4) as usize;
    let mut out = Vec::new();
    for layer in pixels.chunks_exact(layer_bytes).take(layers as usize) {
        let mut level = layer.to_vec();
        let mut side = size as usize;
        out.extend_from_slice(&level);
        while side > 1 {
            let half = side / 2;
            let mut next = Vec::with_capacity(half * half * 4);
            for y in 0..half {
                for x in 0..half {
                    for c in 0..4 {
                        let at = |dx: usize, dy: usize| {
                            u32::from(level[((2 * y + dy) * side + 2 * x + dx) * 4 + c])
                        };
                        next.push(((at(0, 0) + at(1, 0) + at(0, 1) + at(1, 1) + 2) / 4) as u8);
                    }
                }
            }
            out.extend_from_slice(&next);
            level = next;
            side = half;
        }
    }
    out
}
impl PreparedTextures {
    pub fn new(
        pixels: &[u8],
        size: u32,
        layers: u32,
        settings: &[crate::mipmap::Settings],
        forcefield: (&[u8], u32),
    ) -> Self {
        Self::with_split(pixels, size, layers, settings, forcefield, sprite_split())
    }

    fn with_split(
        pixels: &[u8],
        size: u32,
        layers: u32,
        settings: &[crate::mipmap::Settings],
        forcefield: (&[u8], u32),
        split: u32,
    ) -> Self {
        let layer_bytes = (size * size * 4) as usize;
        Self {
            sprites: (0..layers.div_ceil(split).max(1))
                .map(|array| {
                    let first = array * split;
                    let count = layers.saturating_sub(first).clamp(1, split);
                    let start = (first as usize * layer_bytes).min(pixels.len());
                    mip_chain(&pixels[start..], size, settings, first, count)
                })
                .collect(),
            split,
            forcefield: mip_chain(forcefield.0, forcefield.1, &[], 0, 1),
            pbr: None,
        }
    }

    /// With the resource packs' LabPBR `normals` and `specular` maps (`layers` layers of
    /// `size`, as `RenderAssets` holds them; nothing when empty).
    pub fn with_pbr(mut self, normals: &[u8], specular: &[u8], size: u32, layers: u32) -> Self {
        if !normals.is_empty() {
            self.pbr = Some((
                box_mip_chain(normals, size, layers),
                box_mip_chain(specular, size, layers),
                size.trailing_zeros() + 1,
                layers,
                size,
            ));
        }
        self
    }

    /// The same mip chains cut by `split` layers per array: chains made before
    /// the adapter was known, for a GPU with fewer layers per array.
    fn resplit(mut self, layers: u32, split: u32) -> Self {
        if self.split == split || layers <= split.min(self.split) {
            self.split = split;
            return self;
        }
        let levels = self.sprites[0].1;
        let all: Vec<u8> = self
            .sprites
            .drain(..)
            .flat_map(|(bytes, _)| bytes)
            .collect();
        // Every layer's mip levels are contiguous (layer major).
        let layer_bytes = all.len() / layers as usize;
        self.sprites = all
            .chunks(layer_bytes * split as usize)
            .map(|chunk| (chunk.to_vec(), levels))
            .collect();
        self.split = split;
        self
    }
}

fn mip_chain(
    pixels: &[u8],
    size: u32,
    settings: &[crate::mipmap::Settings],
    first: u32,
    layers: u32,
) -> (Vec<u8>, u32) {
    let levels = size.trailing_zeros() + 1;
    let layer_bytes = (size * size * 4) as usize;
    // Exactly the chain's size: cached animation frames count their capacity.
    let chain_bytes: usize = (0..levels).map(|l| layer_bytes >> (2 * l)).sum();
    let mut out = Vec::with_capacity(chain_bytes * layers as usize);
    // Every layer makes its own mips from its own pixels, and a pack's array holds
    // thousands of them: the layers run on every thread, in chunks so a worker
    // allocates one chain per chunk rather than one per layer. The chunks are
    // appended in order, so the chain stays layer major.
    const LAYERS_PER_CHUNK: u32 = 16;
    let chunks: Vec<(u32, u32)> = (0..layers)
        .step_by(LAYERS_PER_CHUNK as usize)
        .map(|start| (start, (layers - start).min(LAYERS_PER_CHUNK)))
        .collect();
    let parts = crate::par::par_map(&chunks, |&(start, count)| {
        mip_chain_layers(
            &pixels[start as usize * layer_bytes..],
            size,
            settings,
            first + start,
            count,
            levels - 1,
            layer_bytes,
        )
    });
    for part in parts {
        out.extend_from_slice(&part);
    }
    debug_assert_eq!(out.len(), chain_bytes * layers as usize);
    (out, levels)
}

/// The mip chain of `layers` layers from `pixels` that start at layer `first`:
/// `layers`' base levels followed by their reductions, layer major.
fn mip_chain_layers(
    pixels: &[u8],
    size: u32,
    settings: &[crate::mipmap::Settings],
    first: u32,
    layers: u32,
    levels: u32,
    layer_bytes: usize,
) -> Vec<u8> {
    let mut out = Vec::with_capacity(
        (0..=levels).map(|l| layer_bytes >> (2 * l)).sum::<usize>() * layers as usize,
    );
    for layer in 0..layers as usize {
        let base = crate::mipmap::Image {
            width: size,
            height: size,
            pixels: pixels[layer * layer_bytes..(layer + 1) * layer_bytes]
                .as_chunks::<4>()
                .0
                .to_vec(),
        };
        let s = settings
            .get(first as usize + layer)
            .copied()
            .unwrap_or_default();
        let has_transparent = s.has_transparent || base.pixels.iter().any(|p| p[3] == 0);
        for image in crate::mipmap::generate(
            base,
            levels,
            crate::mipmap::Settings {
                has_transparent,
                ..s
            },
        ) {
            out.extend(image.pixels.iter().flatten());
        }
    }
    out
}
/// The wgpu instance options, without wgpu's validation of indirect draws
/// in release builds. That validation (meant for untrusted WebGPU content)
/// copies every indirect draw through a compute pass before the render pass
/// and costs about a tenth of `render`'s CPU time on Metal. The draws it checks
/// are the section draws `render` writes from resident sections, in
/// bounds by construction: index counts within `indices`, vertices within
/// the section's arena range, first instances below the origins written that
/// frame. Debug builds keep validating them; `WGPU_VALIDATION_INDIRECT_CALL=1`
/// turns it back on. DirectX 12 keeps it too: there wgpu-core's validation
/// pass is also what passes each draw's first instance to shaders reading
/// `instance_index`, and DirectX 12 is the fallback, where working matters
/// more than CPU time. GPU culling's count draws (`gpu_culling`) are written on
/// the GPU and never validated, in any build or backend: they are in bounds by
/// construction too (that module's doc), and no shader they run reads
/// `instance_index`.
/// `allow_nonconformant` offers the adapters wgpu hides for reporting no Vulkan
/// conformance version: Mesa's hasvk, which drives Ivy Bridge and Haswell,
/// reports 0.0.0.0 while it works. `open_device` asks for them only on a second
/// try at a backend that had no adapter at all. On DirectX 12, `dx12_compiler`
/// compiles the shaders (`gfx/dx12_compiler.rs`), unless `WGPU_DX12_COMPILER`
/// names another.
fn instance_descriptor(
    backends: wgpu::Backends,
    window: Option<&Arc<Window>>,
    allow_nonconformant: bool,
    dx12_compiler: wgpu::Dx12Compiler,
) -> wgpu::InstanceDescriptor {
    let mut descriptor = wgpu::InstanceDescriptor::new_without_display_handle();
    if let Some(window) = window {
        descriptor = descriptor.with_display_handle(Box::new(WindowDisplay(window.clone())));
    }
    descriptor.backends = backends.with_env();
    let dx12 = cfg!(windows) && descriptor.backends.contains(wgpu::Backends::DX12);
    // FXC spends about 2.6 s on every start compiling the pipelines (DXC about
    // 1 s); what it compiled once is kept in the game folder and read back
    // after that (third_party/wgpu-hal/src/dx12/disk_cache.rs).
    #[cfg(windows)]
    if dx12 {
        wgpu::hal::dx12::disk_cache::set_file(dx12_shader_cache(&crate::logging::game_home()));
    }
    if !cfg!(debug_assertions) && !dx12 {
        descriptor
            .flags
            .remove(wgpu::InstanceFlags::VALIDATION_INDIRECT_CALL);
    }
    descriptor.backend_options.dx12.shader_compiler = dx12_compiler;
    let mut descriptor = descriptor.with_env();
    if allow_nonconformant {
        descriptor
            .flags
            .insert(wgpu::InstanceFlags::ALLOW_UNDERLYING_NONCOMPLIANT_ADAPTER);
    }
    descriptor
}
/// Where DX12 keeps the shaders it compiled, inside a game folder.
pub fn dx12_shader_cache(home: &std::path::Path) -> std::path::PathBuf {
    home.join("cache").join("dx12-shaders.bin")
}

/// The window's display connection, so the OpenGL backend creates its EGL
/// display from the X11 or Wayland connection the window lives on. Without
/// one, wgpu-hal falls back to `EGL_MESA_platform_surfaceless`, whose configs
/// cannot present to a window; the backend then reports the surface as
/// incompatible and the game has no OpenGL to fall back to (`gl not
/// compatible with provided surface`).
#[derive(Debug, Clone)]
struct WindowDisplay(Arc<Window>);
impl wgpu::rwh::HasDisplayHandle for WindowDisplay {
    fn display_handle(
        &self,
    ) -> std::result::Result<wgpu::rwh::DisplayHandle<'_>, wgpu::rwh::HandleError> {
        wgpu::rwh::HasDisplayHandle::display_handle(&*self.0)
    }
}
/// The backends to open a device on, each tried in turn until one gives a
/// device. `api` is the player's choice (`GraphicsApi::with_env` applied);
/// `os` is `std::env::consts::OS`; `arch` the machine's (`horizon_system::machine_arch`), not the
/// build's; `wgpu_backend`: whether `WGPU_BACKEND` is set, which picks the backends itself.
/// Auto tries DirectX 12 first on Windows ARM64, the x64 build emulated there included,
/// then Vulkan, based on the device losses seen there (#7513). Other
/// Windows systems try Vulkan, then DirectX 12, then OpenGL; Linux tries Vulkan then
/// OpenGL. Each is tried when the one before has no surface, adapter or device. A native
/// crash in the driver can't be
/// caught in the game; the launcher switches the setting to DirectX 12 after one at
/// the renderer's start (#746). macOS has Metal only.
fn backend_order(
    api: GraphicsApi,
    os: &str,
    arch: &str,
    wgpu_backend: bool,
) -> Vec<wgpu::Backends> {
    use wgpu::Backends;
    if os == "macos" || wgpu_backend {
        return vec![Backends::default()];
    }
    let windows = os == "windows";
    match api {
        GraphicsApi::Auto if windows && arch == "aarch64" => {
            vec![Backends::DX12, Backends::VULKAN, Backends::GL]
        }
        GraphicsApi::Auto if windows => vec![Backends::VULKAN, Backends::DX12, Backends::GL],
        GraphicsApi::Auto => vec![Backends::VULKAN, Backends::GL],
        GraphicsApi::Dx12 if windows => vec![Backends::DX12],
        GraphicsApi::Vulkan | GraphicsApi::Dx12 => vec![Backends::VULKAN],
        GraphicsApi::OpenGl => vec![Backends::GL],
    }
}
fn backends_name(backends: wgpu::Backends) -> &'static str {
    match backends {
        wgpu::Backends::VULKAN => "Vulkan",
        wgpu::Backends::DX12 => "DirectX 12",
        wgpu::Backends::GL => "OpenGL",
        _ => "graphics",
    }
}
/// Whether an adapter renders on the CPU (WARP, llvmpipe).
fn is_software(kind: wgpu::DeviceType) -> bool {
    kind == wgpu::DeviceType::Cpu
}
/// A device and what it came from.
struct Opened {
    instance: wgpu::Instance,
    surface: Option<wgpu::Surface<'static>>,
    adapter: wgpu::Adapter,
    device: wgpu::Device,
    queue: wgpu::Queue,
    device_lost: Arc<std::sync::atomic::AtomicBool>,
    /// The window surface's configuration, set on `device` during the search, and the
    /// surface's capabilities on `adapter`. None offscreen.
    configured: Option<(wgpu::SurfaceConfiguration, wgpu::SurfaceCapabilities)>,
}
/// The adapter as the renderer found it ([`adapter`]): for the crash report, and for whoever
/// describes the machine (telemetry, through an extension of the game).
#[derive(Clone, Debug, Default)]
pub struct Adapter {
    pub name: String,
    pub vendor: String,
    /// `integrated`, `discrete`, `virtual`, `cpu` or `other`.
    pub kind: String,
    pub driver: String,
    pub backend: String,
}

static ADAPTER: std::sync::Mutex<Option<Adapter>> = std::sync::Mutex::new(None);

/// The adapter, once the renderer has one.
pub fn adapter() -> Option<Adapter> {
    ADAPTER.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

/// wgpu's device type in the words reports use.
pub fn gpu_kind(device_type: &str) -> &'static str {
    match device_type {
        "IntegratedGpu" => "integrated",
        "DiscreteGpu" => "discrete",
        "VirtualGpu" => "virtual",
        "Cpu" => "cpu",
        _ => "other",
    }
}

/// Logs the adapter and records it ([`adapter`]) for crash reports and telemetry.
fn describe_adapter(adapter: &wgpu::Adapter) {
    let info = adapter.get_info();
    log::info!(
        "Horizon renderer: {} ({:?}, {:?})",
        info.name,
        info.backend,
        info.device_type
    );
    *ADAPTER.lock().unwrap_or_else(|e| e.into_inner()) = Some(Adapter {
        name: info.name.clone(),
        vendor: horizon_system::gpu_vendor(&info.name, info.vendor).to_owned(),
        kind: gpu_kind(&format!("{:?}", info.device_type)).to_owned(),
        driver: format!("{} {}", info.driver, info.driver_info)
            .trim()
            .to_owned(),
        backend: format!("{:?}", info.backend),
    });
    crate::crash::set_detail(
        "Graphics Adapter",
        format!(
            "{} ({:?}, {:?}), driver {} {}",
            info.name, info.backend, info.device_type, info.driver, info.driver_info
        ),
    );
}
/// Whether an adapter's texture arrays hold the block textures Horizon puts
/// in one array.
fn check_layers(adapter: &wgpu::Adapter, layers: u32) -> Result<u32> {
    let info = adapter.get_info();
    Ok(layer_split(
        adapter.limits().max_texture_array_layers,
        info.backend == wgpu::Backend::Gl,
        adapter_requested_split(&info, requested_split()),
        layers,
    )?)
}
/// Whether the swapchain can take a window of `width` x `height`: each side from 1 to the
/// device's `max_texture_dimension_2d` (`max`), else `Surface::configure` panics. 0 is a
/// minimized window. Past `max`: Windows lets a program size a window up to 65535 pixels
/// (`SetWindowPos`), a 984x65496 client area, when an RTX 5080 takes 32768. Vanilla's
/// `Window.onFramebufferResize` likewise keeps the last size for one that is not positive.
fn drawable_size(width: u32, height: u32, max: u32) -> bool {
    (1..=max).contains(&width) && (1..=max).contains(&height)
}
/// The swapchain format: a non-sRGB target, because vanilla blends and tints in gamma
/// space, and 8-bit, because wgpu gives `Rgba16Float` the linear `ExtendedSrgb` color
/// space: the compositor would read these gamma-encoded values as linear light and wash
/// every colour out.
fn surface_format(
    formats: &[wgpu::TextureFormat],
    default: wgpu::TextureFormat,
) -> wgpu::TextureFormat {
    [
        wgpu::TextureFormat::Bgra8Unorm,
        wgpu::TextureFormat::Rgba8Unorm,
    ]
    .into_iter()
    .find(|f| formats.contains(f))
    .or_else(|| formats.iter().copied().find(|f| !f.is_srgb()))
    .unwrap_or(default)
}
/// The 16-bit frame HDR output draws into where it can (`Renderer::frame_format`). Unorm
/// rather than float: it clips at white after every pass as the 8-bit frame does, so light
/// added past white (the sun on the sky) and then blended over (glass) looks the same.
const WIDE_FRAME: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Unorm;

/// Whether a device with `features` can draw, blend and filter `WIDE_FRAME`, whose
/// format features (`format`) the adapter reports. Drawing into it is an adapter-specific
/// format feature, beyond the format itself.
fn draws_wide_frame(features: wgpu::Features, format: wgpu::TextureFormatFeatures) -> bool {
    features.contains(
        wgpu::Features::TEXTURE_FORMAT_16BIT_NORM
            | wgpu::Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES,
    ) && draws_frame(format)
}

/// Whether a format with the adapter's `format` features takes every usage and flag the
/// passes take of the frame.
fn draws_frame(format: wgpu::TextureFormatFeatures) -> bool {
    format.allowed_usages.contains(
        wgpu::TextureUsages::RENDER_ATTACHMENT
            | wgpu::TextureUsages::TEXTURE_BINDING
            | wgpu::TextureUsages::COPY_SRC
            | wgpu::TextureUsages::COPY_DST,
    ) && format.flags.contains(
        wgpu::TextureFormatFeatureFlags::BLENDABLE | wgpu::TextureFormatFeatureFlags::FILTERABLE,
    )
}

/// The frame of a renderer made under `HORIZON_HDR_FRAME=float` (docs/design/hdr-linear-light.md)
/// with a shader API 0.2 pack picked, or with HDR on and no pack: float, so values over 1
/// survive from the world to the tonemap and the HDR pass.
const LINEAR_FRAME: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;

/// What a renderer's frame is drawn in: `LINEAR_FRAME` under the switch where the device draws
/// it, for a 0.2 pack, or for the layered HDR frame where the surface offers HDR output
/// (`linear`: `(a 0.2 pack picked, the layered HDR frame, the device draws it)`); else
/// `WIDE_FRAME` when it is made with HDR on and Smooth Gradients (`wide_frame`,
/// `output::resolve_frame`), the surface offering HDR output (`hdr_offered`) and the device
/// drawing it (`draws_wide_frame`): an 8-bit frame's steps band in dark gradients, and HDR
/// shows them. Else the swapchain's 8-bit format, as without HDR.
fn frame_format(
    wide_frame: bool,
    hdr_offered: bool,
    device_draws_it: bool,
    swapchain: wgpu::TextureFormat,
    linear: (bool, bool, bool),
) -> wgpu::TextureFormat {
    if linear.2 && (linear.0 || linear.1 && hdr_offered) {
        LINEAR_FRAME
    } else if wide_frame && hdr_offered && device_draws_it {
        WIDE_FRAME
    } else {
        swapchain.remove_srgb_suffix()
    }
}

/// The frame's format, and whether it is the layered HDR frame (HUD coverage): for HDR on
/// under the switch (`hdr_float`), with no pack or a 0.2 one, where the device keeps the HUD
/// on a layer of its own.
fn frame_configuration(
    wide_frame: bool,
    device: (bool, bool, bool, bool),
    swapchain: wgpu::TextureFormat,
    linear_pack: bool,
    hdr_float: bool,
) -> (wgpu::TextureFormat, bool) {
    let (hdr_offered, wide_ok, linear_ok, coverage_ok) = device;
    let layered = hdr_float && hdr_offered && linear_ok && coverage_ok;
    (
        frame_format(
            wide_frame,
            hdr_offered,
            wide_ok,
            swapchain,
            (linear_pack, layered, linear_ok),
        ),
        layered,
    )
}

/// What the hand, the camera overlays and the HUD draw on: the frame, except for a 0.2 pack's
/// float frame without the HUD's layer, which is SDR: the pack's picture finishes on the
/// swapchain's 8-bit format first, so they clip and blend per draw as on vanilla's frame.
fn overlay_format(
    frame: wgpu::TextureFormat,
    layered: bool,
    swapchain: wgpu::TextureFormat,
) -> wgpu::TextureFormat {
    if frame == LINEAR_FRAME && !layered {
        swapchain.remove_srgb_suffix()
    } else {
        frame
    }
}

/// Whether Horizon's tone curve (`Hdr::tonemap`) runs on this frame: on the layered frame's
/// world, unless a 0.2 pack's `[tonemap]` already mapped it. One tone mapping, never two.
fn horizon_tone_curve(layered: bool, world: bool, pack_tonemap: bool) -> bool {
    layered && world && !pack_tonemap
}

/// Opens a device on the first of `backend_order`'s backends that gives one.
/// Offscreen rendering needs no surface, so it can use a GPU the window's
/// display cannot present from (a headless compositor).
async fn open_device(
    window: &Arc<Window>,
    offscreen: bool,
    api: GraphicsApi,
    layers: u32,
) -> Result<Opened> {
    let wgpu_backend = std::env::var_os("WGPU_BACKEND").is_some();
    let mut first = None;
    for (order, allow_cpu) in device_search(
        api.with_env(),
        std::env::consts::OS,
        // The machine's: the x64 build emulated on a Windows ARM64 PC has its GPU too (#7513).
        horizon_system::machine_arch(),
        wgpu_backend,
    ) {
        match open_device_with(window, offscreen, &order, layers, allow_cpu).await {
            Ok(opened) => {
                if let Some(first) = &first {
                    log::warn!(
                        "Horizon renderer: the chosen graphics API gave no GPU ({first}); {}",
                        if allow_cpu {
                            "rendering on the CPU, which is slow"
                        } else {
                            "using the GPU through another one"
                        }
                    );
                }
                return Ok(opened);
            }
            Err(e) => {
                first.get_or_insert(e);
            }
        }
    }
    Err(first.unwrap_or_else(|| NO_GPU_DRIVER.into()))
}
/// The tries `open_device` makes, in turn: the chosen API's backends (`backend_order`) on a
/// real GPU; when the player chose one API, the other backends `Auto` would try, on a real
/// GPU; and last the chosen backends with a software adapter (WARP, llvmpipe) allowed.
/// Software rendering takes most of a weak CPU even on the title screen: an Intel HD 2500 has
/// DirectX 12 only through WARP and its GPU through OpenGL (#5185), and a Fermi GPU has no
/// Vulkan or DirectX 12 driver but OpenGL (#3978). DirectX 12 never falls back to Vulkan: the
/// launcher chooses it after Vulkan crashed in the driver (#746), which the game can't catch.
fn device_search(
    api: GraphicsApi,
    os: &str,
    arch: &str,
    wgpu_backend: bool,
) -> Vec<(Vec<wgpu::Backends>, bool)> {
    let chosen = backend_order(api, os, arch, wgpu_backend);
    let others: Vec<_> = backend_order(GraphicsApi::Auto, os, arch, wgpu_backend)
        .into_iter()
        .filter(|b| !chosen.contains(b))
        .filter(|&b| !(api == GraphicsApi::Dx12 && b == wgpu::Backends::VULKAN))
        .collect();
    let mut tries = vec![(chosen.clone(), false)];
    if !others.is_empty() {
        tries.push((others, false));
    }
    tries.push((chosen, true));
    tries
}
/// `open_device` on `order`'s backends in turn, taking an adapter of the CPU type
/// (software rendering) only when `allow_cpu`.
async fn open_device_with(
    window: &Arc<Window>,
    offscreen: bool,
    order: &[wgpu::Backends],
    layers: u32,
    allow_cpu: bool,
) -> Result<Opened> {
    let mut failures = Vec::new();
    for (attempt, backends) in order.iter().copied().enumerate() {
        let name = backends_name(backends);
        if attempt > 0 {
            log::warn!("Horizon renderer: trying {name} ({})", failures.join("; "));
        }
        match open_with_retry(backends, window, offscreen, layers, allow_cpu).await {
            Ok(opened) => return Ok(opened),
            Err(e) => failures.push(e),
        }
    }
    Err(format!("{NO_GPU_DRIVER} ({})", failures.join("; ")).into())
}
/// Why a backend gave no device: no adapter at all, which a second try with the
/// adapters wgpu hides can still find, or a failure after an adapter.
enum NoDevice {
    NoAdapter(String),
    Failed(String),
}
/// Opens a device on one backend, and on a backend whose adapters wgpu all hid
/// for reporting no Vulkan conformance version tries it again with them shown:
/// Mesa's hasvk drives Ivy Bridge and Haswell, works, and reports 0.0.0.0, so
/// without this the player is told they have no graphics driver while Vulkan
/// runs (a `gl not compatible with provided surface` fallback, or nothing, is
/// all they get). Returns why the backend gave no device.
async fn open_with_retry(
    backends: wgpu::Backends,
    window: &Arc<Window>,
    offscreen: bool,
    layers: u32,
    allow_cpu: bool,
) -> std::result::Result<Opened, String> {
    match open_backend(backends, window, offscreen, false, layers, allow_cpu).await {
        Err(NoDevice::NoAdapter(hidden)) => {
            log::warn!(
                "Horizon renderer: {} has no conformant adapter ({hidden}); asking again for the \
                 ones wgpu hides as non-conformant",
                backends_name(backends)
            );
            match open_backend(backends, window, offscreen, true, layers, allow_cpu).await {
                Ok(opened) => {
                    let info = opened.adapter.get_info();
                    if is_broken_haswell_vulkan(&info) {
                        log::warn!(
                            "Horizon renderer: skipping {} on Vulkan because Mesa hasvk suffers from \
                             vertex corruption on Haswell; falling back to next backend",
                            info.name
                        );
                        return Err(format!(
                            "Mesa hasvk on Haswell ({}) has broken vertex buffers under Vulkan; falling back",
                            info.name
                        ));
                    }
                    log::warn!(
                        "Horizon renderer: using {} although its driver reports no Vulkan \
                         conformance version",
                        info.name
                    );
                    Ok(opened)
                }
                Err(NoDevice::NoAdapter(e)) | Err(NoDevice::Failed(e)) => Err(e),
            }
        }
        Err(NoDevice::Failed(e)) => Err(e),
        Ok(opened) => Ok(opened),
    }
}
/// Intel Haswell (Gen 7.5) Mesa Vulkan driver (`hasvk`) reports 0.0.0.0 and suffers
/// from severe vertex buffer corruption and polygon tearing (#9912), but its OpenGL driver
/// (`crocus`) is fully conformant and stable. Ivy Bridge (`IVB`) keeps Vulkan (#4059).
pub(crate) fn is_broken_haswell_vulkan(info: &wgpu::AdapterInfo) -> bool {
    info.backend == wgpu::Backend::Vulkan
        && info.vendor == 0x8086
        && (matches!(
            info.device,
            0x0400..=0x042f // Haswell Desktop / Mobile
                | 0x0a00..=0x0a2f // Haswell ULT (Ultrabook)
                | 0x0d00..=0x0d2f // Haswell CRW (Crystal Well)
        ) || ["HSW", "Haswell"].iter().any(|tag| info.name.contains(tag)))
}
/// Opens a device on `backends` alone: its surface, adapter and device, or what
/// stopped it. `allow_nonconformant` is `open_with_retry`'s second try; a
/// software adapter is taken only when `allow_cpu`.
async fn open_backend(
    backends: wgpu::Backends,
    window: &Arc<Window>,
    offscreen: bool,
    allow_nonconformant: bool,
    layers: u32,
    allow_cpu: bool,
) -> std::result::Result<Opened, NoDevice> {
    let name = backends_name(backends);
    let open = |dx12_compiler| {
        wgpu::Instance::new(instance_descriptor(
            backends,
            Some(window),
            allow_nonconformant,
            dx12_compiler,
        ))
    };
    let instance = open(wgpu::Dx12Compiler::Fxc);
    let (surface, adapter) = match surface_and_adapter(&instance, window, offscreen).await {
        Ok(found) => found,
        Err(NoDevice::Failed(e)) => return Err(NoDevice::Failed(format!("{name} surface: {e}"))),
        Err(NoDevice::NoAdapter(e)) => return Err(NoDevice::NoAdapter(format!("{name}: {e}"))),
    };
    // NoAdapter, so `open_with_retry` asks again: with the non-conformant
    // adapters shown, wgpu ranks a real GPU (hasvk on Ivy Bridge or Haswell)
    // above a software one. The second try's error is final either way.
    if !allow_cpu && is_software(adapter.get_info().device_type) {
        return Err(NoDevice::NoAdapter(format!(
            "{name}: only a software adapter ({})",
            adapter.get_info().name
        )));
    }
    // An integrated GPU on DirectX 12 compiles with DXC: the same request
    // again, on an instance that loads the DLL beside the game. Anything that
    // goes wrong keeps the FXC instance.
    let forced = dx12_compiler::forced();
    let dxc = if dx12_compiler::wants_dxc(&adapter.get_info(), forced) {
        open_dxc(&adapter.get_info(), window, offscreen, open).await
    } else {
        None
    };
    let dxc_found = dxc.is_some();
    let (instance, surface, adapter) = dxc.unwrap_or((instance, surface, adapter));
    if adapter.get_info().backend == wgpu::Backend::Dx12 {
        crate::crash::set_detail(
            "DirectX 12 shader compiler",
            match (forced, dxc_found) {
                (true, _) => "WGPU_DX12_COMPILER",
                (false, true) => "DXC",
                (false, false) => "FXC",
            },
        );
    }
    // Adapters that could not present to the window: the next one of this backend
    // that can is tried before the next backend (#13493).
    let mut tried = Vec::new();
    let mut adapter = adapter;
    loop {
        let why = match open_adapter(name, window, surface.as_ref(), &adapter, layers).await {
            Ok(adapted) => {
                return Ok(Opened {
                    instance,
                    surface,
                    adapter,
                    device: adapted.device,
                    queue: adapted.queue,
                    device_lost: adapted.device_lost,
                    configured: adapted.configured,
                });
            }
            Err(AdapterFailure::Failed(e)) => return Err(NoDevice::Failed(e)),
            Err(AdapterFailure::CannotPresent(why)) => why,
        };
        let info = adapter.get_info();
        tried.push(info.clone());
        let others = instance.enumerate_adapters(wgpu::Backends::all()).await;
        let choices: Vec<_> = others
            .iter()
            .map(|other| AdapterChoice {
                kind: other.get_info().device_type,
                presents: surface
                    .as_ref()
                    .is_some_and(|s| !s.get_capabilities(other).formats.is_empty()),
                tried: tried.contains(&other.get_info()),
            })
            .collect();
        let Some(next) = pick_adapter(&choices, allow_cpu) else {
            log::warn!(
                "Horizon renderer: skipping {} on {name}: it can't present to the window \
                 ({why}), and no other {name} adapter can",
                info.name
            );
            return Err(NoDevice::Failed(format!(
                "{name}: {} can't present to the window ({why})",
                info.name
            )));
        };
        adapter = others[next].clone();
        log::warn!(
            "Horizon renderer: skipping {} on {name}: it can't present to the window ({why}); \
             trying {}",
            info.name,
            adapter.get_info().name
        );
    }
}
/// A surface for `window` (none `offscreen`) and the adapter `open_backend`
/// asks `instance` for. `Failed` is the surface's error, `NoAdapter` the
/// request's.
async fn surface_and_adapter(
    instance: &wgpu::Instance,
    window: &Arc<Window>,
    offscreen: bool,
) -> std::result::Result<(Option<wgpu::Surface<'static>>, wgpu::Adapter), NoDevice> {
    let surface = if offscreen {
        None
    } else {
        Some(
            instance
                .create_surface(window.clone())
                .map_err(|e| NoDevice::Failed(e.to_string()))?,
        )
    };
    let adapter = instance
        .request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            compatible_surface: surface.as_ref(),
            force_fallback_adapter: false,
            ..Default::default()
        })
        .await
        .map_err(|e| NoDevice::NoAdapter(e.to_string()))?;
    Ok((surface, adapter))
}
/// The DirectX 12 instance, surface and adapter with DXC for the integrated
/// GPU `fxc` (`gfx/dx12_compiler.rs`), `open` making the instance; `None`, with
/// one line in the log, when the DLL is missing, doesn't load, or the instance
/// offers another GPU: the FXC instance stays.
async fn open_dxc(
    fxc: &wgpu::AdapterInfo,
    window: &Arc<Window>,
    offscreen: bool,
    open: impl FnOnce(wgpu::Dx12Compiler) -> wgpu::Instance,
) -> Option<(
    wgpu::Instance,
    Option<wgpu::Surface<'static>>,
    wgpu::Adapter,
)> {
    let Some(dll) = dx12_compiler::bundled_dll() else {
        log::info!(
            "Horizon renderer: no {} beside the game; DirectX 12 compiles shaders with FXC",
            dx12_compiler::DLL
        );
        return None;
    };
    let instance = open(dx12_compiler::dxc(&dll));
    let why = match surface_and_adapter(&instance, window, offscreen).await {
        Ok((surface, adapter)) if dx12_compiler::same_adapter(fxc, &adapter.get_info()) => {
            log::info!(
                "Horizon renderer: DirectX 12 compiles shaders with DXC ({}) on this integrated GPU",
                dll.display()
            );
            return Some((instance, surface, adapter));
        }
        Ok((_, adapter)) => format!("it offered another GPU, {}", adapter.get_info().name),
        Err(NoDevice::Failed(e) | NoDevice::NoAdapter(e)) => e,
    };
    log::warn!(
        "Horizon renderer: DXC from {} didn't load ({why}); DirectX 12 compiles shaders with FXC",
        dll.display()
    );
    None
}
/// What to tell a player whose system offers no usable graphics adapter.
/// Vulkan is loaded at run time (the game links no graphics library), so a
/// missing driver shows here rather than as a missing-DLL dialog.
#[cfg(target_os = "windows")]
const NO_GPU_DRIVER: &str = "No graphics driver with Vulkan, DirectX 12 or OpenGL found: Horizon needs \
     a GPU driver with one of them. Install the latest driver for your NVIDIA, AMD or Intel GPU and \
     start the game again";
#[cfg(target_os = "macos")]
const NO_GPU_DRIVER: &str = "No Metal graphics device found";
#[cfg(not(any(target_os = "windows", target_os = "macos")))]
const NO_GPU_DRIVER: &str = "No Vulkan or OpenGL graphics driver found: Horizon needs the Vulkan loader \
     (libvulkan.so.1) and a Vulkan driver for your GPU (Mesa's, or NVIDIA's), or OpenGL through EGL";
/// Logs a step of the renderer's start, for a native crash in the graphics
/// driver: it leaves no panic or crash report, so the log's last line is the
/// only sign of where the game stopped. The log file is written unbuffered;
/// the flush is for loggers that buffer.
fn startup_step(step: std::fmt::Arguments) {
    log::info!("Horizon renderer: {step}");
    log::logger().flush();
}
/// How far the first frame on a new device got, for `startup_step`.
#[derive(Clone, Copy, PartialEq, Eq)]
enum FirstFrame {
    Acquire,
    Submit,
    Drawn,
}
fn on_off(on: bool) -> &'static str {
    if on { "on" } else { "off" }
}
impl Renderer {
    /// Sections with GPU buffers, for soak tests watching residency.
    pub fn resident_sections(&self) -> usize {
        self.sections.len()
    }
    /// Quads of the far terrain tiles with GPU buffers.
    pub fn lod_quads(&self) -> u64 {
        self.lods
            .values()
            .map(|tile| u64::from(tile.mesh.solid + tile.mesh.translucent))
            .sum()
    }
    /// Tiles and quads of the far terrain tiles with GPU buffers, by level.
    pub fn lod_levels(&self) -> Vec<[u64; 2]> {
        let mut levels = Vec::new();
        for (key, tile) in &self.lods {
            let level = usize::from(key.level);
            if levels.len() <= level {
                levels.resize(level + 1, [0, 0]);
            }
            levels[level][0] += 1;
            levels[level][1] += u64::from(tile.mesh.solid + tile.mesh.translucent);
        }
        levels
    }
    /// Whether solid quads go to the arena in the compact layout
    /// (`packed_quads`), and whether far tiles' own records are on
    /// (`lod::packed::far_packing`).
    pub fn packed_layout(&self) -> [bool; 2] {
        [self.packed_quads, crate::lod::packed::far_packing()]
    }
    /// Bytes of the arena the resident chunk sections hold, and of the one far
    /// tiles hold: the same split `gpu_census` reports, for `status`, so the
    /// compact layouts can be seen working.
    pub fn vertex_bytes_split(&self) -> [u64; 2] {
        let stride = std::mem::size_of::<TerrainVertex>() as u64;
        let used = |s: &SectionGpu| (s.solid_slots + u64::from(s.translucent) * 4) * stride;
        [
            self.sections.values().map(used).sum(),
            self.lods.values().map(|tile| used(&tile.mesh)).sum(),
        ]
    }
    /// Bytes of the section vertex arenas' buffers, and of the vertices
    /// resident sections hold in them.
    pub fn section_vertex_bytes(&self) -> [u64; 2] {
        let stride = std::mem::size_of::<TerrainVertex>() as u64;
        let vertices: u64 = self
            .sections
            .values()
            .chain(self.lods.values().map(|tile| &*tile.mesh))
            .map(|s| s.solid_slots + u64::from(s.translucent) * 4)
            .sum();
        let translucent = self.translucent_arena.as_ref();
        let arenas = self.arena.bytes() + translucent.map_or(0, arena::Arena::bytes);
        [arenas, vertices * stride]
    }
    /// Bytes of the renderer's largest GPU resources, by name (`scene_replay --memory`):
    /// the terrain arenas (their pages, and what resident sections and far tiles use of
    /// them), the quad indices, the block sprites with their mips, the LabPBR maps, flat
    /// draws' buffers and the visibility buffer's.
    pub fn gpu_census(&self) -> Vec<(&'static str, u64)> {
        let stride = std::mem::size_of::<TerrainVertex>() as u64;
        let used = |s: &SectionGpu| (s.solid_slots + u64::from(s.translucent) * 4) * stride;
        let textures = |t: &[wgpu::Texture]| t.iter().map(texture_bytes).sum::<u64>();
        let translucent = self.translucent_arena.as_ref();
        vec![
            ("section_arena_pages", self.arena.bytes()),
            (
                "translucent_arena_pages",
                translucent.map_or(0, arena::Arena::bytes),
            ),
            ("sorted_index_pages", self.sorted_indices.bytes()),
            (
                "near_section_vertices",
                self.sections.values().map(used).sum(),
            ),
            (
                "held_section_vertices",
                self.hold
                    .sections()
                    .filter_map(|held| held.gpu.as_ref())
                    .map(used)
                    .sum(),
            ),
            (
                "far_tile_vertices",
                self.lods.values().map(|tile| used(&tile.mesh)).sum(),
            ),
            ("quad_indices", self.indices.size()),
            ("block_sprites", textures(&self.sprite_textures)),
            (
                "pbr_maps",
                self.pbr_maps
                    .as_ref()
                    .map_or(0, |maps| maps.iter().flatten().map(texture_bytes).sum()),
            ),
            (
                "flat_draws",
                self.flat.as_ref().map_or(0, flat::Flat::bytes),
            ),
            (
                "visibility_buffer",
                self.visbuf.as_ref().map_or(0, visbuffer::Visbuffer::bytes),
            ),
        ]
    }
    /// Device memory the backend's allocator holds, by the label of what holds
    /// it, largest first, with its allocated and reserved totals (Vulkan and
    /// DirectX 12; none on Metal and OpenGL) (`horizon-ctl memory`).
    pub fn gpu_memory_by_label(&self) -> Option<(u64, u64, Vec<(String, u64)>)> {
        let report = self.device.generate_allocator_report()?;
        let mut by_label: HashMap<String, u64> = HashMap::new();
        for a in report.allocations {
            *by_label.entry(a.name).or_default() += a.size;
        }
        let mut labels: Vec<_> = by_label.into_iter().collect();
        labels.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        Some((
            report.total_allocated_bytes,
            report.total_reserved_bytes,
            labels,
        ))
    }
    /// Heap bytes of the renderer's per-section bookkeeping (`horizon-ctl memory`):
    /// resident sections, far tiles, block entities and light sources by section.
    pub fn cpu_census(&self) -> [(&'static str, usize); 4] {
        use std::mem::size_of;
        let per_section = |map_len: usize, lists: usize| {
            map_len * (size_of::<SectionKey>() + size_of::<Vec<([i32; 3], u32)>>() + 1)
                + lists * size_of::<([i32; 3], u32)>()
        };
        let lists = |map: &crate::fast_hash::FxHashMap<SectionKey, Vec<([i32; 3], u32)>>| {
            per_section(
                map.capacity(),
                map.values().map(Vec::capacity).sum::<usize>(),
            )
        };
        let animated: usize = self
            .sections
            .values()
            .map(|s| s.animated_layers.len() * size_of::<u16>())
            .sum();
        [
            ("resident_sections", self.sections.heap_bytes() + animated),
            (
                "far_tiles",
                self.lods.capacity()
                    * (size_of::<crate::lod::TileKey>() + size_of::<lod::Tile>() + 1),
            ),
            ("block_entities", lists(&self.block_entities)),
            ("light_sources", lists(&self.light_blocks)),
        ]
    }
}
impl Renderer {
    pub async fn new(
        window: Arc<Window>,
        offscreen: bool,
        sprite_pixels: &[u8],
        sprite_size: u32,
        sprite_minimum_size: u32,
        layers: u32,
        sprite_mips: &[crate::mipmap::Settings],
        animations: &[SpriteAnimation],
        hud_atlas: (&[u8], u32, u32),
        glint: (&[u8], u32, u32),
        glint_armor: (&[u8], u32, u32),
        forcefield: (&[u8], u32),
        panorama_faces: Arc<panorama::Faces>,
        prepared: Option<PreparedTextures>,
        previous: Option<&Self>,
        api: GraphicsApi,
        translucent_batching: bool,
        shader_pack: bool,
        // The settings and the pack's `request` pick the frame (`output::resolve_frame`),
        // with what this window's display reported to the renderer before (`display`).
        settings: &crate::graphics::Graphics,
        request: output::FrameRequest,
        display: output::DisplayReading,
    ) -> Result<Self> {
        let Opened {
            instance,
            surface,
            adapter,
            device,
            queue,
            device_lost,
            configured,
        } = match previous {
            // Reload resources on the same device; replacing live Vulkan
            // devices can invalidate the driver's swapchain state on the same
            // window.
            Some(renderer) => {
                describe_adapter(&renderer.adapter);
                check_layers(&renderer.adapter, layers)?;
                Opened {
                    surface: if offscreen {
                        None
                    } else {
                        let spare = renderer
                            .spare_surface
                            .lock()
                            .unwrap_or_else(std::sync::PoisonError::into_inner)
                            .take();
                        Some(match spare {
                            Some(surface) => surface,
                            None => renderer.instance.create_surface(window.clone())?,
                        })
                    },
                    instance: renderer.instance.clone(),
                    adapter: renderer.adapter.clone(),
                    device: renderer.device.clone(),
                    queue: renderer.queue.clone(),
                    device_lost: renderer.device_lost.clone(),
                    configured: None,
                }
            }
            None => {
                let _step = crate::spans::startup::step("renderer.open_device");
                open_device(&window, offscreen, api, layers).await?
            }
        };
        // Computed once: the sprite arrays, the shaders' constants and the
        // shader pack atlas all cut the sprites by it.
        let split = check_layers(&adapter, layers)?;
        if split < sprite_split() {
            log::info!("Horizon: {split} block textures per array (the GPU's limit)");
        }
        let info = adapter.get_info();
        crate::graphics::suggest_reduced_far_detail(crate::graphics::lod_capped(&info));
        let size = window.inner_size();
        let max_side = device.limits().max_texture_dimension_2d;
        let configured = match (&surface, configured) {
            (Some(_), Some(configured)) => Some(configured),
            (Some(surface), None) => Some(set_up_surface(
                surface,
                &adapter,
                &device,
                &device_lost,
                surface.get_capabilities(&adapter),
                size,
                previous.map(|renderer| renderer.config.present_mode),
                previous.is_none(),
            )?),
            (None, _) => None,
        };
        let (config, present_modes, hdr_output) = if let Some((config, caps)) = configured {
            let hdr_output = hdr::output(&caps);
            if previous.is_none() {
                log::info!(
                    "Horizon renderer: HDR output {}",
                    hdr_output.map_or("not offered by this display", hdr::Output::name)
                );
            }
            (config, caps.present_modes, hdr_output)
        } else {
            let format = wgpu::TextureFormat::Bgra8Unorm;
            let config = wgpu::SurfaceConfiguration {
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                    | wgpu::TextureUsages::COPY_SRC
                    | wgpu::TextureUsages::COPY_DST,
                format,
                width: size.width.clamp(1, max_side),
                height: size.height.clamp(1, max_side),
                present_mode: wgpu::PresentMode::Immediate,
                desired_maximum_frame_latency: 2,
                alpha_mode: wgpu::CompositeAlphaMode::Opaque,
                view_formats: vec![format],
                color_space: wgpu::SurfaceColorSpace::default(),
            };
            let hdr_output = hdr::offscreen_output();
            // Only where one is named: splitscreen's views draw offscreen too.
            if let Some(output) = hdr_output
                && previous.is_none()
            {
                log::info!(
                    "Horizon renderer: HDR output {} offscreen (HORIZON_HDR_OUTPUT)",
                    output.name()
                );
            }
            (config, Vec::new(), hdr_output)
        };
        let offscreen = surface
            .is_none()
            .then(|| Self::offscreen_target(&device, &config));
        let wide_ok = draws_wide_frame(
            device.features(),
            adapter.get_texture_format_features(WIDE_FRAME),
        );
        let float_ok = draws_frame(adapter.get_texture_format_features(LINEAR_FRAME));
        if request.linear_pack && !float_ok {
            log::warn!(
                "Horizon renderer: the device can't draw a float frame; the shader API 0.2 pack runs on the 8-bit one"
            );
        }
        let coverage_ok = hud_coverage::supported(
            device.limits().max_color_attachments,
            adapter.get_downlevel_capabilities().flags,
            adapter.get_texture_format_features(hud_coverage::FORMAT),
        );
        let hdr_driver = surface.is_some().then(|| output::window_driver(&info));
        let output_caps = output::OutputCapabilities {
            hdr: hdr_output,
            wide_frame: wide_ok,
            float_frame: float_ok,
            hud_layer: coverage_ok,
            held: output::held(settings, hdr_driver.as_deref()),
        };
        let frame = output::resolve_frame(settings, request, output_caps, display, config.format);
        let (frame_format, hud_coverage, overlay_format) =
            (frame.format, frame.hud_layer, frame.overlay_format);
        let linear_light = frame.linear_light;
        if previous.is_none() {
            startup_step(format_args!("creating the pipelines"));
        }
        // A renderer on the same device takes the previous one's shader
        // modules, layouts and pipelines where they come out the same.
        let reuse = previous.map_or_else(Default::default, |r| r.reuse.clone());
        let generation = reuse.begin();
        let kept = reuse.on(&device);
        let pipelines_step = crate::spans::startup::step("renderer.pipelines_textures");
        let terrain_step = crate::spans::startup::step("renderer.terrain_pipelines");
        let gpu_timer = timing::GpuTimer::new(&device, &queue);
        let shader = kept.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Horizon terrain"),
            source: wgpu::ShaderSource::Wgsl(terrain_shader().into()),
        });
        let entry = |binding, visibility, ty| wgpu::BindGroupLayoutEntry {
            binding,
            visibility,
            ty,
            count: None,
        };
        let vf = wgpu::ShaderStages::VERTEX_FRAGMENT;
        let sprite_array = wgpu::BindingType::Texture {
            sample_type: wgpu::TextureSampleType::Float { filterable: true },
            view_dimension: wgpu::TextureViewDimension::D2Array,
            multisampled: false,
        };
        let sprite_arrays = layers.div_ceil(split).max(1);
        if sprite_arrays > 1 {
            log::info!("Horizon: {layers} block textures in {sprite_arrays} arrays");
        }
        let sprite_constants = [
            ("SPRITE_ARRAYS", f64::from(sprite_arrays)),
            ("SPRITE_SPLIT", f64::from(split)),
        ];
        let bindings = kept.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Horizon terrain bindings"),
            entries: &[
                entry(
                    0,
                    vf,
                    wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                ),
                entry(
                    1,
                    vf,
                    wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2Array,
                        multisampled: false,
                    },
                ),
                entry(
                    1 + 1,
                    vf,
                    wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                ),
                entry(
                    3,
                    vf,
                    wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                ),
                entry(
                    4,
                    vf,
                    wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                ),
                entry(5, vf, sprite_array),
                entry(6, vf, sprite_array),
                entry(7, vf, sprite_array),
                entry(8, vf, sprite_array),
                entry(9, vf, sprite_array),
                entry(10, vf, sprite_array),
                entry(11, vf, sprite_array),
                entry(
                    12,
                    vf,
                    wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                ),
            ],
        });
        let layout = kept.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Horizon terrain layout"),
            bind_group_layouts: &[Some(&bindings)],
            ..Default::default()
        });
        let fluid_pipeline = fluid::pipeline(
            &device,
            Some(&layout),
            &shader,
            config.format.remove_srgb_suffix(),
            &sprite_constants,
        );
        let terrain_model_pipelines = terrain_model::pipelines(
            &device,
            Some(&layout),
            &shader,
            config.format.remove_srgb_suffix(),
            &sprite_constants,
        );
        let instance_attributes = wgpu::vertex_attr_array![5=>Float32x4];
        let terrain_buffers = [
            Some(wgpu::VertexBufferLayout {
                array_stride: std::mem::size_of::<TerrainVertex>() as u64,
                step_mode: wgpu::VertexStepMode::Vertex,
                attributes: &TERRAIN_ATTRIBUTES,
            }),
            Some(wgpu::VertexBufferLayout {
                array_stride: 16,
                step_mode: wgpu::VertexStepMode::Instance,
                attributes: &instance_attributes,
            }),
        ];
        let alpha_blend = wgpu::BlendState::ALPHA_BLENDING;
        let debug_line_attributes =
            wgpu::vertex_attr_array![0=>Float32x3,1=>Float32x3,2=>Unorm8x4,3=>Float32];
        let debug_line_buffers = [Some(wgpu::VertexBufferLayout {
            array_stride: 32,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &debug_line_attributes,
        })];
        // The solid terrain's fragment constants without HDR output's lighting, for the
        // frames HDR is off (`SolidPipeline`).
        let plain_constants = [
            sprite_constants[0],
            sprite_constants[1],
            ("HDR_LIGHTING", 0.0),
        ];
        // `hud`: the debug crosshair, drawn with the HUD in the overlays' format, with its
        // coverage or without.
        let pipeline_with = |constants: &[(&str, f64)],
                             layout: &wgpu::PipelineLayout,
                             label: &str,
                             vertex: &str,
                             fragment: &str,
                             buffers: &[Option<wgpu::VertexBufferLayout<'_>>],
                             topology: wgpu::PrimitiveTopology,
                             cull: Option<wgpu::Face>,
                             depth_write: bool,
                             depth_test: bool,
                             blend: Option<wgpu::BlendState>,
                             hud: Option<bool>| {
            let mut constants = constants.to_vec();
            // The debug crosshair draws with the HUD, in gamma.
            constants.extend(linear_blend::frame_constants(
                frame_format,
                linear_light && hud.is_none(),
            ));
            let desc = wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some(vertex),
                    compilation_options: Default::default(),
                    buffers,
                },
                primitive: wgpu::PrimitiveState {
                    topology,
                    cull_mode: cull,
                    ..Default::default()
                },
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: wgpu::TextureFormat::Depth32Float,
                    depth_write_enabled: Some(depth_write),
                    depth_compare: Some(if depth_test {
                        wgpu::CompareFunction::GreaterEqual
                    } else {
                        wgpu::CompareFunction::Always
                    }),
                    stencil: Default::default(),
                    bias: Default::default(),
                }),
                multisample: Default::default(),
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some(fragment),
                    compilation_options: wgpu::PipelineCompilationOptions {
                        constants: &constants,
                        ..Default::default()
                    },
                    targets: &[Some(wgpu::ColorTargetState {
                        format: if hud.is_some() {
                            overlay_format
                        } else {
                            frame_format
                        },
                        blend,
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                multiview_mask: None,
                cache: None,
            };
            if hud == Some(true) {
                hud_coverage::pipeline(
                    &device,
                    &desc,
                    wgpu::ShaderSource::Wgsl(terrain_shader().into()),
                )
                .expect("the debug crosshair's coverage")
            } else {
                kept.create_render_pipeline(&desc)
            }
        };
        let pipeline_in = |layout: &wgpu::PipelineLayout,
                           label: &str,
                           vertex: &str,
                           fragment: &str,
                           buffers: &[Option<wgpu::VertexBufferLayout<'_>>],
                           topology: wgpu::PrimitiveTopology,
                           cull: Option<wgpu::Face>,
                           depth_write: bool,
                           depth_test: bool,
                           blend: Option<wgpu::BlendState>| {
            pipeline_with(
                &sprite_constants,
                layout,
                label,
                vertex,
                fragment,
                buffers,
                topology,
                cull,
                depth_write,
                depth_test,
                blend,
                None,
            )
        };
        // A solid terrain pipeline (back faces culled, depth written, no blend) without HDR
        // output's lighting, and with it where the display offers HDR output.
        let solid_in = |layout: &wgpu::PipelineLayout,
                        label: &str,
                        vertex: &str,
                        fragment: &str,
                        buffers: &[Option<wgpu::VertexBufferLayout<'_>>]| {
            let build = |constants: &[(&str, f64)]| {
                pipeline_with(
                    constants,
                    layout,
                    label,
                    vertex,
                    fragment,
                    buffers,
                    wgpu::PrimitiveTopology::TriangleList,
                    Some(wgpu::Face::Back),
                    true,
                    true,
                    None,
                    None,
                )
            };
            SolidPipeline {
                plain: build(&plain_constants),
                hdr: hdr_output.is_some().then(|| build(&sprite_constants)),
            }
        };
        let pipeline = |label: &str,
                        vertex: &str,
                        fragment: &str,
                        buffers: &[Option<wgpu::VertexBufferLayout<'_>>],
                        topology: wgpu::PrimitiveTopology,
                        cull: Option<wgpu::Face>,
                        depth_write: bool,
                        depth_test: bool,
                        blend: Option<wgpu::BlendState>| {
            pipeline_in(
                &layout,
                label,
                vertex,
                fragment,
                buffers,
                topology,
                cull,
                depth_write,
                depth_test,
                blend,
            )
        };
        let tri = wgpu::PrimitiveTopology::TriangleList;
        let solid = solid_in(
            &layout,
            "Terrain solid",
            "vs_terrain",
            "fs_solid",
            &terrain_buffers,
        );
        // `solid` in two passes (terrain.wgsl's `opaque_quad`, `mesher::Part`), for the
        // world pass without a shader pack: no cutout first, through a shader that never
        // discards, then the rest.
        let solid_opaque = solid_in(
            &layout,
            "Terrain solid opaque",
            "vs_terrain_opaque",
            "fs_opaque",
            &terrain_buffers,
        );
        let solid_cutout = solid_in(
            &layout,
            "Terrain solid cutout",
            "vs_terrain_cutout",
            "fs_solid",
            &terrain_buffers,
        );
        // The water mirror's camera sees the world from the other side of the plane, which
        // flips the winding: its solid terrain culls the front.
        let solid_mirror = pipeline(
            "Terrain solid (mirrored)",
            "vs_terrain",
            "fs_solid_mirror",
            &terrain_buffers,
            tri,
            Some(wgpu::Face::Front),
            true,
            true,
            None,
        );
        let translucent = pipeline(
            "Terrain translucent",
            "vs_terrain",
            "fs_translucent",
            &terrain_buffers,
            tri,
            Some(wgpu::Face::Back),
            true,
            true,
            Some(alpha_blend),
        );
        let translucent_mark = pipeline(
            "Terrain translucent HDR mark",
            "vs_terrain",
            "fs_translucent_mark",
            &terrain_buffers,
            tri,
            Some(wgpu::Face::Back),
            false,
            true,
            Some(flat::ALPHA_ONLY),
        );
        let sky = pipeline(
            "Sky",
            "vs_sky",
            "fs_sky",
            &[],
            tri,
            None,
            false,
            false,
            None,
        );
        let sky_base = pipeline(
            "Sky base",
            "vs_sky",
            "fs_sky_base",
            &[],
            tri,
            None,
            false,
            false,
            None,
        );
        let sunrise_attributes = wgpu::vertex_attr_array![0=>Float32x4, 1=>Float32];
        let sunrise_buffers = [Some(wgpu::VertexBufferLayout {
            array_stride: 20,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &sunrise_attributes,
        })];
        let sunrise_blend = wgpu::BlendState {
            color: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::SrcAlpha,
                dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                operation: wgpu::BlendOperation::Add,
            },
            // The sky's HDR alpha is the destination's; the fan only tints it.
            alpha: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::Zero,
                dst_factor: wgpu::BlendFactor::One,
                operation: wgpu::BlendOperation::Add,
            },
        };
        let sunrise_pipeline = pipeline(
            "Sunrise fan",
            "vs_sunrise",
            "fs_sunrise",
            &sunrise_buffers,
            tri,
            None,
            false,
            false,
            Some(sunrise_blend),
        );
        let dark_disc = pipeline(
            "Dark disc",
            "vs_sky",
            "fs_dark_disc",
            &[],
            tri,
            None,
            false,
            false,
            Some(alpha_blend),
        );
        let depth_reset = kept.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Hand depth reset"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_sky"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: Default::default(),
            depth_stencil: Some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth32Float,
                depth_write_enabled: Some(true),
                depth_compare: Some(wgpu::CompareFunction::Always),
                stencil: Default::default(),
                bias: Default::default(),
            }),
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_depth_reset"),
                compilation_options: wgpu::PipelineCompilationOptions {
                    constants: &sprite_constants,
                    ..Default::default()
                },
                targets: &[Some(wgpu::ColorTargetState {
                    format: frame_format,
                    blend: None,
                    write_mask: wgpu::ColorWrites::empty(),
                })],
            }),
            multiview_mask: None,
            cache: None,
        });
        let lines = pipeline(
            "Block outline",
            "vs_line",
            "fs_line",
            &debug_line_buffers,
            wgpu::PrimitiveTopology::TriangleStrip,
            None,
            true,
            true,
            Some(alpha_blend),
        );
        // RenderTypes.secondaryBlockOutline: the black stroke under a high contrast outline.
        let lines_secondary = pipeline(
            "Block outline secondary",
            "vs_line",
            "fs_line",
            &debug_line_buffers,
            wgpu::PrimitiveTopology::TriangleStrip,
            None,
            false,
            true,
            Some(alpha_blend),
        );
        // RenderPipelines.LINES: DepthStencilState.DEFAULT, so an opaque hitbox or
        // chunk border keeps the water and clouds drawn after it behind it.
        let debug_lines_opaque = pipeline(
            "Debug lines opaque",
            "vs_debug_line",
            "fs_debug_line",
            &debug_line_buffers,
            wgpu::PrimitiveTopology::TriangleStrip,
            None,
            true,
            true,
            Some(alpha_blend),
        );
        let debug_lines = pipeline(
            "Debug lines",
            "vs_debug_line",
            "fs_debug_line",
            &debug_line_buffers,
            wgpu::PrimitiveTopology::TriangleStrip,
            None,
            false,
            true,
            Some(alpha_blend),
        );
        let debug_lines_on_top = pipeline(
            "Debug lines on top",
            "vs_debug_line",
            "fs_debug_line",
            &debug_line_buffers,
            wgpu::PrimitiveTopology::TriangleStrip,
            None,
            false,
            false,
            Some(alpha_blend),
        );
        // The debug crosshair with the HUD: with its coverage on the layered frame, and in the
        // overlays' format where that isn't the frame's.
        let crosshair = |coverage| {
            pipeline_with(
                &sprite_constants,
                &layout,
                "3d crosshair",
                "vs_debug_line",
                "fs_debug_line",
                &debug_line_buffers,
                wgpu::PrimitiveTopology::TriangleStrip,
                None,
                false,
                false,
                Some(alpha_blend),
                Some(coverage),
            )
        };
        let debug_crosshair = hud_coverage.then(|| crosshair(true));
        let overlay_crosshair = (overlay_format != frame_format).then(|| crosshair(false));
        drop(terrain_step);
        let globals = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Horizon globals"),
            size: std::mem::size_of::<Globals>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let hand_globals = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("First-person hand globals"),
            size: std::mem::size_of::<Globals>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let crosshair_globals = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Debug crosshair globals"),
            size: std::mem::size_of::<Globals>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let mirror_globals = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Water mirror globals"),
            size: std::mem::size_of::<Globals>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let prepared = match prepared {
            Some(prepared) => prepared.resplit(layers, split),
            None => PreparedTextures::with_split(
                sprite_pixels,
                sprite_size,
                layers,
                sprite_mips,
                forcefield,
                split,
            ),
        };
        let mut mip_levels = 1;
        let gl = adapter.get_info().backend == wgpu::Backend::Gl;
        // Keep every colour layer's companion, split by the same device limit. Native
        // material stages read the first array; terrain packs stitch all of them.
        let pbr_maps = prepared
            .pbr
            .map(|(normals, specular, levels, map_layers, size)| {
                let layer_bytes = normals.len() / map_layers as usize;
                (0..map_layers.div_ceil(split))
                    .map(|array| {
                        let first = array * split;
                        let used = (map_layers - first).min(split);
                        let count = array_layers(used, gl);
                        [("LabPBR normals", &normals), ("LabPBR specular", &specular)].map(
                            |(label, data)| {
                                let start = first as usize * layer_bytes;
                                let mut data =
                                    data[start..start + used as usize * layer_bytes].to_vec();
                                data.resize(layer_bytes * count as usize, 0);
                                device.create_texture_with_data(
                                    &queue,
                                    &wgpu::TextureDescriptor {
                                        label: Some(label),
                                        size: wgpu::Extent3d {
                                            width: size,
                                            height: size,
                                            depth_or_array_layers: count,
                                        },
                                        mip_level_count: levels,
                                        sample_count: 1,
                                        dimension: wgpu::TextureDimension::D2,
                                        format: wgpu::TextureFormat::Rgba8Unorm,
                                        usage: wgpu::TextureUsages::TEXTURE_BINDING
                                            | wgpu::TextureUsages::COPY_SRC
                                            | wgpu::TextureUsages::COPY_DST,
                                        view_formats: &[],
                                    },
                                    wgpu::util::TextureDataOrder::LayerMajor,
                                    &data,
                                )
                            },
                        )
                    })
                    .collect()
            });
        let sprite_textures: Vec<wgpu::Texture> = prepared
            .sprites
            .into_iter()
            .enumerate()
            .map(|(array, (mut mips, levels))| {
                let first = array as u32 * split;
                let used = layers.saturating_sub(first).clamp(1, split);
                let count = array_layers(used, gl);
                // The extra layers are empty.
                mips.resize(mips.len() / used as usize * count as usize, 0);
                mip_levels = levels;
                device.create_texture_with_data(
                    &queue,
                    &wgpu::TextureDescriptor {
                        label: Some("Block sprites"),
                        size: wgpu::Extent3d {
                            width: sprite_size,
                            height: sprite_size,
                            depth_or_array_layers: count,
                        },
                        mip_level_count: levels,
                        sample_count: 1,
                        dimension: wgpu::TextureDimension::D2,
                        format: wgpu::TextureFormat::Rgba8Unorm,
                        usage: wgpu::TextureUsages::TEXTURE_BINDING
                            | wgpu::TextureUsages::COPY_DST
                            // Shader packs' atlas copies them (pack_atlas.rs).
                            | wgpu::TextureUsages::COPY_SRC
                            | wgpu::TextureUsages::STORAGE_BINDING,
                        view_formats: &[],
                    },
                    wgpu::util::TextureDataOrder::LayerMajor,
                    &mips,
                )
            })
            .collect();
        let material_animations = animations
            .iter()
            .filter(|a| a.has_materials())
            .cloned()
            .collect();
        let animations = animate::SpriteAnimator::new(
            &device,
            animations,
            sprite_size,
            sprite_mips,
            &sprite_textures,
            split,
            animate::SpriteAnimator::compute(&adapter, &device),
        );
        let active_layer_words = animations.active_layer_words();
        let sprite_unused = device
            .create_texture(&wgpu::TextureDescriptor {
                label: Some("Block sprites (unused)"),
                size: wgpu::Extent3d {
                    width: 1,
                    height: 1,
                    depth_or_array_layers: array_layers(1, gl),
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8Unorm,
                usage: wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            })
            .create_view(&wgpu::TextureViewDescriptor {
                dimension: Some(wgpu::TextureViewDimension::D2Array),
                ..Default::default()
            });
        let sprite_views = sprite_views(&sprite_textures, &sprite_unused, None);
        let sprite_sampler = device.create_sampler(&sprite_sampler_descriptor(1));
        let sprite_nearest = device.create_sampler(&SPRITE_NEAREST);
        let lightmap = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Lightmap"),
            size: wgpu::Extent3d {
                width: 16,
                height: 16,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let lightmap_view = lightmap.create_view(&Default::default());
        let lightmap_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("Lightmap"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let emission_stats =
            hdr_stats::EmissionStats::new(&device, split, sprite_textures.len() as u32);
        let group = terrain_group(
            &device,
            "Horizon terrain resources",
            &bindings,
            &globals,
            &sprite_views,
            &sprite_sampler,
            &lightmap_view,
            &lightmap_sampler,
            &emission_stats.view,
        );
        let fluid_sampler = device.create_sampler(&fluid::SAMPLER);
        let fluid_group = terrain_group(
            &device,
            "Still fluid resources",
            &bindings,
            &globals,
            &sprite_views,
            &fluid_sampler,
            &lightmap_view,
            &lightmap_sampler,
            &emission_stats.view,
        );
        let terrain_model_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("Native cutout model (unmipped)"),
            lod_max_clamp: 0.0,
            ..fluid::SAMPLER
        });
        let terrain_model_group = terrain_group(
            &device,
            "Native cutout model resources",
            &bindings,
            &globals,
            &sprite_views,
            &terrain_model_sampler,
            &lightmap_view,
            &lightmap_sampler,
            &emission_stats.view,
        );
        let crosshair_group = terrain_group(
            &device,
            "Debug crosshair resources",
            &bindings,
            &crosshair_globals,
            &sprite_views,
            &sprite_sampler,
            &lightmap_view,
            &lightmap_sampler,
            &emission_stats.view,
        );
        let mirror_group = terrain_group(
            &device,
            "Water mirror resources",
            &bindings,
            &mirror_globals,
            &sprite_views,
            &sprite_sampler,
            &lightmap_view,
            &lightmap_sampler,
            &emission_stats.view,
        );
        let index_quads = 1 << 16;
        let indices = Self::quad_indices(&device, index_quads);
        let instance_capacity = 4096;
        let instances = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Section origins"),
            size: instance_capacity * 16,
            usage: wgpu::BufferUsages::VERTEX
                | wgpu::BufferUsages::COPY_DST
                | wgpu::BufferUsages::STORAGE,
            mapped_at_creation: false,
        });
        let outline = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Outline"),
            size: 32 * 2 * MAX_OUTLINE_EDGES,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let debug_line_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Debug lines"),
            size: 32 * 4096,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let (atlas_pixels, atlas_width, atlas_height) = hud_atlas;
        let limit = device.limits().max_texture_dimension_2d;
        if atlas_width.max(atlas_height) > limit {
            return Err(format!(
                "the shared atlas is {atlas_width}x{atlas_height}; this GPU allows {limit}x{limit}"
            )
            .into());
        }
        let atlas = device.create_texture_with_data(
            &queue,
            &wgpu::TextureDescriptor {
                label: Some("Shared atlas"),
                size: wgpu::Extent3d {
                    width: atlas_width,
                    height: atlas_height,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8Unorm,
                // COPY_SRC: a shader pack's weather copies the rain and snow sprites out.
                usage: wgpu::TextureUsages::TEXTURE_BINDING
                    | wgpu::TextureUsages::COPY_DST
                    | wgpu::TextureUsages::COPY_SRC,
                view_formats: &[],
            },
            wgpu::util::TextureDataOrder::LayerMajor,
            atlas_pixels,
        );
        let atlas_view = atlas.create_view(&Default::default());
        // vanilla's `GlintSampler`: enchanted_glint_item as its own texture,
        // repeated per pixel (`clamp: false`), linear without mipmaps
        // (`blur: true`).
        let (glint_pixels, glint_width, glint_height) = glint;
        let glint_texture = device.create_texture_with_data(
            &queue,
            &wgpu::TextureDescriptor {
                label: Some("Enchanted glint"),
                size: wgpu::Extent3d {
                    width: glint_width,
                    height: glint_height,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8Unorm,
                usage: wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            },
            wgpu::util::TextureDataOrder::LayerMajor,
            glint_pixels,
        );
        let glint_view = glint_texture.create_view(&Default::default());
        let glint_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("Glint"),
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::Repeat,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        // ARMOR_ENTITY_GLINT_TEXTURING's sampler is vanilla's GlintSampler: the
        // same repeat and linear filtering as the item glint.
        let (glint_armor_pixels, glint_armor_width, glint_armor_height) = glint_armor;
        let glint_armor_texture = device.create_texture_with_data(
            &queue,
            &wgpu::TextureDescriptor {
                label: Some("Enchanted armor glint"),
                size: wgpu::Extent3d {
                    width: glint_armor_width,
                    height: glint_armor_height,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8Unorm,
                usage: wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            },
            wgpu::util::TextureDataOrder::LayerMajor,
            glint_armor_pixels,
        );
        let glint_armor_view = glint_armor_texture.create_view(&Default::default());
        let mut hud = HudRenderer::new(
            &device,
            overlay_format,
            &atlas_view,
            &glint_view,
            &glint_armor_view,
            &glint_sampler,
            hud_coverage,
        )?;
        hud.atlas_texture = Some(atlas.clone());
        let entity_step = crate::spans::startup::step("renderer.entity_pipelines");
        let entity_shader = kept.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Horizon entities"),
            source: wgpu::ShaderSource::Wgsl(entity_shader().into()),
        });
        let static_entity_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Horizon static block entities"),
            source: wgpu::ShaderSource::Wgsl(static_entity_shader().into()),
        });
        let entity_bindings = kept.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Entity bindings"),
            entries: &[
                entry(
                    0,
                    vf,
                    wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                ),
                entry(
                    1,
                    vf,
                    wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                ),
                entry(
                    2,
                    vf,
                    wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                ),
                entry(
                    3,
                    vf,
                    wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                ),
                entry(
                    4,
                    vf,
                    wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                ),
                entry(
                    5,
                    vf,
                    wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                ),
                entry(
                    6,
                    vf,
                    wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                ),
                entry(
                    7,
                    vf,
                    wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                ),
            ],
        });
        let static_entity_bindings =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("Static block-entity transform"),
                entries: &[entry(
                    0,
                    wgpu::ShaderStages::VERTEX,
                    wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                )],
            });
        let weather = weather::Weather::new(
            &device,
            (frame_format, linear_light),
            &entity_bindings,
            &entity_shader,
        );
        let atlas_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("Atlas"),
            ..Default::default()
        });
        let entity_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Entity resources"),
            layout: &entity_bindings,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: globals.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&atlas_view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(&atlas_sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::TextureView(&lightmap_view),
                },
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: wgpu::BindingResource::Sampler(&lightmap_sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 5,
                    resource: wgpu::BindingResource::TextureView(&glint_view),
                },
                wgpu::BindGroupEntry {
                    binding: 6,
                    resource: wgpu::BindingResource::Sampler(&glint_sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 7,
                    resource: wgpu::BindingResource::TextureView(&glint_armor_view),
                },
            ],
        });
        let static_entity_transform =
            device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("Static block-entity camera position"),
                contents: bytemuck::bytes_of(&StaticEntityUniform {
                    camera_block: [0; 4],
                    camera_fraction: [0.0; 4],
                }),
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            });
        let static_entity_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Static block-entity transform"),
            layout: &static_entity_bindings,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: static_entity_transform.as_entire_binding(),
            }],
        });
        let hand_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("First-person hand resources"),
            layout: &entity_bindings,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: hand_globals.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&atlas_view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(&atlas_sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::TextureView(&lightmap_view),
                },
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: wgpu::BindingResource::Sampler(&lightmap_sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 5,
                    resource: wgpu::BindingResource::TextureView(&glint_view),
                },
                wgpu::BindGroupEntry {
                    binding: 6,
                    resource: wgpu::BindingResource::Sampler(&glint_sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 7,
                    resource: wgpu::BindingResource::TextureView(&glint_armor_view),
                },
            ],
        });
        let entity_layout = kept.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Entity layout"),
            bind_group_layouts: &[Some(&entity_bindings)],
            ..Default::default()
        });
        let static_entity_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Static block-entity layout"),
            bind_group_layouts: &[Some(&entity_bindings), Some(&static_entity_bindings)],
            ..Default::default()
        });
        let cloud_bindings = kept.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Cloud bindings"),
            entries: &[entry(
                0,
                wgpu::ShaderStages::VERTEX,
                wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
            )],
        });
        let cloud_uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Cloud transforms and colors"),
            size: std::mem::size_of::<crate::clouds::Uniform>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let cloud_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Cloud resources"),
            layout: &cloud_bindings,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: cloud_uniform.as_entire_binding(),
            }],
        });
        let cloud_layout = kept.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Cloud layout"),
            bind_group_layouts: &[Some(&entity_bindings), Some(&cloud_bindings)],
            ..Default::default()
        });
        let cloud_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Cloud vertices"),
            size: 4 * std::mem::size_of::<Vertex>() as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let entity_attributes = wgpu::vertex_attr_array![0=>Float32x3,1=>Float32x2,2=>Uint32,3=>Unorm8x4,4=>Uint8x4,5=>Snorm8x4,6=>Float32x2];
        let entity_vertex = [Some(wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Vertex>() as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &entity_attributes,
        })];
        let static_entity_origin_attributes = wgpu::vertex_attr_array![7=>Sint32x3];
        let static_entity_vertex = [
            Some(wgpu::VertexBufferLayout {
                array_stride: std::mem::size_of::<Vertex>() as u64,
                step_mode: wgpu::VertexStepMode::Vertex,
                attributes: &entity_attributes,
            }),
            Some(wgpu::VertexBufferLayout {
                array_stride: std::mem::size_of::<[i32; 3]>() as u64,
                step_mode: wgpu::VertexStepMode::Instance,
                attributes: &static_entity_origin_attributes,
            }),
        ];
        #[allow(clippy::too_many_arguments)]
        let entity_pipeline_in = |format,
                                  linear,
                                  label,
                                  cull_mode,
                                  depth_write,
                                  compare,
                                  bias,
                                  blend,
                                  clouds,
                                  fragment: &str,
                                  write_mask| {
            kept.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(if clouds {
                    &cloud_layout
                } else {
                    &entity_layout
                }),
                vertex: wgpu::VertexState {
                    module: &entity_shader,
                    entry_point: Some(if clouds { "vs_cloud" } else { "vs_entity" }),
                    compilation_options: Default::default(),
                    buffers: &entity_vertex,
                },
                primitive: wgpu::PrimitiveState {
                    cull_mode,
                    ..Default::default()
                },
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: wgpu::TextureFormat::Depth32Float,
                    depth_write_enabled: Some(depth_write),
                    depth_compare: Some(compare),
                    stencil: Default::default(),
                    bias,
                }),
                multisample: Default::default(),
                fragment: Some(wgpu::FragmentState {
                    module: &entity_shader,
                    entry_point: Some(fragment),
                    compilation_options: wgpu::PipelineCompilationOptions {
                        constants: &linear_blend::frame_constants(frame_format, linear),
                        ..Default::default()
                    },
                    targets: &[Some(wgpu::ColorTargetState {
                        format,
                        blend: Some(blend),
                        write_mask,
                    })],
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        #[allow(clippy::too_many_arguments)]
        let entity_pipeline_with =
            |label, cull_mode, depth_write, compare, bias, blend, clouds, fragment, write_mask| {
                entity_pipeline_in(
                    frame_format,
                    linear_light,
                    label,
                    cull_mode,
                    depth_write,
                    compare,
                    bias,
                    blend,
                    clouds,
                    fragment,
                    write_mask,
                )
            };
        let entity_pipeline = |label, cull_mode, depth_write, compare, bias, blend, clouds| {
            entity_pipeline_with(
                label,
                cull_mode,
                depth_write,
                compare,
                bias,
                blend,
                clouds,
                "fs_entity",
                wgpu::ColorWrites::ALL,
            )
        };
        // RenderPipelines.WATER_MASK: the boat's water patch writes depth only.
        let water_mask_pipeline = entity_pipeline_with(
            "Water mask",
            None,
            true,
            wgpu::CompareFunction::GreaterEqual,
            Default::default(),
            alpha_blend,
            false,
            "fs_water_mask",
            wgpu::ColorWrites::empty(),
        );
        let crumbling_pipeline = entity_pipeline(
            "Breaking cracks",
            Some(wgpu::Face::Back),
            false,
            wgpu::CompareFunction::GreaterEqual,
            wgpu::DepthBiasState {
                constant: 10,
                slope_scale: 1.0,
                clamp: 0.0,
            },
            CRUMBLING_BLEND,
            false,
        );
        // RenderPipelines.ENTITY_SHADOW: translucent, back-face culled, depth
        // test without writes. The quads lie exactly on the block tops like
        // vanilla's, facing +Y so looking up from below culls them, but
        // terrain and shadows are separate draw paths, so their fp32 depth
        // interpolations disagree by a few dozen ulps of the fragment depth
        // and the shadow's rim z-fights with the block top (speckle that
        // re-rolls as the camera drifts a subpixel). A constant bias of 128
        // ulps covers that spread at any resolution while staying far below
        // the 0.001 a model's feet sit higher, so shadows never bleed onto
        // the feet.
        let shadow_blob_pipeline = entity_pipeline_with(
            "Entity shadows",
            Some(wgpu::Face::Back),
            false,
            wgpu::CompareFunction::GreaterEqual,
            wgpu::DepthBiasState {
                constant: 128,
                slope_scale: 0.015,
                clamp: 0.0,
            },
            alpha_blend,
            false,
            "fs_entity_shadow",
            wgpu::ColorWrites::ALL,
        );
        // RenderPipelines.ITEM_TRANSLUCENT: blended, depth tested and, like
        // every vanilla pipeline that does not turn it off, depth writing.
        // `LevelRenderer.executeClassicTransparency` draws the translucent
        // features before the translucent terrain, so without the writes
        // ice or water behind a dropped slime block or glass item would blend
        // over it and the item would all but vanish (#7031, #7033).
        let translucent_item_pipeline = entity_pipeline(
            "Translucent items",
            None,
            true,
            wgpu::CompareFunction::GreaterEqual,
            Default::default(),
            alpha_blend,
            false,
        );
        let cloud_pipeline = entity_pipeline(
            "Clouds",
            Some(wgpu::Face::Back),
            true,
            wgpu::CompareFunction::GreaterEqual,
            Default::default(),
            alpha_blend,
            true,
        );
        let hdr_cloud_mark_pipeline = entity_pipeline_with(
            "HDR cloud mark",
            Some(wgpu::Face::Back),
            false,
            wgpu::CompareFunction::GreaterEqual,
            Default::default(),
            wgpu::BlendState::REPLACE,
            true,
            "fs_hdr_cloud_mark",
            wgpu::ColorWrites::ALPHA,
        );
        // RenderPipelines.TEXT_SEE_THROUGH: translucent, no depth test or
        // writes, culled like every world text pipeline (TEXT_SNIPPET keeps
        // RenderPipeline.Builder's default): a name tag or text display hides
        // its back.
        let see_through_pipeline = entity_pipeline_with(
            "See-through text",
            Some(wgpu::Face::Back),
            false,
            wgpu::CompareFunction::Always,
            Default::default(),
            alpha_blend,
            false,
            "fs_entity_unmarked",
            wgpu::ColorWrites::ALL,
        );
        // RenderPipelines.TEXT_POLYGON_OFFSET: a text display's text and
        // background win against the surface they lie on (bias 1, 10), from
        // the front only.
        let text_polygon_offset_pipeline = entity_pipeline_with(
            "Polygon offset text",
            Some(wgpu::Face::Back),
            true,
            wgpu::CompareFunction::GreaterEqual,
            wgpu::DepthBiasState {
                constant: 10,
                slope_scale: 1.0,
                clamp: 0.0,
            },
            alpha_blend,
            false,
            "fs_entity_unmarked",
            wgpu::ColorWrites::ALL,
        );
        let world_text_pipeline = world_text::pipeline(
            &device,
            Some(&entity_layout),
            &entity_shader,
            (frame_format, linear_light),
        );
        // RenderPipelines.ENTITY_SOLID: a surface of an entity that is coplanar with
        // terrain wins the tie in vanilla (the boat's floor over the trapdoor on the
        // ground it is stopped by, #5950). Terrain and entities are separate draw
        // paths here, so their fp32 depths of the same plane differ by a few dozen
        // ulps and the tie came out either way, as a jagged strip of the trapdoor
        // through the hull's floor. The same constant as the shadows' covers that
        // spread; a model's feet sit 0.001 above the ground, far beyond it. HDR output's
        // marks on what it draws take it too (`hdr_entity_mark_pipeline`).
        let entity_bias = wgpu::DepthBiasState {
            constant: 128,
            slope_scale: 0.0,
            clamp: 0.0,
        };
        let entity_pipeline = entity_pipeline(
            "Entities",
            None,
            true,
            wgpu::CompareFunction::GreaterEqual,
            entity_bias,
            alpha_blend,
            false,
        );
        // The hand as `entity_pipeline` draws it, in the overlays' format and gamma: it draws
        // after a 0.2 pack's tonemap.
        let overlay_hand = (overlay_format != frame_format || linear_light).then(|| {
            entity_pipeline_in(
                overlay_format,
                false,
                "First-person hand",
                None,
                true,
                wgpu::CompareFunction::GreaterEqual,
                entity_bias,
                alpha_blend,
                false,
                "fs_entity",
                wgpu::ColorWrites::ALL,
            )
        });
        let static_entity_pipeline =
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("Static block entities"),
                layout: Some(&static_entity_layout),
                vertex: wgpu::VertexState {
                    module: &static_entity_shader,
                    entry_point: Some("vs_static_entity"),
                    compilation_options: Default::default(),
                    buffers: &static_entity_vertex,
                },
                primitive: wgpu::PrimitiveState::default(),
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: wgpu::TextureFormat::Depth32Float,
                    depth_write_enabled: Some(true),
                    depth_compare: Some(wgpu::CompareFunction::GreaterEqual),
                    stencil: Default::default(),
                    bias: Default::default(),
                }),
                multisample: Default::default(),
                fragment: Some(wgpu::FragmentState {
                    module: &static_entity_shader,
                    entry_point: Some("fs_entity_unmarked"),
                    compilation_options: wgpu::PipelineCompilationOptions {
                        constants: &linear_blend::frame_constants(frame_format, linear_light),
                        ..Default::default()
                    },
                    targets: &[Some(wgpu::ColorTargetState {
                        format: frame_format,
                        // fs_entity's discard handles binary cutout alpha. Any
                        // sprite with fractional alpha stays on the blended
                        // dynamic entity path.
                        blend: None,
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                multiview_mask: None,
                cache: None,
            });
        // RenderPipelines.ENERGY_SWIRL: BlendFunction.ADDITIVE (one, one), no culling; no depth
        // writes either: vanilla sorts the quads far to near, so every face passes the test.
        let one_one = wgpu::BlendComponent {
            src_factor: wgpu::BlendFactor::One,
            dst_factor: wgpu::BlendFactor::One,
            operation: wgpu::BlendOperation::Add,
        };
        let energy_swirl_pipeline = entity_pipeline_with(
            "Energy swirls",
            None,
            false,
            wgpu::CompareFunction::GreaterEqual,
            Default::default(),
            wgpu::BlendState {
                color: one_one,
                alpha: one_one,
            },
            false,
            "fs_energy_swirl",
            wgpu::ColorWrites::ALL,
        );
        // HDR output's mark on light-giving particles (entity.wgsl fs_hdr_mark).
        let hdr_mark_pipeline = entity_pipeline_with(
            "HDR particle mark",
            None,
            false,
            wgpu::CompareFunction::GreaterEqual,
            Default::default(),
            wgpu::BlendState::REPLACE,
            false,
            "fs_hdr_mark",
            wgpu::ColorWrites::ALPHA,
        );
        // The same at `entity_pipeline`'s depth bias, for what it drew (entities, the hand,
        // translucent particles): without it the marks test a hair behind the depth it left
        // and never land.
        let hdr_entity_mark_pipeline = entity_pipeline_with(
            "HDR entity mark",
            None,
            false,
            wgpu::CompareFunction::GreaterEqual,
            entity_bias,
            wgpu::BlendState::REPLACE,
            false,
            "fs_hdr_mark",
            wgpu::ColorWrites::ALPHA,
        );
        let hdr_hand_unmark_pipeline = entity_pipeline_with(
            "HDR hand unmark",
            None,
            false,
            wgpu::CompareFunction::GreaterEqual,
            entity_bias,
            wgpu::BlendState::REPLACE,
            false,
            "fs_hdr_hand_unmark",
            wgpu::ColorWrites::ALPHA,
        );
        // Attribute 7: 6 is the entity vertex buffer's glint UV.
        let lightning_attributes = wgpu::vertex_attr_array![7=>Float32x3];
        let lightning_vertex = [
            entity_vertex[0].clone(),
            Some(wgpu::VertexBufferLayout {
                array_stride: std::mem::size_of::<[f32; 3]>() as u64,
                step_mode: wgpu::VertexStepMode::Instance,
                attributes: &lightning_attributes,
            }),
        ];
        // BlendFunction.LIGHTNING: src alpha, one; no depth writes.
        let additive_descriptor = wgpu::RenderPipelineDescriptor {
            label: Some("Additive entities"),
            layout: Some(&entity_layout),
            vertex: wgpu::VertexState {
                module: &entity_shader,
                entry_point: Some("vs_entity"),
                compilation_options: Default::default(),
                buffers: &entity_vertex,
            },
            primitive: wgpu::PrimitiveState {
                cull_mode: None,
                ..Default::default()
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth32Float,
                depth_write_enabled: Some(false),
                depth_compare: Some(wgpu::CompareFunction::GreaterEqual),
                stencil: Default::default(),
                bias: Default::default(),
            }),
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState {
                module: &entity_shader,
                entry_point: Some("fs_celestial"),
                compilation_options: wgpu::PipelineCompilationOptions {
                    constants: &linear_blend::frame_constants(frame_format, linear_light),
                    ..Default::default()
                },
                targets: &[Some(wgpu::ColorTargetState {
                    format: frame_format,
                    blend: Some(wgpu::BlendState {
                        color: wgpu::BlendComponent {
                            src_factor: wgpu::BlendFactor::SrcAlpha,
                            dst_factor: wgpu::BlendFactor::One,
                            operation: wgpu::BlendOperation::Add,
                        },
                        alpha: wgpu::BlendComponent {
                            src_factor: wgpu::BlendFactor::SrcAlpha,
                            dst_factor: wgpu::BlendFactor::One,
                            operation: wgpu::BlendOperation::Add,
                        },
                    }),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        };
        let additive_pipeline = kept.create_render_pipeline(&additive_descriptor);
        // `RenderPipelines.DRAGON_RAYS` uses DepthStencilState.DEFAULT:
        // unlike eyes, a ray hides the faces and rays behind it.
        let dragon_rays_pipeline = kept.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Dragon death rays"),
            primitive: wgpu::PrimitiveState {
                cull_mode: Some(wgpu::Face::Back),
                ..additive_descriptor.primitive
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                depth_write_enabled: Some(true),
                ..additive_descriptor.depth_stencil.clone().unwrap()
            }),
            ..additive_descriptor.clone()
        });
        // HDR output's mark on lightning bolts: fs_hdr_mark over the bolts' own vertex layout,
        // as hdr_mark_pipeline marks entities and particles (only alpha written).
        let mark_targets = [Some(wgpu::ColorTargetState {
            format: frame_format,
            blend: Some(wgpu::BlendState::REPLACE),
            write_mask: wgpu::ColorWrites::ALPHA,
        })];
        let hdr_mark_lightning_pipeline =
            kept.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("HDR lightning mark"),
                vertex: wgpu::VertexState {
                    entry_point: Some("vs_lightning"),
                    buffers: &lightning_vertex,
                    ..additive_descriptor.vertex.clone()
                },
                fragment: Some(wgpu::FragmentState {
                    module: &entity_shader,
                    entry_point: Some("fs_hdr_mark"),
                    compilation_options: Default::default(),
                    targets: &mark_targets,
                }),
                ..additive_descriptor.clone()
            });
        let lightning_pipeline = kept.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Lightning"),
            vertex: wgpu::VertexState {
                entry_point: Some("vs_lightning"),
                buffers: &lightning_vertex,
                ..additive_descriptor.vertex.clone()
            },
            ..additive_descriptor
        });
        let celestial_pipeline = kept.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Celestial bodies"),
            layout: Some(&entity_layout),
            vertex: wgpu::VertexState {
                module: &entity_shader,
                entry_point: Some("vs_entity"),
                compilation_options: Default::default(),
                buffers: &entity_vertex,
            },
            primitive: wgpu::PrimitiveState {
                cull_mode: None,
                ..Default::default()
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth32Float,
                depth_write_enabled: Some(false),
                depth_compare: Some(wgpu::CompareFunction::Always),
                stencil: Default::default(),
                bias: Default::default(),
            }),
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState {
                module: &entity_shader,
                entry_point: Some("fs_sky_celestial"),
                compilation_options: wgpu::PipelineCompilationOptions {
                    constants: &linear_blend::frame_constants(frame_format, linear_light),
                    ..Default::default()
                },
                targets: &[Some(wgpu::ColorTargetState {
                    format: frame_format,
                    // RenderPipelines.CELESTIAL: additive (src alpha, one), with
                    // the colour premultiplied in fs_sky_celestial. Its alpha is
                    // how much a texel adds: HDR output (hdr.wgsl) lets the sun's
                    // and moon's discs shine; nothing else reads the frame's alpha.
                    blend: Some(wgpu::BlendState {
                        color: wgpu::BlendComponent {
                            src_factor: wgpu::BlendFactor::One,
                            dst_factor: wgpu::BlendFactor::One,
                            operation: wgpu::BlendOperation::Add,
                        },
                        alpha: wgpu::BlendComponent {
                            src_factor: wgpu::BlendFactor::Zero,
                            dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                            operation: wgpu::BlendOperation::Add,
                        },
                    }),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });
        drop(entity_step);
        // WorldBorderRenderer: misc/forcefield.png as a MipmappedTexture
        // (repeat, linear minification with mipmaps, nearest magnification).
        let forcefield_size = forcefield.1;
        let (forcefield_mips, forcefield_levels) = prepared.forcefield;
        let forcefield_texture = device.create_texture_with_data(
            &queue,
            &wgpu::TextureDescriptor {
                label: Some("Forcefield"),
                size: wgpu::Extent3d {
                    width: forcefield_size,
                    height: forcefield_size,
                    depth_or_array_layers: 1,
                },
                mip_level_count: forcefield_levels,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8Unorm,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            },
            wgpu::util::TextureDataOrder::LayerMajor,
            &forcefield_mips,
        );
        let forcefield_view = forcefield_texture.create_view(&Default::default());
        let forcefield_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("Forcefield"),
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::Repeat,
            min_filter: wgpu::FilterMode::Linear,
            mag_filter: wgpu::FilterMode::Nearest,
            mipmap_filter: wgpu::MipmapFilterMode::Linear,
            ..Default::default()
        });
        let border_uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("World border"),
            size: std::mem::size_of::<BorderUniform>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let border_vertices = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("World border vertices"),
            size: 16 * std::mem::size_of::<crate::border_render::BorderVertex>() as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let border_bindings = kept.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("World border bindings"),
            entries: &[
                entry(
                    0,
                    vf,
                    wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                ),
                entry(
                    1,
                    vf,
                    wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                ),
                entry(
                    2,
                    vf,
                    wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                ),
                entry(
                    3,
                    vf,
                    wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                ),
            ],
        });
        let border_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("World border resources"),
            layout: &border_bindings,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: globals.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&forcefield_view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(&forcefield_sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: border_uniform.as_entire_binding(),
                },
            ],
        });
        let border_layout = kept.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("World border layout"),
            bind_group_layouts: &[Some(&border_bindings)],
            ..Default::default()
        });
        let border_shader = kept.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Horizon world border"),
            source: wgpu::ShaderSource::Wgsl(border_shader().into()),
        });
        let border_attributes = wgpu::vertex_attr_array![0=>Float32x3,1=>Float32x2];
        // RenderPipelines.WORLD_BORDER: BlendFunction.OVERLAY, no culling,
        // depth writes with a polygon offset of 3 towards the camera.
        let border_pipeline = kept.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("World border"),
            layout: Some(&border_layout),
            vertex: wgpu::VertexState {
                module: &border_shader,
                entry_point: Some("vs_border"),
                compilation_options: Default::default(),
                buffers: &[Some(wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<crate::border_render::BorderVertex>() as u64,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &border_attributes,
                })],
            },
            primitive: wgpu::PrimitiveState {
                cull_mode: None,
                ..Default::default()
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth32Float,
                depth_write_enabled: Some(true),
                depth_compare: Some(wgpu::CompareFunction::GreaterEqual),
                stencil: Default::default(),
                bias: wgpu::DepthBiasState {
                    constant: 3,
                    slope_scale: 3.0,
                    clamp: 0.0,
                },
            }),
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState {
                module: &border_shader,
                entry_point: Some("fs_border"),
                compilation_options: wgpu::PipelineCompilationOptions {
                    constants: &linear_blend::frame_constants(frame_format, linear_light),
                    ..Default::default()
                },
                targets: &[Some(wgpu::ColorTargetState {
                    format: frame_format,
                    blend: Some(wgpu::BlendState {
                        color: wgpu::BlendComponent {
                            src_factor: wgpu::BlendFactor::SrcAlpha,
                            dst_factor: wgpu::BlendFactor::One,
                            operation: wgpu::BlendOperation::Add,
                        },
                        alpha: wgpu::BlendComponent {
                            src_factor: wgpu::BlendFactor::One,
                            dst_factor: wgpu::BlendFactor::Zero,
                            operation: wgpu::BlendOperation::Add,
                        },
                    }),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });
        let screen_shader = kept.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Horizon screen effects"),
            source: wgpu::ShaderSource::Wgsl(include_str!("screen.wgsl").into()),
        });
        let screen_attributes =
            wgpu::vertex_attr_array![0=>Float32x2,1=>Float32x2,2=>Float32x4,3=>Float32x4];
        let screen_vertex = [Some(wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<crate::screen_effects::ScreenVertex>() as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &screen_attributes,
        })];
        let vignette_blend = wgpu::BlendState {
            color: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::Zero,
                dst_factor: wgpu::BlendFactor::OneMinusSrc,
                operation: wgpu::BlendOperation::Add,
            },
            alpha: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::Zero,
                dst_factor: wgpu::BlendFactor::One,
                operation: wgpu::BlendOperation::Add,
            },
        };
        // One per blend and `.png.mcmeta` sampling (screen_effects::SAMPLE_*):
        // the shader is specialised, so a pixel pays for none of the others.
        let screen_pipelines: [wgpu::RenderPipeline; 16] = std::array::from_fn(|i| {
            let blur = i as u32 & crate::screen_effects::SAMPLE_BLUR != 0;
            let (entry, blend) = match (i & 4 == 0, blur) {
                (true, false) => ("fs_screen", alpha_blend),
                (true, true) => ("fs_screen_blur", alpha_blend),
                (false, false) => ("fs_vignette", vignette_blend),
                (false, true) => ("fs_vignette_blur", vignette_blend),
            };
            let clamp = i as u32 & crate::screen_effects::SAMPLE_CLAMP != 0;
            let constants = [("CLAMP", f64::from(u8::from(clamp)))];
            let compilation_options = wgpu::PipelineCompilationOptions {
                constants: &constants,
                ..Default::default()
            };
            kept.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("Screen effects"),
                layout: Some(&entity_layout),
                vertex: wgpu::VertexState {
                    module: &screen_shader,
                    entry_point: Some("vs_screen"),
                    compilation_options: compilation_options.clone(),
                    buffers: &screen_vertex,
                },
                primitive: wgpu::PrimitiveState {
                    cull_mode: None,
                    ..Default::default()
                },
                depth_stencil: None,
                multisample: Default::default(),
                fragment: Some(wgpu::FragmentState {
                    module: &screen_shader,
                    entry_point: Some(entry),
                    compilation_options,
                    targets: &[Some(wgpu::ColorTargetState {
                        format: overlay_format,
                        blend: Some(blend),
                        // 8 and up: colour only, for a tint of the world in HDR output.
                        write_mask: if i & 8 == 0 {
                            wgpu::ColorWrites::ALL
                        } else {
                            wgpu::ColorWrites::COLOR
                        },
                    })],
                }),
                multiview_mask: None,
                cache: None,
            })
        });
        let screen_capacity = 64;
        let screen_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Screen effect vertices"),
            size: screen_capacity
                * std::mem::size_of::<crate::screen_effects::ScreenVertex>() as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let celestial_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Celestial vertices"),
            size: 2048 * 4 * std::mem::size_of::<Vertex>() as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let sunrise_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Sunrise fan vertices"),
            size: (sunrise_fan::VERTICES * 5 * std::mem::size_of::<f32>()) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let entity_capacity = 1 << 14;
        let entity_skin = entity_skin::EntitySkin::new(
            &device,
            animate::SpriteAnimator::compute(&adapter, &device),
        );
        let entity_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Entity vertices"),
            size: entity_capacity * std::mem::size_of::<Vertex>() as u64,
            usage: entity_skin::EntitySkin::usage(entity_skin.is_some()),
            mapped_at_creation: false,
        });
        let static_entity_index_quads = 1 << 14;
        let static_entity_indices = Self::quad_indices(&device, static_entity_index_quads);
        let indirect_execution = adapter
            .get_downlevel_capabilities()
            .flags
            .contains(wgpu::DownlevelFlags::INDIRECT_EXECUTION);
        let flat_wanted = flat::wanted(&adapter, &device);
        // Metal draws solid terrain in two passes (`split_solid`): meshes put all
        // their opaque quads first, so each pass draws its part in a run or two.
        crate::mesher::group_parts(adapter.get_info().backend == wgpu::Backend::Metal);
        // Readable as storage where flat draws run: a shader pack's terrain program reads them.
        let arena = arena::Arena::new(
            &device,
            std::mem::size_of::<TerrainVertex>() as u64,
            1 << 20,
            flat_wanted,
        );
        // Sections as one index list per frame where multi-draw is emulated.
        let mut flat = flat_wanted.then(|| {
            let buffers = [Some(wgpu::VertexBufferLayout {
                array_stride: std::mem::size_of::<TerrainVertex>() as u64,
                step_mode: wgpu::VertexStepMode::Vertex,
                attributes: &TERRAIN_ATTRIBUTES,
            })];
            let info = adapter.get_info();
            // Metal's per-section path drops cutout leaves (the canopy above trunks vanishes),
            // and cave culling can drop the listed count below MIN_SECTIONS on a normal view:
            // so Metal draws vanilla terrain flat at any count.
            let min_sections = if flat::is_broken_intel_vulkan(&info)
                || flat::is_adreno_dx12(&info)
                || info.backend == wgpu::Backend::Metal
                || std::env::var("HORIZON_FLAT_DRAWS").as_deref() == Ok("1")
            {
                0
            } else if flat::for_vanilla(&info) {
                flat::MIN_SECTIONS
            } else {
                // Only a shader pack's terrain and shadow pass draw flat here.
                usize::MAX
            };
            flat::Flat::new(
                &device,
                &bindings,
                arena.page_vertices(),
                min_sections,
                indirect_execution,
                |layout, label, vertex, fragment, blend, cull, pulled| {
                    let buffers: &[Option<wgpu::VertexBufferLayout<'_>>] =
                        if pulled { &[] } else { &buffers };
                    pipeline_in(
                        layout, label, vertex, fragment, buffers, tri, cull, true, true, blend,
                    )
                },
                |layout, label, vertex, fragment| solid_in(layout, label, vertex, fragment, &[]),
            )
        });
        // With flat draws' page groups the per-section solid draws read their vertices from
        // the page too (`Flat::page_group`), in either layout of the solid quads
        // (`mesher::packed`): only the origins stay a vertex buffer, at slot 1.
        let (solid, solid_opaque, solid_cutout, solid_mirror) = match &flat {
            Some(f) => {
                let pulled_layout = kept.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                    label: Some("Horizon terrain layout (pulled)"),
                    bind_group_layouts: &[Some(&bindings), Some(&f.draw_layout)],
                    ..Default::default()
                });
                let origins = [None, terrain_buffers[1].clone()];
                let solid = |label: &str, vertex: &str, fragment: &str| {
                    solid_in(&pulled_layout, label, vertex, fragment, &origins)
                };
                (
                    solid("Terrain solid", "vs_section", "fs_solid"),
                    solid("Terrain solid opaque", "vs_section_opaque", "fs_opaque"),
                    solid("Terrain solid cutout", "vs_section_cutout", "fs_solid"),
                    pipeline_in(
                        &pulled_layout,
                        "Terrain solid (mirrored)",
                        "vs_section",
                        "fs_solid_mirror",
                        &origins,
                        tri,
                        Some(wgpu::Face::Front),
                        true,
                        true,
                        None,
                    ),
                )
            }
            None => (solid, solid_opaque, solid_cutout, solid_mirror),
        };
        // The solid quads in the compact layout, while no shader pack reads the arena
        // (`follow_packed_layout`).
        let packed_quads = flat.is_some() && !shader_pack && packed_wanted();
        packed::pack_quads(packed_quads);
        // A native mod's world warp builds its pipelines with these layouts once it is given one.
        let world_warps = world_warp::WorldWarps::new(world_warp::Kit {
            terrain: bindings.clone(),
            pages: flat.as_ref().map(|f| f.draw_layout.clone()),
            entity: entity_bindings.clone(),
            static_entity: static_entity_bindings.clone(),
            format: frame_format,
            linear: linear_light,
            sprite_constants,
            hdr_output: hdr_output.is_some(),
        });
        let visbuf = flat
            .as_ref()
            .filter(|_| visbuffer::wanted(&device))
            .map(|f| {
                log::info!(
                    "Horizon: solid terrain through a visibility buffer (HORIZON_VISBUFFER)"
                );
                visbuffer::Visbuffer::new(
                    &device,
                    &bindings,
                    f,
                    (frame_format, linear_light),
                    &sprite_constants,
                )
            });
        let multi_draw = device
            .features()
            .contains(wgpu::Features::INDIRECT_FIRST_INSTANCE)
            && indirect_execution;
        let batchable = translucent_batchable(&adapter.get_info(), multi_draw);
        crate::graphics::offer_translucent_batching(batchable);
        let translucent_batching = batchable && translucent_batching;
        let gpu_culling_offer = gpu_culling::offer(
            &adapter.get_info(),
            gpu_culling::supported(
                &adapter.get_info(),
                device.features(),
                adapter.get_downlevel_capabilities().flags,
                &device.limits(),
                multi_draw,
                flat.as_ref().map(|f| f.min_sections),
            ),
            flat.is_some(),
        );
        // Not again after the device was lost in one of its frames, until Horizon restarts.
        let gpu_culling_offer = if crate::graphics::gpu_culling_lost() {
            crate::graphics::GpuCullingOffer::DeviceLost
        } else {
            gpu_culling_offer
        };
        crate::graphics::offer_gpu_culling(gpu_culling_offer);
        // A new renderer runs no shader pack: the game loads the picked one on its
        // first world frame (`shader_pack`: the picked pack draws flat, a `shaders/`
        // pack or one of Horizon's own with `[shadow]`), and the frames before it, the
        // level loading screen's among them, keep the layout the sections upload in
        // (`draw_frame`). It starts as that pack's, a `shaders/` pack taken to draw
        // terrain: the pack loading keeps it, and only one whose terrain and shadow
        // pass draw none meshes the sections again.
        let (sorted_indices, translucent_arena) = arena::Arena::translucent_arenas(
            &device,
            std::mem::size_of::<TerrainVertex>() as u64,
            flat.is_some(),
            translucent_split(translucent_batching, flat.is_some(), shader_pack, false),
        );
        if let (Some(flat), Some(arena)) = (&mut flat, &translucent_arena) {
            flat.set_translucent_page_vertices(arena.page_vertices());
        }
        // Without indirect draws (OpenGL before 4.0 without indirect execution, or
        // before 4.2 + GL_ARB_shader_draw_parameters without indirect first instance)
        // even creating an indirect buffer fails; one draw per section reads
        // `draws` on the CPU instead.
        let indirect = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Section draws"),
            size: 1024 * 20,
            usage: if multi_draw {
                wgpu::BufferUsages::INDIRECT | wgpu::BufferUsages::COPY_DST
            } else {
                wgpu::BufferUsages::COPY_DST
            },
            mapped_at_creation: false,
        });
        // HORIZON_GPU_CULL=0 turns occlusion culling off, `all` culls every
        // frame however few its draws and little it hides, `shader` tests in
        // the shader as on Metal (A/B checks).
        let cull_env = std::env::var("HORIZON_GPU_CULL");
        let adapter_info = adapter.get_info();
        let cull = cull::Cull::supported(&adapter, &device)
            .filter(|_| occlusion_culling_requested(&adapter_info, cull_env.as_deref().ok()))
            .map(|depth_test| match cull_env.as_deref() {
                Ok("shader") => cull::DepthTest::InShader,
                _ => depth_test,
            })
            .map(|depth_test| {
                let mut cull = cull::Cull::new(&device, frame_format, depth_test);
                cull.always = cull_env.as_deref() == Ok("all");
                if !cull.always && adapter_info.backend == wgpu::Backend::Metal {
                    cull.min_sections = cull::TILED_MIN_SECTIONS;
                }
                cull
            });
        let depth = Self::depth(&device, &config);
        let world_post = world_post::Post::new(&device, frame_format);
        let uploads = upload::Uploads::new(&device);
        let section_uploads = upload::Uploads::with_chunk(&device, SECTION_UPLOAD_CHUNK);
        let stars = stars::Stars::new(&device, (frame_format, linear_light));
        let custom_sky = custom_sky::CustomSky::new(&device, (frame_format, linear_light));
        let entity_outline = entity_outline::EntityOutline::new(
            &device,
            (&entity_shader, &entity_layout, &entity_vertex),
            overlay_format,
        );
        let panorama = panorama::Panorama::new(&device, frame_format, panorama_faces);
        drop(pipelines_step);
        reuse.finish(generation);
        if previous.is_some() {
            log::info!(
                "Horizon renderer: rebuilt on the same device ({} shader modules, layouts and pipelines kept)",
                reuse.len()
            );
        }
        if previous.is_none() {
            startup_step(format_args!(
                "pipelines ready (multi-draw {}, occlusion culling {}, GPU culling {}, GPU timer {})",
                on_off(multi_draw),
                on_off(cull.is_some()),
                if gpu_culling_offer == crate::graphics::GpuCullingOffer::Offered {
                    "available"
                } else {
                    "unavailable"
                },
                on_off(gpu_timer.is_some()),
            ));
        }
        let world_primitives =
            world_primitives::WorldPrimitives::new(&device, (frame_format, linear_light));
        Ok(Self {
            display: None,
            reading: display,
            display_fresh: false,
            instance,
            adapter,
            surface: surface.map(Arc::new),
            spare_surface: std::sync::Mutex::new(None),
            offscreen,
            in_flight: std::collections::VecDeque::new(),
            paced_elsewhere: false,
            first_frame: if previous.is_none() {
                FirstFrame::Acquire
            } else {
                FirstFrame::Drawn
            },
            gpu_timer,
            gpu_times: Vec::new(),
            lightmap_pixels: [0; 16 * 16 * 4],
            last_wait: std::time::Duration::ZERO,
            last_encode: std::time::Duration::ZERO,
            window,
            device,
            reuse,
            queue: submit::Queue::new(queue, submit::wanted()),
            deferred: None,
            device_lost,
            config,
            solid,
            solid_opaque,
            solid_cutout,
            translucent,
            translucent_mark,
            world_warps,
            flat_on: flat.is_some(),
            packed_quads,
            cull_on: cull.is_some(),
            sides_on: true,
            gpu_culling: None,
            gpu_culling_offer,
            gpu_frame: false,
            #[cfg(debug_assertions)]
            gpu_checks: 0,
            cull,
            flat,
            visbuf,
            sky,
            sky_base,
            sunrise_pipeline,
            sunrise_buffer,
            dark_disc,
            depth_reset,
            lines,
            lines_secondary,
            core_lines: None,
            core_sky: None,
            debug_lines_opaque,
            debug_lines,
            debug_lines_on_top,
            debug_crosshair,
            overlay_crosshair,
            overlay_hand,
            debug_line_buffer,
            debug_line_capacity: 4096,
            crosshair_group,
            crosshair_globals,
            solid_mirror,
            mirror_globals,
            mirror_group,
            mirror: None,
            entity_pipeline,
            static_entity_pipeline,
            hdr_mark_pipeline,
            hdr_entity_mark_pipeline,
            hdr_hand_unmark_pipeline,
            hdr_mark_lightning_pipeline,
            weather,
            crumbling_pipeline,
            shadow_blob_pipeline,
            translucent_item_pipeline,
            water_mask_pipeline,
            cloud_pipeline,
            hdr_cloud_mark_pipeline,
            cloud_mesh: Default::default(),
            cloud_buffer,
            cloud_capacity: 4,
            cloud_uniform,
            cloud_group,
            additive_pipeline,
            dragon_rays_pipeline,
            energy_swirl_pipeline,
            lightning_pipeline,
            lightning: lightning::Renderer::new(),
            see_through_pipeline,
            world_primitives,
            text_polygon_offset_pipeline,
            world_text_pipeline,
            fluid_pipeline,
            terrain_model_pipelines,
            celestial_pipeline,
            celestial_buffer,
            end_sky: end_sky::EndSky::default(),
            stars,
            custom_sky,
            custom_sky_draws: Vec::new(),
            entity_outline,
            panorama,
            border_pipeline,
            border_group,
            border_uniform,
            border_vertices,
            screen_pipelines,
            screen_buffer,
            screen_capacity,
            entity_group,
            entity_bindings,
            static_entity_group,
            static_entity_transform,
            static_entity_camera: None,
            static_entity_catalog_revision: 0,
            static_entity_frame_stats: Default::default(),
            hand_group,
            hand_globals,
            entity_buffer,
            entity_capacity,
            static_entity_sections: HashMap::default(),
            static_entity_indices,
            static_entity_index_quads,
            entity_batch: EntityBatch::default(),
            entity_skin,
            group,
            fluid_group,
            fluid_sampler,
            terrain_model_group,
            terrain_model_sampler,
            globals,
            pbr_maps,
            lightmap,
            sprite_textures,
            sprite_unused,
            sprite_views,
            mip_levels,
            mipmaps: mip_levels - 1,
            terrain_layout: bindings,
            terrain_sprites: sprite_constants.map(|(_, v)| v),
            sprite_split: split,
            sprite_sampler,
            sprite_nearest,
            texture_filtering: 0,
            max_anisotropy: 1,
            lightmap_view,
            lightmap_sampler,
            animations,
            material_animations,
            active_animated_layers: vec![0; active_layer_words],
            uploads,
            section_uploads,
            section_copies: None,
            section_copy_bytes: 0,
            shrink_gate: ShrinkGate::default(),
            lightmap_uploaded: false,
            sprite_minimum_size,
            sprite_native: sprite_mips.iter().map(|s| s.native_size).collect(),
            mipmap_option: 4,
            indices,
            index_quads,
            instances,
            instance_capacity,
            outline,
            depth,
            sections: resident::Resident::default(),
            lods: FxHashMap::default(),
            lod_cull_free: Vec::new(),
            lod_cull_next: 0,
            lod_cutout: Default::default(),
            lod_retiring: Default::default(),
            lod_leaving: Vec::new(),
            hold: Default::default(),
            lod_pending: None,
            detail_columns: Default::default(),
            detail_revision: 0,
            arena,
            translucent_arena,
            translucent_batching,
            sections_dropped: false,
            sorted_indices,
            graveyard: Default::default(),
            translucent_sorter: translucency::Sorter::default(),
            translucency_cursor: 0,
            indirect,
            indirect_capacity: 1024,
            multi_draw,
            visible: Vec::new(),
            visible_lods: Vec::new(),
            shadow_visible: Vec::new(),
            translucent_draws: Vec::new(),
            origins: Vec::new(),
            draws: Vec::new(),
            draw_pages: Vec::new(),
            phase2_draws: Vec::new(),
            phase2_pages: Vec::new(),
            solid_runs: page_runs::PageRuns::default(),
            phase2_runs: page_runs::PageRuns::default(),
            shadow_runs: page_runs::PageRuns::default(),
            block_entities: Default::default(),
            block_entity_section_revisions: Default::default(),
            block_entity_membership_revisions: Default::default(),
            section_uploaded: Default::default(),
            newest_upload: None,
            newest_lod_fade: None,
            block_entity_revision: next_block_entity_revision(),
            light_blocks: Default::default(),
            light_revision: 0,
            occlusion: occlusion::Occlusion::new(crate::chunk::MIN_Y, crate::chunk::HEIGHT / 16),
            layout: crate::chunk::ColumnLayout::VANILLA,
            world_post,
            frame_gen: frame_gen::FrameGen::new(),
            entity_atlas: atlas_view.clone(),
            hdr: None,
            hdr_lighting_setting: true,
            emission_stats,
            output_caps,
            output_rejected: None,
            // Nothing is presented as HDR yet: `update_output` applies the settings.
            output: output::resolve_output(settings, output_caps, display, frame, false),
            frame_kept_logged: false,
            hdr_marker: None,
            hdr_driver,
            pack_post: None,
            pack_loading: None,
            native: None,
            pass_timing: pack_timing::PassTiming::logging(),
            pass_timing_gap: std::time::Duration::ZERO,
            own_timing: None,
            pack_entity_rest: Vec::new(),
            cloud_bindings,
            pack_weather: None,
            pack_atlas: None,
            pack_material_atlases: None,
            shared_material_maps: [None, None],
            pack_terrain_flat: false,
            separate_ao: false,
            pack_pages: Vec::new(),
            outline_edges_scratch: Vec::new(),
            outline_lines_scratch: Vec::new(),
            debug_lines_scratch: Vec::new(),
            screen_vertices_scratch: Vec::new(),
            lod_leaving_scratch: crate::fast_hash::FxHashSet::default(),
            shadow_boxed_scratch: Vec::new(),
            shadow_seen_scratch: crate::fast_hash::FxHashSet::default(),
            shadow_translucent_scratch: Vec::new(),
            pack_lava_layers: Vec::new(),
            hud,
            menu_blur: None,
            post_effects: None,
            readback: readback::Readback::default(),
            info,
            suspended: !drawable_size(size.width, size.height, max_side),
            acquire_validation_failures: 0,
            surface_unconfigured: false,
            drawn_sections: 0,
            section_draws: 0,
            section_draw_calls: 0,
            frame_serial: 0,
            stats_frame: 0,
            uploaded_bytes: 0,
            started: Instant::now(),
            screenshot: None,
            screenshot_result: None,
            screenshot_hdr: None,
            screenshot_quality: 1,
            screenshot_halvings: 0,
            gamma: 0.5,
            improved_transparency: false,
            oit: None,
            fullbright: false,
            animated_textures: true,
            present_modes,
            capture_copies: std::env::var_os("HORIZON_RENDER_CAPTURE")
                .is_some()
                .then(capture::Copies::default),
            capture: None,
            capture_result: None,
        })
    }
    /// The first-person hand's quads, which follow `first` quads of the entity buffer.
    fn draw_hand(&self, pass: &mut wgpu::RenderPass<'_>, quads: u32, first: u32) {
        pass.set_pipeline(&self.entity_pipeline);
        pass.set_bind_group(0, &self.hand_group, &[]);
        pass.set_index_buffer(self.indices.slice(..), wgpu::IndexFormat::Uint32);
        pass.set_vertex_buffer(0, self.entity_buffer.slice(..));
        pass.draw_indexed(0..quads * 6, (first * 4) as i32, 0..1);
        if light_marks(self.output.frame.format, self.hdr.is_some()) {
            // Drawn into the world: the light sources it holds are marked as the world's are.
            pass.set_pipeline(&self.hdr_entity_mark_pipeline);
            pass.draw_indexed(0..quads * 6, (first * 4) as i32, 0..1);
        }
    }
    /// HDR lighting on the clouds just drawn: the sky's value (`fs_hdr_cloud_mark`).
    fn mark_clouds(&self, pass: &mut wgpu::RenderPass<'_>, quads: u32) {
        if !light_marks(self.output.frame.format, self.hdr.is_some())
            || self.hdr.as_ref().is_none_or(|h| !h.lighting)
        {
            return;
        }
        pass.set_pipeline(&self.hdr_cloud_mark_pipeline);
        pass.set_bind_group(0, &self.entity_group, &[]);
        pass.set_bind_group(1, &self.cloud_group, &[]);
        pass.set_vertex_buffer(0, self.cloud_buffer.slice(..));
        pass.set_index_buffer(self.indices.slice(..), wgpu::IndexFormat::Uint32);
        pass.draw_indexed(0..quads * 6, 0, 0..1);
        pass.set_bind_group(0, &self.group, &[]);
    }
    /// HDR lighting on translucent terrain, over the draws that just blended it.
    fn mark_translucent_terrain(
        &self,
        pass: &mut wgpu::RenderPass<'_>,
        flat: Option<&flat::Flat>,
        solid_draws: u32,
        translucent_draws: u32,
    ) {
        if !light_marks(self.output.frame.format, self.hdr.is_some())
            || self.hdr.as_ref().is_none_or(|h| !h.lighting)
        {
            return;
        }
        match flat {
            Some(flat) => {
                pass.set_pipeline(&flat.translucent_mark);
                flat.draw(
                    pass,
                    flat.translucent_calls(),
                    flat_vertices(&self.arena, self.translucent_arena.as_ref()),
                );
                pass.set_bind_group(0, &self.group, &[]);
            }
            None => {
                pass.set_pipeline(&self.translucent_mark);
                pass.set_vertex_buffer(1, self.instances.slice(..));
                self.draw_sections(pass, &self.indirect, 0, solid_draws, translucent_draws);
            }
        }
        pass.set_index_buffer(self.indices.slice(..), wgpu::IndexFormat::Uint32);
    }
    /// The shared atlas's LabPBR maps (`HudAtlas::material_maps`), uploaded for the shader packs
    /// loaded from now on; a map too large for this GPU is left out.
    pub fn set_shared_material_maps(&mut self, maps: &[Option<crate::paged_maps::PagedMap>; 2]) {
        let limit = self.device.limits().max_texture_dimension_2d;
        let sizes: Vec<String> = (maps.iter().zip(["normals", "specular"]))
            .filter_map(|(m, name)| {
                m.as_ref()
                    .map(|m| format!("{name} {}x{}", m.width, m.height))
            })
            .collect();
        if !sizes.is_empty() {
            let bytes: usize = maps.iter().flatten().map(|m| m.pixels.len()).sum();
            log::info!(
                "shared atlas LabPBR maps: {} ({:.1} MiB)",
                sizes.join(", "),
                bytes as f64 / f64::from(1 << 20)
            );
        }
        self.shared_material_maps = maps.each_ref().map(|map| {
            let map = map.as_ref().filter(|m| m.width.max(m.height) <= limit)?;
            Some(self.device.create_texture_with_data(
                &self.queue,
                &wgpu::TextureDescriptor {
                    label: Some("Shared atlas material map"),
                    size: wgpu::Extent3d {
                        width: map.width,
                        height: map.height,
                        depth_or_array_layers: 1,
                    },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: wgpu::TextureFormat::Rgba8Unorm,
                    usage: wgpu::TextureUsages::TEXTURE_BINDING,
                    view_formats: &[],
                },
                wgpu::util::TextureDataOrder::LayerMajor,
                &map.pixels,
            ))
        });
    }
    /// The pack's OptiFine custom sky layer textures (`crate::custom_sky::CustomSky::load`).
    pub fn set_custom_sky(&mut self, pack: &crate::custom_sky::CustomSky) {
        self.custom_sky.set_pack(&self.device, &self.queue, pack);
    }
    /// The custom sky layers the next frame draws (`CustomSky::draws`): the game refills it each
    /// frame, empty when the world's sky has none.
    pub fn custom_sky_draws(&mut self) -> &mut Vec<crate::custom_sky::Draw> {
        &mut self.custom_sky_draws
    }
    /// The resource packs' `core/rendertype_lines` (`HudAtlas::lines_shader`) for the block
    /// outline, or the built-in one.
    pub fn set_lines_shader(&mut self, program: Option<&crate::core_shader::CoreProgram>) {
        let format = self.output.frame.format;
        // A pack's GLSL writes vanilla's gamma: a frame of linear light keeps the built-in one.
        let program = program.filter(|_| !self.output.frame.linear_light);
        self.core_lines =
            program.and_then(|program| core_lines::CoreLines::new(&self.device, format, program));
    }
    /// The resource packs' `core/sky` and `core/stars` (`HudAtlas::sky_shader`,
    /// `stars_shader`), or the built-in sky and stars.
    pub fn set_sky_shaders(
        &mut self,
        disc: Option<&crate::core_shader::CoreProgram>,
        stars: Option<&crate::core_shader::CoreProgram>,
    ) {
        // As `set_lines_shader`: not on a frame of linear light.
        let keep = !self.output.frame.linear_light;
        let (disc, stars) = (disc.filter(|_| keep), stars.filter(|_| keep));
        self.core_sky = core_sky::CoreSky::new(&self.device, self.output.frame.format, disc, stars);
    }
    /// LevelRenderer.submitBlockOutline: the buffer holds `edges` per stroke, the black
    /// secondary stroke first when high contrast is on.
    fn draw_outline(&self, pass: &mut wgpu::RenderPass<'_>, edges: u32, high_contrast: bool) {
        pass.set_vertex_buffer(0, self.outline.slice(..));
        // Both strokes are LINES: a pack's rendertype_lines draws them.
        if let Some(lines) = &self.core_lines {
            if high_contrast {
                lines.draw(pass, true, 0..edges);
            }
            let first = if high_contrast { edges } else { 0 };
            lines.draw(pass, false, first..first + edges);
            pass.set_bind_group(0, &self.group, &[]);
            return;
        }
        if high_contrast {
            pass.set_pipeline(&self.lines_secondary);
            pass.draw(0..4, 0..edges);
        }
        pass.set_pipeline(&self.lines);
        let first = if high_contrast { edges } else { 0 };
        pass.draw(0..4, first..first + edges);
    }
    fn quad_indices(device: &wgpu::Device, quads: u32) -> wgpu::Buffer {
        let indices: Vec<u32> = (0..quads)
            .flat_map(|q| {
                let b = q * 4;
                [b, b + 1, b + 2, b, b + 2, b + 3]
            })
            .collect();
        device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Quad indices"),
            contents: bytemuck::cast_slice(&indices),
            usage: wgpu::BufferUsages::INDEX,
        })
    }
    fn offscreen_target(
        device: &wgpu::Device,
        config: &wgpu::SurfaceConfiguration,
    ) -> wgpu::Texture {
        device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Offscreen frame"),
            size: wgpu::Extent3d {
                width: config.width,
                height: config.height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: config.format,
            usage: config.usage,
            view_formats: &config.view_formats,
        })
    }
    /// Hand the window surface over to the renderer that replaces this one on
    /// the same device (it takes `spare_surface`).
    pub fn release_surface(&mut self) {
        self.frame_gen.stop();
        self.queue.wait();
        *self
            .spare_surface
            .get_mut()
            .unwrap_or_else(std::sync::PoisonError::into_inner) =
            self.surface.take().and_then(|s| Arc::try_unwrap(s).ok());
    }
    /// Reattach a window after surface loss or a failed renderer replacement.
    pub fn recreate_surface(&mut self) -> Result<()> {
        self.queue.wait();
        if self.offscreen.is_some() {
            return Ok(());
        }
        // The surface set aside for a replacement that failed, else a new one
        // (Wayland allows only one tearing-control object per window surface,
        // so the old one goes first).
        let spare = self
            .spare_surface
            .get_mut()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take();
        self.frame_gen.stop();
        self.surface = None;
        let surface = match spare {
            Some(surface) => surface,
            None => self.instance.create_surface(self.window.clone())?,
        };
        let configured = Self::configure(
            &self.device,
            &surface,
            &self.surface_config(),
            self.hdr.as_ref(),
            &self.device_lost,
        );
        if let Err(e) = &configured {
            log::warn!(
                "Horizon renderer: the new window surface did not configure ({e}); retrying"
            );
        }
        self.surface_unconfigured = configured.is_err();
        self.surface = Some(Arc::new(surface));
        self.surface_recreated();
        Ok(())
    }
    /// The world's depth, which occlusion culling's test and post effects may read.
    fn depth(device: &wgpu::Device, config: &wgpu::SurfaceConfiguration) -> wgpu::TextureView {
        device
            .create_texture(&wgpu::TextureDescriptor {
                label: Some("Horizon depth"),
                size: wgpu::Extent3d {
                    width: config.width,
                    height: config.height,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Depth32Float,
                // TEXTURE_BINDING: a post effect may read the depth buffer.
                // COPY_SRC: improved transparency and a native shader pack's
                // `opaque_depth` copy it.
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                    | wgpu::TextureUsages::TEXTURE_BINDING
                    | wgpu::TextureUsages::COPY_SRC,
                view_formats: &[],
            })
            .create_view(&Default::default())
    }
    /// VSync on presents with Fifo, capped to the display's refresh rate in a
    /// window and in fullscreen alike; Horizon's present-mode setting applies
    /// only with VSync off (`window::chosen_present_mode`).
    pub fn set_present_mode(&mut self, vsync: bool, mode: crate::graphics::PresentMode) {
        use crate::graphics::PresentMode as P;
        use crate::window::{PresentMode, chosen_present_mode};
        let supported: Vec<PresentMode> = self
            .present_modes
            .iter()
            .filter_map(|m| match m {
                wgpu::PresentMode::Fifo => Some(PresentMode::Fifo),
                wgpu::PresentMode::Mailbox => Some(PresentMode::Mailbox),
                wgpu::PresentMode::Immediate => Some(PresentMode::Immediate),
                _ => None,
            })
            .collect();
        let setting = match mode {
            P::Auto => None,
            P::Fifo => Some(PresentMode::Fifo),
            P::Mailbox => Some(PresentMode::Mailbox),
            P::Immediate => Some(PresentMode::Immediate),
        };
        let mode = match chosen_present_mode(&supported, vsync, setting) {
            Some(PresentMode::Mailbox) => wgpu::PresentMode::Mailbox,
            Some(PresentMode::Immediate) => wgpu::PresentMode::Immediate,
            _ => wgpu::PresentMode::Fifo,
        };
        if self.config.present_mode != mode && self.surface.is_some() {
            self.config.present_mode = mode;
            self.configure_surface();
        }
    }
    /// Follows the window to `width` x `height`. A size the swapchain can't take
    /// ([`drawable_size`]) keeps the last one and draws nothing until the next.
    pub fn resize(&mut self, width: u32, height: u32) {
        let max = self.device.limits().max_texture_dimension_2d;
        self.suspended = !drawable_size(width, height, max);
        if self.suspended {
            return;
        }
        self.config.width = width;
        self.config.height = height;
        if self.surface.is_some() {
            self.configure_surface();
        } else {
            self.offscreen = Some(Self::offscreen_target(&self.device, &self.config));
        }
        self.depth = Self::depth(&self.device, &self.config);
    }
    /// Whether sections are drawn as one index list per frame (`flat`).
    pub fn flat_draws(&self) -> bool {
        self.flat_on
    }
    /// Switches between flat and per-section draws, for A/B checks.
    pub fn set_flat_draws(&mut self, on: bool) -> Result<()> {
        if on && self.flat.is_none() {
            return Err("flat section draws are off on this device".into());
        }
        self.flat_on = on;
        Ok(())
    }
    /// Whether one of Horizon's own packs reads the water mirror, which needs the frame's
    /// `water_plane`.
    pub fn wants_water_plane(&self) -> bool {
        self.native.as_ref().is_some_and(|n| n.frame.reflects)
    }
    /// Whether solid quads facing away from the eye are left out.
    pub fn section_sides(&self) -> bool {
        self.sides_on
    }
    /// Switches leaving out solid quads facing away, for A/B checks.
    pub fn set_section_sides(&mut self, on: bool) {
        self.sides_on = on;
    }
    /// Whether solid sections are occlusion culled (`cull`).
    pub fn section_cull(&self) -> bool {
        self.cull_on
    }
    /// Switches occlusion culling, for A/B checks.
    pub fn set_section_cull(&mut self, on: bool) -> Result<()> {
        if on && self.cull.is_none() {
            return Err("occlusion culling is off on this device".into());
        }
        self.cull_on = on;
        Ok(())
    }
    /// Of the boxes occlusion culling's last test read back drew, how many
    /// it did not see and how many it drew, and the share of solid indices
    /// it hid.
    pub fn culled_sections(&self) -> Option<([usize; 2], f32)> {
        if self.gpu_frame {
            // GPU culling's own test, on Vulkan.
            let gpu = self.gpu_culling.as_ref().filter(|_| self.cull_on);
            return gpu.and_then(|g| g.hidden());
        }
        self.cull
            .as_ref()
            .filter(|_| self.cull_on)
            .map(|c| c.hidden)
    }
    /// Whether the near solid sections are culled and drawn on the GPU where
    /// frames allow it (`gpu_culling`).
    pub fn gpu_culling(&self) -> bool {
        self.gpu_culling.is_some()
    }
    /// Whether the last frame GPU culling drew ran its occlusion test.
    pub fn gpu_occluding(&self) -> bool {
        self.gpu_frame && self.gpu_culling.as_ref().is_some_and(|g| g.occluding())
    }
    /// Whether the last frame drew them so.
    pub fn gpu_frame(&self) -> bool {
        self.gpu_frame
    }
    /// Of the sections GPU culling keeps records of, those it leaves to the CPU
    /// while they fade (`SectionGpu::gpu_fading`); `None` while it is off.
    pub fn gpu_fading(&self) -> Option<u32> {
        self.gpu_culling.as_ref().map(|g| g.records.fading())
    }
    /// Switches GPU culling: the GPU Culling setting sets it (`apply_ui_settings`, after
    /// every settings change and every new renderer), and so does the control word. It
    /// turns on only where it is offered (`gpu_culling_offer`); unchanged, it does nothing.
    /// Its pass, its records of the resident sections and their flags while they
    /// fade are made when it is switched on (the next world frame decides the
    /// flags, `mark_fades`) and dropped when it is switched off, with the walk
    /// marks the flags gave: nothing is kept for it while it is off but the
    /// sections' other walk marks (`SectionGpu::listed`) and the cave-culling
    /// graph's bits (`occlusion::Reach`), which are the same either way.
    pub fn set_gpu_culling(&mut self, on: bool) -> Result<()> {
        if !on {
            if self.gpu_culling.take().is_some() {
                forget_records(&mut self.sections, self.layout.sections());
            }
            self.gpu_frame = false;
            return Ok(());
        }
        if self.gpu_culling.is_some() {
            return Ok(());
        }
        if let Some(why) = self.gpu_culling_offer.reason() {
            return Err(why.into());
        }
        let test = self
            .cull
            .as_ref()
            .filter(|c| c.early())
            .map(|_| self.output.frame.format);
        let Some(mut gpu) = gpu_culling::GpuCulling::new(&self.device, test) else {
            // Not offered from now on: its switch greys out.
            self.offer_gpu_culling(crate::graphics::GpuCullingOffer::Unsupported);
            return Err("GPU culling could not be set up on this device".into());
        };
        let sections = self.layout.sections();
        for (key, s, bounds) in self.sections.iter_mut() {
            if s.gpu_recorded(key, sections) {
                s.gpu_id = Some(gpu.records.insert(s.record(key, bounds)));
            }
        }
        self.gpu_culling = Some(gpu);
        #[cfg(debug_assertions)]
        {
            self.gpu_checks = 0;
        }
        Ok(())
    }
    /// GPU culling offered or not from now on, here and in the settings' switch.
    fn offer_gpu_culling(&mut self, offer: crate::graphics::GpuCullingOffer) {
        self.gpu_culling_offer = offer;
        crate::graphics::offer_gpu_culling(offer);
    }
    /// Whether GPU culling is offered here, and why not (the status's `gpu_culling_offer`).
    pub fn gpu_culling_offer(&self) -> crate::graphics::GpuCullingOffer {
        self.gpu_culling_offer
    }
    /// Debug builds: GPU culling's records are the sections' as they are now,
    /// compaction's moves included: each section it keeps a record of
    /// (`SectionGpu::gpu_recorded`) has its own, no record is left without its
    /// section, and each page they count is the arena's. A record left behind
    /// would draw another section's quads, or from a freed page. And each
    /// section's walk mark is `SectionGpu::listed`: one unmarked that should be
    /// would leave its translucent quads, or all of it, undrawn. And the bits
    /// the pass copies of the cave-culling graph are its cells'
    /// (`occlusion::Reach`): one off would hide a section the walk shows. And,
    /// at `f.now`, a recorded section draws faded only while it is flagged
    /// (`SectionGpu::gpu_fading`, its record's `FADING`), and an end is waited
    /// for while it is unless it leaves or is held: one the pass drew whole
    /// would pop in, one flagged for good would cost the CPU its draws.
    #[cfg(debug_assertions)]
    fn check_gpu_records(&self, f: FadeInputs<'_>) {
        let Some(gpu) = &self.gpu_culling else {
            return;
        };
        assert!(
            self.occlusion.reach_is_exact(),
            "the reach bits are the graph's cells"
        );
        let (records, sections) = (&gpu.records, self.layout.sections());
        let pending: crate::fast_hash::FxHashSet<SectionKey> = gpu.fades.pending().collect();
        let mut pages = vec![0; records.page_sections().len()];
        let (mut live, mut flagged) = (0, 0);
        for (key, s, bounds) in self.sections.iter() {
            let recorded = s.gpu_recorded(key, sections);
            assert_eq!(s.gpu_id.is_some(), recorded, "{key:?}: a record");
            let marked = self.sections.is_marked(key);
            assert_eq!(marked, s.listed(key, sections), "{key:?}: its walk mark");
            assert!(
                !s.gpu_fading || s.gpu_id.is_some(),
                "{key:?}: flagged without a record"
            );
            assert!(
                s.gpu_id.is_none() || s.gpu_fading || !s.drawn_faded(key, f),
                "{key:?}: GPU culling draws it whole while it fades"
            );
            assert!(
                !s.gpu_fading || s.fade_end(key, f).is_none() || pending.contains(&key),
                "{key:?}: flagged, with no end waited for"
            );
            if let Some(id) = s.gpu_id {
                let record = s.record(key, bounds);
                assert_eq!(records.at(id), Some(&record), "{key:?}: its record {id}");
                pages[s.page as usize] += 1;
                live += 1;
                flagged += u32::from(s.gpu_fading);
            }
        }
        assert_eq!(records.live(), live, "records of sections gone");
        assert_eq!(records.fading(), flagged, "flagged records");
        assert_eq!(records.page_sections(), pages, "records per page");
        for (page, &n) in pages.iter().enumerate() {
            assert!(
                n == 0 || self.arena.has_page(page as u32),
                "records on freed page {page}"
            );
        }
    }
    pub fn section_count(&self) -> usize {
        self.sections.len()
    }
    /// Whether the running native pack gives any block a `[[lights]]` colour.
    pub fn has_light_colors(&self) -> bool {
        self.native
            .as_ref()
            .is_some_and(|native| !native.light_colors.is_empty())
    }
    /// The running native pack's `[[lights]]` colour of the block `name`.
    pub fn light_color(&self, name: &str) -> Option<[f32; 3]> {
        let native = self.native.as_ref()?;
        horizon_shader_api::manifest::light_color(&native.light_colors, name)
    }
    /// The running native pack's `[[particle_lights]]`.
    pub fn particle_lights(&self) -> Option<&[horizon_shader_api::manifest::ParticleLight]> {
        self.native.as_ref().map(|n| n.particle_lights.as_slice())
    }
    /// Whether one of Horizon's own packs with `[shadow]` draws, whose shadow pass takes the
    /// frame's `shadow_only` quads.
    pub fn native_shadow(&self) -> bool {
        self.native.as_ref().is_some_and(|n| n.shadow.is_some())
    }
    /// What a shader pack's shadow pass draws of the entities this frame: (entities or block
    /// entities other than the player, so those Horizon's culling hides from the camera are
    /// kept for it; the local player, so its model is built in first person too). (false,
    /// false) without a pack or its shadow pass.
    pub fn pack_shadow_casters(&self) -> (bool, bool) {
        let Some((_, Some(post))) = &self.pack_post else {
            return (false, false);
        };
        let c = post.shadow_casters();
        ((c.entities && !c.only_player) || c.blocks, c.entities)
    }

    /// A standard pack replaces mobs' vanilla blob shadows only when its compiled
    /// shadow path actually draws entities other than the player.
    pub fn pack_entity_shadows(&self) -> bool {
        self.pack_post
            .as_ref()
            .and_then(|(_, post)| post.as_ref())
            .is_some_and(|post| {
                let casters = post.shadow_casters();
                casters.entities && !casters.only_player
            })
    }

    pub fn has_section(&self, key: SectionKey) -> bool {
        self.sections.contains(key)
    }
    /// Bring the cave culling graph up to date for `camera` without waiting:
    /// a search this starts runs on its worker while the frame is prepared,
    /// and `render` takes it if it has finished.
    pub fn start_occlusion(&mut self, camera: DVec3, distance: f32) {
        self.occlusion.update(camera, (distance / 16.0) as i32 + 2);
    }
    /// Whether the camera's current section graph knows this section is
    /// reachable. `None` means the graph cannot safely answer for this camera
    /// or section, so callers should keep the geometry.
    pub fn section_occlusion_visible(
        &self,
        key: SectionKey,
        camera_section: SectionKey,
    ) -> Option<bool> {
        self.occlusion.visible_for_camera(key, camera_section)
    }
    pub fn upload(&mut self, key: SectionKey, mesh: &SectionMesh) {
        if let Some(copies) = &mut self.capture_copies {
            copies.section(key, mesh);
        }
        if self.hold.holds(key) && self.shown(key) && self.upload_held(key, mesh) {
            return;
        }
        if let Some(gpu) = self.hold.take(key).and_then(|held| held.gpu) {
            self.release_mesh(gpu);
        }
        let facts = MeshFacts::of(mesh);
        let fading = self.before_mesh(key, &facts);
        let section = self.upload_mesh(mesh);
        self.after_mesh(key, &facts, section, fading);
    }
    /// What goes with section `key`'s new mesh, before its quads go in: its old mesh is
    /// released. The fade it keeps.
    fn before_mesh(&mut self, key: SectionKey, mesh: &MeshFacts) -> Option<Instant> {
        self.occlusion.set(key, mesh.visibility);
        let changed = if mesh.block_entities.is_empty() {
            self.block_entities.remove(&key).is_some()
        } else {
            let old = self
                .block_entities
                .insert(key, mesh.block_entities.to_vec());
            old.as_deref() != Some(mesh.block_entities)
        };
        if changed {
            self.block_entity_revision = next_block_entity_revision();
            self.block_entity_membership_revisions
                .insert(key, self.block_entity_revision);
        }
        if !mesh.block_entities.is_empty()
            || self.block_entity_membership_revisions.contains_key(&key)
        {
            // Packed light can change with an unchanged BE list. Keep a
            // tombstone for formerly occupied sections until unload so a
            // deleted last entity invalidates only its own cached section.
            self.block_entity_section_revisions
                .insert(key, next_block_entity_revision());
        }
        let lights_changed = if mesh.lights.is_empty() {
            self.light_blocks.remove(&key).is_some()
        } else {
            let old = self.light_blocks.insert(key, mesh.lights.to_vec());
            old.as_deref() != Some(mesh.lights)
        };
        if lights_changed {
            self.light_revision += 1;
        }
        if !self.lod_retiring.is_empty() {
            self.lod_retiring.remove(&key);
        }
        if mesh.translucent && !self.sections.contains(key) {
            // New translucent terrain, not an edit's remesh: large pages wait for it.
            self.shrink_gate.wait(Instant::now(), SHRINK_SETTLE);
        }
        let fading = match self.sections.find(key) {
            Some(old) => old.fading,
            None if !self.lods.is_empty() => {
                let now = std::time::Instant::now();
                self.newest_lod_fade = Some(now);
                Some(now)
            }
            None => None,
        };
        if fading.is_none() && !self.section_uploaded.contains_key(&key) {
            let now = std::time::Instant::now();
            self.section_uploaded.insert(key, now);
            self.newest_upload = Some(now);
        }
        self.release_section(key);
        fading
    }
    /// Section `key`'s new mesh drawn from now on, its quads uploaded (`section`: none when it
    /// has none, or the arena had no room).
    fn after_mesh(
        &mut self,
        key: SectionKey,
        mesh: &MeshFacts,
        section: Option<SectionGpu>,
        fading: Option<Instant>,
    ) {
        let section = section.map(|section| SectionGpu { fading, ..section });
        self.set_detail(key, section.is_some() || mesh.empty);
        if let Some(mut section) = section {
            let sections = self.layout.sections();
            if let Some(gpu) = &mut self.gpu_culling
                && section.gpu_recorded(key, sections)
            {
                // A fade starts with its section: flagged from the first frame that
                // draws it, once a frame has decided the flags (`mark_fades`).
                if let Some(fade_ms) = gpu.fades.fade_ms {
                    let f = FadeInputs {
                        now: Instant::now(),
                        fade_ms,
                        uploaded: &self.section_uploaded,
                        retiring: &self.lod_retiring,
                    };
                    section.gpu_fading = decide_fade(&section, key, &mut gpu.fades, f);
                }
                section.gpu_id = Some(gpu.records.insert(section.record(key, mesh.bounds)));
            }
            let marked = section.listed(key, sections);
            self.sections
                .insert_marked(key, section, mesh.bounds, marked);
        }
    }
    /// Whether section `key` has its mesh on screen (`set_detail`): an empty one counts.
    fn shown(&self, key: SectionKey) -> bool {
        self.detail_columns
            .get(&(key.0, key.2))
            .is_some_and(|mask| mask.contains(key.1 as usize))
    }
    /// The bytes a section's quads take in the arenas.
    fn mesh_bytes(gpu: Option<&SectionGpu>) -> u64 {
        let stride = std::mem::size_of::<TerrainVertex>() as u64;
        gpu.map_or(0, |s| {
            (s.solid_slots + u64::from(s.translucent) * 4) * stride
        })
    }
    /// A mod's hold on the terrain's picture (gfx/hold.rs, `WorldHold`), asked once a frame
    /// before the frame's uploads: while held, a section on screen that is meshed again waits,
    /// uploaded, behind the mesh drawn. What no longer waits takes its place on this frame.
    pub fn hold_world(&mut self, ask: &WorldHold) {
        let waiting = match self.hold.ask(ask, Instant::now()) {
            hold::Release::None => return,
            hold::Release::All => self.hold.take_all(),
            hold::Release::Outside => self.hold.take_outside(),
        };
        self.place_held(waiting);
    }
    /// Whether a mod's hold covers chunk column `column` (`hold_world`): what changes there waits.
    pub fn world_holds_column(&self, column: (i32, i32)) -> bool {
        self.hold.holds((column.0, 0, column.1))
    }
    /// Section `key`'s new mesh waits behind the hold, uploaded. False when the arena has no
    /// room for it: what waited takes its place (giving the old meshes back), the hold is
    /// spent, and this one goes in as ever.
    fn upload_held(&mut self, key: SectionKey, mesh: &SectionMesh) -> bool {
        let facts = MeshFacts::of(mesh);
        let gpu = self.upload_mesh(mesh);
        if gpu.is_none() && !facts.empty {
            log::warn!("Horizon: no memory left for a held world's new sections: let go");
            self.hold.spend();
            let waiting = self.hold.take_all();
            self.place_held(waiting);
            return false;
        }
        let bytes = Self::mesh_bytes(gpu.as_ref());
        let held = HeldMesh {
            gpu,
            visibility: facts.visibility,
            block_entities: facts.block_entities.to_vec(),
            lights: facts.lights.to_vec(),
            bounds: facts.bounds,
            translucent: facts.translucent,
            empty: facts.empty,
        };
        let (old, over) = self.hold.wait(key, held, bytes);
        if let Some(gpu) = old.and_then(|old| old.gpu) {
            self.release_mesh(gpu);
        }
        if over {
            log::warn!(
                "Horizon: a held world's new sections came to {} MiB: let go",
                self.hold.bytes() >> 20
            );
            self.hold.spend();
            let waiting = self.hold.take_all();
            self.place_held(waiting);
        }
        true
    }
    /// Sections that waited behind the hold take their place, on this frame.
    fn place_held(&mut self, waiting: Vec<(SectionKey, HeldMesh)>) {
        if waiting.is_empty() {
            return;
        }
        let (count, started) = (waiting.len(), Instant::now());
        let bytes: u64 = waiting
            .iter()
            .map(|(_, held)| Self::mesh_bytes(held.gpu.as_ref()))
            .sum();
        for (key, mut held) in waiting {
            let gpu = held.gpu.take();
            let facts = held.facts();
            let fading = self.before_mesh(key, &facts);
            self.after_mesh(key, &facts, gpu, fading);
        }
        log::debug!(
            "Horizon: {count} held sections ({} MiB) took their place in {:.1} ms",
            bytes >> 20,
            started.elapsed().as_secs_f64() * 1e3
        );
    }
    fn upload_mesh(&mut self, mesh: &SectionMesh) -> Option<SectionGpu> {
        let solid = (mesh.solid.len() / 4) as u32;
        let translucent = (mesh.translucent.len() / 4) as u32;
        if solid + translucent == 0 {
            return None;
        }
        let max = solid.max(translucent);
        if max > self.index_quads {
            self.index_quads = max.next_power_of_two();
            self.indices = Self::quad_indices(&self.device, self.index_quads);
        }
        let encoder = self.section_copies.get_or_insert_with(|| {
            self.device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("Section uploads"),
                })
        });
        // The solid quads, and the translucent ones after them unless they have
        // an arena of their own. With video and system memory both full, the
        // section is not drawn. The solid ones in the compact layout where the
        // mesher made it and the passes read it (`packed_quads`).
        let packed = self.packed_quads && !mesh.packed.is_empty() && solid > 0;
        let solid_slots = if packed {
            mesh.packed.len() as u64
        } else {
            mesh.solid.len() as u64
        };
        let split = self.translucent_arena.is_some();
        let main = if split {
            solid_slots
        } else {
            solid_slots + mesh.translucent.len() as u64
        };
        let (page, start) = if main > 0 {
            self.arena.alloc(&self.device, encoder, main)?
        } else {
            (0, 0)
        };
        let release_main = |arena: &mut arena::Arena| {
            if main > 0 {
                arena.release(page, start, main);
            }
        };
        let translucent_vertices = mesh.translucent.len() as u64;
        let translucent_at = match &mut self.translucent_arena {
            Some(arena) if translucent > 0 => {
                match arena.alloc(&self.device, encoder, translucent_vertices) {
                    Some(at) => at,
                    None => {
                        release_main(&mut self.arena);
                        return None;
                    }
                }
            }
            _ => (page, start + solid_slots),
        };
        let sorted_indices = if translucent > 0 {
            let indices =
                self.sorted_indices
                    .alloc(&self.device, encoder, u64::from(translucent) * 6);
            if indices.is_none() {
                release_main(&mut self.arena);
                if let Some(arena) = &mut self.translucent_arena {
                    arena.release(translucent_at.0, translucent_at.1, translucent_vertices);
                }
                return None;
            }
            indices
        } else {
            None
        };
        // Flat draws name the blocks of its ranges by its id: its range in `arena` and,
        // split, its translucent quads' in `translucent_arena` (`flat::TRANSLUCENT_KEYS`).
        let translucent_key = (split && translucent > 0)
            .then(|| (translucent_at.0 + flat::TRANSLUCENT_KEYS, translucent_at.1));
        let flat_id = self.flat.as_mut().and_then(|f| {
            let ranges = [(main > 0).then_some((page, start, main))]
                .into_iter()
                .chain([translucent_key.map(|(key, first)| (key, first, translucent_vertices))]);
            let mut id = None;
            for (key, first, vertices) in ranges.flatten() {
                let uploads = &mut self.section_uploads;
                match id {
                    None => id = Some(f.add(&self.device, encoder, uploads, key, first, vertices)),
                    Some(id) => f.place(&self.device, encoder, uploads, id, key, first, vertices),
                }
            }
            id
        });
        let stride = std::mem::size_of::<TerrainVertex>() as u64;
        if solid_slots > 0 {
            self.arena.write_sources(
                &self.device,
                encoder,
                &mut self.section_uploads,
                page,
                start,
                solid_slots,
                if packed { &[] } else { &mesh.solid_sources },
            );
        }
        if translucent_vertices > 0 {
            self.translucent_arena
                .as_mut()
                .unwrap_or(&mut self.arena)
                .write_sources(
                    &self.device,
                    encoder,
                    &mut self.section_uploads,
                    translucent_at.0,
                    translucent_at.1,
                    translucent_vertices,
                    &mesh.translucent_sources,
                );
        }
        let translucent_arena = self.translucent_arena.as_ref().unwrap_or(&self.arena);
        let solid_bytes: &[u8] = if packed {
            bytemuck::cast_slice(&mesh.packed)
        } else {
            bytemuck::cast_slice(&mesh.solid)
        };
        for (bytes, arena, (part_page, first)) in [
            (solid_bytes, &self.arena, (page, start)),
            (
                bytemuck::cast_slice(&mesh.translucent),
                translucent_arena,
                translucent_at,
            ),
        ] {
            if !bytes.is_empty() {
                self.uploaded_bytes += bytes.len() as u64;
                self.section_copy_bytes += bytes.len() as u64;
                self.section_uploads.buffer(
                    encoder,
                    arena.buffer(part_page),
                    first * stride,
                    bytes,
                );
            }
        }
        if self.section_copy_bytes >= SECTION_COPY_LIMIT
            && let Some(copies) = self.take_section_copies()
        {
            self.queue.submit([copies]);
        }
        let animated_layers =
            collect_animated_layers(mesh, |layer| self.animations.is_animated(layer));
        Some(SectionGpu {
            page,
            start,
            translucent_at,
            solid,
            solid_slots,
            packed,
            packed_far: packed && mesh.packed_far,
            sides: if mesh.sides.quads() == solid {
                mesh.sides
            } else {
                Sides::unsorted(solid)
            },
            translucent,
            sort: translucency::SortState::new(
                mesh.translucent
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .map(|q| translucency::quad_center(q[0].pos, q[2].pos))
                    .collect(),
            ),
            sorted_indices,
            flat_id,
            gpu_id: None,
            gpu_fading: false,
            fading: None,
            leaving: None,
            animated_layers,
        })
    }
    /// The section copies recorded since the last submission, closed.
    fn take_section_copies(&mut self) -> Option<wgpu::CommandBuffer> {
        let encoder = self.section_copies.take()?;
        self.section_copy_bytes = 0;
        self.section_uploads.finish(&encoder);
        Some(encoder.finish())
    }
    /// Draws `count` section draws from `first` with the arenas and section
    /// origins bound: one indirect multi-draw, or a loop without it.
    /// Consecutive draws in one arena page share a vertex buffer binding,
    /// keeping the list's order (translucent sections go back to front).
    /// `indirect` holds the list's draws from `base`. Leaves the quad
    /// indices bound, as the draws after it expect.
    fn draw_sections(
        &self,
        pass: &mut wgpu::RenderPass<'_>,
        indirect: &wgpu::Buffer,
        base: u32,
        first: u32,
        count: u32,
    ) {
        self.draw_sections_from(pass, indirect, base, first, count, false);
    }
    /// `draw_sections` through the solid pipelines that read the arena page themselves
    /// (`vs_section`): each page's group is bound as their group 1.
    fn draw_solid_sections(
        &self,
        pass: &mut wgpu::RenderPass<'_>,
        indirect: &wgpu::Buffer,
        base: u32,
        first: u32,
        count: u32,
    ) {
        self.draw_sections_from(pass, indirect, base, first, count, self.flat.is_some());
    }
    fn draw_sections_from(
        &self,
        pass: &mut wgpu::RenderPass<'_>,
        indirect: &wgpu::Buffer,
        base: u32,
        first: u32,
        count: u32,
        pulled: bool,
    ) {
        let (draws, pages) = (&self.draws[..], &self.draw_pages[..]);
        let (mut i, end) = (first as usize, (first + count) as usize);
        while i < end {
            let (page, index_page) = pages[i];
            let run = pages[i..end]
                .iter()
                .take_while(|&&p| p == (page, index_page))
                .count();
            // Translucent draws, and only they, have sorted indices; split, their large
            // pages are bound up to their last range in use (`Arena::binding`).
            let (vertices, indices) = match (&self.translucent_arena, index_page) {
                (Some(arena), Some(p)) => (arena.binding(page), self.sorted_indices.binding(p)),
                (_, p) => (
                    self.arena.buffer(page).slice(..),
                    p.map_or(&self.indices, |p| self.sorted_indices.buffer(p))
                        .slice(..),
                ),
            };
            pass.set_vertex_buffer(0, vertices);
            if pulled
                && index_page.is_none()
                && let Some(group) = self.flat.as_ref().and_then(|f| f.page_group(page))
            {
                pass.set_bind_group(1, group, &[]);
            }
            pass.set_index_buffer(indices, wgpu::IndexFormat::Uint32);
            if self.multi_draw {
                let offset = (i - base as usize) as u64 * 20;
                pass.multi_draw_indexed_indirect(indirect, offset, run as u32);
            } else {
                for d in &draws[i..i + run] {
                    pass.draw_indexed(d[2]..d[2] + d[0], d[3] as i32, d[4]..d[4] + 1);
                }
            }
            i += run;
        }
        if pages[first as usize..end]
            .iter()
            .any(|(_, sorted)| sorted.is_some())
        {
            pass.set_index_buffer(self.indices.slice(..), wgpu::IndexFormat::Uint32);
        }
    }
    fn release_section(&mut self, key: SectionKey) {
        if let Some(s) = self.sections.remove(key) {
            self.release_mesh(s);
        }
    }
    fn release_mesh(&mut self, s: SectionGpu) {
        if let (Some(flat), Some(id)) = (&mut self.flat, s.flat_id) {
            flat.remove(id);
        }
        if let (Some(gpu), Some(id)) = (&mut self.gpu_culling, s.gpu_id) {
            gpu.records.remove(id);
        }
        if let Some((page, start)) = s.sorted_indices {
            self.sorted_indices
                .release(page, start, u64::from(s.translucent) * 6);
        }
        let (main, translucent) = s.vertex_ranges(self.translucent_arena.is_some());
        if let Some((page, start, vertices)) = main {
            self.arena.release(page, start, vertices);
        }
        if let (Some(arena), Some((page, start, vertices))) =
            (&mut self.translucent_arena, translucent)
        {
            arena.release(page, start, vertices);
        }
    }
    /// Video memory the arena keeps because fragmentation holds its pages
    /// partly used (a render distance that went down, far tiles that were
    /// replaced): moves the meshes off the sparsest page, a few MiB a frame,
    /// and `Arena::trim` returns the page once it is empty. Frames with the
    /// arena in system memory (video memory full) are slow, so this is what
    /// keeps the terrain in video memory. The section arena's: the large pages
    /// of the translucent arena and the sorted indices are `shrink_translucent`'s.
    /// Returns what is left of the frame's `COMPACT_BUDGET`.
    fn compact_arena(&mut self) -> u64 {
        let mut budget = COMPACT_BUDGET;
        if self.arena.closing().is_none()
            && let Some(page) = self.arena.sparsest()
        {
            self.arena.close(page);
        }
        let Some(page) = self.arena.closing() else {
            return budget;
        };
        let encoder = self.section_copies.get_or_insert_with(|| {
            self.device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("Section uploads"),
                })
        });
        let meshes = self
            .sections
            .values_mut()
            .chain(self.lods.values_mut().flat_map(|tile| tile.meshes_mut()))
            // (meshes waiting behind a hold move with the rest, or they would keep the page)
            .chain(
                self.hold
                    .sections_mut()
                    .filter_map(|held| held.gpu.as_mut()),
            );
        let flat = self
            .flat
            .as_mut()
            .map(|f| (f, &self.device, &mut self.section_uploads));
        // GPU culling's records move with the sections.
        let gpu = self.gpu_culling.as_mut().map(|g| &mut g.records);
        let split = self.translucent_arena.is_some();
        let (arena, part) = (&mut self.arena, Part::Section { split });
        if !move_off_page(arena, encoder, flat, gpu, meshes, page, part, &mut budget) {
            // The other pages have no room after all.
            arena.reopen();
        }
        // With every mesh moved off, the page stays closed so this frame's
        // uploads can't refill it: `trim` returns it and clears `closing`.
        budget
    }
    /// Shrinks the large pages of the translucent arena and the sorted indices
    /// once terrain has settled (`set_terrain_loading`): one far larger than what
    /// it holds moves into a new page of their size (`begin_shrink`), through
    /// `move_off_page` with what `compact_arena` left of the frame's budget
    /// (`left`), and `trim` returns it once empty. One page at a time, vertices
    /// first; only starts wait, a move under way finishes even if terrain comes
    /// again (stopped, the page would stay closed while the new one fills).
    fn shrink_translucent(&mut self, left: u64) {
        let Some(vertices) = self.translucent_arena.as_mut() else {
            return; // split off: nothing to shrink
        };
        let now = Instant::now();
        let mut arenas = [
            (vertices, Part::Translucent),
            (&mut self.sorted_indices, Part::Indices),
        ];
        if arenas.iter().all(|(arena, _)| arena.closing().is_none()) {
            if !self.shrink_gate.may_start(now, left) {
                return;
            }
            // The next start in a second, or `SHRINK_RETRY` after a try found no room.
            let mut wait = SHRINK_SETTLE;
            for (arena, part) in &mut arenas {
                let meshes = self
                    .sections
                    .values_mut()
                    .chain(self.lods.values_mut().flat_map(|tile| tile.meshes_mut()))
                    .chain(
                        self.hold
                            .sections_mut()
                            .filter_map(|held| held.gpu.as_mut()),
                    );
                match begin_shrink(arena, &self.device, meshes, *part) {
                    Some(true) => break,
                    Some(false) => wait = SHRINK_RETRY, // the other arena may still go
                    None => {}
                }
            }
            self.shrink_gate.wait(now, wait);
        }
        let Some((arena, part, page)) = arenas.into_iter().find_map(|(arena, part)| {
            let page = arena.closing()?;
            Some((arena, part, page))
        }) else {
            return;
        };
        let encoder = self.section_copies.get_or_insert_with(|| {
            self.device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("Section uploads"),
                })
        });
        let meshes = self
            .sections
            .values_mut()
            .chain(self.lods.values_mut().flat_map(|tile| tile.meshes_mut()))
            // (meshes waiting behind a hold move with the rest, or they would keep the page)
            .chain(
                self.hold
                    .sections_mut()
                    .filter_map(|held| held.gpu.as_mut()),
            );
        // As many quads a frame either way: a quad's indices are a quarter of its bytes.
        let cap = match part {
            Part::Indices => COMPACT_BUDGET / 4,
            _ => COMPACT_BUDGET,
        };
        let mut budget = left.min(cap);
        // Flat draws name translucent quads' blocks by their section's id; indices have none.
        let flat = match part {
            Part::Translucent => self.flat.as_mut(),
            _ => None,
        }
        .map(|f| (f, &self.device, &mut self.section_uploads));
        // GPU culling's records follow the section arena alone.
        if !move_off_page(arena, encoder, flat, None, meshes, page, part, &mut budget) {
            // New ranges took the room: both pages stay, drawn, until a later try.
            arena.reopen();
            self.shrink_gate.wait(now, SHRINK_RETRY);
        }
    }
    /// Block-entity blocks of every meshed section (see `SectionMesh::block_entities`).
    pub fn block_entities(&self) -> impl Iterator<Item = &([i32; 3], u32)> {
        self.block_entities.values().flatten()
    }
    /// `block_entities` of the sections far enough into their fade-in to draw them
    /// (`LevelExtractor.extractVisibleBlockEntities`: `getVisibility(now, fade) >= 0.3`).
    pub fn visible_block_entities(
        &self,
        fade_seconds: f32,
    ) -> impl Iterator<Item = &([i32; 3], u32)> {
        let fade = fade_millis(fade_seconds);
        let now = std::time::Instant::now();
        let all = !self.sections_fading_in(fade, now);
        self.block_entities
            .iter()
            .filter(move |(key, _)| {
                all || chunk_visibility(self.section_elapsed_ms(**key, now), fade)
                    >= CHUNK_VISIBILITY_THRESHOLD
            })
            .flat_map(|(_, blocks)| blocks)
    }
    /// Whether a section may still be fading in over `fade_ms` (vanilla's chunk fade): one was
    /// uploaded more recently than that. Otherwise every section is whole.
    fn sections_fading_in(&self, fade_ms: u64, now: std::time::Instant) -> bool {
        fade_ms > 0
            && self
                .newest_upload
                .is_some_and(|t| (now.saturating_duration_since(t).as_millis() as u64) < fade_ms)
    }
    /// Whether a section may be fading in or out over far terrain (`SectionGpu::fade`):
    /// otherwise every section's code is 0.
    fn sections_fading_over_lods(&self, now: std::time::Instant) -> bool {
        !self.lod_retiring.is_empty()
            || !self.lod_leaving.is_empty()
            || self
                .newest_lod_fade
                .is_some_and(|t| now.saturating_duration_since(t) < lod::FADE)
    }
    /// Block entities in sections which can intersect the camera frustum.
    /// The section bounds include the same one-block margin as the per-block
    /// check in the game. Sections wholly inside the frustum skip that finer
    /// check; sections outside it are kept only when a renderer needs to draw
    /// geometry extending beyond its block (beacon beams, gateways, outlines).
    pub fn visible_block_entities_in_frustum<'a>(
        &'a self,
        fade_seconds: f32,
        camera: DVec3,
        frustum: Option<Frustum>,
        keep_section: impl Fn(u32) -> bool + 'a,
        keep_long_range: impl Fn(u32) -> bool + 'a,
    ) -> impl Iterator<Item = (&'a ([i32; 3], u32), bool)> + 'a {
        let fade = fade_millis(fade_seconds);
        let now = Instant::now();
        // No section uploaded within the fade: all are whole, none is looked up.
        let all = !self.sections_fading_in(fade, now);
        let min_y = self.layout.min_y();
        self.block_entities
            .iter()
            .filter(move |(key, _)| {
                all || chunk_visibility(self.section_elapsed_ms(**key, now), fade)
                    >= CHUNK_VISIBILITY_THRESHOLD
            })
            .filter_map(move |(key, blocks)| {
                let origin = DVec3::new(
                    f64::from(key.0 * 16 - 1),
                    f64::from(min_y + key.1 * 16 - 1),
                    f64::from(key.2 * 16 - 1),
                ) - camera;
                let min = origin.as_vec3();
                // The last block's expanded bound ends at section_origin + 17.
                let max = min + Vec3::splat(18.0);
                let distance_squared = Vec3::ZERO.max(min).min(max).length_squared();
                let in_range = distance_squared <= 64.0 * 64.0
                    || (distance_squared <= 256.0 * 256.0
                        && blocks.iter().any(|(_, state)| keep_long_range(*state)));
                if !in_range {
                    return None;
                }
                let (intersects, fully_inside) = frustum.map_or((true, true), |view| {
                    let fully_inside = view.contains(min, max);
                    (fully_inside || view.intersects(min, max), fully_inside)
                });
                (intersects || blocks.iter().any(|(_, state)| keep_section(*state)))
                    .then_some((blocks, fully_inside))
            })
            .flat_map(|(blocks, fully_inside)| {
                blocks.iter().map(move |block| (block, fully_inside))
            })
    }
    /// The block entities of the sections a shader pack's shadow pass drew in the last frame
    /// (`shadow_visible`), within the 64 blocks `visible_block_entities_in_frustum` keeps.
    /// Our probe packs' Iris 1.11.7 shadow maps hold a block entity wherever the shadow pass
    /// draws its section's terrain, whether or not the camera's view takes it in (#8464).
    pub fn shadow_pass_block_entities(
        &self,
        camera: DVec3,
    ) -> impl Iterator<Item = &([i32; 3], u32)> + '_ {
        block_entities_within_reach(
            self.shadow_visible.iter().map(|v| v.key),
            &self.block_entities,
            camera,
            self.layout.min_y(),
        )
    }
    /// Whether a shader pack's shadow pass drew the section holding block `pos` in the last
    /// frame (`shadow_pass_block_entities`).
    pub fn shadow_pass_drew(&self, pos: [i32; 3]) -> bool {
        let key = (
            pos[0] >> 4,
            (pos[1] - self.layout.min_y()) >> 4,
            pos[2] >> 4,
        );
        self.shadow_visible.iter().any(|v| v.key == key)
    }
    /// Milliseconds since `key` was uploaded; a section not in `section_uploaded` is old.
    fn section_elapsed_ms(&self, key: SectionKey, now: std::time::Instant) -> u64 {
        self.section_uploaded.get(&key).map_or(u64::MAX, |t| {
            now.saturating_duration_since(*t).as_millis() as u64
        })
    }
    /// A value that changes whenever `block_entities` gives something else.
    pub fn block_entity_revision(&self) -> u64 {
        self.block_entity_revision
    }
    /// Both mesh/light and block-entity membership revisions for a section
    /// key (`x`, section-y relative to the dimension's minimum, `z`).
    pub fn block_entity_section_revision(
        &self,
        section: [i32; 3],
    ) -> Option<StaticSectionRevision> {
        let key = (section[0], section[1], section[2]);
        Some(StaticSectionRevision {
            mesh: *self.block_entity_section_revisions.get(&key)?,
            block_entities: *self.block_entity_membership_revisions.get(&key)?,
        })
    }
    /// Block-entity cache work and upload volume from the last rendered frame.
    pub fn static_block_entity_frame_stats(&self) -> StaticCacheFrameStats {
        self.static_entity_frame_stats
    }
    /// Drops every section, for a level whose chunks hold `layout`.
    pub fn clear_sections(&mut self, layout: crate::chunk::ColumnLayout) {
        if let Some(copies) = &mut self.capture_copies {
            *copies = capture::Copies::default();
        }
        self.block_entities.clear();
        self.block_entity_section_revisions.clear();
        self.block_entity_membership_revisions.clear();
        self.section_uploaded.clear();
        self.newest_upload = None;
        self.newest_lod_fade = None;
        self.block_entity_revision = next_block_entity_revision();
        self.sections.clear();
        // (a hold on land that is gone ends; the arenas' clearing below takes what waited)
        self.hold.clear();
        self.lods.clear();
        self.lod_cutout.clear();
        self.lod_retiring.clear();
        self.lod_leaving.clear();
        self.lod_pending = None;
        self.detail_columns.clear();
        self.detail_revision += 1;
        // A dimension change drops every page at once: freed later, off this thread.
        self.graveyard.add(self.arena.clear());
        if let Some(gpu) = &mut self.gpu_culling {
            gpu.records.clear();
            // The chunk fade-in time they were decided with stays.
            gpu.fades.clear();
        }
        if let Some(arena) = &mut self.translucent_arena {
            self.graveyard.add(arena.clear());
        }
        if let Some(flat) = &mut self.flat {
            flat.clear();
        }
        self.graveyard.add(self.sorted_indices.clear());
        self.translucency_cursor = 0;
        if layout == self.layout {
            self.occlusion.clear();
        } else {
            self.layout = layout;
            self.occlusion = occlusion::Occlusion::new(layout.min_y(), layout.sections() as i32);
            self.sections.set_min_y(layout.min_y());
        }
    }
    /// Lays the sections out as `translucent_split` says for the shader pack that
    /// runs, before a world frame draws them. A change drops every section and far
    /// tile, as a new renderer has none, and changes the sorted indices' arena with
    /// them (large pages beside the translucent arena, else pages flat draws bind):
    /// the game meshes them again (`take_dropped_sections`).
    fn follow_translucent_layout(&mut self) {
        let pack = self.pack_post.as_ref().and_then(|p| p.1.as_ref());
        let split = translucent_split(
            self.translucent_batching,
            self.flat.is_some() && self.flat_on,
            pack.is_some_and(pack_post::PackPost::has_terrain),
            // One of Horizon's own packs with `[shadow]` draws from the same lists (`shadow_view`).
            pack.is_some_and(pack_post::PackPost::has_shadow) || self.native_shadow(),
        );
        if split == self.translucent_arena.is_some() {
            return;
        }
        self.sections_dropped |=
            self.sections.len() > 0 || !self.lods.is_empty() || self.lod_pending.is_some();
        self.clear_sections(self.layout);
        (self.sorted_indices, self.translucent_arena) = arena::Arena::translucent_arenas(
            &self.device,
            std::mem::size_of::<TerrainVertex>() as u64,
            self.flat.is_some(),
            split,
        );
        if let (Some(flat), Some(arena)) = (&mut self.flat, &self.translucent_arena) {
            flat.set_translucent_page_vertices(arena.page_vertices());
        }
    }
    /// Uploads solid quads in the compact layout (`packed_quads`) while no shader pack runs:
    /// a pack's terrain and shadow programs, and Horizon's own packs' stages, read the
    /// arena's vertices as `TerrainVertex`es. Leaving it drops every section and far tile
    /// for the game to mesh again, as `follow_translucent_layout` does.
    fn follow_packed_layout(&mut self) {
        let pack = self.pack_post.as_ref().and_then(|p| p.1.as_ref()).is_some();
        let want = self.flat.is_some() && !pack && self.native.is_none() && packed_wanted();
        if want == self.packed_quads {
            return;
        }
        let held_packed = |held: &HeldMesh| held.gpu.as_ref().is_some_and(|s| s.packed);
        if !want && self.hold.sections().any(held_packed) {
            // (held meshes in the layout going away take their place, and go with the rest)
            self.hold.spend();
            let waiting = self.hold.take_all();
            self.place_held(waiting);
        }
        self.packed_quads = want;
        packed::pack_quads(want);
        let laid_out = self.sections.values().any(|s| s.packed)
            || self.lods.values().any(|t| t.drawn().packed);
        if !want && laid_out {
            self.sections_dropped = true;
            self.clear_sections(self.layout);
        }
    }
    /// Whether every section and far tile was dropped since the last call, to be
    /// laid out again (`follow_translucent_layout`): the game meshes them again,
    /// as for a new renderer.
    pub fn take_dropped_sections(&mut self) -> bool {
        std::mem::take(&mut self.sections_dropped)
    }
    /// Whether terrain is on its way (the view not all in, events, chunks or
    /// uploads still waiting, far tiles sampled): a large translucent page shrinks
    /// once that, new translucent sections (`upload`) and far tiles to upload
    /// (`upload_lods`) have been over for `SHRINK_SETTLE`, not at the low point
    /// between a view's columns leaving and the next one's arriving, after which
    /// it would grow back by whole copies.
    pub fn set_terrain_loading(&mut self, loading: bool) {
        if loading {
            self.shrink_gate.wait(Instant::now(), SHRINK_SETTLE);
        }
    }
    /// A column whose every section has its mesh (`lod_detail_columns`).
    fn complete_column(&self) -> SectionMask {
        SectionMask::below(self.layout.sections())
    }
    pub fn lod_detail_columns(&self) -> impl Iterator<Item = (i32, i32)> + '_ {
        let complete = self.complete_column();
        self.detail_columns
            .iter()
            .filter_map(move |(&column, &mask)| (mask == complete).then_some(column))
    }
    /// Changes whenever `lod_detail_columns` does.
    pub fn lod_detail_revision(&self) -> u64 {
        self.detail_revision
    }
    /// Whether the section at `key` has its mesh, for `lod_detail_columns`.
    fn set_detail(&mut self, key: SectionKey, meshed: bool) {
        let column = (key.0, key.2);
        let mask = self
            .detail_columns
            .get(&column)
            .copied()
            .unwrap_or_default();
        let mut next = mask;
        if meshed {
            next.insert(key.1 as usize);
        } else {
            next.remove(key.1 as usize);
        }
        let complete = self.complete_column();
        if (mask == complete) != (next == complete) {
            self.detail_revision += 1;
        }
        if next.is_empty() {
            self.detail_columns.remove(&column);
        } else {
            self.detail_columns.insert(column, next);
        }
    }
    /// Every section of these columns goes: one lookup per column where the
    /// tables are by column, and the meshes' ranges freed in one batch. A
    /// teleport removes a few thousand columns in one frame.
    /// A section whose column far terrain now draws fades out over it, chunk by
    /// chunk as it came, and is released once it has (`release_left`): taken
    /// away at once, the column's detail dropped to the far terrain's in one
    /// frame.
    fn leave_section(&mut self, key: SectionKey) {
        let now = std::time::Instant::now();
        if let Some(section) = self.sections.find_mut(key)
            && section.leaving.is_none()
        {
            section.leaving = Some(now);
            self.lod_leaving.push((key, now));
            if let Some(gpu) = &mut self.gpu_culling {
                hold_fading(&mut self.sections, &mut gpu.records, key);
            }
        }
    }
    /// Releases the sections that have faded out (`leave_section`).
    fn release_left(&mut self, now: std::time::Instant) {
        let mut done = Vec::new();
        self.lod_leaving.retain(|&(key, since)| {
            let left = now.saturating_duration_since(since) >= lod::FADE;
            if left {
                done.push((key, since));
            }
            !left
        });
        for (key, since) in done {
            // Meshed again since (the player came back): it stays.
            if self
                .sections
                .find(key)
                .is_some_and(|s| s.leaving == Some(since))
            {
                self.release_section(key);
            }
        }
    }
    pub fn remove_columns(&mut self, columns: impl IntoIterator<Item = (i32, i32)>) {
        let columns: Vec<(i32, i32)> = columns.into_iter().collect();
        if let Some(copies) = &mut self.capture_copies {
            copies.remove_columns(&columns);
        }
        if !self.hold.is_empty() && !columns.is_empty() {
            let gone = columns.iter().copied().collect();
            let held = self.hold.take_columns(&gone);
            self.release_meshes(held.into_iter().filter_map(|held| held.gpu).collect());
        }
        // Most of the view at once (a teleport at render distance 32 drops some 3700
        // columns): a pass over each store instead of a lookup per section key, which
        // took 10 ms of one frame. Far terrain fades the sections out instead.
        let keys = columns.len() * self.layout.sections();
        if self.lods.is_empty()
            && self.lod_cutout.is_empty()
            && keys >= self.sections.len().max(BULK_REMOVAL_KEYS)
        {
            self.remove_columns_at_once(&columns);
            return;
        }
        let mut released = Vec::new();
        let mut block_entities = false;
        for (x, z) in columns {
            if self.detail_columns.remove(&(x, z)) == Some(self.complete_column()) {
                self.detail_revision += 1;
            }
            self.occlusion.remove_column(x, z);
            let retiring = !self.lod_cutout.is_empty() && self.lod_cutout.contains(&(x, z));
            for sy in 0..self.layout.sections() as i32 {
                let key = (x, sy, z);
                block_entities |= self.block_entities.remove(&key).is_some();
                self.block_entity_section_revisions.remove(&key);
                self.block_entity_membership_revisions.remove(&key);
                if self.light_blocks.remove(&key).is_some() {
                    self.light_revision += 1;
                }
                self.section_uploaded.remove(&key);
                if retiring {
                    // Distant tiles still leave its column out: drawn until they don't.
                    self.lod_retiring.insert(key);
                    if let Some(gpu) = &mut self.gpu_culling {
                        hold_fading(&mut self.sections, &mut gpu.records, key);
                    }
                } else if !self.lods.is_empty() && self.sections.contains(key) {
                    // Far terrain draws the column: it fades out over it.
                    self.leave_section(key);
                } else if let Some(section) = self.sections.remove(key) {
                    released.push(section);
                }
            }
        }
        if block_entities {
            self.block_entity_revision = next_block_entity_revision();
        }
        self.release_meshes(released);
    }
    /// `remove_columns` for a large share of the resident sections: each store filtered
    /// once by column.
    fn remove_columns_at_once(&mut self, columns: &[(i32, i32)]) {
        let gone: crate::fast_hash::FxHashSet<(i32, i32)> = columns.iter().copied().collect();
        let complete = self.complete_column();
        for &(x, z) in columns {
            if self.detail_columns.remove(&(x, z)) == Some(complete) {
                self.detail_revision += 1;
            }
        }
        self.occlusion.remove_columns(&gone);
        let kept = |key: &SectionKey| !gone.contains(&(key.0, key.2));
        let before = self.block_entities.len();
        self.block_entities.retain(|key, _| kept(key));
        if self.block_entities.len() != before {
            self.block_entity_revision = next_block_entity_revision();
        }
        let before = self.light_blocks.len();
        self.light_blocks.retain(|key, _| kept(key));
        self.light_revision += (before - self.light_blocks.len()) as u64;
        self.section_uploaded.retain(|key, _| kept(key));
        let released = self.sections.remove_columns(&gone);
        self.release_meshes(released);
    }
    /// Gives the removed sections' quads back to their arenas; the sections themselves
    /// (their sort centres) drop on a thread of their own when there are many.
    fn release_meshes(&mut self, released: Vec<SectionGpu>) {
        if let Some(flat) = &mut self.flat {
            for id in released.iter().filter_map(|s| s.flat_id) {
                flat.remove(id);
            }
        }
        if let Some(gpu) = &mut self.gpu_culling {
            gpu.records
                .remove_all(released.iter().filter_map(|s| s.gpu_id));
        }
        self.sorted_indices
            .release_all(released.iter().filter_map(|s| {
                let (page, start) = s.sorted_indices?;
                Some((page, start, u64::from(s.translucent) * 6))
            }));
        let split = self.translucent_arena.is_some();
        self.arena
            .release_all(released.iter().filter_map(|s| s.vertex_ranges(split).0));
        if let Some(arena) = &mut self.translucent_arena {
            arena.release_all(released.iter().filter_map(|s| s.vertex_ranges(true).1));
        }
        if released.len() >= BULK_REMOVAL_KEYS {
            arena::free_later(released);
        }
    }
    /// Horizon's resolution scale (world pixels per window pixel) and FXAA.
    /// Configures `surface` and, while HDR output is on, sends its HDR metadata
    /// again: a new swapchain starts without it. Errs when `surface` was left
    /// unconfigured (a validation error, or a lost device, which wgpu reports
    /// to no error scope).
    fn configure(
        device: &wgpu::Device,
        surface: &wgpu::Surface<'_>,
        config: &wgpu::SurfaceConfiguration,
        hdr: Option<&hdr::Hdr>,
        device_lost: &std::sync::atomic::AtomicBool,
    ) -> std::result::Result<(), String> {
        let validation = device.push_error_scope(wgpu::ErrorFilter::Validation);
        let memory = device.push_error_scope(wgpu::ErrorFilter::OutOfMemory);
        surface.configure(device, config);
        let memory = pollster::block_on(memory.pop());
        if let Some(e) = memory.or(pollster::block_on(validation.pop())) {
            return Err(e.to_string());
        }
        if device_lost.load(std::sync::atomic::Ordering::Relaxed) {
            return Err("the graphics device was lost".into());
        }
        if let Some(hdr) = hdr {
            horizon_hdr::set_metadata(device, surface, hdr.declared_peak(), hdr.black);
        }
        Ok(())
    }
    /// Configures the window surface again. One that fails is still the one
    /// wgpu configured before, which acquisition reports as a validation error.
    fn configure_surface(&mut self) {
        self.frame_gen.stop();
        // The submit thread may still present to the old swapchain.
        self.queue.wait();
        if let Some(surface) = &self.surface
            && let Err(e) = Self::configure(
                &self.device,
                surface,
                &self.surface_config(),
                self.hdr.as_ref(),
                &self.device_lost,
            )
        {
            log::warn!("Horizon renderer: the window surface did not configure ({e})");
        }
    }
    /// HDR calibration's test picture (`crate::hdr_calibration`) while it runs, else None.
    pub fn set_hdr_calibration(&mut self, calibration: Option<HdrCalibration>) {
        let Some(hdr) = &mut self.hdr else {
            return;
        };
        if hdr.calibration == calibration {
            return;
        }
        let declared = hdr.declared_peak();
        hdr.calibration = calibration;
        if hdr.declared_peak() != declared {
            self.send_hdr_metadata();
        }
    }
    fn send_hdr_metadata(&self) {
        if let (Some(surface), Some(hdr)) = (&self.surface, &self.hdr) {
            horizon_hdr::set_metadata(&self.device, surface, hdr.declared_peak(), hdr.black);
        }
    }
    /// The swapchain's configuration: `config` (what every pipeline draws in), or
    /// HDR10 or scRGB while HDR output is on.
    fn surface_config(&self) -> wgpu::SurfaceConfiguration {
        output_apply::surface_config(&self.config, self.hdr.as_ref().map(|hdr| hdr.output))
    }
    /// This renderer's output as applied, with what its destination offers and reports.
    pub fn output(&self) -> &output::EffectiveOutput {
        &self.output
    }
    /// What this renderer's display last reported: a renderer made again for the window
    /// starts from it (`Renderer::new`'s `display`).
    pub fn display_reading(&self) -> output::DisplayReading {
        self.reading
    }
    /// Whether HDR output is on: the setting, where the display offers it.
    pub fn hdr_on(&self) -> bool {
        self.output.hdr_on()
    }
    /// The F3 screen's HDR line while the setting is on: the swapchain HDR presents to with
    /// its paper white and peak, or that the surface offers none.
    pub fn hdr_line(&self) -> String {
        match &self.hdr {
            Some(hdr) => format!(
                "HDR: {}, {}/{} nits",
                hdr.output.name(),
                hdr.paper_white.round(),
                hdr.peak.round()
            ),
            None => "HDR: not offered by this display".into(),
        }
    }
    /// The HDR Lighting setting (`Hdr::look`).
    pub fn set_hdr_lighting(&mut self, on: bool) {
        self.hdr_lighting_setting = on;
        if let Some(hdr) = &mut self.hdr {
            hdr.look = on;
        }
    }
    /// Whether this renderer's frame is the one a renderer made now would draw in
    /// (`requested_frame`). Under `HORIZON_HDR_FRAME=float` the game makes the renderer again
    /// when it isn't (docs/design/hdr-linear-light.md, "When it changes").
    pub fn frame_follows(
        &self,
        settings: &crate::graphics::Graphics,
        request: output::FrameRequest,
    ) -> bool {
        self.requested_frame(settings, request) == self.output.frame
    }
    /// The frame a renderer made now would draw in (`output::resolve_frame`) for `settings`
    /// and the pack's `request`, with what this renderer's destination offers and its display
    /// reports.
    pub fn requested_frame(
        &self,
        settings: &crate::graphics::Graphics,
        request: output::FrameRequest,
    ) -> output::FrameConfiguration {
        output::resolve_frame(
            settings,
            request,
            self.caps_for(settings),
            self.reading,
            self.config.format,
        )
    }
    /// Measures which pixels of each block sprite may shine (`hdr_stats`), at the current
    /// masks. Only with HDR output on: without it the frame's alpha is never read.
    pub fn measure_emission(&mut self) {
        if let Some(hdr) = &self.hdr {
            let views = sprite_views(&self.sprite_textures, &self.sprite_unused, None);
            self.emission_stats
                .measure(&self.device, &self.queue, &views, hdr.masks);
        }
    }
    /// The share of real frames frame generation shows a made frame before (1 while it
    /// keeps up), `None` while it isn't running.
    /// The posed models' store size (`SkinFrame::base`), with entity skinning.
    pub fn skinned_models(&self) -> Option<u32> {
        self.entity_skin
            .as_ref()
            .map(entity_skin::EntitySkin::stored)
    }

    /// Whether the game may pose ordinary entity models on the GPU (`Frame::skin`).
    /// Standard packs need every bridge stage; native packs, frame generation and
    /// native observers of the CPU entity stream retain their existing fallback.
    pub fn skins_entities(&self) -> bool {
        self.entity_skin.is_some()
            && !self.frame_gen.on
            && self.pack_post.as_ref().is_none_or(|(_, post)| {
                post.as_ref().is_some_and(|post| post.skins_entities())
                    && !native_render::render_needs().entity_spans
            })
            && self.native.is_none()
    }

    /// Whether the layers tagged `OIT_ENTITY` (the eyes render type's, horse
    /// markings, a slime's outer cube) may be posed on the GPU too
    /// (`mobs::skin::SkinModels::tagged`): only with improved transparency off,
    /// where they are ordinary entity quads. On, `EntityBatch` sorts them into
    /// its stages by the CPU vertices' `OIT_ENTITY` tag.
    pub fn skins_tagged(&self) -> bool {
        self.skins_entities() && !self.improved_transparency && self.pack_post.is_none()
    }

    pub fn frame_generation_share(&self) -> Option<f32> {
        self.frame_gen.made_share
    }
    /// Frame generation on or off (FSR 3.1's frame interpolation).
    pub fn set_frame_generation(&mut self, on: bool) {
        if !on {
            self.frame_gen.stop();
        }
        self.frame_gen.on = on;
    }
    /// `upscale`: output pixels per rendered pixel, for TAA to rebuild (1: native).
    /// `sharpness`: how much TAA's upscaled picture is sharpened, 0..=1.
    pub fn set_world_post(
        &mut self,
        scale: f32,
        antialias: bool,
        taa: bool,
        upscale: f32,
        sharpness: f32,
    ) {
        self.world_post.set_sharpness(&self.device, sharpness);
        let taa = taa && self.world_post.supports_taa();
        self.world_post.scale = scale.clamp(0.25, 2.0);
        self.world_post.antialias = antialias && !taa;
        self.world_post.taa = taa;
        self.world_post.upscale = if taa { upscale.clamp(1.0, 4.0) } else { 1.0 };
    }
    /// Runs `post` over the world from now on (`None`: no pack), for the dimension `dimension`;
    /// `block_ids` gives each source state its model and fluid material ids (two values
    /// per state); emission and ownership come from the mesher's quad records.
    /// Returns the programs that run and the notes of loading them, or why the pack can't run:
    /// the dimension then runs without a pack.
    pub fn set_shader_pack(
        &mut self,
        dimension: i32,
        pack: Option<horizon_shaders::Post>,
        block_ids: &[i32],
    ) -> std::result::Result<Option<(Vec<String>, Vec<String>)>, pack_post::Refused> {
        if pack.is_none() {
            self.pack_atlas = None;
            self.pack_material_atlases = None;
        }
        let instance = wgpu::vertex_attr_array![5=>Float32x4];
        let vertex = wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<TerrainVertex>() as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &TERRAIN_ATTRIBUTES,
        };
        let instances = wgpu::VertexBufferLayout {
            array_stride: 16,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &instance,
        };
        let buffers = [Some(vertex), Some(instances)];
        let flat = self.pack_terrain_flat;
        let sprite_rects = self
            .pack_atlas
            .as_ref()
            .map_or_else(Vec::new, |a| a.rects_table());
        let post = pack.map(|post| {
            let world = pack_post::World {
                atlas: self.pack_atlas.as_ref().map(|a| (a.view.clone(), a.size())),
                sprite_rects: &sprite_rects,
                material_maps: self
                    .pack_material_atlases
                    .as_ref()
                    .map(|a| [a[0].view.clone(), a[1].view.clone()]),
                lightmap: self.lightmap_view.clone(),
                gui_size: self
                    .hud
                    .atlas_texture
                    .as_ref()
                    .map_or([1, 1], |t| [t.width(), t.height()]),
                terrain: self.pack_atlas.as_ref().map(|_| pack_post::Geometry {
                    buffers: if flat { &buffers[..1] } else { &buffers },
                    table: self.flat.as_ref().filter(|_| flat).map(|f| &f.draw_layout),
                    depth_write: true,
                    atlas: None,
                    material_maps: true,
                    ids: None,
                    cull: true,
                }),
                block_ids,
                celestial: self
                    .hud
                    .atlas_texture
                    .as_ref()
                    .map(|t| t.create_view(&Default::default())),
                clouds: Some(&self.cloud_bindings),
                weather: true,
                hand: true,
            };
            pack_post::PackPost::new(
                &self.device,
                &self.queue,
                &self.adapter,
                post,
                self.output.frame.format,
                world,
                &std::sync::atomic::AtomicBool::new(false),
            )
        });
        self.pack_loading = None;
        self.install_shader_pack(dimension, post.transpose())
    }
    /// Runs the pack `set_shader_pack` or `load_shader_pack` built (`None`: no pack), or none when
    /// it can't run, for the dimension `dimension`; the programs that run and their notes.
    fn install_shader_pack(
        &mut self,
        dimension: i32,
        post: std::result::Result<Option<pack_post::PackPost>, pack_post::Refused>,
    ) -> std::result::Result<Option<(Vec<String>, Vec<String>)>, pack_post::Refused> {
        let mut post = match post {
            Ok(post) => post,
            Err(e) => {
                self.pack_post = Some((dimension, None));
                self.pack_atlas = None;
                self.pack_material_atlases = None;
                return Err(e);
            }
        };
        if let Some(post) = post.as_mut() {
            post.set_paged_maps(
                (self.shared_material_maps.each_ref())
                    .map(|t| t.as_ref().map(|t| t.create_view(&Default::default()))),
            );
        }
        let report = post.as_ref().map(|p| {
            let programs = p.programs().into_iter().map(str::to_owned).collect();
            (programs, p.notes.clone())
        });
        self.pack_post = Some((dimension, post));
        if self.pack_post.as_ref().is_some_and(|p| p.1.is_some()) {
            self.native = None;
        }
        Ok(report)
    }
    /// Makes the running shader pack, of either kind, forget what it kept from earlier frames:
    /// the next frame draws as a freshly loaded pack's first. Nothing to do without a pack.
    pub fn reset_shader_pack_history(&mut self) {
        // A frame still on the submit thread uses the targets and readbacks replaced
        // here.
        self.land_frame();
        if let Some((_, Some(post))) = &mut self.pack_post {
            post.reset_history(&self.device);
        }
        if let Some(native) = &mut self.native {
            native.frame.reset_history();
            if let Some(post) = &mut native.post {
                post.reset_history();
            }
        }
    }
    /// The running shader pack's kind: `iris` (a `shaders/` pack) or `native` (one of
    /// Horizon's own).
    pub fn shader_pack_kind(&self) -> Option<&'static str> {
        match (&self.pack_post, &self.native) {
            (Some((_, Some(_))), _) => Some("iris"),
            (_, Some(_)) => Some("native"),
            _ => None,
        }
    }
    /// `screenshot-sequence`: waits (30 seconds at most) for the frame just drawn to finish on
    /// the GPU, and takes the centre depth a `shaders/` pack copied in it, which its next frame
    /// reads: that frame's, or an error, never an older one. The last frame's PNG has already
    /// waited for it, without a limit, as `screenshot` does.
    pub fn finish_sequence_frame(&mut self) -> Result<()> {
        self.device
            .poll(wgpu::PollType::Wait {
                submission_index: None,
                timeout: Some(std::time::Duration::from_secs(30)),
            })
            .map_err(|e| format!("the frame didn't finish on the GPU: {e}"))?;
        if self.device_lost() {
            return Err("the graphics device was lost".into());
        }
        if let Some((_, Some(post))) = &mut self.pack_post {
            post.take_center_depth(&self.device)?;
        }
        Ok(())
    }
    /// As `set_shader_pack` with a pack, but its pipelines build on a thread of their own while
    /// the game keeps drawing without one: the driver compiles a big pack's programs for seconds.
    /// `poll_shader_pack` runs it once they are built. `label` and `started` come back with it.
    pub fn load_shader_pack(
        &mut self,
        dimension: i32,
        pack: horizon_shaders::Post,
        block_ids: Vec<i32>,
        label: String,
        started: std::time::Instant,
    ) {
        let device = self.device.clone();
        let queue = self.queue.clone();
        let adapter = self.adapter.clone();
        let format = self.output.frame.format;
        let atlas = self.pack_atlas.as_ref().map(|a| (a.view.clone(), a.size()));
        let sprite_rects = self
            .pack_atlas
            .as_ref()
            .map_or_else(Vec::new, |a| a.rects_table());
        let material_maps = self
            .pack_material_atlases
            .as_ref()
            .map(|a| [a[0].view.clone(), a[1].view.clone()]);
        let lightmap = self.lightmap_view.clone();
        let gui_size = self
            .hud
            .atlas_texture
            .as_ref()
            .map_or([1, 1], |t| [t.width(), t.height()]);
        let celestial = self
            .hud
            .atlas_texture
            .as_ref()
            .map(|t| t.create_view(&Default::default()));
        let flat = self.pack_terrain_flat;
        let table = (self.flat.as_ref())
            .filter(|_| flat)
            .map(|f| f.draw_layout.clone());
        let terrain = self.pack_atlas.is_some();
        let clouds = self.cloud_bindings.clone();
        let cancel = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let cancelled = cancel.clone();
        let build = move || {
            let instance = wgpu::vertex_attr_array![5=>Float32x4];
            let vertex = wgpu::VertexBufferLayout {
                array_stride: std::mem::size_of::<TerrainVertex>() as u64,
                step_mode: wgpu::VertexStepMode::Vertex,
                attributes: &TERRAIN_ATTRIBUTES,
            };
            let instances = wgpu::VertexBufferLayout {
                array_stride: 16,
                step_mode: wgpu::VertexStepMode::Instance,
                attributes: &instance,
            };
            let buffers = [Some(vertex), Some(instances)];
            let world = pack_post::World {
                atlas,
                sprite_rects: &sprite_rects,
                material_maps,
                lightmap,
                gui_size,
                terrain: terrain.then(|| pack_post::Geometry {
                    buffers: if flat { &buffers[..1] } else { &buffers },
                    table: table.as_ref(),
                    depth_write: true,
                    atlas: None,
                    material_maps: true,
                    ids: None,
                    cull: true,
                }),
                block_ids: &block_ids,
                celestial,
                clouds: Some(&clouds),
                weather: true,
                hand: true,
            };
            pack_post::PackPost::new(&device, &queue, &adapter, pack, format, world, &cancelled)
        };
        // A pack is not the game's code: a panic building it turns the pack off (#3525).
        let thread = std::thread::Builder::new()
            .name("Shader pack".into())
            .spawn(move || crate::crash::contain(build));
        match thread {
            Ok(thread) => {
                // The last pack doesn't draw this dimension meanwhile.
                self.pack_post = None;
                self.pack_loading = Some(PackLoading {
                    dimension,
                    thread,
                    label,
                    started,
                    _cancel: CancelOnDrop(cancel),
                });
            }
            Err(e) => {
                log::error!("shader pack {label}: no thread to build it on: {e}");
                // Off for this dimension, as a pack that fails, not tried again every frame.
                self.pack_loading = None;
                self.pack_post = Some((dimension, None));
            }
        }
    }
    /// The pack `load_shader_pack` builds, once its thread is done: it runs from now on, as
    /// `set_shader_pack` would run it. Its dimension, label, start and what `set_shader_pack`
    /// returns.
    #[allow(clippy::type_complexity)]
    pub fn poll_shader_pack(
        &mut self,
    ) -> Option<(
        i32,
        String,
        std::time::Instant,
        std::result::Result<Option<(Vec<String>, Vec<String>)>, pack_post::Refused>,
    )> {
        if !self.pack_loading.as_ref()?.thread.is_finished() {
            return None;
        }
        let loading = self.pack_loading.take()?;
        let built = match loading.thread.join() {
            Ok(Ok(built)) => built,
            Ok(Err(panic)) => Err(pack_post::Refused {
                reason: format!("building it panicked: {panic}"),
                notes: Vec::new(),
            }),
            Err(_) => Err(pack_post::Refused {
                reason: "its thread panicked".into(),
                notes: Vec::new(),
            }),
        };
        let report = self.install_shader_pack(loading.dimension, built.map(Some));
        Some((loading.dimension, loading.label, loading.started, report))
    }
    /// Whether a `shaders/` pack is still building (`load_shader_pack`).
    pub fn shader_pack_loading(&self) -> bool {
        self.pack_loading.is_some()
    }
    /// Drops the running shader pack, of either kind.
    pub fn drop_shader_packs(&mut self) {
        self.pack_loading = None;
        self.pack_post = None;
        self.native = None;
        self.pack_atlas = None;
        self.separate_ao = false;
    }
    /// Sets whether shader pack terrain inputs pass vertex alpha to `hz_Color.a` (`separateAo=true`).
    pub fn set_separate_ao(&mut self, separate_ao: bool) {
        if self.separate_ao != separate_ao {
            self.separate_ao = separate_ao;
            if let Some(atlas) = &mut self.pack_atlas {
                atlas.separate_ao = separate_ao;
            }
        }
    }
    /// Lava's sprite layers (still and flowing), whose faces a pack's solid terrain draws.
    pub fn set_pack_lava_layers(&mut self, layers: Vec<u16>) {
        self.pack_lava_layers = layers;
    }
    /// Runs the sky, material and post stages of one of Horizon's own shader packs from now
    /// on (`None`: no pack), with its options' values; replaces a `shaders/` pack. Returns the
    /// stages' labels; on an error the pack that ran keeps running.
    pub fn set_native_shader_pack(
        &mut self,
        pack: Option<(
            horizon_shader_api::Report,
            Vec<(String, f64)>,
            Vec<(u32, u32)>,
            Vec<(u32, u32)>,
            block_light::LightTable,
        )>,
    ) -> std::result::Result<Vec<String>, String> {
        let linear = pack
            .as_ref()
            .is_some_and(|p| p.0.manifest.api == horizon_shader_api::Api::V0_2);
        if (linear || self.output.frame.format == LINEAR_FRAME)
            && linear != (self.output.frame.format == LINEAR_FRAME)
            && !std::mem::replace(&mut self.frame_kept_logged, true)
        {
            log::info!(
                "Horizon renderer: the {:?} frame stays until the renderer is made again; a shader pack picked or dropped since runs on it",
                self.output.frame.format
            );
        }
        let Some((report, constants, sprite_tags, sprite_materials, lights)) = pack else {
            self.native = None;
            return Ok(Vec::new());
        };
        let terrain = native_material::TerrainShape {
            globals: &self.terrain_layout,
            flat: self.flat.as_ref().map(|f| &f.draw_layout),
            sprites: self.terrain_sprites,
            format: self.output.frame.format,
            linear: self.output.frame.linear_light,
            sprite_tags: &sprite_tags,
            sprite_materials: &sprite_materials,
            entities: &self.entity_bindings,
            weather_groups: [&self.weather.entity_layout, &self.weather.time_layout],
            pbr: self.pbr_maps.as_ref().and_then(|arrays| arrays.first()),
        };
        let (pack, labels) = native_pack::NativePack::new(
            &self.device,
            &self.queue,
            &terrain,
            report,
            &constants,
            lights,
        )?;
        self.native = pack.draws().then_some(pack);
        if self.native.is_some() {
            self.pack_post = None;
            self.pack_atlas = None;
            self.pack_material_atlases = None;
        }
        Ok(labels)
    }
    /// What a shader pack's gbuffers programs read from Horizon's geometry: the sky's, and
    /// terrain's in the pack's block atlas (made here when missing; none when it does not fit).
    pub fn pack_inputs(&mut self) -> horizon_shaders::GameInputs {
        let terrain = self.pack_terrain_inputs();
        // A geometry stage reads flat draws' vertices itself, through their index buffer.
        let terrain_geometry = terrain
            .clone()
            .filter(|_| self.pack_terrain_flat)
            .map(|mut inputs| {
                inputs.declarations.push_str(
                    "layout(set = 5, binding = 0) readonly buffer HzIndices { uint hz_indices[]; };\n",
                );
                horizon_shaders::GeometryInputs {
                    inputs,
                    pull: pack_atlas::PULL.to_owned(),
                    index: "hz_indices[@]".to_owned(),
                }
            });
        horizon_shaders::GameInputs {
            terrain_parts: terrain
                .as_ref()
                .map(|t| pack_atlas::terrain_parts(t, &self.pack_lava_layers)),
            terrain,
            terrain_geometry,
            entity_geometry: Some(pack_entities::geometry_inputs()),
            sky: Some(pack_sky::inputs()),
            celestial: Some(pack_sky::celestial_inputs()),
            entities: Some(pack_entities::inputs()),
            clouds: Some(pack_entities::cloud_inputs()),
            weather: Some(pack_weather::inputs()),
            lines: Some(pack_lines::inputs()),
            hand: Some(pack_hand::inputs()),
        }
    }

    fn pack_terrain_inputs(&mut self) -> Option<horizon_shaders::GbuffersInputs> {
        // Flat draws whenever the device has them (`render` keeps them on with a pack).
        let flat = self.flat.is_some() && self.flat_on;
        // Flat draws read each sprite's place from a table: the atlas is stitched as the game's.
        // Other draws need the grid.
        if !flat && self.pack_atlas.as_ref().is_some_and(|a| a.is_stitched()) {
            self.pack_atlas = None;
        }
        if self.pack_atlas.is_none() {
            let mut encoder = self.device.create_command_encoder(&Default::default());
            let layers = self
                .sprite_textures
                .iter()
                .map(|t| t.depth_or_array_layers())
                .sum();
            let sprite = self.sprite_textures[0].width();
            // Vanilla's padding counts the anisotropy bit only with anisotropic filtering.
            let anisotropy_bit = if self.texture_filtering == 2 {
                u32::from(self.max_anisotropy).trailing_zeros()
            } else {
                0
            };
            let stitched = flat
                .then(|| {
                    pack_atlas::PackAtlas::stitched(
                        &self.device,
                        &mut encoder,
                        &self.sprite_textures,
                        self.sprite_split,
                        sprite,
                        &self.sprite_native,
                        self.mipmap_option,
                        anisotropy_bit,
                    )
                })
                .flatten();
            if flat && stitched.is_none() {
                log::warn!(
                    "shader pack block atlas: the sprites don't stitch on this device, a grid instead"
                );
            }
            let mut atlas = stitched.or_else(|| {
                pack_atlas::PackAtlas::new(
                    &self.device,
                    &mut encoder,
                    &self.sprite_textures,
                    self.sprite_split,
                    layers,
                    sprite,
                )
            });
            if let Some(atlas) = &mut atlas {
                atlas.separate_ao = self.separate_ao;
            }
            self.pack_material_atlases = atlas.as_ref().and_then(|colour| {
                let maps = self.pbr_maps.as_ref()?;
                let make = |kind: usize, encoder: &mut wgpu::CommandEncoder| {
                    let arrays: Vec<_> = maps.iter().map(|pair| pair[kind].clone()).collect();
                    colour.material_for(&self.device, encoder, &arrays, layers)
                };
                Some([make(0, &mut encoder)?, make(1, &mut encoder)?])
            });
            self.pack_atlas = atlas;
            self.queue.submit([encoder.finish()]);
        }
        self.pack_terrain_flat = flat;
        Some(
            self.pack_atlas
                .as_ref()?
                .terrain_inputs(self.pack_terrain_flat),
        )
    }
    /// Times the shader pack's passes on the GPU while `on` (the F3 overlay is open), or always
    /// with `HORIZON_SHADER_PACK_TIMING`.
    pub fn set_pass_timing(&mut self, on: bool) {
        self.pass_timing = on || pack_timing::PassTiming::logging();
    }
    /// Times a frame at most once per `gap` while passes are timed (the F3 overlay needs far fewer
    /// samples than a measurement). `HORIZON_SHADER_PACK_TIMING` keeps every sample.
    pub fn set_pass_timing_gap(&mut self, gap: std::time::Duration) {
        self.pass_timing_gap = if pack_timing::PassTiming::logging() {
            std::time::Duration::ZERO
        } else {
            gap
        };
    }
    /// The running shader pack's timer, of either kind.
    fn pass_timer<'a>(
        pack: &'a mut Option<(i32, Option<pack_post::PackPost>)>,
        native: &'a mut Option<native_pack::NativePack>,
    ) -> Option<&'a mut Option<pack_timing::PassTiming>> {
        match (pack, native) {
            (Some((_, Some(post))), _) => Some(&mut post.timing),
            (_, Some(native)) => Some(&mut native.timing),
            _ => None,
        }
    }
    /// Each pass of the running shader pack with its median GPU time in milliseconds, while
    /// they are timed (`set_pass_timing`).
    pub fn shader_pass_times(&self) -> Vec<(String, f32)> {
        let timer = match (&self.pack_post, &self.native) {
            (Some((_, Some(post))), _) => post.timing.as_ref(),
            (_, Some(native)) => native.timing.as_ref(),
            _ => None,
        };
        timer
            .map(pack_timing::PassTiming::medians)
            .unwrap_or_default()
    }
    /// Each of Horizon's own passes with its median GPU time in milliseconds, while they are
    /// timed (`set_pass_timing`).
    pub fn own_pass_times(&self) -> Vec<(String, f32)> {
        self.own_timing
            .as_ref()
            .map(pack_timing::PassTiming::medians)
            .unwrap_or_default()
    }
    /// The macros this renderer gives shader packs: the GL extensions this device has
    /// (subgroups, when it has them), and the vendor, renderer and mipmap levels
    /// (`Macros::define_renderer`).
    pub fn define_pack_macros(&self, macros: &mut horizon_shaders::Macros) {
        macros.define_renderer(self.mipmaps);
        if self.device.features().contains(wgpu::Features::SUBGROUP) {
            for ext in [
                "basic",
                "vote",
                "arithmetic",
                "ballot",
                "shuffle",
                "shuffle_relative",
            ] {
                macros.define(format!("MC_GL_KHR_shader_subgroup_{ext}"), "");
            }
        }
    }

    /// The dimension the shader pack was last set for.
    pub fn shader_pack_dimension(&self) -> Option<i32> {
        let loading = self.pack_loading.as_ref().map(|l| l.dimension);
        loading.or_else(|| self.pack_post.as_ref().map(|p| p.0))
    }
    /// The Texture Filtering and Max Anisotropy options (`filtering`: 0 none, 1 RGSS, 2
    /// anisotropic; `bit`: `maxAnisotropyBit`). All take LevelRenderer's chunk layer sampler
    /// (linear, clamped, `bit` anisotropy when anisotropic) and terrain.wgsl's sampleNearest,
    /// or sampleRGSS for RGSS.
    pub fn set_texture_filtering(&mut self, filtering: i32, bit: i32) {
        let filtering = filtering.clamp(0, 2) as u32;
        let anisotropy = if filtering == 2 {
            sampler_anisotropy(bit)
        } else {
            1
        };
        if filtering == self.texture_filtering && anisotropy == self.max_anisotropy {
            return;
        }
        self.texture_filtering = filtering;
        self.max_anisotropy = anisotropy;
        self.sprite_sampler = self
            .device
            .create_sampler(&sprite_sampler_descriptor(anisotropy));
        self.rebuild_terrain_group();
    }
    fn rebuild_terrain_group(&mut self) {
        self.sprite_views = sprite_views(
            &self.sprite_textures,
            &self.sprite_unused,
            Some(self.mipmaps + 1),
        );
        self.group = terrain_group(
            &self.device,
            "Horizon terrain resources",
            &self.terrain_layout,
            &self.globals,
            &self.sprite_views,
            &self.sprite_sampler,
            &self.lightmap_view,
            &self.lightmap_sampler,
            &self.emission_stats.view,
        );
        self.fluid_group = terrain_group(
            &self.device,
            "Still fluid resources",
            &self.terrain_layout,
            &self.globals,
            &self.sprite_views,
            &self.fluid_sampler,
            &self.lightmap_view,
            &self.lightmap_sampler,
            &self.emission_stats.view,
        );
        self.terrain_model_group = terrain_group(
            &self.device,
            "Native cutout model resources",
            &self.terrain_layout,
            &self.globals,
            &self.sprite_views,
            &self.terrain_model_sampler,
            &self.lightmap_view,
            &self.lightmap_sampler,
            &self.emission_stats.view,
        );
    }
    /// Block sprite layers per array on this device: a sprite's array is its layer over it, its
    /// layer in that array the rest (`block_sprites::Lookup`).
    pub fn sprite_split(&self) -> u32 {
        self.sprite_split
    }
    /// The Mipmap Levels option: block textures sample at most `levels`
    /// reduced copies (0: none).
    pub fn set_mipmap_levels(&mut self, levels: u32) {
        self.mipmap_option = levels;
        let levels = crate::sprites::sprite_mipmap_limit(
            levels,
            self.sprite_textures[0].width(),
            self.sprite_minimum_size,
            self.mip_levels,
        );
        if levels == self.mipmaps {
            return;
        }
        self.mipmaps = levels;
        self.rebuild_terrain_group();
    }
    /// The views `render` draws `frame` from.
    pub fn views(&self, frame: &Frame) -> ViewSet {
        ViewSet::single(self.screen_view(
            frame.fov_degrees,
            frame.yaw,
            frame.pitch,
            frame.depth_far,
        ))
    }
    /// The view frustum `render` will cull with, for culling geometry built
    /// before the frame (entities, block entities) the same way.
    pub fn camera_frustum(
        &self,
        fov_degrees: f32,
        yaw: f32,
        pitch: f32,
        bob: Mat4,
        depth_far: f32,
    ) -> Frustum {
        let views = ViewSet::single(self.screen_view(fov_degrees, yaw, pitch, depth_far));
        Frustum {
            planes: frustum(views.cull().view_proj(bob)),
        }
    }
    /// A conservative view for geometry built before TAA chooses this frame's
    /// jitter. Cover a whole rendered pixel on each side, at the configured
    /// resolution scale/upscale, rather than borrowing the previous frame's size.
    pub fn camera_frustum_with_jitter(
        &self,
        fov_degrees: f32,
        yaw: f32,
        pitch: f32,
        bob: Mat4,
        depth_far: f32,
    ) -> Frustum {
        let views = ViewSet::single(self.screen_view(fov_degrees, yaw, pitch, depth_far));
        taa_frustum(
            views.cull().view_proj(bob),
            self.world_post
                .taa_render_size([self.config.width, self.config.height]),
        )
    }
    fn screen_view(&self, fov_degrees: f32, yaw: f32, pitch: f32, far: f32) -> View {
        let aspect = self.config.width as f32 / self.config.height.max(1) as f32;
        View::screen(yaw, pitch, fov_degrees, aspect, far)
    }
    fn update_lightmap(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        sky: &Sky,
        darkness: f32,
        gamma: f32,
        night_vision: f32,
        flicker: f32,
        boss_darken: f32,
    ) {
        // lightmap.fsh: per (block, sky) level.
        let brightness = |level: f32| level / (4.0 - 3.0 * level);
        let mut pixels = [0u8; 16 * 16 * 4];
        for s in 0..16 {
            for b in 0..16 {
                let block_level = b as f32 / 15.0;
                let sky_level = s as f32 / 15.0;
                let block_brightness = brightness(block_level) * (1.4 + flicker);
                let sky_brightness = brightness(sky_level) * sky.sky_light_factor;
                // NightVisionColor defaults to 0x999999.
                let mut color =
                    Vec3::from(sky.ambient_light_color).max(Vec3::splat(0.6 * night_vision));
                color += Vec3::from(sky.sky_light_color) * sky_brightness;
                let parabolic = (2.0 * block_level - 1.0).powi(2);
                let block_color = Vec3::from(sky.block_light_tint).lerp(Vec3::ONE, 0.9 * parabolic);
                color += block_color * block_brightness;
                // lightmap.fsh: a boss bar mixes the light toward 70/60/60%.
                color = color.lerp(color * Vec3::new(0.7, 0.6, 0.6), boss_darken);
                color -= Vec3::splat(darkness);
                color = color.clamp(Vec3::ZERO, Vec3::ONE);
                let max = color.max_element().max(1e-6);
                let inverted = 1.0 - max;
                let scaled = 1.0 - inverted.powi(4);
                let not_gamma = color * (scaled / max);
                color = color.lerp(not_gamma, gamma);
                let i = (s * 16 + b) * 4;
                pixels[i] = (color.x * 255.0) as u8;
                pixels[i + 1] = (color.y * 255.0) as u8;
                pixels[i + 2] = (color.z * 255.0) as u8;
                pixels[i + 3] = 255;
            }
        }
        if self.fullbright {
            pixels.fill(255);
        }
        // The lightmap changes with the time of day, flicker and effects,
        // not every frame: upload only what differs from the texture.
        if pixels == self.lightmap_pixels && self.lightmap_uploaded {
            return;
        }
        self.uploads.texture(
            encoder,
            wgpu::TexelCopyTextureInfo {
                texture: &self.lightmap,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            16,
            16,
            &pixels,
        );
        self.lightmap_pixels = pixels;
        self.lightmap_uploaded = true;
    }
    fn animate(&mut self, encoder: &mut wgpu::CommandEncoder, tick: u64, cull_inactive: bool) {
        let active = cull_inactive.then_some(&self.active_animated_layers[..]);
        self.animations.animate_modes(
            encoder,
            &mut self.uploads,
            tick,
            active,
            self.own_timing.as_mut(),
        );
        if let Some(arrays) = &self.pbr_maps {
            for animation in &self.material_animations {
                if !self.animations.changed().contains(&animation.layer) {
                    continue;
                }
                let Some(pair) = arrays.get((animation.layer / self.sprite_split) as usize) else {
                    continue;
                };
                for kind in 0..2 {
                    let Some(pixels) = animation.material_pixels(kind, tick) else {
                        continue;
                    };
                    let texture = &pair[kind];
                    let from = (pixels.len() / 4).isqrt() as u32;
                    for level in 0..texture.mip_level_count() {
                        let size = texture.width() >> level;
                        let pixels = crate::sprites::resample(pixels, from, size);
                        self.uploads.texture(
                            encoder,
                            wgpu::TexelCopyTextureInfo {
                                texture,
                                mip_level: level,
                                origin: wgpu::Origin3d {
                                    x: 0,
                                    y: 0,
                                    z: animation.layer % self.sprite_split,
                                },
                                aspect: wgpu::TextureAspect::All,
                            },
                            size,
                            size,
                            &pixels,
                        );
                    }
                }
            }
            if let Some(atlases) = &self.pack_material_atlases {
                for kind in 0..2 {
                    let arrays: Vec<_> = arrays.iter().map(|pair| pair[kind].clone()).collect();
                    atlases[kind].copy(
                        &self.device,
                        encoder,
                        &arrays,
                        self.animations.changed().iter().copied(),
                    );
                }
            }
        }
        if let Some(atlas) = &self.pack_atlas {
            atlas.copy(
                &self.device,
                encoder,
                &self.sprite_textures,
                self.animations.changed().iter().copied(),
            );
        }
    }
    pub fn render(&mut self, frame: &Frame) -> Result<bool> {
        // A lost device cannot accept uploads either. Leave recovery to the
        // owner instead of submitting atlas patches to the invalid device.
        if self.device_lost() {
            return Err("the graphics device was lost".into());
        }
        if let Some(request) = self.capture.take() {
            let result = self.write_capture(&request, frame);
            self.capture_result = Some(result.map_err(|e| e.to_string()));
        }
        let scaled = self.screenshot_size();
        let rendered = match scaled {
            Some(size) => self.draw_scaled_frame(frame, size),
            None => self.draw_frame(frame, None, self.output),
        };
        // A frame that was not drawn still submits the section copies and
        // atlas patches (glyphs, skins, maps), which the caller already took.
        if self.device_lost() {
            return Err("the graphics device was lost".into());
        }
        if !matches!(rendered, Ok(true)) && !frame.hud.patches.is_empty() {
            let section_copies = self.section_copies.take();
            let has_section_copies = section_copies.is_some();
            let mut encoder = section_copies.unwrap_or_else(|| {
                self.device
                    .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                        label: Some("Horizon atlas patches"),
                    })
            });
            self.hud
                .upload_patches(&mut self.uploads, &mut encoder, &frame.hud.patches);
            self.uploads.finish(&encoder);
            if has_section_copies {
                self.section_copy_bytes = 0;
                self.section_uploads.finish(&encoder);
            }
            self.queue.submit([encoder.finish()]);
        } else if let Some(copies) = self.take_section_copies() {
            self.queue.submit([copies]);
        }
        // A scaled screenshot's frame never reached the window.
        rendered.map(|drawn| drawn && scaled.is_none())
    }
    /// Whether the device's loss callback has fired. A surface reconfigure
    /// alone cannot restore resources belonging to a lost device.
    pub fn device_lost(&self) -> bool {
        self.device_lost.load(std::sync::atomic::Ordering::Relaxed)
    }
    /// The frame size of the pending `scaled` screenshot when it is larger than the
    /// window: the window's times the Resolution Scale (when above 100%) and
    /// `screenshot_quality`, cut down to what one texture and one readback buffer hold,
    /// and halved for each time it ran out of video memory.
    fn screenshot_size(&self) -> Option<[u32; 2]> {
        let request = self.screenshot.as_ref()?;
        if !request.scaled || request.size.is_some() || self.suspended {
            return None;
        }
        let window = [self.config.width, self.config.height];
        let limits = self.device.limits();
        let factor = scaled_screenshot_factor(
            window,
            self.world_post.scale,
            self.screenshot_quality,
            limits.max_texture_dimension_2d,
            limits.max_buffer_size,
        ) / 2f32.powi(self.screenshot_halvings as i32);
        (factor > 1.0).then(|| window.map(|side| (side as f32 * factor) as u32))
    }
    /// Draws one frame at `size` into a texture of its own, for the pending screenshot:
    /// every pass sizes itself by `config`, which says `size` for this frame, the HUD
    /// keeps its layout at a larger scale, and the world is drawn at `size` (a Resolution
    /// Scale below 100% stays as low). The window keeps its last frame.
    fn draw_scaled_frame(&mut self, frame: &Frame, size: [u32; 2]) -> Result<bool> {
        let window = [self.config.width, self.config.height];
        let mut config = self.config.clone();
        [config.width, config.height] = size;
        config.usage |= wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC;
        // A frame the overlays draw on past 8 bits (16-bit, or the layered float frame), read
        // back through an 8-bit copy (`take_readback`). A 0.2 pack's float frame in SDR keeps
        // the swapchain's format here, which its picture finishes on (`Readback::finish`).
        if self.output.frame.overlay_format != config.format.remove_srgb_suffix() {
            config.format = self.output.frame.format;
            config.view_formats = vec![self.output.frame.format];
            config.usage |= wgpu::TextureUsages::TEXTURE_BINDING;
        }
        // The layered float frame keeps HDR: its world's light and HUD layer make the picture
        // (`Hdr::sdr_screenshot`), in HDR's own targets at `size`; it presents nothing, as the
        // swapchain is set aside. Other frames draw without HDR, into a target of their own.
        let output = self.output.scaled_screenshot();
        let layered = output.hdr_on();
        // Out of video memory (an invalid texture's view is a validation error):
        // tried again at half the size next frame; the window keeps its last one.
        let checks = GpuChecks::push(&self.device);
        let target = (!layered).then(|| Self::offscreen_target(&self.device, &config));
        let depth = Self::depth(&self.device, &config);
        if let Some(e) = checks.pop() {
            log::warn!(
                "Horizon renderer: no memory for a {}x{} screenshot ({e}); trying half the size",
                size[0],
                size[1]
            );
            self.screenshot_halvings += 1;
            return Ok(false);
        }
        let scale = self.world_post.scale;
        self.world_post.scale = scale / scale.max(1.0);
        self.hud.zoom = size[0] as f32 / window[0] as f32;
        [self.config.width, self.config.height] = size;
        let surface = self.surface.take();
        let hdr = self.hdr.take_if(|_| !layered);
        let offscreen = std::mem::replace(&mut self.offscreen, target);
        let window_depth = std::mem::replace(&mut self.depth, depth);
        // The frame's own targets at `size` can run out of memory too.
        let drawn = self.draw_frame(frame, Some(GpuChecks::push(&self.device)), output);
        self.depth = window_depth;
        self.offscreen = offscreen;
        match hdr {
            Some(hdr) => self.hdr = Some(hdr),
            // Not kept for the window, which makes its own at its size next frame.
            None => {
                if let Some(hdr) = &mut self.hdr {
                    hdr.release_targets();
                }
            }
        }
        self.surface = surface;
        [self.config.width, self.config.height] = window;
        self.hud.zoom = 1.0;
        self.world_post.scale = scale;
        drawn
    }
    /// `checks`: the error scopes of a scaled screenshot's frame, checked before its PNG is written.
    /// `output`: what this frame is drawn for, the window's output or a screenshot's.
    fn draw_frame(
        &mut self,
        frame: &Frame,
        checks: Option<GpuChecks>,
        output: output::EffectiveOutput,
    ) -> Result<bool> {
        debug_assert_eq!(output.encoding, self.hdr.as_ref().map(|hdr| hdr.output));
        // Its steps as spans (HORIZON_SPIKES): each lap names the step that just ended.
        let mut laps = crate::spans::laps();
        let landing = Instant::now();
        self.land_frame();
        // Waiting for the previous frame (offscreen: at most two on the GPU), not work.
        let landed = landing.elapsed();
        self.queue.take_waited();
        // World frames alone follow the shader pack: the game loads the picked one
        // before it draws one. Title frames draw no terrain and keep the layout
        // through a pack dropped (Shader Options) or not loaded yet (`Renderer::new`),
        // so the next world's sections, which the level loading screen uploads, stay.
        if !frame.title {
            self.follow_translucent_layout();
            self.follow_packed_layout();
        }
        let released = self.arena.trim();
        self.graveyard.add(released);
        let left = self.compact_arena();
        if let Some(arena) = &mut self.translucent_arena {
            self.graveyard.add(arena.trim());
        }
        self.graveyard.add(self.sorted_indices.trim());
        arena::free_later(self.graveyard.advance());
        self.shrink_translucent(left);
        if let Some(flat) = &mut self.flat {
            let translucent = self.translucent_arena.as_ref();
            flat.trim(|key| match key.checked_sub(flat::TRANSLUCENT_KEYS) {
                Some(page) => translucent.is_some_and(|a| a.has_page(page)),
                None => self.arena.has_page(key),
            });
        }
        if self.suspended {
            return Ok(false);
        }
        let started = Instant::now();
        if self.first_frame == FirstFrame::Acquire {
            // Once: a minimized window acquires nothing for a while.
            startup_step(format_args!("acquiring the first frame"));
            self.first_frame = FirstFrame::Submit;
        }
        if self.surface_unconfigured
            && let Some(surface) = &self.surface
        {
            // Acquiring from it would panic: configure it again, or skip the
            // frame. A lost device never configures it.
            if self.device_lost.load(std::sync::atomic::Ordering::Relaxed) {
                return Err("the graphics device was lost".into());
            }
            self.frame_gen.stop();
            if Self::configure(
                &self.device,
                surface,
                &self.surface_config(),
                self.hdr.as_ref(),
                &self.device_lost,
            )
            .is_err()
            {
                return Ok(false);
            }
            self.surface_unconfigured = false;
        }
        laps.lap("r_prepare");
        // Frame generation: its present thread acquires and presents, this frame draws
        // into frames of its own. Not over menus that blur the frame, a shader pack's
        // own post passes, HDR (or the 16-bit frame it was started with) or the title screen.
        let fg = self.frame_gen.on
            && !self.frame_gen.paused
            && self.hdr.is_none()
            && self.output.frame.format == self.config.format.remove_srgb_suffix()
            && !frame.title
            && frame.menu_blur.is_none()
            && self.pack_post.as_ref().is_none_or(|p| p.1.is_none())
            && self.native.as_ref().is_none_or(|n| n.post.is_none())
            && (self.surface.is_none()
                || self.config.usage.contains(wgpu::TextureUsages::COPY_DST));
        if !fg {
            self.frame_gen.stop();
        }
        // Waits for the submit thread since the last frame's present (this frame's uploads).
        let queued = self.queue.take_waited();
        let acquire = Instant::now();
        let fg_frames = fg
            .then(|| {
                self.frame_gen.frames(
                    &self.device,
                    &self.queue,
                    self.surface.as_ref(),
                    self.offscreen.as_ref(),
                    &self.window,
                    &self.config,
                )
            })
            .flatten();
        // The present thread failed: this frame acquires as without frame generation.
        let fg = fg_frames.is_some();
        let mut fg_made = false;
        let acquired = if fg {
            None
        } else {
            self.surface.as_ref().map(|s| s.get_current_texture())
        };
        self.last_wait = landed + queued + acquire.elapsed();
        // Inside the acquire's time already.
        self.queue.take_waited();
        let (surface_frame, reconfigure) = match acquired {
            None => (None, false),
            Some(wgpu::CurrentSurfaceTexture::Success(f)) => (Some(f), false),
            Some(wgpu::CurrentSurfaceTexture::Suboptimal(f)) => (Some(f), true),
            Some(wgpu::CurrentSurfaceTexture::Timeout | wgpu::CurrentSurfaceTexture::Occluded) => {
                return Ok(false);
            }
            Some(wgpu::CurrentSurfaceTexture::Outdated) => {
                self.configure_surface();
                return Ok(false);
            }
            Some(wgpu::CurrentSurfaceTexture::Lost) => {
                self.recreate_surface()?;
                return Ok(false);
            }
            Some(wgpu::CurrentSurfaceTexture::Validation) => {
                // Acquire caught a validation error: the swapchain cannot be
                // used as configured (wgpu reports a configuration raced with
                // a submission this way). Give it a fresh configuration like
                // an Outdated one, and once that has not helped, reattach the
                // window like a Lost one — a fullscreen loading screen then
                // skips frames instead of failing every one of them.
                if self.acquire_validation_failures == 0 {
                    self.configure_surface();
                } else {
                    self.recreate_surface()?;
                }
                self.acquire_validation_failures =
                    self.acquire_validation_failures.saturating_add(1);
                return Ok(false);
            }
        };
        if surface_frame.is_some() {
            self.acquire_validation_failures = 0;
            self.frame_gen.paused = false;
        }
        let size = [self.config.width, self.config.height];
        let target_texture = match (&surface_frame, &mut self.hdr) {
            _ if let Some((real, _)) = &fg_frames => real.clone(),
            (_, Some(hdr)) => hdr.frame(&self.device, size),
            // A screenshot of a swapchain without COPY_SRC (OpenGL), and a 16-bit frame
            // with HDR off, draw into a texture of their own, then onto the swapchain or
            // the offscreen target.
            (f, None) => {
                let image = match f {
                    Some(f) => f.texture.clone(),
                    None => self.offscreen.clone().ok_or("no offscreen target")?,
                };
                self.readback
                    .target(
                        &self.device,
                        &image,
                        self.output.frame.format,
                        self.screenshot.is_some(),
                    )
                    .unwrap_or(image)
            }
        };
        // One command buffer for the frame, after the section copies staged
        // since the last one: Metal commits every command buffer wgpu-core
        // makes (one per run of copies and two per pass) on its own.
        let section_copies = self.section_copies.take();
        let has_section_copies = section_copies.is_some();
        let mut encoder = section_copies.unwrap_or_else(|| {
            self.device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("Horizon frame"),
                })
        });
        // Horizon's own passes are timed like a shader pack's, from the frame's first pass.
        if self.own_timing.is_some() != self.pass_timing {
            self.own_timing = self
                .pass_timing
                .then(|| pack_timing::PassTiming::new(&self.device, &self.queue))
                .flatten();
        }
        if let Some(t) = &mut self.own_timing {
            t.gap = self.pass_timing_gap;
            t.begin(&self.device, &self.queue);
        }
        if self.animated_textures {
            self.hud
                .animate_sprites(&mut self.uploads, &mut encoder, frame.ticks);
        }
        self.update_lightmap(
            &mut encoder,
            &frame.sky,
            frame.darkness.1,
            (self.gamma - frame.darkness.0).max(0.0),
            frame.night_vision,
            frame.block_flicker,
            frame.boss_darken,
        );
        let views = self.views(frame);
        let pose = frame.bob * frame.spin;
        let view_proj = views.primary().view_proj(pose);
        laps.lap("r_acquire");
        // TAA runs where the world post pass does, not under a shader pack's own.
        let taa = !frame.title
            && self.pack_post.as_ref().is_none_or(|p| p.1.is_none())
            && self.native.as_ref().is_none_or(|n| n.post.is_none());
        let hdr_lighting = self.native.is_none()
            && self.pack_post.as_ref().is_none_or(|p| p.1.is_none())
            && std::env::var_os("HORIZON_HDR_LIGHTING").is_none_or(|v| v != "0");
        if let Some(hdr) = &mut self.hdr {
            hdr.lighting = hdr_lighting && hdr.look;
        }
        let hdr_gain = no_pack_hdr_gain(
            self.hdr.as_ref().is_some_and(|h| h.layered()),
            self.shader_pack_kind().is_some(),
            !frame.title,
        );
        let (hdr_settings, hdr_shares) = self
            .hdr
            .as_ref()
            .map_or(([0.0; 4], [0.0; 4]), |h| h.draw_params(!frame.title));
        self.world_post.baked_light(
            hdr_gain,
            hdr_settings,
            hdr_shares,
            self.hdr.as_ref().map_or([[0.0; 4]; 2], hdr::Hdr::windows),
        );
        let jitter = self.world_post.jitter(
            [self.config.width, self.config.height],
            taa,
            view_proj,
            frame.camera,
        );
        let jittered = jitter * view_proj;
        if frame.title {
            self.panorama.prepare(
                &self.device,
                &self.queue,
                self.config.width,
                self.config.height,
                frame.yaw,
                frame.pitch,
            );
        } else {
            self.panorama.release();
        }
        let view = views.primary().view;
        // The hand's bob and view (`hand_globals`).
        let hand_view = frame.bob * view;
        let sun_angle = frame.sky.sun_angle.to_radians();
        // Celestial bodies rotate around the X axis; angle 0 = noon.
        let sun = Vec3::new(0.0, sun_angle.cos(), -sun_angle.sin());
        let (fog_color, fog) = if let Some(f) = frame.fog {
            (
                f.color,
                [
                    f.environmental_start,
                    f.environmental_end,
                    f.render_distance_start,
                    f.render_distance_end,
                ],
            )
        } else if frame.underwater {
            (
                [
                    frame.water_fog[0],
                    frame.water_fog[1],
                    frame.water_fog[2],
                    1.0,
                ],
                [-8.0, 96.0, -8.0, 96.0],
            )
        } else {
            let d = frame.distance;
            (
                [
                    frame.sky.fog_color[0],
                    frame.sky.fog_color[1],
                    frame.sky.fog_color[2],
                    1.0,
                ],
                // FogRenderer: render-distance fog starts at max(d - 16, 0)... d.
                {
                    let (start, end) = frame.environment_fog.unwrap_or((f32::MAX, f32::MAX));
                    [start, end, (d - d.min(64.0) * 0.25).max(0.0), d]
                },
            )
        };
        let globals = Globals {
            view_proj: jittered.to_cols_array_2d(),
            fog_color,
            fog,
            sky_color: [
                frame.sky.sky_color[0],
                frame.sky.sky_color[1],
                frame.sky.sky_color[2],
                1.0,
            ],
            misc: [
                1.0 - frame.sky.sky_light_factor,
                frame.game_time,
                views.primary().aspect,
                views.primary().tan_half_fov,
            ],
            inverse_view: view.inverse().to_cols_array_2d(),
            sunrise: frame.sky.sunrise_color,
            sun: [sun.x, sun.y, sun.z, frame.sky.star_brightness],
            fog_ends: [
                frame.fog.map_or(frame.distance.min(512.0), |f| f.sky_end),
                frame.fog.map_or(2048.0, |f| f.cloud_end),
                if frame.dark_disc { 1.0 } else { 0.0 },
                frame.glint_alpha,
            ],
            end_portal_sprites: frame.end_portal_sprites,
            crumbling_sprite: frame.crumbling_sprite,
            time: [
                frame.shader_game_time,
                f32::from(frame.nether_lighting),
                frame.dynamic_lights.len().min(crate::dynamic_light::MAX) as f32,
                frame.glint_ticks,
            ],
            outline_params: outline_params(self.config.width, self.config.height),
            dynamic_lights: {
                let mut lights = [[0.0; 4]; crate::dynamic_light::MAX];
                for (to, from) in lights.iter_mut().zip(frame.dynamic_lights) {
                    *to = *from;
                }
                lights
            },
            colored_lights: {
                let mut lights = [[[0.0; 4]; 2]; crate::dynamic_light::MAX];
                for (to, from) in lights.iter_mut().zip(frame.colored_lights) {
                    *to = *from;
                }
                lights
            },
            colored_count: [
                frame.colored_lights.len().min(crate::dynamic_light::MAX) as u32,
                // PvP & HUD's hit colour: entity.wgsl's hurt texel, 0 for vanilla's.
                frame.pvp.hurt_texel,
                // PvP & HUD's glint colour: entity.wgsl's `tinted_glint`, 0 for vanilla's.
                frame.pvp.glint,
                0,
            ],
            emissive_layers: {
                let mut layers = [[0; 4]; crate::native_emissive::MAX];
                for (to, from) in layers.iter_mut().zip(frame.emissive_layers) {
                    *to = *from;
                }
                layers
            },
            emissive_count: [
                frame.emissive_layers.len().min(crate::native_emissive::MAX) as u32,
                0,
                0,
                0,
            ],
            sunrise_fan: sunrise_fan::normals(frame.sky.sunrise_color[3], sun),
            filtering: [
                self.texture_filtering as f32,
                self.sprite_textures[0].width() as f32,
                // The views' last level (`rebuild_terrain_group`), when a sprite is one texel
                // there: RGSS takes one tap from it on (terrain.wgsl's `rotated_grid`).
                if self.sprite_textures[0].width() >> self.mipmaps == 1 {
                    self.mipmaps as f32
                } else {
                    0.0
                },
                0.0,
            ],
            hdr_windows: self.hdr.as_ref().map_or([[0.0; 4]; 2], hdr::Hdr::windows),
            hdr_light: [
                frame.sky.sky_light_factor,
                // The underwater overlay: the sky and fog are the water's own colour there.
                if frame.screen.iter().any(|q| q.world_tint) {
                    0.0
                } else {
                    frame.sky.sky_light_factor
                },
                // Only HDR output reads the solid terrain's lighting alpha.
                if self.hdr.is_some() { 1.0 } else { 0.0 },
                f32::from(u8::from(hdr_gain)),
            ],
            hdr_settings,
            hdr_shares,
        };
        self.uploads
            .buffer(&mut encoder, &self.globals, 0, bytemuck::bytes_of(&globals));
        // The sunrise fan as sixteen triangles. Its clip positions use the sky's
        // own view (no TAA jitter, bob or spin: `fs_sky`'s `view_ray` has none),
        // and its apex weight interpolates the barycentric the shader used to
        // compute for every sky pixel.
        if frame.sky.sunrise_color[3] > 0.001 && !frame.title && frame.skybox {
            let sky_view_proj = views.primary().view_proj(Mat4::IDENTITY);
            let mut mesh = [[0.0f32; 5]; sunrise_fan::VERTICES];
            for (to, from) in mesh
                .iter_mut()
                .zip(sunrise_fan::mesh(frame.sky.sunrise_color[3], sun))
            {
                let clip = sky_view_proj * glam::Vec4::new(from[0], from[1], from[2], 1.0);
                *to = [clip.x, clip.y, clip.z, clip.w, from[3]];
            }
            self.uploads.buffer(
                &mut encoder,
                &self.sunrise_buffer,
                0,
                bytemuck::cast_slice(&mesh),
            );
        }
        laps.lap("r_targets");
        // GameRenderer.render3dHud: the hand's projection at hudFov, with the
        // bob in its pose rather than the projection (same product).
        let hand_projection = Mat4::from_cols_array(&crate::camera::hand_projection(
            frame.hand_fov_degrees,
            self.config.width as f32,
            self.config.height.max(1) as f32,
            frame.depth_far,
        ));
        let hand_view_proj = hand_projection * frame.bob * view;
        let hand_globals = Globals {
            view_proj: hand_view_proj.to_cols_array_2d(),
            ..globals
        };
        self.uploads.buffer(
            &mut encoder,
            &self.hand_globals,
            0,
            bytemuck::bytes_of(&hand_globals),
        );
        // GameRenderer.render3dHud: DebugCrosshairRenderer draws with the
        // hand's projection but no bob, and FogMode.NONE (no fog colour).
        let crosshair_globals = Globals {
            view_proj: (hand_projection * view).to_cols_array_2d(),
            fog_color: [0.0; 4],
            ..globals
        };
        self.uploads.buffer(
            &mut encoder,
            &self.crosshair_globals,
            0,
            bytemuck::bytes_of(&crosshair_globals),
        );
        let pack_terrain = !frame.title
            && self
                .pack_post
                .as_ref()
                .and_then(|p| p.1.as_ref())
                .is_some_and(|p| p.has_terrain());
        // A native mod's world warp (`world_warp`), asked once a frame. While it draws, nothing is
        // left out by where it was (the frustum, the caves, occlusion, faces turned away): the warp
        // may bring it into view. Its frames keep to the per-section draws its pipelines are built
        // for. A shader pack that draws the terrain itself (`pack_terrain`) isn't warped.
        let warp_on = !frame.title && {
            let warp = native_render::world_warp();
            self.world_warps.frame(
                &self.device,
                &mut self.uploads,
                &mut encoder,
                &warp,
                frame.camera,
                // (the frame's partial tick: `weather::Time::new`'s third value)
                frame.weather_time.values[2],
                !pack_terrain,
            )
        };
        // Frustum culling and camera-relative origins.
        let cull_proj = views.cull().view_proj(pose);
        let view = Frustum {
            planes: if warp_on {
                [Vec4::W; 6]
            } else {
                frustum(cull_proj)
            },
        };
        let camera = frame.camera;
        let camera_section = camera_section(camera, self.layout.min_y());
        // The water mirror draws the same lists: they take in what its camera sees too
        // (above the screen's top, the far shore, the trees against the sky).
        let mirror_plane = frame
            .water_plane
            .filter(|_| !frame.title && !warp_on && self.wants_water_plane());
        let mirror = mirror_plane.map(|plane| Frustum {
            planes: frustum(mirror_view_projection(
                cull_proj,
                plane - frame.camera.y as f32,
            )),
        });
        // Solid quads facing away from the eye are left out: the bob moves
        // the eye off `camera`, so it is found from the projection. With the mirror, those
        // facing its eye are kept too: it sits straight below, at this camera-relative y.
        let eye = projection_center(cull_proj).filter(|_| self.sides_on && !warp_on);
        let mirror_low = mirror_plane
            .zip(eye)
            .map(|(plane, eye)| 2.0 * (plane - frame.camera.y as f32) - eye.y);
        let culling = !frame.title && frame.cave_culling && !warp_on;
        if culling {
            crate::spans::time("occlusion", || {
                self.start_occlusion(camera, frame.detail_distance)
            });
        }
        // A shader pack's shadow pass draws the sections in its camera's box, all of them (the
        // occlusion graph follows the eye, not the light), within shadowDistanceRenderMul; for
        // an Iris-format pack, only those whose shadow can fall into the camera's view.
        let shadow_view = self
            .pack_post
            .as_ref()
            .and_then(|p| p.1.as_ref())
            .filter(|_| !frame.title)
            .and_then(|p| p.shadow_view(frame))
            .or_else(|| {
                // One of Horizon's own packs with `[shadow]` uses the same lists.
                let native = self.native.as_ref().filter(|_| !frame.title)?;
                native.shadow.as_ref()?;
                native.frame.shadow_view(frame)
            });
        // `LevelRenderer.extractSectionDrawGroups`' fade-in time, for the instances.
        let fade_ms = fade_millis(frame.chunk_fade);
        // GPU culling's fading sections, decided at `fades.now`: before the gate, the record
        // check and `prepare`, which sends their flags, and no later than the instances' `now`,
        // so a section the pass draws whole is whole there too. Title frames draw no terrain
        // (a loading screen's fade-in time is 0): they leave them as they are.
        let fades = FadeInputs {
            now: Instant::now(),
            fade_ms,
            uploaded: &self.section_uploaded,
            retiring: &self.lod_retiring,
        };
        if !frame.title
            && let Some(gpu) = &mut self.gpu_culling
        {
            let (sections, rows) = (&mut self.sections, self.layout.sections());
            mark_fades(sections, &mut gpu.records, &mut gpu.fades, rows, fades);
        }
        // The near solid sections culled and drawn on the GPU (`gpu_culling`): never where a
        // shader pack's terrain, a shadow pass or the water mirror draws the CPU's lists, nor
        // under a native pack's `[displacement]`, whose grid takes its quads from the walk's
        // `visible`. Decided before the walk, so that a pass whose buffers no longer fit can
        // still switch GPU culling off.
        let radius = (frame.detail_distance / 16.0) as i32;
        let displaced = self
            .native
            .as_ref()
            .is_some_and(|n| n.displacement.is_some());
        let mut gpu_frame = self.gpu_culling.is_some()
            && !frame.title
            && !pack_terrain
            && shadow_view.is_none()
            && mirror_plane.is_none()
            && !displaced
            && !warp_on;
        #[cfg(debug_assertions)]
        if gpu_frame {
            if self.gpu_checks.is_multiple_of(64) {
                self.check_gpu_records(fades);
            }
            self.gpu_checks = self.gpu_checks.wrapping_add(1);
        }
        self.frame_serial += 1;
        let serial = self.frame_serial;
        // GPU frames' statistics come back a frame or a few later (`GpuCulling::collect`).
        if let Some(gpu) = &mut self.gpu_culling
            && let Some((frame, stats)) = gpu.collect(&self.device)
        {
            self.show_stats(frame, stats);
        }
        // The walk reads the graph GPU culling is given (checked after it).
        #[cfg(debug_assertions)]
        let graph_given = self.occlusion.generation();
        if gpu_frame && let Some(gpu) = &mut self.gpu_culling {
            let input = gpu_culling::FrameInputs {
                camera,
                min_y: self.sections.min_y(),
                sections: self.layout.sections() as i32,
                radius,
                frustum: &view,
                reach: self.occlusion.reach().filter(|_| culling),
                eye,
                view_proj: jittered,
                occlude: self.cull_on,
            };
            let prepared = crate::spans::time("gpu culling", || {
                gpu.prepare(
                    &self.device,
                    &self.queue,
                    &mut self.uploads,
                    &mut encoder,
                    &input,
                )
            });
            // A frame whose planes leave the test's integers (the camera millions of blocks
            // above or below the level), or whose eye is not a number, is the CPU's.
            gpu_frame = prepared == gpu_culling::Prepared::Gpu;
            if prepared == gpu_culling::Prepared::OutOfMemory {
                log::warn!(
                    "Horizon renderer: GPU culling off until the next start: its buffers do not fit in video memory"
                );
                self.offer_gpu_culling(crate::graphics::GpuCullingOffer::OutOfMemory);
                let _ = self.set_gpu_culling(false);
            }
        }
        self.gpu_frame = gpu_frame;
        let reachable = &self.occlusion;
        let cutout = &self.lod_cutout;
        // Sections fading out (`leave_section`) are past the render distance:
        // drawn there until they have, or they popped out at once.
        self.lod_leaving_scratch.clear();
        self.lod_leaving_scratch
            .extend(self.lod_leaving.iter().map(|&((x, _, z), _)| (x, z)));
        let leaving = &self.lod_leaving_scratch;
        let mut visible = std::mem::take(&mut self.visible);
        visible.clear();
        crate::spans::time("visibility", || {
            // GPU culling draws the solid quads of the near whole sections it has records of:
            // within the render distance the walk lists only those with translucent quads,
            // without a record or fading (`SectionGpu::listed`).
            self.sections.walk(
                camera,
                radius,
                &Frusta(view, mirror),
                |k| !culling || reachable.contains(k),
                |x, z| {
                    (!cutout.is_empty() && cutout.contains(&(x, z)))
                        || (!leaving.is_empty() && leaving.contains(&(x, z)))
                },
                gpu_frame,
                &mut visible,
            )
        });
        #[cfg(debug_assertions)]
        assert!(
            !gpu_frame || self.occlusion.generation() == graph_given,
            "the walk reads the graph GPU culling was given"
        );
        let now = std::time::Instant::now();
        let mut visible_lods = std::mem::take(&mut self.visible_lods);
        visible_lods.clear();
        if frame.title {
            visible.clear();
        } else {
            visible_lods.extend(
                self.lods
                    .iter()
                    .filter_map(|(&key, tile)| tile.visible(key, camera, &view, now)),
            );
            visible_lods.sort_unstable_by_key(|v| (v.distance, v.key));
        }
        let mut shadow_visible = std::mem::take(&mut self.shadow_visible);
        shadow_visible.clear();
        if let Some(v) = &shadow_view {
            let radius = (frame.detail_distance / 16.0) as i32;
            let light = ShadowCull {
                light: shadow_box(v),
                casters: v.view_casters.then(|| shadow_casters(cull_proj, v.light)),
                reach: v.distance,
            };
            let mut boxed = std::mem::take(&mut self.shadow_boxed_scratch);
            let mut seen = std::mem::take(&mut self.shadow_seen_scratch);
            crate::spans::time("shadow visibility", || {
                // Within the unculled distance every section, then those in the light's box.
                if let Some(d) = v.unculled {
                    let all = ShadowCull {
                        light: Frustum {
                            planes: [glam::Vec4::W; 6],
                        },
                        casters: None,
                        reach: v.distance,
                    };
                    let near = radius.min((d / 16.0).ceil() as i32);
                    self.sections.visible(
                        camera,
                        near,
                        &all,
                        |_| true,
                        |_, _| false,
                        &mut shadow_visible,
                    );
                }
                boxed.clear();
                self.sections
                    .visible(camera, radius, &light, |_| true, |_, _| false, &mut boxed);
                seen.clear();
                seen.extend(shadow_visible.iter().map(|s| s.key));
                shadow_visible.extend(boxed.drain(..).filter(|s| !seen.contains(&s.key)));
            });
            self.shadow_boxed_scratch = boxed;
            self.shadow_seen_scratch = seen;
            // Nearest the light first, so the depth test rejects what lies under it.
            let light = v.light;
            shadow_visible.sort_unstable_by(|a, b| {
                let height = |s: &resident::Visible| glam::Vec3::from(s.origin).dot(light);
                height(b).total_cmp(&height(a))
            });
        }
        // A `shaders/` pack that draws the terrain, or its shadow, sees it as Iris does.
        let shaders_pack = !frame.title
            && self
                .pack_post
                .as_ref()
                .and_then(|p| p.1.as_ref())
                .is_some_and(|p| p.has_terrain() || p.has_shadow());
        if shaders_pack {
            let from = pack_terrain_camera(camera);
            for v in visible.iter_mut().chain(shadow_visible.iter_mut()) {
                v.origin = resident::origin(v.key, self.sections.min_y(), from)
                    .as_vec3()
                    .to_array();
            }
        }
        if self.animated_textures {
            self.active_animated_layers.fill(0);
            for v in &visible {
                let s = self.sections.get(v.slot);
                for &layer in &*s.animated_layers {
                    let layer = layer as usize;
                    self.active_animated_layers[layer / u64::BITS as usize] |=
                        1_u64 << (layer % u64::BITS as usize);
                }
            }
            for v in &visible_lods {
                if let Some(tile) = self.lods.get(&v.key) {
                    for &layer in &*tile.drawn().animated_layers {
                        let layer = layer as usize;
                        self.active_animated_layers[layer / u64::BITS as usize] |=
                            1_u64 << (layer % u64::BITS as usize);
                    }
                    for &layer in &*tile.mesh.animated_layers {
                        let layer = layer as usize;
                        self.active_animated_layers[layer / u64::BITS as usize] |=
                            1_u64 << (layer % u64::BITS as usize);
                    }
                }
            }
            for v in &shadow_visible {
                let s = self.sections.get(v.slot);
                for &layer in &*s.animated_layers {
                    let layer = layer as usize;
                    self.active_animated_layers[layer / u64::BITS as usize] |=
                        1_u64 << (layer % u64::BITS as usize);
                }
            }
            // Visibility is collected before terrain draw commands are recorded,
            // so upload the current mask here and keep animation updates ahead
            // of every draw in this command buffer. A GPU frame's walk leaves out
            // the sections GPU culling draws: every layer animates.
            self.animate(&mut encoder, frame.ticks, !frame.title && !gpu_frame);
        }
        let listed = visible.len() + visible_lods.len();
        let instances = listed + shadow_visible.len();
        if instances as u64 > self.instance_capacity {
            self.instance_capacity = (instances as u64).next_power_of_two();
            self.instances = self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("Section origins"),
                size: self.instance_capacity * 16,
                usage: wgpu::BufferUsages::VERTEX
                    | wgpu::BufferUsages::COPY_DST
                    | wgpu::BufferUsages::STORAGE,
                mapped_at_creation: false,
            });
        }
        self.origins.clear();
        let (sections, retiring) = (&self.sections, &self.lod_retiring);
        laps.lap("r_cameras");
        // `LevelRenderer.extractSectionDrawGroups`: each section's `getVisibility`. Once
        // nothing is fading (the usual frame), every section is whole without a lookup.
        let fading_in = self.sections_fading_in(fade_ms, now);
        let fading_over_lods = self.sections_fading_over_lods(now);
        let fade_code = move |section: &SectionGpu, key: &SectionKey| {
            if fading_over_lods {
                section.fade(retiring.contains(key))
            } else {
                0
            }
        };
        let uploaded = &self.section_uploaded;
        let visibility = move |key: SectionKey| {
            if !fading_in {
                return 1.0;
            }
            section_visibility(uploaded.get(&key), fade_ms, now)
        };
        self.origins.extend(
            visible
                .iter()
                .map(|v| {
                    let code = fade_code(sections.get(v.slot), &v.key);
                    section_instance(v.origin, code, visibility(v.key))
                })
                .chain(visible_lods.iter().map(|v| lod::instance(v.origin, v.fade)))
                .chain(
                    shadow_visible
                        .iter()
                        .map(|v| section_instance(v.origin, 0, visibility(v.key))),
                ),
        );
        if !self.origins.is_empty() {
            self.uploads.buffer(
                &mut encoder,
                &self.instances,
                0,
                bytemuck::cast_slice(&self.origins),
            );
        }
        // `[displacement]`: the solid quads of the sections it reaches, nearest first.
        if let Some(d) = self.native.as_mut().and_then(|n| n.displacement.as_mut()) {
            let near: smallvec::SmallVec<[displace::Near<'_>; displace::MAX_SECTIONS]> =
                if frame.title {
                    smallvec::SmallVec::new()
                } else {
                    visible
                        .iter()
                        .filter(|v| {
                            let o = glam::Vec3::from(v.origin);
                            let closest = glam::Vec3::ZERO.clamp(o, o + 16.0);
                            closest.length() < d.distance
                        })
                        .filter_map(|v| {
                            let s = self.sections.get(v.slot);
                            (s.solid > 0).then(|| displace::Near {
                                buffer: self.arena.buffer(s.page),
                                start: s.start,
                                quads: s.solid,
                                origin: v.origin,
                            })
                        })
                        .collect()
                };
            d.prepare(&self.queue, &mut encoder, &near, view_proj);
        }
        // Solid sections front to back, then translucent ones back to front,
        // as one list of indirect draws over the section arena.
        self.draws.clear();
        self.draw_pages.clear();
        // Occlusion culling leaves the sections its last result did not see
        // to phase 2, after the others, with instance counts of 0. Whether
        // the frame has draws enough goes by the last one's.
        // A shader pack drawing terrain draws all of it: occlusion culling reads the depth of
        // Horizon's own terrain draw.
        let mut cull = self
            .cull
            .as_mut()
            .filter(|c| {
                self.cull_on
                    && !frame.title
                    && !pack_terrain
                    && self.multi_draw
                    // Occlusion is found from the camera, not the mirror's eye.
                    && mirror.is_none()
                    && !warp_on
                    // It tests the CPU's lists, which leave out GPU culling's sections.
                    && !gpu_frame
                    && visible.len() >= c.min_sections
            })
            .and_then(|cull| cull.begin(&self.device).then_some(cull));
        self.phase2_draws.clear();
        self.phase2_pages.clear();
        self.solid_runs.clear();
        self.phase2_runs.clear();
        // On Metal, phase 1 goes the opaque quads of every list, then the cutout
        // ones (`Part`), each solid pass drawing its own: `[..opaque_draws]`,
        // then the rest; phase 2 likewise. A fading list's quads all draw in the
        // cutout pass.
        let split_solid = self.info.backend == wgpu::Backend::Metal && !warp_on;
        let parts = |fading: bool| -> &'static [(u8, Quads)] {
            match (split_solid, fading) {
                (false, _) => &[(0, Quads::All)],
                (true, false) => &[(0, Quads::Opaque), (1, Quads::Cutout)],
                (true, true) => &[(1, Quads::All)],
            }
        };
        let (mut opaque_draws, mut phase2_opaque) = (0, 0);
        debug_assert!(
            !(split_solid && gpu_frame),
            "GPU culling is never offered on Metal"
        );
        // In a GPU frame the GPU draws the solid quads of the whole sections within the
        // render distance that have records (`SectionGpu::gpu_draws_solid`), and the walk
        // listed those among them with translucent quads. Here stay those fading in or out
        // (`gpu_fading`), those without a record and those past the distance (far terrain's
        // cutout, and columns leaving).
        let from = resident::camera_column(camera);
        // Listed sections whose solid quads the pass draws: it counts them among the
        // sections it kept.
        let mut gpu_solid = 0;
        for (i, v) in visible.iter().enumerate() {
            let s = self.sections.get(v.slot);
            if gpu_frame
                && s.gpu_draws_solid(crate::streaming::in_view_distance(
                    from, radius, v.key.0, v.key.2,
                ))
            {
                gpu_solid += 1;
                continue;
            }
            let first_phase = cull.as_ref().is_none_or(|c| c.first(v.slot));
            let (draws, pages) = if first_phase {
                (&mut self.draws, &mut self.draw_pages)
            } else {
                (&mut self.phase2_draws, &mut self.phase2_pages)
            };
            let instances = u32::from(first_phase);
            let mut indices = 0;
            let parts = parts(fade_code(s, &v.key) != 0);
            // Each part's (group, first draw, draws).
            let mut runs = [(0, 0, 0); 2];
            for (run, &(group, part)) in runs.iter_mut().zip(parts) {
                let start = draws.len();
                facing_runs_from(&s.sides, eye, mirror_low, v.origin, part, |first, quads| {
                    indices += quads * 6;
                    let base = s.solid_base() + first * 4;
                    draws.push([quads * 6, instances, 0, base, i as u32]);
                    pages.push((s.page, None));
                });
                *run = (group, start, draws.len() - start);
            }
            let runs = &runs[..parts.len()];
            if first_phase {
                for &(group, start, len) in runs {
                    if group == 0 {
                        opaque_draws += len;
                    }
                    self.solid_runs
                        .push_in(group, v.distance, s.page, start, len, [0; 3]);
                }
                if let Some(c) = &mut cull {
                    c.drawn(
                        i,
                        v.slot,
                        indices,
                        cull::section_box(self.sections.bounds(v.slot)),
                    );
                }
            } else if let Some(c) = &mut cull {
                // One test sees the section's draws in both passes.
                let total: usize = runs.iter().map(|r| r.2).sum();
                if let Some(test) = c.deferred(
                    i,
                    v.slot,
                    total,
                    indices,
                    cull::section_box(self.sections.bounds(v.slot)),
                ) {
                    for &(group, start, len) in runs {
                        if group == 0 {
                            phase2_opaque += len;
                        }
                        let tag = [test, u32::from(group), 0];
                        self.phase2_runs
                            .push_in(group, v.distance, s.page, start, len, tag);
                    }
                }
            }
        }
        // Distant terrain tiles are occlusion tested as sections are, with their meshes'
        // bounds, but for those fading or with a stand-in (`lod::Tile::cull_bounds`).
        for (i, v) in visible_lods.iter().enumerate() {
            let tile = &self.lods[&v.key];
            let s = tile.drawn();
            let index = visible.len() + i;
            let tested = cull.as_ref().and(tile.cull_bounds(v.fade));
            let slot = cull::TILE | tile.cull_slot;
            let first_phase = tested.is_none() || cull.as_ref().is_some_and(|c| c.first(slot));
            let (draws, pages) = if first_phase {
                (&mut self.draws, &mut self.draw_pages)
            } else {
                (&mut self.phase2_draws, &mut self.phase2_pages)
            };
            let instances = u32::from(first_phase);
            let mut indices = 0;
            let parts = parts(v.fade != 0);
            let mut runs = [(0, 0, 0); 2];
            for (run, &(group, part)) in runs.iter_mut().zip(parts) {
                let start = draws.len();
                facing_runs_from(&s.sides, eye, mirror_low, v.origin, part, |first, quads| {
                    indices += quads * 6;
                    let base = s.solid_base() + first * 4;
                    draws.push([quads * 6, instances, 0, base, index as u32]);
                    pages.push((s.page, None));
                });
                *run = (group, start, draws.len() - start);
            }
            let runs = &runs[..parts.len()];
            let (Some(bounds), Some(c)) = (tested, &mut cull) else {
                for &(group, start, len) in runs {
                    if group == 0 {
                        opaque_draws += len;
                    }
                    self.solid_runs
                        .push_in(group, v.distance, s.page, start, len, [0; 3]);
                }
                continue;
            };
            if first_phase {
                for &(group, start, len) in runs {
                    if group == 0 {
                        opaque_draws += len;
                    }
                    self.solid_runs
                        .push_in(group, v.distance, s.page, start, len, [0; 3]);
                }
                c.drawn(index, slot, indices, bounds);
            } else {
                let total: usize = runs.iter().map(|r| r.2).sum();
                if let Some(test) = c.deferred(index, slot, total, indices, bounds) {
                    for &(group, start, len) in runs {
                        if group == 0 {
                            phase2_opaque += len;
                        }
                        let tag = [test, u32::from(group), 0];
                        self.phase2_runs
                            .push_in(group, v.distance, s.page, start, len, tag);
                    }
                }
            }
        }
        // Phase 2's draws are tested where grouping left them.
        let runs = self
            .phase2_runs
            .regroup(&mut self.phase2_draws, &mut self.phase2_pages, 0);
        if let Some(c) = &mut cull {
            for r in runs {
                c.place(r.tag[0], r.tag[1], r.start, r.len);
            }
        }
        self.solid_runs
            .regroup(&mut self.draws, &mut self.draw_pages, 0);
        let phase1_draws = self.draws.len() as u32;
        // Draws `[..opaque_end]` hold opaque quads, `[cutout_start..phase1_draws]` cutout ones.
        let (opaque_end, cutout_start) = if split_solid {
            (opaque_draws, opaque_draws)
        } else {
            (phase1_draws as usize, 0)
        };
        self.draws.extend_from_slice(&self.phase2_draws);
        self.draw_pages.extend_from_slice(&self.phase2_pages);
        let culling = cull.is_some();
        let solid_draws = self.draws.len() as u32;
        // Phase 2's draws `[phase1_draws..phase2_cutout]` hold opaque quads, the rest
        // cutout ones (without the split, all of them go through one pipeline).
        let phase2_cutout = phase1_draws + phase2_opaque as u32;
        laps.lap("r_section_groups");
        // LevelRenderer: nearby sections each frame, plus a rotating eighth
        // (at least 15) of the visible list. Cached views avoid needless sorts.
        self.translucent_draws.clear();
        let count = visible.len() + visible_lods.len();
        let budget = (count / 8).max(15).min(count);
        self.translucency_cursor %= count.max(1);
        for (i, v) in visible.iter().enumerate().rev() {
            let s = self.sections.get_mut(v.slot);
            if let Some((index_page, index_start)) = s.sorted_indices {
                let nearby = (v.key.0 - camera_section.0).abs() <= 2
                    && (v.key.1 - camera_section.1).abs() <= 2
                    && (v.key.2 - camera_section.2).abs() <= 2;
                let scheduled = (i + count - self.translucency_cursor) % count < budget;
                let origin = [
                    v.key.0 * 16,
                    self.layout.min_y() + v.key.1 * 16,
                    v.key.2 * 16,
                ];
                if (!s.sort.is_sorted() || nearby || scheduled)
                    && let Some(indices) =
                        s.sort
                            .update(camera.to_array(), origin, &mut self.translucent_sorter)
                {
                    self.uploads.buffer(
                        &mut encoder,
                        self.sorted_indices.buffer(index_page),
                        index_start * 4,
                        bytemuck::cast_slice(indices),
                    );
                }
                let (page, first) = s.translucent_at;
                self.translucent_draws.push((
                    v.distance,
                    i,
                    [
                        s.translucent * 6,
                        1,
                        index_start as u32,
                        first as u32,
                        i as u32,
                    ],
                    (page, Some(index_page)),
                ));
            }
        }
        // Sections went back to front (`visible` is sorted by distance, then
        // key): tiles' water only needs sorting in when there is some.
        let section_translucent = self.translucent_draws.len();
        for (i, v) in visible_lods.iter().enumerate() {
            let instance = visible.len() + i;
            let tile = self.lods.get_mut(&v.key).expect("visible resident tile");
            let s = tile.drawn_mut();
            if let Some((index_page, index_start)) = s.sorted_indices {
                let scheduled = (instance + count - self.translucency_cursor) % count < budget;
                if (!s.sort.is_sorted() || scheduled)
                    && let Some(indices) = s.sort.update_lod(
                        camera.to_array(),
                        v.key.origin(),
                        &mut self.translucent_sorter,
                    )
                {
                    self.uploads.buffer(
                        &mut encoder,
                        self.sorted_indices.buffer(index_page),
                        index_start * 4,
                        bytemuck::cast_slice(indices),
                    );
                }
                let (page, first) = s.translucent_at;
                self.translucent_draws.push((
                    v.distance,
                    instance,
                    [
                        s.translucent * 6,
                        1,
                        index_start as u32,
                        first as u32,
                        instance as u32,
                    ],
                    (page, Some(index_page)),
                ));
            }
        }
        // Near water and distant water share one back-to-front draw order.
        if self.translucent_draws.len() > section_translucent {
            // Distant water goes after the near chunks', and replaced tiles'
            // after all: each is a hair behind the one before, so depth leaves
            // it only where that one has none. Back to front together, a far
            // tile's water under a near chunk's (one fading in, or brought
            // into view by a longer render distance) blended twice and looked
            // opaque. The cost: far water just behind near water shows no
            // surface of its own there.
            let tier = |instance: usize| match instance.checked_sub(visible.len()) {
                None => 0,
                Some(tile) if visible_lods[tile].old => 2,
                Some(_) => 1,
            };
            self.translucent_draws
                .sort_unstable_by_key(|&(distance, instance, ..)| {
                    (
                        tier(instance),
                        std::cmp::Reverse(distance),
                        std::cmp::Reverse(instance),
                    )
                });
        }
        for &(_, _, draw, pages) in &self.translucent_draws {
            self.draws.push(draw);
            self.draw_pages.push(pages);
        }
        self.translucency_cursor += budget;
        let translucent_draws = self.draws.len() as u32 - solid_draws;
        let section_draws = self.draws.len();
        // The shadow camera's sections after the camera's: quads that can face the light (all of
        // them for `every_face`), then translucent ones (in their last sorted order; depth
        // decides there).
        let shadow_first = self.draws.len() as u32;
        let mut shadow_translucent = std::mem::take(&mut self.shadow_translucent_scratch);
        shadow_translucent.clear();
        self.shadow_runs.clear();
        if let Some(view) = &shadow_view {
            let base = listed;
            // How far below the nearest section to the light (squared, as bits, as `PageRuns`
            // takes distances): each band's draws go by page, a call per page and band.
            let height = |v: &resident::Visible| glam::Vec3::from(v.origin).dot(view.light);
            let top = shadow_visible.first().map_or(0.0, height);
            for (k, v) in shadow_visible.iter().enumerate() {
                let s = self.sections.get_mut(v.slot);
                let instance = (base + k) as u32;
                let depth = f64::from(top - height(v)).max(0.0);
                let distance = (depth * depth).to_bits();
                // A section the camera never saw has no sorted indices yet: sorted once now.
                if let Some((index_page, index_start)) = s.sorted_indices
                    && !s.sort.is_sorted()
                {
                    let origin = [
                        v.key.0 * 16,
                        self.layout.min_y() + v.key.1 * 16,
                        v.key.2 * 16,
                    ];
                    if let Some(indices) =
                        s.sort
                            .update(camera.to_array(), origin, &mut self.translucent_sorter)
                    {
                        self.uploads.buffer(
                            &mut encoder,
                            self.sorted_indices.buffer(index_page),
                            index_start * 4,
                            bytemuck::cast_slice(indices),
                        );
                    }
                }
                let before = self.draws.len();
                shadow_runs(&s.sides, view, v.origin, |first, quads| {
                    let start = s.solid_base() + first * 4;
                    self.draws.push([quads * 6, 1, 0, start, instance]);
                    self.draw_pages.push((s.page, None));
                });
                let len = self.draws.len() - before;
                self.shadow_runs.push(distance, s.page, before, len, [0; 3]);
                if let Some((index_page, index_start)) = s.sorted_indices {
                    let (page, start) = s.translucent_at;
                    shadow_translucent.push((
                        distance,
                        [
                            s.translucent * 6,
                            1,
                            index_start as u32,
                            start as u32,
                            instance,
                        ],
                        (page, Some(index_page)),
                    ));
                }
            }
        }
        let shadow_solid = self.draws.len() as u32 - shadow_first;
        self.shadow_runs
            .regroup(&mut self.draws, &mut self.draw_pages, shadow_first as usize);
        self.shadow_runs.clear();
        for (distance, draw, pages) in shadow_translucent.drain(..) {
            self.shadow_runs
                .push(distance, pages.0, self.draws.len(), 1, [0; 3]);
            self.draws.push(draw);
            self.draw_pages.push(pages);
        }
        self.shadow_translucent_scratch = shadow_translucent;
        self.shadow_runs.regroup(
            &mut self.draws,
            &mut self.draw_pages,
            (shadow_first + shadow_solid) as usize,
        );
        let shadow_translucent = self.draws.len() as u32 - shadow_first - shadow_solid;
        laps.lap("r_nearby");
        // Metal replays a multi-draw as one draw call per entry.
        let mut section_draw_calls = if self.multi_draw && self.info.backend != wgpu::Backend::Metal
        {
            (0..section_draws)
                .filter(|&i| {
                    i == 0
                        || i == phase1_draws as usize
                        || i == solid_draws as usize
                        || self.draw_pages[i] != self.draw_pages[i - 1]
                })
                .count()
        } else {
            section_draws
        };
        // GPU culling's draw calls, one per page; its draws come back with its statistics
        // (`GpuCulling::copy_stats`).
        if gpu_frame && let Some(gpu) = &self.gpu_culling {
            section_draw_calls += gpu.draw_calls();
        }
        let debug_depth = frame.debug_lines.len() as u32;
        let debug_opaque = frame
            .debug_lines
            .iter()
            .filter(|l| debug_line_opaque(l))
            .count() as u32;
        let debug_top = frame.debug_lines_on_top.len() as u32;
        let crosshair = frame.debug_crosshair.len() as u32;
        if debug_depth + debug_top + crosshair > 0 {
            let needed = u64::from(debug_depth + debug_top + crosshair);
            if needed > self.debug_line_capacity {
                self.debug_line_capacity = needed.next_power_of_two();
                self.debug_line_buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("Debug lines"),
                    size: 32 * self.debug_line_capacity,
                    usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                });
            }
            let lines = &mut self.debug_lines_scratch;
            lines.clear();
            lines.extend(
                debug_lines_opaque_first(frame.debug_lines)
                    .chain(frame.debug_lines_on_top)
                    .chain(frame.debug_crosshair)
                    .copied(),
            );
            self.uploads.buffer(
                &mut encoder,
                &self.debug_line_buffer,
                0,
                bytemuck::cast_slice(lines),
            );
        }
        self.weather.prepare(
            &self.device,
            &mut encoder,
            &mut self.uploads,
            frame.weather,
            frame.weather_time,
        );
        // Native mods' world primitives, drawn as the world is: its jittered projection, the
        // line widths in the frame's pixels as the gizmo lines'.
        let world_primitives = !frame.title && {
            let (uploads, encoder) = (&mut self.uploads, &mut encoder);
            self.world_primitives.prepare(
                &self.device,
                &mut |buffer, bytes| uploads.buffer(encoder, buffer, 0, bytes),
                &frame.world_primitives,
                jittered,
                [self.config.width, self.config.height],
                &self.entity_atlas,
            )
        };
        let outline_edges = frame.outline.len().min(MAX_OUTLINE_EDGES as usize);
        if outline_edges > 0 {
            // The same per-line colour and pixel width layout as the gizmo lines. A high
            // contrast outline is two strokes: the black one first, the bright one after it.
            outline_vertices_into(
                &frame.outline[..outline_edges],
                camera,
                &mut self.outline_edges_scratch,
            );
            let width = outline_width(self.config.width);
            let edges = &self.outline_edges_scratch;
            let lines = &mut self.outline_lines_scratch;
            lines.clear();
            let stroke = |color: u32, width: f32, lines: &mut Vec<[f32; 8]>| {
                for e in edges {
                    let mut line = [0.0; 8];
                    line[..6].copy_from_slice(e);
                    line[6] = f32::from_bits(color);
                    line[7] = width;
                    lines.push(line);
                }
            };
            // A custom outline (PvP & HUD) takes the bright stroke's colour, scales both widths.
            let (custom, scale) = frame.pvp.outline.map_or((None, 1.0), |(c, s)| (Some(c), s));
            if frame.outline_high_contrast {
                stroke(0xff00_0000, 7.0 * scale, lines);
                stroke(custom.unwrap_or(0xff57_ffe1), width * scale, lines);
            } else {
                stroke(custom.unwrap_or(0x6600_0000), width * scale, lines);
            }
            self.uploads
                .buffer(&mut encoder, &self.outline, 0, bytemuck::cast_slice(lines));
            if let Some(core) = &self.core_lines {
                let size = [self.config.width, self.config.height];
                let block = crate::post_chain::Globals {
                    glint_alpha: frame.glint_alpha,
                    ..frame.post_effects.globals
                };
                let [sky_end, clouds_end, ..] = globals.fog_ends;
                let data = core_lines::uniforms(
                    block.bytes(size, frame.post_effects.menu_blur_radius as i32),
                    views.primary(),
                    pose,
                    jitter,
                    globals.outline_params[3],
                    fog_color,
                    [fog[0], fog[1], fog[2], fog[3], sky_end, clouds_end],
                );
                self.uploads.buffer(&mut encoder, core.uniform(), 0, &data);
            }
        }
        // A pack's core/sky and core/stars: the disc with the sky colour, the stars with their
        // brightness, both turned with the camera only (SkyRenderer's model view).
        if let Some(core) = &self.core_sky
            && !frame.title
        {
            let size = [self.config.width, self.config.height];
            let block = crate::post_chain::Globals {
                glint_alpha: frame.glint_alpha,
                ..frame.post_effects.globals
            }
            .bytes(size, frame.post_effects.menu_blur_radius as i32);
            let [sky_end, clouds_end, ..] = globals.fog_ends;
            let fog = [fog[0], fog[1], fog[2], fog[3], sky_end, clouds_end];
            let view = views.primary();
            // Far enough for the disc's rim, 512 out: the sky draws without a depth test.
            let fov = (2.0 * view.tan_half_fov.atan()).to_degrees();
            let projection = jitter * world_projection(fov, view.aspect, 2048.0) * pose;
            let [r, g, b, ..] = frame.sky.sky_color;
            let sky =
                core_sky::uniforms(block, projection, view.view, [r, g, b, 1.0], fog_color, fog);
            let stars = frame.stars.map(|(rotation, brightness)| {
                let b = brightness.clamp(0.0, 1.0);
                core_sky::uniforms(
                    block,
                    projection,
                    view.view * rotation,
                    [b; 4],
                    fog_color,
                    fog,
                )
            });
            core.prepare(&mut self.uploads, &mut encoder, &sky, stars.as_deref());
        }
        let skin = frame.skin.filter(|skin| match &mut self.entity_skin {
            Some(skinner) => skinner.register(&self.device, &mut self.uploads, &mut encoder, skin),
            None => false,
        });
        let pack_skin = !frame.title
            && skin.is_some()
            && self
                .pack_post
                .as_ref()
                .and_then(|(_, post)| post.as_ref())
                .is_some_and(|post| post.skins_entities());
        // While a shader pack draws entities, the runs of `entity_spans` its programs draw are
        // left out of Horizon's own entity draw.
        let mut entities = frame.entities;
        if !frame.title
            && let Some((_, Some(post))) = &mut self.pack_post
            && post.has_entities()
        {
            post.prepare_entities(
                &self.device,
                &self.queue,
                frame.entities,
                frame.entity_spans,
                frame.crumbling_sprite,
                frame.end_portal_sprites[1],
                &mut self.pack_entity_rest,
                skin.filter(|_| pack_skin),
                (frame.entity_motion, frame.entity_motion_posed),
            );
            entities = &self.pack_entity_rest;
        }
        // GameRenderer.useImprovedTransparency. A shader pack keeps its own
        // path.
        let oit_on = self.improved_transparency
            && !frame.title
            && !warp_on
            && self.pack_post.as_ref().is_none_or(|p| p.1.is_none())
            && self.native.is_none()
            && oit::supported(&self.device);
        let [
            entity_quads,
            text_quads,
            crumbling_quads,
            translucent_crumbling_quads,
            water_mask_quads,
            swirl_quads,
            shadow_quads,
            translucent_particle_quads,
            translucent_item_quads,
            oit_entity_quads,
            fluid_quads,
            world_text_quads,
            world_text_offset_quads,
            native_solid_quads,
            native_cutout_quads,
            native_cutout_mipped_quads,
        ] = self.entity_batch.update(
            entities,
            frame.additive,
            frame.hand,
            frame.see_through,
            oit_on,
        );
        self.entity_batch.append_shadow_only(frame.shadow_only);
        // The posed models' vertices come first: the ordinary entities' draw takes them in.
        let posed = if pack_skin {
            0
        } else {
            skin.map_or(0, |s| s.vertices)
        };
        let entity_quads = entity_quads + posed / 4;
        let weather_quads = frame.weather.len() as u32 / 4;
        let world_quads = entity_quads
            + text_quads
            + crumbling_quads
            + translucent_crumbling_quads
            + water_mask_quads
            + swirl_quads
            + shadow_quads
            + translucent_particle_quads
            + translucent_item_quads
            + oit_entity_quads
            + fluid_quads
            + world_text_quads
            + world_text_offset_quads
            + native_solid_quads
            + native_cutout_quads
            + native_cutout_mipped_quads;
        laps.lap("r_draw_lists");
        // Where the tail of materials the classic and the improved passes
        // both draw ends: the shadows, particles and items count back from it.
        let base_quads = world_quads
            - oit_entity_quads
            - fluid_quads
            - world_text_quads
            - world_text_offset_quads
            - native_solid_quads
            - native_cutout_quads
            - native_cutout_mipped_quads;
        let native_clouds = self.native.as_ref().is_none_or(|n| n.vanilla_clouds);
        let cloud_quads = if let Some(input) = frame.clouds.as_ref().filter(|_| native_clouds) {
            let (rebuilt, uniform) =
                crate::spans::time("clouds", || self.cloud_mesh.prepare(input));
            let vertices = self.cloud_mesh.vertices.len() as u64;
            if rebuilt && vertices > 0 {
                if vertices > self.cloud_capacity {
                    self.cloud_capacity = vertices.next_power_of_two();
                    self.cloud_buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
                        label: Some("Cloud vertices"),
                        size: self.cloud_capacity * std::mem::size_of::<Vertex>() as u64,
                        usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                        mapped_at_creation: false,
                    });
                }
                self.uploads.buffer(
                    &mut encoder,
                    &self.cloud_buffer,
                    0,
                    bytemuck::cast_slice(&self.cloud_mesh.vertices),
                );
            }
            self.uploads.buffer(
                &mut encoder,
                &self.cloud_uniform,
                0,
                bytemuck::bytes_of(&uniform),
            );
            (vertices / 4) as u32
        } else {
            0
        };
        self.lightning.prepare(
            &self.device,
            &mut self.uploads,
            &mut encoder,
            &frame.lightning,
            camera,
        );
        // Frame generation's made frame draws its own hand, halfway, after the others.
        let made_hand_vertex = (posed as usize + self.entity_batch.combined.len()) as i32;
        if fg {
            self.frame_gen
                .halfway_hand(frame.hand, hand_view, &mut self.entity_batch.combined);
            self.frame_gen.track_entities(
                &self.device,
                &self.entity_atlas,
                frame.entities,
                frame.entity_motion,
                frame.camera,
            );
        }
        let all_entities = &self.entity_batch.combined;
        let hand_quads = (frame.hand.len() / 4) as u32;
        let additive_quads = (frame.additive.len() / 4) as u32 - self.entity_batch.moved_additive;
        let see_through_quads = (frame.see_through.len() / 4) as u32;
        let shadow_only_quads = (frame.shadow_only.len() / 4) as u32;
        // Glowing entities' silhouettes posed on the GPU go last, past every
        // material (the outline pass draws them from there).
        let outline_base = posed + all_entities.len() as u32;
        let outlined = skin.map_or(0, |s| s.outline_vertices);
        let entity_vertices = u64::from(outline_base) + u64::from(outlined);
        if entity_vertices > self.entity_capacity {
            self.entity_capacity = entity_vertices.next_power_of_two();
            self.entity_buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("Entity vertices"),
                size: self.entity_capacity * std::mem::size_of::<Vertex>() as u64,
                usage: entity_skin::EntitySkin::usage(self.entity_skin.is_some()),
                mapped_at_creation: false,
            });
        }
        if !all_entities.is_empty() {
            self.uploads.buffer(
                &mut encoder,
                &self.entity_buffer,
                u64::from(posed) * std::mem::size_of::<Vertex>() as u64,
                bytemuck::cast_slice(all_entities),
            );
        }
        if let (Some(skinner), Some(skin)) = (&mut self.entity_skin, skin) {
            if pack_skin {
                let target = self
                    .pack_post
                    .as_ref()
                    .and_then(|(_, post)| post.as_ref())
                    .and_then(|post| post.skin_target())
                    .expect("GPU pack entity target");
                skinner.dispatch_pack(
                    &self.device,
                    &mut self.uploads,
                    &mut encoder,
                    skin,
                    target,
                    &self.entity_buffer,
                    outline_base,
                );
            } else {
                skinner.dispatch(
                    &self.device,
                    &mut self.uploads,
                    &mut encoder,
                    skin,
                    &self.entity_buffer,
                    0,
                    outline_base,
                );
            }
        }
        let static_sections = frame.static_block_entities;
        let mut static_bytes_uploaded = 0;
        let catalog_revision = frame.static_block_entity_stats.snapshot_revision;
        if catalog_revision != self.static_entity_catalog_revision {
            self.static_entity_sections.retain(|section, _| {
                static_sections
                    .binary_search_by_key(section, |snapshot| snapshot.section)
                    .is_ok()
            });
            for snapshot in static_sections {
                let vertices = snapshot.vertices.len() as u64;
                let gpu = self
                    .static_entity_sections
                    .entry(snapshot.section)
                    .or_insert_with(|| {
                        let capacity = vertices.next_power_of_two().max(4);
                        let origin_buffer =
                            self.device
                                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                                    label: Some("Static block-entity section origin"),
                                    contents: bytemuck::bytes_of(&snapshot.origin),
                                    usage: wgpu::BufferUsages::VERTEX,
                                });
                        static_bytes_uploaded += std::mem::size_of_val(&snapshot.origin) as u64;
                        GpuStaticEntitySection {
                            buffer: self.device.create_buffer(&wgpu::BufferDescriptor {
                                label: Some("Static block-entity section vertices"),
                                size: capacity * std::mem::size_of::<Vertex>() as u64,
                                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                                mapped_at_creation: false,
                            }),
                            origin_buffer,
                            origin: snapshot.origin,
                            capacity,
                            revision: 0,
                            vertices: 0,
                        }
                    });
                if gpu.origin != snapshot.origin {
                    gpu.origin_buffer =
                        self.device
                            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                                label: Some("Static block-entity section origin"),
                                contents: bytemuck::bytes_of(&snapshot.origin),
                                usage: wgpu::BufferUsages::VERTEX,
                            });
                    gpu.origin = snapshot.origin;
                    static_bytes_uploaded += std::mem::size_of_val(&snapshot.origin) as u64;
                }
                if gpu.revision == snapshot.revision {
                    continue;
                }
                if vertices > gpu.capacity {
                    gpu.capacity = vertices.next_power_of_two();
                    gpu.buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
                        label: Some("Static block-entity section vertices"),
                        size: gpu.capacity * std::mem::size_of::<Vertex>() as u64,
                        usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                        mapped_at_creation: false,
                    });
                }
                self.uploads.buffer(
                    &mut encoder,
                    &gpu.buffer,
                    0,
                    bytemuck::cast_slice(&snapshot.vertices),
                );
                static_bytes_uploaded += vertices * std::mem::size_of::<Vertex>() as u64;
                gpu.revision = snapshot.revision;
                gpu.vertices = vertices as u32;
            }
            self.static_entity_catalog_revision = catalog_revision;
        }
        let static_entity_quads = frame
            .visible_static_block_entity_sections
            .iter()
            .filter_map(|section| self.static_entity_sections.get(section))
            .map(|section| section.vertices / 4)
            .sum::<u32>();
        let max_static_section_quads = frame
            .visible_static_block_entity_sections
            .iter()
            .filter_map(|section| self.static_entity_sections.get(section))
            .map(|section| section.vertices / 4)
            .max()
            .unwrap_or(0);
        if max_static_section_quads > self.static_entity_index_quads {
            self.static_entity_index_quads = max_static_section_quads.next_power_of_two();
            self.static_entity_indices =
                Self::quad_indices(&self.device, self.static_entity_index_quads);
        }
        if !frame.visible_static_block_entity_sections.is_empty()
            && self.static_entity_camera != Some(frame.camera)
        {
            let camera_floor = [
                frame.camera.x.floor() as i32,
                frame.camera.y.floor() as i32,
                frame.camera.z.floor() as i32,
            ];
            let transform = StaticEntityUniform {
                camera_block: [camera_floor[0], camera_floor[1], camera_floor[2], 0],
                camera_fraction: [
                    (frame.camera.x - f64::from(camera_floor[0])) as f32,
                    (frame.camera.y - f64::from(camera_floor[1])) as f32,
                    (frame.camera.z - f64::from(camera_floor[2])) as f32,
                    0.0,
                ],
            };
            self.uploads.buffer(
                &mut encoder,
                &self.static_entity_transform,
                0,
                bytemuck::bytes_of(&transform),
            );
            static_bytes_uploaded += std::mem::size_of::<StaticEntityUniform>() as u64;
            self.static_entity_camera = Some(frame.camera);
        }
        self.static_entity_frame_stats = frame.static_block_entity_stats;
        self.static_entity_frame_stats.static_bytes_uploaded = static_bytes_uploaded;
        self.entity_outline.prepare(
            &self.device,
            &mut self.uploads,
            &mut encoder,
            frame.glowing,
            (outline_base, outlined / 4),
            [self.config.width, self.config.height],
        );
        if let Some((rotation, brightness)) = frame.stars {
            self.stars.prepare(
                &mut self.uploads,
                &mut encoder,
                jittered * rotation,
                brightness,
                &globals,
            );
        }
        self.custom_sky.prepare(
            &mut self.uploads,
            &mut encoder,
            jittered,
            &self.custom_sky_draws,
            &globals,
        );
        let celestial_vertices = frame.celestial.len().min(2048 * 4);
        if frame.end_sky && !frame.title {
            self.end_sky.prepare(&self.device, frame.end_sky_sprite);
        }
        if celestial_vertices > 0 {
            self.uploads.buffer(
                &mut encoder,
                &self.celestial_buffer,
                0,
                bytemuck::cast_slice(&frame.celestial[..celestial_vertices]),
            );
        }
        let max_quads =
            (world_quads + additive_quads + hand_quads + see_through_quads + shadow_only_quads)
                .max(cloud_quads)
                .max(if frame.lightning.bolts.is_empty() {
                    0
                } else {
                    lightning::QUADS
                })
                .max(weather_quads)
                .max(frame.glowing.len() as u32 / 4)
                .max(outlined / 4);
        if max_quads > self.index_quads {
            self.index_quads = max_quads.next_power_of_two();
            self.indices = Self::quad_indices(&self.device, self.index_quads);
        }
        if let Some(b) = &frame.border {
            if b.rebuilt {
                self.uploads.buffer(
                    &mut encoder,
                    &self.border_vertices,
                    0,
                    bytemuck::cast_slice(&b.vertices),
                );
            }
            let t = &b.transforms;
            let uniform = BorderUniform {
                color: t.color,
                offset: [
                    t.model_offset[0],
                    t.model_offset[1],
                    t.model_offset[2],
                    t.texture_offset,
                ],
            };
            self.uploads.buffer(
                &mut encoder,
                &self.border_uniform,
                0,
                bytemuck::bytes_of(&uniform),
            );
        }
        laps.lap("r_materials");
        // Every upload before the frame's first pass: a pass ends wgpu-core's
        // command buffer, so copies between passes would need one more.
        if !frame.screen.is_empty() {
            let vertices = &mut self.screen_vertices_scratch;
            vertices.clear();
            vertices.extend(frame.screen.iter().flat_map(|q| q.vertices));
            if vertices.len() as u64 > self.screen_capacity {
                self.screen_capacity = (vertices.len() as u64).next_power_of_two();
                self.screen_buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("Screen effect vertices"),
                    size: self.screen_capacity
                        * std::mem::size_of::<crate::screen_effects::ScreenVertex>() as u64,
                    usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                });
            }
            self.uploads.buffer(
                &mut encoder,
                &self.screen_buffer,
                0,
                bytemuck::cast_slice(vertices),
            );
        }
        self.hud.prepare(
            &self.device,
            &mut self.uploads,
            &mut encoder,
            frame.hud,
            [self.config.width, self.config.height],
        );
        if culling && let Some(cull) = &mut self.cull {
            cull.upload(
                &self.device,
                &mut self.uploads,
                &mut encoder,
                &self.draws[phase1_draws as usize..solid_draws as usize],
                &self.instances,
                &self.globals,
            );
        }
        let flat_frame = match &mut self.flat {
            Some(flat)
                if self.flat_on
                    // They copy the CPU's lists alone (GPU culling is never offered
                    // where vanilla frames draw flat).
                    && !gpu_frame
                    && !warp_on
                    && !self.draws.is_empty()
                    && (listed >= flat.min_sections
                        || pack_terrain
                        || shadow_view.is_some()) =>
            {
                // The visibility buffer's phase 2 IDs need each call's page (`visbuffer`).
                flat.page_instances = self.visbuf.is_some()
                    && !pack_terrain
                    && shadow_view.is_none()
                    && self.native.as_ref().is_none_or(|n| n.material.is_none());
                let sections = &self.sections;
                let lods = &self.lods;
                let sorted = &self.sorted_indices;
                let phase2 = self.cull.as_ref().filter(|_| culling).map(|c| &c.phase2);
                crate::spans::time("flatten", || {
                    flat.prepare(
                        &self.device,
                        &mut encoder,
                        &mut self.uploads,
                        &self.draws,
                        &self.draw_pages,
                        phase1_draws as usize,
                        solid_draws as usize,
                        [shadow_first, shadow_first + shadow_solid].map(|d| d as usize),
                        [opaque_end, cutout_start, phase2_cutout as usize],
                        phase2,
                        self.translucent_arena.is_some(),
                        visible
                            .iter()
                            .chain(&shadow_visible)
                            .filter_map(|v| {
                                let section = sections.get(v.slot);
                                let code = fade_code(section, &v.key);
                                Some((
                                    section.flat_id?,
                                    section_instance(v.origin, code, visibility(v.key)),
                                ))
                            })
                            .chain(visible_lods.iter().filter_map(|v| {
                                let id = lods[&v.key].drawn().flat_id?;
                                Some((id, lod::instance(v.origin, v.fade)))
                            })),
                        |page| sorted.buffer(page).clone(),
                        self.own_timing.as_mut(),
                    )
                })
            }
            _ => false,
        };
        // Each page's group binds the page for the solid draws, flat or not.
        if let Some(flat) = &mut self.flat {
            let arena = &self.arena;
            flat.bind_pages(&self.device, |page| {
                arena.has_page(page).then(|| arena.buffer(page))
            });
        }
        if flat_frame {
            section_draw_calls = self.flat.as_ref().map_or(0, |f| f.shadow_calls[0]);
        } else if self.multi_draw && !self.draws.is_empty() {
            if self.draws.len() as u64 > self.indirect_capacity {
                self.indirect_capacity = (self.draws.len() as u64).next_power_of_two();
                self.indirect = self.device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("Section draws"),
                    size: self.indirect_capacity * 20,
                    usage: wgpu::BufferUsages::INDIRECT | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                });
            }
            self.uploads.buffer(
                &mut encoder,
                &self.indirect,
                0,
                bytemuck::cast_slice(&self.draws),
            );
        }
        let view_target = target_texture.create_view(&wgpu::TextureViewDescriptor {
            format: Some(self.output.frame.format),
            ..Default::default()
        });
        if let Some(timer) = &mut self.gpu_timer {
            timer.begin(&mut encoder);
        }
        // The frame's statistics (`show_stats`): a CPU frame's at once; a GPU frame's when the
        // pass's counts come back, with the CPU's part of it: the sections the walk listed but
        // those whose solid quads the pass drew (it counts them), its draws and every call.
        let cpu = [listed - gpu_solid, section_draws, section_draw_calls];
        if gpu_frame && let Some(gpu) = &mut self.gpu_culling {
            gpu.dispatch(&mut encoder, self.own_timing.as_mut());
        } else {
            self.show_stats(serial, cpu);
        }
        // The world's targets: the screen, or the scaled target of `post`.
        let pack_post = !frame.title && self.pack_post.as_ref().is_some_and(|p| p.1.is_some());
        // One of Horizon's own packs reads the same world target.
        let native_post = !frame.title && self.native.as_ref().is_some_and(|n| n.post.is_some());
        if let Some(timer) = Self::pass_timer(&mut self.pack_post, &mut self.native)
            && timer.is_some() != self.pass_timing
        {
            *timer = self
                .pass_timing
                .then(|| pack_timing::PassTiming::new(&self.device, &self.queue))
                .flatten();
        }
        if let Some(Some(t)) = Self::pass_timer(&mut self.pack_post, &mut self.native) {
            t.gap = self.pass_timing_gap;
        }
        let pack_reads_world = pack_post || native_post;
        laps.lap("r_uploads");
        // Phase 2 of occlusion culling draws over the depth, and a shader pack or TAA reads it.
        let keep_depth = pack_reads_world
            || (taa && self.world_post.taa)
            || oit_on
            || native_render::render_needs().world_depth
            || fg;
        // The pack's program reads the vertices this frame's draws bind.
        let pack_draws_terrain = pack_terrain && flat_frame == self.pack_terrain_flat;
        // The shadow camera's lists, drawn by the pack's shadow programs.
        let pack_draws_shadow = shadow_view.is_some() && flat_frame == self.pack_terrain_flat;
        let pack_draws_sky = pack_post
            && self
                .pack_post
                .as_ref()
                .and_then(|p| p.1.as_ref())
                .is_some_and(|p| p.has_sky());
        let pack_draws_celestial = pack_post
            && self
                .pack_post
                .as_ref()
                .and_then(|p| p.1.as_ref())
                .is_some_and(|p| p.has_celestial());
        let pack_draws_water = pack_draws_terrain
            && self
                .pack_post
                .as_ref()
                .and_then(|p| p.1.as_ref())
                .is_some_and(|p| p.has_water());
        let pack_draws_clouds = pack_post
            && self
                .pack_post
                .as_ref()
                .and_then(|p| p.1.as_ref())
                .is_some_and(|p| p.has_clouds());
        let pack_draws_outline = pack_post
            && self
                .pack_post
                .as_ref()
                .and_then(|p| p.1.as_ref())
                .is_some_and(|p| p.has_lines());
        let pack_draws_hand = pack_post
            && self
                .pack_post
                .as_ref()
                .and_then(|p| p.1.as_ref())
                .is_some_and(|p| p.has_hand());
        let pack_draws_weather = pack_post
            && self
                .pack_post
                .as_ref()
                .and_then(|p| p.1.as_ref())
                .is_some_and(|p| p.has_weather());
        let scaled = self.world_post.prepare(
            &self.device,
            [self.config.width, self.config.height],
            pack_reads_world || fg,
        );
        let (world_view, world_depth) = match &scaled {
            Some((color, depth)) => (color, depth),
            None => (&view_target, &self.depth),
        };
        if culling && let Some(cull) = &mut self.cull {
            cull.bind_depth(&self.device, world_depth);
        }
        // The hand draws at the end of the world pass, after a depth reset, when nothing runs
        // between them (a scaled world's post pass, HDR's world copy, a native mod's
        // `native_world_drawn`): one render pass fewer, ~10 µs of wgpu's encoding a frame.
        let hand_in_world = (frame.hand.len() / 4) > 0
            && !pack_draws_hand
            && scaled.is_none()
            && self.hdr.is_none()
            // Frame generation draws the hand over the real and the made frame after the world.
            && fg_frames.is_none()
            && !native_render::world_drawn_hooked();
        if oit_on {
            let size = self
                .world_post
                .size()
                .filter(|_| scaled.is_some())
                .unwrap_or([self.config.width, self.config.height]);
            let (device, layout, sprites) =
                (&self.device, &self.terrain_layout, self.terrain_sprites);
            let clouds = [0, 1].map(|i| self.cloud_pipeline.get_bind_group_layout(i));
            let format = self.output.frame.format;
            let flat = self
                .flat
                .as_ref()
                .map(|f| f.translucent.get_bind_group_layout(1));
            let features = [
                self.entity_pipeline.get_bind_group_layout(0),
                self.weather.pipeline().get_bind_group_layout(1),
                self.border_pipeline.get_bind_group_layout(0),
            ];
            self.oit
                .get_or_insert_with(|| {
                    oit::Oit::new(
                        device,
                        layout,
                        flat.as_ref(),
                        [&features[0], &features[1], &features[2]],
                        [&clouds[0], &clouds[1]],
                        sprites,
                        format,
                    )
                })
                .prepare(
                    &self.device,
                    &self.queue,
                    size,
                    &views.primary().projection,
                    [water_mask_quads > 0, cloud_quads > 0],
                );
        }
        let oit = self.oit.as_ref().filter(|_| oit_on);
        let cull = self.cull.as_ref().filter(|_| culling && solid_draws > 0);
        let pack_sky_clear = pack_draws_sky
            .then(|| self.pack_post.as_ref().and_then(|p| p.1.as_ref()))
            .flatten()
            .and_then(|p| p.sky_clear());
        let clear = if let Some([r, g, b, a]) = pack_sky_clear {
            wgpu::Color {
                r: f64::from(r),
                g: f64::from(g),
                b: f64::from(b),
                a: f64::from(a),
            }
        } else if frame.title || (frame.end_sky && !frame.underwater && sky_open(frame)) {
            wgpu::Color::BLACK
        } else {
            wgpu::Color {
                r: f64::from(fog_color[0]),
                g: f64::from(fog_color[1]),
                b: f64::from(fog_color[2]),
                a: 1.0,
            }
        };
        let clear = linear_blend::stored_clear(clear, self.output.frame.linear_light);
        let sky_visible = sky_open(frame);
        laps.lap("r_world_pass");
        // One of Horizon's own packs: this frame's inputs; in a timed frame its sky draws in a
        // pass of its own, so that it has a time of its own.
        let mut native_sky_pass = false;
        if !frame.title
            && let Some(native) = &mut self.native
        {
            let size = self
                .world_post
                .size()
                .filter(|_| scaled.is_some())
                .unwrap_or([self.config.width, self.config.height]);
            native
                .frame
                .write(&self.queue, frame, size, output.display());
            native.frame.block_light.update(
                &self.queue,
                frame.camera,
                &self.light_blocks,
                self.light_revision,
            );
            if let Some(t) = &mut native.timing {
                t.begin(&self.device, &self.queue);
                native_sky_pass =
                    native.sky.is_some() && t.recording() && sky_visible && frame.skybox;
            }
        }
        // One of Horizon's own packs with `[shadow]`: the solid terrain the light sees, from the
        // shadow camera's lists, into the pack's shadow map (a pack without one gets its 1×1 map
        // cleared to "far" once).
        if !frame.title
            && let Some(native) = &mut self.native
        {
            native.frame.shadow_map.clear_blank(&mut encoder);
        }
        if !frame.title
            && shadow_view.is_some()
            && let Some(native) = &self.native
            && let Some(shadow) = &native.shadow
        {
            let mut pass = native.frame.shadow_map.begin(
                &mut encoder,
                "Native shader pack shadow",
                self.own_timing.as_mut(),
            );
            pass.set_bind_group(0, &self.group, &[]);
            pass.set_bind_group(horizon_shader_api::stages::GROUP, &shadow.group, &[]);
            if flat_frame
                && let Some(flat) = &self.flat
                && let Some(pipeline) = &shadow.flat
            {
                pass.set_pipeline(pipeline);
                let [first, translucent] = flat.shadow_calls;
                flat.draw(
                    &mut pass,
                    &flat.calls[first..translucent],
                    flat_vertices(&self.arena, self.translucent_arena.as_ref()),
                );
            } else if !flat_frame {
                pass.set_pipeline(&shadow.solid);
                pass.set_index_buffer(self.indices.slice(..), wgpu::IndexFormat::Uint32);
                pass.set_vertex_buffer(1, self.instances.slice(..));
                self.draw_sections(&mut pass, &self.indirect, 0, shadow_first, shadow_solid);
            }
            // `[displacement]`'s grid casts in place of the flat quads it took over.
            if let Some((d, picks, pipeline)) = native
                .displacement
                .as_ref()
                .and_then(|d| Some((d, d.shadow.as_ref()?, shadow.displaced.as_ref()?)))
            {
                d.draw(&mut pass, picks, pipeline);
            }
            drop(pass);
            let mut pass = native.frame.shadow_map.begin_translucent(
                &mut encoder,
                "Native shader pack translucent shadow",
                self.own_timing.as_mut(),
            );
            pass.set_bind_group(0, &self.group, &[]);
            pass.set_bind_group(horizon_shader_api::stages::GROUP, &shadow.group, &[]);
            if flat_frame
                && let Some(flat) = &self.flat
                && let Some(pipeline) = &shadow.flat_translucent
            {
                pass.set_pipeline(pipeline);
                let [_, translucent] = flat.shadow_calls;
                flat.draw(
                    &mut pass,
                    &flat.calls[translucent..],
                    flat_vertices(&self.arena, self.translucent_arena.as_ref()),
                );
            } else if !flat_frame {
                pass.set_pipeline(&shadow.translucent);
                pass.set_index_buffer(self.indices.slice(..), wgpu::IndexFormat::Uint32);
                pass.set_vertex_buffer(1, self.instances.slice(..));
                self.draw_sections(
                    &mut pass,
                    &self.indirect,
                    0,
                    shadow_first + shadow_solid,
                    shadow_translucent,
                );
            }
            drop(pass);
            if entity_quads > 0 || shadow_only_quads > 0 {
                let mut pass = native
                    .frame
                    .shadow_map
                    .begin_entities(&mut encoder, "Native shader pack entity shadow");
                pass.set_pipeline(&shadow.entity);
                pass.set_bind_group(0, &self.entity_group, &[]);
                pass.set_bind_group(horizon_shader_api::stages::GROUP, &shadow.group, &[]);
                pass.set_vertex_buffer(0, self.entity_buffer.slice(..));
                pass.set_index_buffer(self.indices.slice(..), wgpu::IndexFormat::Uint32);
                pass.draw_indexed(0..entity_quads * 6, 0, 0..1);
                // At the end of the buffer (`EntityBatch::append_shadow_only`).
                let first = entity_vertices as u32 - shadow_only_quads * 4;
                pass.draw_indexed(0..shadow_only_quads * 6, first as i32, 0..1);
            }
        }
        if native_sky_pass
            && let Some(native) = &mut self.native
            && let Some(sky) = &native.sky
        {
            if let Some(t) = &mut native.timing {
                let format = self.output.frame.format;
                t.mark(&self.device, &mut encoder, (world_view, format));
            }
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some(&sky.label),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: world_view,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(clear),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: world_depth,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(0.0),
                        store: wgpu::StoreOp::Discard,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: native.timing.as_mut().and_then(|t| t.writes(&sky.label)),
                ..Default::default()
            });
            pass.set_bind_group(0, &self.group, &[]);
            sky.draw(&mut pass, &native.frame);
        }
        laps.lap("r_pack_inputs");
        // The water mirror, for one of Horizon's own packs that reads it: the world as a
        // camera mirrored about the water plane sees it, into a half-resolution target the
        // material stage samples as `reflection`.
        let mut reflection = None;
        if let Some(plane) = mirror_plane {
            let world = self
                .world_post
                .size()
                .filter(|_| scaled.is_some())
                .unwrap_or([self.config.width, self.config.height]);
            let size = native_post::reflection_size(world);
            if self.mirror.as_ref().is_none_or(|m| m.size != size) {
                let target = |label: &'static str, format, usage| {
                    self.device
                        .create_texture(&wgpu::TextureDescriptor {
                            label: Some(label),
                            size: wgpu::Extent3d {
                                width: size[0],
                                height: size[1],
                                depth_or_array_layers: 1,
                            },
                            mip_level_count: 1,
                            sample_count: 1,
                            dimension: wgpu::TextureDimension::D2,
                            format,
                            usage,
                            view_formats: &[],
                        })
                        .create_view(&Default::default())
                };
                self.mirror = Some(MirrorTarget {
                    size,
                    color: target(
                        "Water mirror",
                        self.output.frame.format,
                        wgpu::TextureUsages::RENDER_ATTACHMENT
                            | wgpu::TextureUsages::TEXTURE_BINDING,
                    ),
                    depth: target(
                        "Water mirror depth",
                        wgpu::TextureFormat::Depth32Float,
                        wgpu::TextureUsages::RENDER_ATTACHMENT,
                    ),
                });
            }
            if let Some(mirror) = &self.mirror {
                // Mirrored about y = `plane` (camera-relative).
                let plane = plane - frame.camera.y as f32;
                let inverse_view = mirror_inverse_view(globals.inverse_view, plane);
                // fog_ends.w: the plane, below which fs_solid_mirror cuts the world away
                // (only entity.wgsl reads the glint strength there, and no entity draws here).
                let mut fog_ends = globals.fog_ends;
                fog_ends[3] = plane;
                let mirror_globals = Globals {
                    view_proj: mirror_view_projection(view_proj, plane).to_cols_array_2d(),
                    inverse_view,
                    fog_ends,
                    hdr_light: [
                        globals.hdr_light[0],
                        globals.hdr_light[1],
                        globals.hdr_light[2],
                        0.0,
                    ],
                    ..globals
                };
                self.uploads.buffer(
                    &mut encoder,
                    &self.mirror_globals,
                    0,
                    bytemuck::bytes_of(&mirror_globals),
                );
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("Water mirror"),
                    timestamp_writes: self
                        .own_timing
                        .as_mut()
                        .and_then(|t| t.writes("Water mirror")),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &mirror.color,
                        resolve_target: None,
                        depth_slice: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(clear),
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                        view: &mirror.depth,
                        depth_ops: Some(wgpu::Operations {
                            load: wgpu::LoadOp::Clear(0.0),
                            store: wgpu::StoreOp::Discard,
                        }),
                        stencil_ops: None,
                    }),
                    ..Default::default()
                });
                pass.set_bind_group(0, &self.mirror_group, &[]);
                if sky_visible && frame.skybox {
                    pass.set_pipeline(&self.sky);
                    pass.draw(0..3, 0..1);
                }
                // The solid terrain: occlusion culling is off with a mirror, so every draw
                // is phase 1's.
                if flat_frame && let Some(flat) = &self.flat {
                    pass.set_pipeline(&flat.solid_mirror);
                    let vertices = flat_vertices(&self.arena, self.translucent_arena.as_ref());
                    flat.draw(&mut pass, &flat.calls[..flat.solid_calls], vertices);
                } else {
                    pass.set_pipeline(&self.solid_mirror);
                    pass.set_index_buffer(self.indices.slice(..), wgpu::IndexFormat::Uint32);
                    pass.set_vertex_buffer(1, self.instances.slice(..));
                    self.draw_solid_sections(&mut pass, &self.indirect, 0, 0, solid_draws);
                }
                drop(pass);
                reflection = Some(mirror.color.clone());
            }
        }
        if let Some(native) = &mut self.native {
            let reflection =
                reflection.map(|view| native.frame.linear_mirror(&self.device, &mut encoder, view));
            native
                .frame
                .set_reflection(&self.device, reflection.as_ref());
        }
        // The material stage's copies of the world before translucent terrain, the size of
        // this frame's targets.
        if let Some(native) = &mut self.native
            && native.material.is_some()
            && !frame.title
        {
            let color = scaled.as_ref().map(|(c, _)| c.texture());
            native
                .frame
                .fit_opaque(&self.device, world_depth.texture(), color);
        }
        if let Some(native) = self.native.as_mut().filter(|_| !frame.title) {
            let writes = native.material.as_ref().is_some_and(|m| m.writes_surface);
            let depth = world_depth.texture();
            let size = if writes {
                [depth.width(), depth.height()]
            } else {
                [1, 1]
            };
            native.frame.fit_surface(&self.device, size);
        }
        laps.lap("r_mirror");
        // One of Horizon's own packs with a material stage: terrain draws with its pipelines,
        // which read the pack's frame at group 3.
        let material = self
            .native
            .as_ref()
            .filter(|_| !frame.title && !warp_on)
            .and_then(|n| Some((n.material.as_ref()?, &n.frame.group)));
        // Without HDR output, the solid shaders without its lighting (`SolidPipeline`).
        let hdr = self.hdr.is_some();
        // A world warp's pipelines in place of the terrain's and the entities' (`warp_on`).
        let warped = self.world_warps.pipelines().filter(|_| warp_on);
        let warp_group = self.world_warps.group().filter(|_| warp_on);
        let solid_pipeline = match warped {
            Some(w) => w.solid.get(hdr),
            None => material.map_or(self.solid.get(hdr), |(m, _)| &m.solid),
        };
        // Two solid passes only where a discard costs the GPU its hidden surface
        // removal: Apple's tiled GPUs, behind Metal. Elsewhere the second pass
        // would double the solid draw calls and vertex work for nothing.
        let split_solid = self.info.backend == wgpu::Backend::Metal && !warp_on;
        let translucent_pipeline = match warped {
            Some(w) => &w.translucent,
            None => material.map_or(&self.translucent, |(m, _)| &m.translucent),
        };
        let pack_flat = material.and_then(|(m, _)| m.flat.as_ref());
        // A material stage's translucent pass reads a copy of the depth the solid terrain
        // left: it is stored across the pass that ends before the copy.
        let keep_depth = keep_depth || material.is_some();
        // GPU culling's occlusion test splits the world pass as cull.rs's does.
        let gpu_occluding = gpu_frame && self.gpu_culling.as_ref().is_some_and(|g| g.occluding());
        let pack_frame = |pass: &mut wgpu::RenderPass<'_>| {
            if let Some((_, group)) = material {
                pass.set_bind_group(horizon_shader_api::stages::GROUP, group, &[]);
            }
        };
        // A pack's solid terrain that calls `set_surface` also writes the surface target, in
        // passes of its own between the sky and everything else.
        let surface = self
            .native
            .as_ref()
            .filter(|n| {
                !frame.title
                    && !pack_draws_terrain
                    && !warp_on
                    && n.material.as_ref().is_some_and(|m| m.writes_surface)
            })
            .map(|n| n.frame.surface_view.clone());
        // Solid terrain through the visibility buffer (`visbuffer`): flat frames without a
        // pack drawing terrain, a material stage, a surface target or a shadow pass.
        let vis_frame = flat_frame
            && !frame.title
            && !pack_terrain
            && !pack_draws_terrain
            && material.is_none()
            && surface.is_none()
            && shadow_view.is_none()
            && self
                .arena
                .page_buffers()
                .all(|(page, _)| page < visbuffer::MAX_PAGES)
            && self.visbuf.is_some();
        if vis_frame && let (Some(vis), Some(flat)) = (&mut self.visbuf, &self.flat) {
            let depth = world_depth.texture();
            vis.fit(&self.device, [depth.width(), depth.height()], flat);
        }
        {
            let attachments = [Some(wgpu::RenderPassColorAttachment {
                view: world_view,
                resolve_target: None,
                depth_slice: None,
                ops: wgpu::Operations {
                    load: if native_sky_pass {
                        wgpu::LoadOp::Load
                    } else {
                        wgpu::LoadOp::Clear(clear)
                    },
                    store: wgpu::StoreOp::Store,
                },
            })];
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Horizon world"),
                timestamp_writes: self.own_timing.as_mut().and_then(|t| t.writes("World")),
                color_attachments: &attachments,
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: world_depth,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(0.0),
                        // The visibility buffer's passes, and those after an occlusion test,
                        // draw on after it.
                        store: if cull.is_some() || gpu_occluding || keep_depth || vis_frame {
                            wgpu::StoreOp::Store
                        } else {
                            wgpu::StoreOp::Discard
                        },
                    }),
                    stencil_ops: None,
                }),
                ..Default::default()
            });
            pass.set_bind_group(0, &self.group, &[]);
            pack_frame(&mut pass);
            if frame.title || (sky_visible && (frame.skybox || frame.end_sky)) {
                if !frame.title && frame.skybox && !pack_draws_sky && !native_sky_pass {
                    match self
                        .native
                        .as_ref()
                        .and_then(|n| Some((n.sky.as_ref()?, &n.frame)))
                    {
                        Some((sky, native)) => sky.draw(&mut pass, native),
                        None => {
                            match self.core_sky.as_ref().filter(|c| c.draws_disc()) {
                                Some(core) => {
                                    core.draw_disc(&mut pass);
                                    pass.set_bind_group(0, &self.group, &[]);
                                }
                                None => {
                                    pass.set_pipeline(&self.sky_base);
                                    pass.draw(0..3, 0..1);
                                }
                            }
                            if frame.sky.sunrise_color[3] > 0.001 {
                                pass.set_pipeline(&self.sunrise_pipeline);
                                pass.set_vertex_buffer(0, self.sunrise_buffer.slice(..));
                                pass.draw(0..sunrise_fan::VERTICES as u32, 0..1);
                            }
                            // OptiFine custom sky layers, over the disc and sunrise and under the
                            // sun, moon and stars.
                            if self.custom_sky.is_drawing() {
                                self.custom_sky.draw(&mut pass);
                                pass.set_bind_group(0, &self.group, &[]);
                            }
                        }
                    }
                }
                if frame.title {
                    self.panorama.draw(&mut pass);
                    pass.set_bind_group(0, &self.group, &[]);
                } else if celestial_vertices > 0 || frame.end_sky {
                    pass.set_pipeline(&self.celestial_pipeline);
                    pass.set_bind_group(0, &self.entity_group, &[]);
                    pass.set_index_buffer(self.indices.slice(..), wgpu::IndexFormat::Uint32);
                    if frame.end_sky {
                        self.end_sky.draw(&mut pass);
                        // The End's custom sky layers go over its sky box.
                        if self.custom_sky.is_drawing() && !pack_draws_sky && self.native.is_none()
                        {
                            self.custom_sky.draw(&mut pass);
                            pass.set_pipeline(&self.celestial_pipeline);
                            pass.set_bind_group(0, &self.entity_group, &[]);
                        }
                    }
                    if celestial_vertices > 0 && !pack_draws_celestial {
                        // The Overworld's bodies start with the sun's quad
                        // (`Game::celestial_geometry`): a pack sky drawing its own skips it.
                        let own_sun = !frame.end_sky
                            && self
                                .native
                                .as_ref()
                                .is_some_and(|n| n.sky.is_some() && !n.vanilla_sun);
                        let first = if own_sun { 6 } else { 0 };
                        pass.set_vertex_buffer(0, self.celestial_buffer.slice(..));
                        pass.draw_indexed(first..(celestial_vertices as u32 / 4) * 6, 0, 0..1);
                    }
                    pass.set_bind_group(0, &self.group, &[]);
                }
                let own_stars = self
                    .native
                    .as_ref()
                    .is_some_and(|n| n.sky.is_some() && !n.vanilla_stars);
                if !frame.title && frame.stars.is_some() && !pack_draws_sky && !own_stars {
                    match self.core_sky.as_ref().filter(|c| c.draws_stars()) {
                        Some(core) => core.draw_stars(&mut pass),
                        None => self.stars.draw(&mut pass),
                    }
                    pass.set_bind_group(0, &self.group, &[]);
                }
                if !frame.title && frame.dark_disc && !pack_draws_sky {
                    pass.set_pipeline(&self.dark_disc);
                    pass.draw(0..3, 0..1);
                }
            }
            let terrain_pass =
                |encoder: &mut wgpu::CommandEncoder, surface: &wgpu::TextureView, clear: bool| {
                    // Its lifetime forgotten: a closure cannot tie it to `encoder`'s.
                    let mut pass = surface_pass(encoder, world_view, surface, world_depth, clear)
                        .forget_lifetime();
                    pass.set_bind_group(0, &self.group, &[]);
                    pack_frame(&mut pass);
                    pass
                };
            // The occlusion test's pipeline draws into the world's colour alone.
            let plain_pass = |encoder: &mut wgpu::CommandEncoder| {
                let mut pass =
                    world_pass_after_culling(encoder, world_view, world_depth, true, None, "")
                        .forget_lifetime();
                pass.set_bind_group(0, &self.group, &[]);
                pack_frame(&mut pass);
                pass
            };
            if let Some(surface) = &surface {
                drop(pass);
                pass = terrain_pass(&mut encoder, surface, true);
            }
            match self
                .flat
                .as_ref()
                .filter(|_| flat_frame && !pack_draws_terrain)
            {
                _ if pack_draws_terrain => {
                    pass.set_index_buffer(self.indices.slice(..), wgpu::IndexFormat::Uint32);
                }
                // The visibility buffer: the sky stays in the world pass's colour; the solid
                // calls draw their IDs and depth (phase 1, the occlusion test, phase 2), then
                // the resolve shades them, and the world pass goes on over both.
                Some(flat) if vis_frame => {
                    let vis = self.visbuf.as_ref().expect("a visibility buffer frame");
                    let vertices = flat_vertices(&self.arena, self.translucent_arena.as_ref());
                    drop(pass);
                    let mut ids =
                        vis.id_pass(&mut encoder, world_depth, true, self.own_timing.as_mut());
                    ids.set_bind_group(0, &self.group, &[]);
                    vis.draw_ids(&mut ids, flat, vertices);
                    drop(ids);
                    if let Some(cull) = cull {
                        // The test's pipeline draws into the world's colour.
                        let mut test = plain_pass(&mut encoder);
                        cull.test(&mut test);
                        drop(test);
                        cull.test_pass(&mut encoder, self.own_timing.as_mut());
                        flat.flatten_phase2(&mut encoder, self.own_timing.as_mut());
                        let mut ids =
                            vis.id_pass(&mut encoder, world_depth, false, self.own_timing.as_mut());
                        ids.set_bind_group(0, &self.group, &[]);
                        vis.draw_ids_phase2(&mut ids, flat, vertices);
                    }
                    vis.resolve(
                        &self.device,
                        &mut encoder,
                        world_view,
                        &self.group,
                        flat,
                        &self.arena,
                        self.own_timing.as_mut(),
                    );
                    pass = world_pass_after_culling(
                        &mut encoder,
                        world_view,
                        world_depth,
                        keep_depth,
                        self.own_timing.as_mut(),
                        "World after visibility buffer",
                    );
                    pass.set_bind_group(0, &self.group, &[]);
                    pack_frame(&mut pass);
                    pass.set_index_buffer(self.indices.slice(..), wgpu::IndexFormat::Uint32);
                }
                Some(flat) => {
                    let vertices = flat_vertices(&self.arena, self.translucent_arena.as_ref());
                    // Without a pack, on Metal, the opaque pass, then the cutout one
                    // (terrain.wgsl's `opaque_quad`), each over phase 1's and phase 2's
                    // calls with quads for it (phase 2's counted from its first).
                    let [opaque, cutout, phase2_split] = flat.pass_calls;
                    let phase2_split = phase2_split - flat.phase1_calls;
                    let phase2_calls = flat.solid_calls - flat.phase1_calls;
                    type Calls = std::ops::Range<usize>;
                    let passes: &[(&wgpu::RenderPipeline, Calls, Calls)] = match pack_flat {
                        Some(p) => &[(&p.0, 0..flat.phase1_calls, 0..phase2_calls)],
                        None if split_solid => &[
                            (flat.solid_opaque.get(hdr), 0..opaque, 0..phase2_split),
                            (
                                flat.solid_cutout.get(hdr),
                                cutout..flat.phase1_calls,
                                phase2_split..phase2_calls,
                            ),
                        ],
                        None => &[(flat.solid.get(hdr), 0..flat.phase1_calls, 0..phase2_calls)],
                    };
                    let draw = |pass: &mut wgpu::RenderPass<'_>| {
                        for (pipeline, calls, _) in passes {
                            pass.set_pipeline(pipeline);
                            flat.draw(pass, &flat.calls[calls.clone()], vertices);
                        }
                    };
                    match cull {
                        // As below: phase 1, the test, then phase 2's draws
                        // that the test saw, copied after it.
                        Some(cull) => {
                            draw(&mut pass);
                            if surface.is_some() {
                                drop(pass);
                                pass = plain_pass(&mut encoder);
                            }
                            cull.test(&mut pass);
                            drop(pass);
                            cull.test_pass(&mut encoder, self.own_timing.as_mut());
                            flat.flatten_phase2(&mut encoder, self.own_timing.as_mut());
                            pass = match &surface {
                                Some(surface) => terrain_pass(&mut encoder, surface, false),
                                None => world_pass_after_culling(
                                    &mut encoder,
                                    world_view,
                                    world_depth,
                                    keep_depth,
                                    self.own_timing.as_mut(),
                                    "World after culling",
                                ),
                            };
                            pass.set_bind_group(0, &self.group, &[]);
                            pack_frame(&mut pass);
                            for (pipeline, _, calls) in passes {
                                pass.set_pipeline(pipeline);
                                flat.draw_phase2(&mut pass, calls.clone(), vertices);
                            }
                        }
                        // Without culling, phase 1 is all the solid calls.
                        None => draw(&mut pass),
                    }
                    pass.set_bind_group(0, &self.group, &[]);
                    pass.set_index_buffer(self.indices.slice(..), wgpu::IndexFormat::Uint32);
                }
                None => {
                    // Without a native material, on Metal, the opaque pass, then the
                    // cutout one (terrain.wgsl's `opaque_quad`), each over phase 1's
                    // and phase 2's lists with quads for it.
                    let (opaque, cutout) = (opaque_end as u32, cutout_start as u32);
                    type Draws = std::ops::Range<u32>;
                    let passes: &[(&wgpu::RenderPipeline, Draws, Draws)] = match material {
                        None if split_solid => &[
                            (
                                self.solid_opaque.get(hdr),
                                0..opaque,
                                phase1_draws..phase2_cutout,
                            ),
                            (
                                self.solid_cutout.get(hdr),
                                cutout..phase1_draws,
                                phase2_cutout..solid_draws,
                            ),
                        ],
                        _ => &[(solid_pipeline, 0..phase1_draws, phase1_draws..solid_draws)],
                    };
                    // The vanilla solid pipelines read the page themselves; a native
                    // material's take it as vertices.
                    let pulled = material.is_none() && self.flat.is_some();
                    pass.set_index_buffer(self.indices.slice(..), wgpu::IndexFormat::Uint32);
                    pass.set_vertex_buffer(1, self.instances.slice(..));
                    if let Some(group) = warp_group {
                        pass.set_bind_group(2, group, &[]);
                    }
                    match cull {
                        Some(cull) => {
                            // Phase 1 and the boxes against its depth (in
                            // its pass, or one of their own on Metal), then
                            // what they found in sight, in a pass that loads
                            // both targets (it reads the test's results).
                            for (pipeline, draws, _) in passes {
                                pass.set_pipeline(pipeline);
                                self.draw_sections_from(
                                    &mut pass,
                                    &self.indirect,
                                    0,
                                    draws.start,
                                    draws.len() as u32,
                                    pulled,
                                );
                            }
                            if surface.is_some() {
                                drop(pass);
                                pass = plain_pass(&mut encoder);
                            }
                            cull.test(&mut pass);
                            drop(pass);
                            cull.test_pass(&mut encoder, self.own_timing.as_mut());
                            pass = match &surface {
                                Some(surface) => terrain_pass(&mut encoder, surface, false),
                                None => world_pass_after_culling(
                                    &mut encoder,
                                    world_view,
                                    world_depth,
                                    keep_depth,
                                    self.own_timing.as_mut(),
                                    "World after culling",
                                ),
                            };
                            pass.set_bind_group(0, &self.group, &[]);
                            pack_frame(&mut pass);
                            pass.set_index_buffer(
                                self.indices.slice(..),
                                wgpu::IndexFormat::Uint32,
                            );
                            pass.set_vertex_buffer(1, self.instances.slice(..));
                            for (pipeline, _, draws) in passes {
                                pass.set_pipeline(pipeline);
                                self.draw_sections_from(
                                    &mut pass,
                                    &cull.phase2,
                                    phase1_draws,
                                    draws.start,
                                    draws.len() as u32,
                                    pulled,
                                );
                            }
                        }
                        // Without culling, phase 1 is all the solid draws; GPU culling's
                        // own test (`GpuCulling::occluding`) leaves the CPU's to phase 2.
                        None => {
                            let gpu = self.gpu_culling.as_ref().filter(|_| gpu_frame);
                            let occluding = gpu.is_some_and(|g| g.occluding());
                            let page_group = |page: u32| {
                                pulled
                                    .then(|| self.flat.as_ref()?.page_group(page))
                                    .flatten()
                            };
                            for (pipeline, draws, _) in passes {
                                pass.set_pipeline(pipeline);
                                // GPU culling's draws first, all within the render distance.
                                if let Some(gpu) = gpu {
                                    gpu.draw(&mut pass, &self.arena, page_group, 0);
                                    pass.set_vertex_buffer(1, self.instances.slice(..));
                                }
                                if !occluding {
                                    self.draw_sections_from(
                                        &mut pass,
                                        &self.indirect,
                                        0,
                                        draws.start,
                                        draws.len() as u32,
                                        pulled,
                                    );
                                }
                            }
                            // As cull.rs's arm: the boxes against phase 1's depth, then
                            // phase 2 and the CPU's draws in a pass that loads both targets.
                            if let Some(gpu) = gpu.filter(|_| occluding) {
                                if surface.is_some() {
                                    drop(pass);
                                    pass = plain_pass(&mut encoder);
                                }
                                gpu.test(&mut pass);
                                drop(pass);
                                gpu.compact(&mut encoder, self.own_timing.as_mut());
                                pass = match &surface {
                                    Some(surface) => terrain_pass(&mut encoder, surface, false),
                                    None => world_pass_after_culling(
                                        &mut encoder,
                                        world_view,
                                        world_depth,
                                        keep_depth,
                                        self.own_timing.as_mut(),
                                        "World after culling",
                                    ),
                                };
                                pass.set_bind_group(0, &self.group, &[]);
                                pack_frame(&mut pass);
                                pass.set_index_buffer(
                                    self.indices.slice(..),
                                    wgpu::IndexFormat::Uint32,
                                );
                                for (pipeline, draws, _) in passes {
                                    pass.set_pipeline(pipeline);
                                    gpu.draw(&mut pass, &self.arena, page_group, 1);
                                    pass.set_vertex_buffer(1, self.instances.slice(..));
                                    self.draw_sections_from(
                                        &mut pass,
                                        &self.indirect,
                                        0,
                                        draws.start,
                                        draws.len() as u32,
                                        pulled,
                                    );
                                }
                            }
                        }
                    }
                }
            }
            // `[displacement]`'s grid over the flat quads it took over, writing its depth.
            if let Some(n) = self
                .native
                .as_ref()
                .filter(|_| !frame.title && !pack_draws_terrain && !warp_on)
                && let Some(d) = &n.displacement
                && let Some(pipeline) = n.material.as_ref().and_then(|m| m.displaced.as_ref())
            {
                pass.set_bind_group(0, &self.group, &[]);
                pack_frame(&mut pass);
                d.draw(&mut pass, &d.view, pipeline);
            }
            if surface.is_some() {
                drop(pass);
                pass = plain_pass(&mut encoder);
            }
            // A world warp's entity pipelines read its group 2.
            if let Some(group) = warp_group {
                pass.set_bind_group(2, group, &[]);
            }
            let hdr_mark = warped.map_or(&self.hdr_mark_pipeline, |w| &w.hdr_mark);
            let hdr_entity_mark =
                warped.map_or(&self.hdr_entity_mark_pipeline, |w| &w.hdr_entity_mark);
            if static_entity_quads > 0 {
                pass.set_pipeline(
                    warped.map_or(&self.static_entity_pipeline, |w| &w.static_entities),
                );
                pass.set_bind_group(0, &self.entity_group, &[]);
                pass.set_bind_group(1, &self.static_entity_group, &[]);
                pass.set_index_buffer(
                    self.static_entity_indices.slice(..),
                    wgpu::IndexFormat::Uint32,
                );
                for section in frame.visible_static_block_entity_sections {
                    let Some(gpu) = self.static_entity_sections.get(section) else {
                        continue;
                    };
                    let quads = gpu.vertices / 4;
                    if quads == 0 {
                        continue;
                    }
                    pass.set_vertex_buffer(0, gpu.buffer.slice(..));
                    pass.set_vertex_buffer(1, gpu.origin_buffer.slice(..));
                    pass.draw_indexed(0..quads * 6, 0, 0..1);
                }
                pass.set_bind_group(0, &self.group, &[]);
            }
            pass.set_index_buffer(self.indices.slice(..), wgpu::IndexFormat::Uint32);
            if entity_quads > 0 {
                pass.set_pipeline(warped.map_or(&self.entity_pipeline, |w| &w.entities));
                pass.set_bind_group(0, &self.entity_group, &[]);
                pass.set_vertex_buffer(0, self.entity_buffer.slice(..));
                pass.draw_indexed(0..entity_quads * 6, 0, 0..1);
                if light_marks(self.output.frame.format, self.hdr.is_some()) {
                    pass.set_pipeline(hdr_entity_mark);
                    pass.draw_indexed(0..entity_quads * 6, 0, 0..1);
                }
                pass.set_bind_group(0, &self.group, &[]);
            }
            if text_quads > 0 && !oit_on {
                pass.set_pipeline(warped.map_or(&self.text_polygon_offset_pipeline, |w| &w.text));
                pass.set_bind_group(0, &self.entity_group, &[]);
                pass.set_vertex_buffer(0, self.entity_buffer.slice(..));
                pass.draw_indexed(0..text_quads * 6, (entity_quads * 4) as i32, 0..1);
                pass.set_bind_group(0, &self.group, &[]);
            }
            // FeatureRenderDispatcher.PreparedFrame.executeTranslucent: the shadows
            // come before the translucent items, which then cover them (#7026).
            if shadow_quads > 0 && !oit_on {
                pass.set_pipeline(warped.map_or(&self.shadow_blob_pipeline, |w| &w.entity_shadows));
                pass.set_bind_group(0, &self.entity_group, &[]);
                pass.set_vertex_buffer(0, self.entity_buffer.slice(..));
                pass.draw_indexed(
                    0..shadow_quads * 6,
                    ((base_quads
                        - translucent_item_quads
                        - translucent_particle_quads
                        - shadow_quads)
                        * 4) as i32,
                    0..1,
                );
                pass.set_bind_group(0, &self.group, &[]);
            }
            if translucent_item_quads > 0 && !oit_on {
                pass.set_pipeline(
                    warped.map_or(&self.translucent_item_pipeline, |w| &w.translucent_items),
                );
                pass.set_bind_group(0, &self.entity_group, &[]);
                pass.set_vertex_buffer(0, self.entity_buffer.slice(..));
                pass.draw_indexed(
                    0..translucent_item_quads * 6,
                    ((base_quads - translucent_item_quads) * 4) as i32,
                    0..1,
                );
                if light_marks(self.output.frame.format, self.hdr.is_some()) {
                    // A glowing item in the world (Item sheet quads), as entities are.
                    pass.set_pipeline(hdr_mark);
                    pass.draw_indexed(
                        0..translucent_item_quads * 6,
                        ((base_quads - translucent_item_quads) * 4) as i32,
                        0..1,
                    );
                }
                pass.set_bind_group(0, &self.group, &[]);
            }
            // Native terrain models use the block sprite arrays with their own
            // alpha/mipmap policies and ordinary opaque depth. Storage follows
            // the existing materials without moving their indices or frame tails.
            let mut first =
                world_quads - native_solid_quads - native_cutout_quads - native_cutout_mipped_quads;
            for (i, count) in [
                native_solid_quads,
                native_cutout_quads,
                native_cutout_mipped_quads,
            ]
            .into_iter()
            .enumerate()
            {
                if count > 0 {
                    pass.set_pipeline(&self.terrain_model_pipelines[i]);
                    pass.set_bind_group(
                        0,
                        if i == 1 {
                            &self.terrain_model_group
                        } else {
                            &self.fluid_group
                        },
                        &[],
                    );
                    pass.set_vertex_buffer(0, self.entity_buffer.slice(..));
                    pass.draw_indexed(0..count * 6, (first * 4) as i32, 0..1);
                }
                first += count;
            }
            pass.set_bind_group(0, &self.group, &[]);
            // Still-fluid block entities draw as their own ordinary sorted
            // translucent material, before terrain transparency. This material
            // always writes depth and never joins the improved-transparency queues.
            if fluid_quads > 0 {
                pass.set_pipeline(&self.fluid_pipeline);
                pass.set_bind_group(0, &self.fluid_group, &[]);
                pass.set_vertex_buffer(0, self.entity_buffer.slice(..));
                pass.draw_indexed(
                    0..fluid_quads * 6,
                    ((base_quads + oit_entity_quads) * 4) as i32,
                    0..1,
                );
                pass.set_bind_group(0, &self.group, &[]);
            }
            // Cracks multiply the existing light; scalar OIT coverage cannot carry that factor.
            if crumbling_quads > 0 && (!oit_on || hdr_gain) {
                pass.set_pipeline(&self.crumbling_pipeline);
                pass.set_bind_group(0, &self.entity_group, &[]);
                pass.set_vertex_buffer(0, self.entity_buffer.slice(..));
                pass.draw_indexed(
                    0..crumbling_quads * 6,
                    ((entity_quads + text_quads) * 4) as i32,
                    0..1,
                );
                pass.set_bind_group(0, &self.group, &[]);
            }
            // BoatRenderer's water patch (RenderTypes.waterMask): depth only,
            // before translucent terrain, so no water shows inside the boat.
            if water_mask_quads > 0 && !oit_on {
                pass.set_pipeline(&self.water_mask_pipeline);
                pass.set_bind_group(0, &self.entity_group, &[]);
                pass.set_vertex_buffer(0, self.entity_buffer.slice(..));
                pass.draw_indexed(
                    0..water_mask_quads * 6,
                    ((entity_quads + text_quads + crumbling_quads + translucent_crumbling_quads)
                        * 4) as i32,
                    0..1,
                );
                pass.set_bind_group(0, &self.group, &[]);
            }
            if !frame.lightning.bolts.is_empty() && !oit_on {
                pass.set_pipeline(&self.lightning_pipeline);
                pass.set_bind_group(0, &self.entity_group, &[]);
                self.lightning.draw(&mut pass, &frame.lightning);
                if light_marks(self.output.frame.format, self.hdr.is_some()) {
                    pass.set_pipeline(&self.hdr_mark_lightning_pipeline);
                    self.lightning.draw(&mut pass, &frame.lightning);
                }
                pass.set_bind_group(0, &self.group, &[]);
            }
            if swirl_quads > 0 && !oit_on {
                pass.set_pipeline(&self.energy_swirl_pipeline);
                pass.set_bind_group(0, &self.entity_group, &[]);
                pass.set_vertex_buffer(0, self.entity_buffer.slice(..));
                pass.draw_indexed(
                    0..swirl_quads * 6,
                    ((entity_quads
                        + text_quads
                        + crumbling_quads
                        + translucent_crumbling_quads
                        + water_mask_quads)
                        * 4) as i32,
                    0..1,
                );
                pass.set_bind_group(0, &self.group, &[]);
            }
            if additive_quads > 0 && !oit_on {
                pass.set_bind_group(0, &self.entity_group, &[]);
                pass.set_vertex_buffer(0, self.entity_buffer.slice(..));
                let mut first = 0;
                for run in frame.additive.as_chunks::<4>().0.chunk_by(|a, b| {
                    (a[0].layer & 0xffff == DRAGON_RAYS_OVERLAY)
                        == (b[0].layer & 0xffff == DRAGON_RAYS_OVERLAY)
                }) {
                    let rays = run[0][0].layer & 0xffff == DRAGON_RAYS_OVERLAY;
                    pass.set_pipeline(if rays {
                        &self.dragon_rays_pipeline
                    } else {
                        &self.additive_pipeline
                    });
                    let end = first + run.len() as u32;
                    pass.draw_indexed(first * 6..end * 6, (world_quads * 4) as i32, 0..1);
                    first = end;
                }
                if light_marks(self.output.frame.format, self.hdr.is_some()) {
                    // The dragon's eyes and death rays, marked as entities are.
                    pass.set_pipeline(&self.hdr_mark_pipeline);
                    pass.draw_indexed(0..additive_quads * 6, (world_quads * 4) as i32, 0..1);
                }
                pass.set_bind_group(0, &self.group, &[]);
            }
            if outline_edges > 0 && !frame.outline_after_terrain && !pack_draws_outline {
                self.draw_outline(&mut pass, outline_edges as u32, frame.outline_high_contrast);
            }
            // DrawableGizmoPrimitives' opaque group writes depth, the translucent one does not.
            if debug_opaque > 0 {
                pass.set_pipeline(&self.debug_lines_opaque);
                pass.set_vertex_buffer(0, self.debug_line_buffer.slice(..));
                pass.draw(0..4, 0..debug_opaque);
            }
            if debug_depth > debug_opaque {
                pass.set_pipeline(&self.debug_lines);
                pass.set_vertex_buffer(0, self.debug_line_buffer.slice(..));
                pass.draw(0..4, debug_opaque..debug_depth);
            }
            if debug_top > 0 {
                pass.set_pipeline(&self.debug_lines_on_top);
                pass.set_vertex_buffer(0, self.debug_line_buffer.slice(..));
                pass.draw(0..4, debug_depth..debug_depth + debug_top);
            }
            // One of Horizon's own packs reads the world before translucent terrain
            // (`opaque_depth`, `opaque_color`): the pass ends, the targets are copied, and a
            // pass that loads both carries on.
            if material.is_some()
                && let Some(native) = &self.native
            {
                drop(pass);
                let depth = world_depth.texture();
                encoder.copy_texture_to_texture(
                    depth.as_image_copy(),
                    native.frame.opaque_depth.as_image_copy(),
                    depth.size(),
                );
                if let Some((color, _)) = &scaled {
                    native.frame.copy_opaque(&self.device, &mut encoder, color);
                }
                pass = world_pass_after_culling(
                    &mut encoder,
                    world_view,
                    world_depth,
                    keep_depth,
                    self.own_timing.as_mut(),
                    "World after copy",
                );
                pass.set_bind_group(0, &self.group, &[]);
                pack_frame(&mut pass);
            }
            match self.flat.as_ref().filter(|_| flat_frame) {
                // The pack draws translucent terrain after its deferred programs.
                _ if pack_draws_water => {}
                // LevelRenderer.executeOit in place of the classic blend; what
                // follows draws over its composite in a pass of its own.
                flat if let Some(oit) = oit => {
                    drop(pass);
                    // LevelRenderer.executeOitWaterMask and CloudRenderer.renderOit.
                    let mask = |pass: &mut wgpu::RenderPass<'_>| {
                        pass.set_pipeline(&self.water_mask_pipeline);
                        pass.set_bind_group(0, &self.entity_group, &[]);
                        pass.set_index_buffer(self.indices.slice(..), wgpu::IndexFormat::Uint32);
                        pass.set_vertex_buffer(0, self.entity_buffer.slice(..));
                        pass.draw_indexed(
                            0..water_mask_quads * 6,
                            ((entity_quads
                                + text_quads
                                + crumbling_quads
                                + translucent_crumbling_quads)
                                * 4) as i32,
                            0..1,
                        );
                    };
                    let clouds =
                        |pass: &mut wgpu::RenderPass<'_>, pipeline: &wgpu::RenderPipeline| {
                            pass.set_pipeline(pipeline);
                            pass.set_bind_group(0, &self.entity_group, &[]);
                            pass.set_bind_group(1, &self.cloud_group, &[]);
                            pass.set_index_buffer(
                                self.indices.slice(..),
                                wgpu::IndexFormat::Uint32,
                            );
                            pass.set_vertex_buffer(0, self.cloud_buffer.slice(..));
                            pass.draw_indexed(0..cloud_quads * 6, 0, 0..1);
                        };
                    // "Features, World Border, Weather": what follows the opaque
                    // world in the classic pass, one stage at a time.
                    let features = |pass: &mut wgpu::RenderPass<'_>, stage: oit::Stage| {
                        let f = oit.features();
                        let s = stage as usize;
                        pass.set_index_buffer(self.indices.slice(..), wgpu::IndexFormat::Uint32);
                        pass.set_bind_group(0, &self.entity_group, &[]);
                        pass.set_vertex_buffer(0, self.entity_buffer.slice(..));
                        // Quads of a material, `first` quads into the buffer.
                        let quads = |pass: &mut wgpu::RenderPass<'_>,
                                     pipeline: &wgpu::RenderPipeline,
                                     count: u32,
                                     first: u32| {
                            if count > 0 {
                                pass.set_pipeline(pipeline);
                                pass.draw_indexed(0..count * 6, (first * 4) as i32, 0..1);
                            }
                        };
                        quads(pass, &f.text[s], text_quads, entity_quads);
                        if !hdr_gain {
                            quads(
                                pass,
                                &f.crumbling[s],
                                crumbling_quads,
                                entity_quads + text_quads,
                            );
                            quads(
                                pass,
                                &f.crumbling[s],
                                translucent_crumbling_quads,
                                entity_quads + text_quads + crumbling_quads,
                            );
                        }
                        quads(
                            pass,
                            &f.swirl[s],
                            swirl_quads,
                            entity_quads
                                + text_quads
                                + crumbling_quads
                                + translucent_crumbling_quads
                                + water_mask_quads,
                        );
                        quads(
                            pass,
                            &f.shadow[s],
                            shadow_quads,
                            base_quads
                                - translucent_item_quads
                                - translucent_particle_quads
                                - shadow_quads,
                        );
                        quads(
                            pass,
                            &f.entity[s],
                            translucent_particle_quads,
                            base_quads - translucent_item_quads - translucent_particle_quads,
                        );
                        quads(
                            pass,
                            &f.entity[s],
                            translucent_item_quads,
                            base_quads - translucent_item_quads,
                        );
                        // Translucent, emissive, eyes and breeze wind models:
                        // the same fragment, their light and shading are in
                        // the vertices.
                        quads(pass, &f.entity[s], oit_entity_quads, base_quads);
                        if !frame.lightning.bolts.is_empty() {
                            pass.set_pipeline(&f.lightning[s]);
                            self.lightning.draw(pass, &frame.lightning);
                            pass.set_vertex_buffer(0, self.entity_buffer.slice(..));
                        }
                        quads(pass, &f.additive[s], additive_quads, world_quads);
                        self.weather
                            .draw_with(pass, &f.weather[s], &self.entity_group);
                        if let Some(b) = frame.border.as_ref().filter(|_| !frame.title) {
                            pass.set_pipeline(&f.border[s]);
                            pass.set_bind_group(0, &self.border_group, &[]);
                            pass.set_vertex_buffer(0, self.border_vertices.slice(..));
                            for &side in &b.sides {
                                let first = side as u32 * 6;
                                pass.draw_indexed(first..first + 6, 0, 0..1);
                            }
                        }
                    };
                    let extras = oit::Extras {
                        water_mask: (water_mask_quads > 0).then_some(&mask as _),
                        clouds: (cloud_quads > 0).then_some(&clouds as _),
                        features: Some(&features),
                    };
                    // The draw closure below borrows all of `self`, so the timer is taken out.
                    let mut own_timing = self.own_timing.take();
                    oit.run(
                        &mut encoder,
                        world_view,
                        world_depth,
                        &|pass, stage| {
                            pass.set_bind_group(0, &self.group, &[]);
                            // Flat section draws take the flat pipelines.
                            if let Some((flat, pipeline)) = flat.zip(oit.flat(stage)) {
                                pass.set_pipeline(pipeline);
                                flat.draw(
                                    pass,
                                    flat.translucent_calls(),
                                    flat_vertices(&self.arena, self.translucent_arena.as_ref()),
                                );
                                return;
                            }
                            pass.set_pipeline(oit.terrain(stage));
                            pass.set_index_buffer(
                                self.indices.slice(..),
                                wgpu::IndexFormat::Uint32,
                            );
                            pass.set_vertex_buffer(1, self.instances.slice(..));
                            self.draw_sections(
                                pass,
                                &self.indirect,
                                0,
                                solid_draws,
                                translucent_draws,
                            );
                        },
                        &extras,
                        own_timing.as_mut(),
                    );
                    self.own_timing = own_timing;
                    pass = world_pass_after_culling(
                        &mut encoder,
                        world_view,
                        world_depth,
                        true,
                        self.own_timing.as_mut(),
                        "World after OIT",
                    );
                    pass.set_bind_group(0, &self.group, &[]);
                    pass.set_index_buffer(self.indices.slice(..), wgpu::IndexFormat::Uint32);
                    if cloud_quads > 0 && !pack_draws_clouds {
                        self.mark_clouds(&mut pass, cloud_quads);
                    }
                    // HDR output's mark on what shines in the stages, as the classic
                    // pass marks it after drawing it: eyes, the dragon's eyes and rays,
                    // firework sparks and lightning.
                    if light_marks(self.output.frame.format, self.hdr.is_some()) {
                        pass.set_pipeline(&self.hdr_mark_pipeline);
                        pass.set_bind_group(0, &self.entity_group, &[]);
                        pass.set_vertex_buffer(0, self.entity_buffer.slice(..));
                        for (quads, first) in [
                            (oit_entity_quads, base_quads),
                            (additive_quads, world_quads),
                            (translucent_item_quads, base_quads - translucent_item_quads),
                            (
                                translucent_particle_quads,
                                base_quads - translucent_item_quads - translucent_particle_quads,
                            ),
                        ] {
                            if quads > 0 {
                                pass.draw_indexed(0..quads * 6, (first * 4) as i32, 0..1);
                            }
                        }
                        if !frame.lightning.bolts.is_empty() {
                            pass.set_pipeline(&self.hdr_mark_lightning_pipeline);
                            self.lightning.draw(&mut pass, &frame.lightning);
                        }
                        pass.set_bind_group(0, &self.group, &[]);
                    }
                    self.mark_translucent_terrain(&mut pass, flat, solid_draws, translucent_draws);
                }
                Some(flat) => {
                    pass.set_pipeline(pack_flat.map_or(&flat.translucent, |p| &p.1));
                    flat.draw(
                        &mut pass,
                        flat.translucent_calls(),
                        flat_vertices(&self.arena, self.translucent_arena.as_ref()),
                    );
                    pass.set_bind_group(0, &self.group, &[]);
                    pass.set_index_buffer(self.indices.slice(..), wgpu::IndexFormat::Uint32);
                    self.mark_translucent_terrain(
                        &mut pass,
                        Some(flat),
                        solid_draws,
                        translucent_draws,
                    );
                }
                None => {
                    pass.set_pipeline(translucent_pipeline);
                    if let Some(group) = warp_group {
                        pass.set_bind_group(2, group, &[]);
                    }
                    pass.set_index_buffer(self.indices.slice(..), wgpu::IndexFormat::Uint32);
                    pass.set_vertex_buffer(1, self.instances.slice(..));
                    self.draw_sections(
                        &mut pass,
                        &self.indirect,
                        0,
                        solid_draws,
                        translucent_draws,
                    );
                    // (its HDR mark would fall where the water was before the warp moved it)
                    if !warp_on {
                        self.mark_translucent_terrain(
                            &mut pass,
                            None,
                            solid_draws,
                            translucent_draws,
                        );
                    }
                }
            }
            // LevelRenderer.executeTranslucentAfterTerrain: vanilla's
            // translucent particle layer draws after translucent terrain —
            // the quads write depth, so before it they would hide the glass
            // or water behind them.
            if translucent_particle_quads > 0 && !oit_on {
                pass.set_pipeline(warped.map_or(&self.entity_pipeline, |w| &w.entities));
                if let Some(group) = warp_group {
                    pass.set_bind_group(2, group, &[]);
                }
                pass.set_bind_group(0, &self.entity_group, &[]);
                pass.set_index_buffer(self.indices.slice(..), wgpu::IndexFormat::Uint32);
                pass.set_vertex_buffer(0, self.entity_buffer.slice(..));
                pass.draw_indexed(
                    0..translucent_particle_quads * 6,
                    ((base_quads - translucent_item_quads - translucent_particle_quads) * 4) as i32,
                    0..1,
                );
                if light_marks(self.output.frame.format, self.hdr.is_some()) {
                    // Fireworks: their sparks are translucent-layer particles.
                    pass.set_pipeline(hdr_entity_mark);
                    pass.draw_indexed(
                        0..translucent_particle_quads * 6,
                        ((base_quads - translucent_item_quads - translucent_particle_quads) * 4)
                            as i32,
                        0..1,
                    );
                }
                pass.set_bind_group(0, &self.group, &[]);
            }
            // Native world text is an ordinary depth-tested overlay, after
            // particles (also after the OIT composite) and before the hand.
            // Its own geometry is independent of block-entity lifetime.
            if world_text_quads + world_text_offset_quads > 0 {
                pass.set_bind_group(0, &self.entity_group, &[]);
                pass.set_index_buffer(self.indices.slice(..), wgpu::IndexFormat::Uint32);
                pass.set_vertex_buffer(0, self.entity_buffer.slice(..));
                let first = base_quads + oit_entity_quads + fluid_quads;
                for (pipeline, count, offset) in [
                    (&self.world_text_pipeline, world_text_quads, first),
                    (
                        &self.text_polygon_offset_pipeline,
                        world_text_offset_quads,
                        first + world_text_quads,
                    ),
                ] {
                    if count > 0 {
                        pass.set_pipeline(pipeline);
                        pass.draw_indexed(0..count * 6, (offset * 4) as i32, 0..1);
                    }
                }
                pass.set_bind_group(0, &self.group, &[]);
            }
            // LevelRenderer.executeClassicTransparency: translucent targets'
            // outlines follow terrain, so its blending cannot wash them out.
            if outline_edges > 0 && frame.outline_after_terrain && !pack_draws_outline {
                self.draw_outline(&mut pass, outline_edges as u32, frame.outline_high_contrast);
            }
            if translucent_crumbling_quads > 0 && (!oit_on || hdr_gain) {
                pass.set_pipeline(&self.crumbling_pipeline);
                pass.set_bind_group(0, &self.entity_group, &[]);
                pass.set_vertex_buffer(0, self.entity_buffer.slice(..));
                pass.draw_indexed(
                    0..translucent_crumbling_quads * 6,
                    ((entity_quads + text_quads + crumbling_quads) * 4) as i32,
                    0..1,
                );
            }
            if cloud_quads > 0 && !pack_draws_clouds && !oit_on {
                pass.set_pipeline(&self.cloud_pipeline);
                pass.set_bind_group(0, &self.entity_group, &[]);
                pass.set_bind_group(1, &self.cloud_group, &[]);
                pass.set_vertex_buffer(0, self.cloud_buffer.slice(..));
                pass.draw_indexed(0..cloud_quads * 6, 0, 0..1);
                self.mark_clouds(&mut pass, cloud_quads);
            }
            // LevelRenderer.executeClassicTransparency: weather after translucent
            // terrain and clouds, so the water behind the rain is drawn first.
            pass.set_index_buffer(self.indices.slice(..), wgpu::IndexFormat::Uint32);
            if !pack_draws_weather && !oit_on {
                let native_weather = self
                    .native
                    .as_ref()
                    .filter(|_| !frame.title)
                    .and_then(|n| Some((n.weather.as_ref()?, &n.frame.group)));
                self.weather
                    .draw(&mut pass, &self.entity_group, native_weather);
                pass.set_bind_group(0, &self.group, &[]);
            }
            // LevelRenderer: the world border after translucent terrain and weather.
            if let Some(b) = frame.border.as_ref().filter(|_| !frame.title && !oit_on) {
                pass.set_pipeline(&self.border_pipeline);
                pass.set_bind_group(0, &self.border_group, &[]);
                pass.set_index_buffer(self.indices.slice(..), wgpu::IndexFormat::Uint32);
                pass.set_vertex_buffer(0, self.border_vertices.slice(..));
                for &side in &b.sides {
                    let first = side as u32 * 6;
                    pass.draw_indexed(first..first + 6, 0, 0..1);
                }
                pass.set_bind_group(0, &self.group, &[]);
            }
            // LevelRenderer.executeSeeThrough, after the rest of the world.
            if see_through_quads > 0 {
                pass.set_pipeline(&self.see_through_pipeline);
                pass.set_bind_group(0, &self.entity_group, &[]);
                pass.set_vertex_buffer(0, self.entity_buffer.slice(..));
                pass.draw_indexed(
                    0..see_through_quads * 6,
                    ((world_quads + additive_quads + hand_quads) * 4) as i32,
                    0..1,
                );
            }
            // Native mods' primitives: after the entities and the rest of the world, into its
            // targets (its resolution, reversed depth and TAA), before the hand.
            if world_primitives {
                self.world_primitives.draw(&mut pass);
            }
            if hand_in_world {
                pass.set_pipeline(&self.depth_reset);
                pass.set_bind_group(0, &self.group, &[]);
                pass.draw(0..3, 0..1);
                self.draw_hand(&mut pass, hand_quads, world_quads + additive_quads);
            }
        }
        if let Some((color, depth)) = &scaled
            && pack_post
            && let Some((dimension, Some(mut post))) = self.pack_post.take()
        {
            let size = self.world_post.size().unwrap_or([1, 1]);
            let weather = match &self.hud.atlas_texture {
                Some(atlas) if pack_draws_weather => self
                    .pack_weather
                    .get_or_insert_with(|| pack_weather::PackWeather::new(&self.device))
                    .prepare(
                        &self.device,
                        &mut encoder,
                        &mut self.uploads,
                        atlas,
                        frame.weather,
                        frame.weather_time,
                        &self.indices,
                    ),
                _ => None,
            };
            post.set_weather(weather);
            let flat = self.flat.as_ref().filter(|_| flat_frame);
            self.pack_pages.clear();
            if flat.is_some() && (pack_draws_terrain || pack_draws_shadow) {
                self.pack_pages.extend(
                    self.arena
                        .page_buffers()
                        .map(|(p, b)| (p, b.clone(), self.arena.sources(p).cloned())),
                );
            }
            let pages = &self.pack_pages;
            let mut terrain =
                |rp: &mut wgpu::RenderPass<'_>,
                 list: pack_post::TerrainList,
                 page: &dyn Fn(&mut wgpu::RenderPass<'_>, u32),
                 geometry: Option<(u32, &wgpu::BindGroupLayout)>| {
                    use pack_post::TerrainList as L;
                    let shadow = matches!(list, L::ShadowSolid | L::ShadowTranslucent);
                    if !(if shadow {
                        pack_draws_shadow
                    } else {
                        pack_draws_terrain
                    }) {
                        return;
                    }
                    match flat {
                        Some(flat) => {
                            let [shadow, shadow_translucent] = flat.shadow_calls;
                            let calls = match list {
                                L::Solid => &flat.calls[..flat.solid_calls],
                                L::Translucent => flat.translucent_calls(),
                                L::ShadowSolid => &flat.calls[shadow..shadow_translucent],
                                L::ShadowTranslucent => &flat.calls[shadow_translucent..],
                            };
                            match geometry {
                                Some((instances, layout)) => {
                                    let indices =
                                        self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                                            label: Some("Shader pack geometry indices"),
                                            layout,
                                            entries: &[wgpu::BindGroupEntry {
                                                binding: 0,
                                                resource: flat.indices.as_entire_binding(),
                                            }],
                                        });
                                    rp.set_bind_group(5, &indices, &[]);
                                    let vertices =
                                        flat_vertices(&self.arena, self.translucent_arena.as_ref());
                                    flat.draw_geometry(rp, calls, vertices, page, instances);
                                }
                                None => flat.draw_with(
                                    rp,
                                    calls,
                                    flat_vertices(&self.arena, self.translucent_arena.as_ref()),
                                    page,
                                ),
                            }
                        }
                        // A geometry stage reads flat draws' indices; without them it draws nothing.
                        None if geometry.is_some() => {}
                        None => {
                            rp.set_index_buffer(self.indices.slice(..), wgpu::IndexFormat::Uint32);
                            rp.set_vertex_buffer(1, self.instances.slice(..));
                            let (first, count) = match list {
                                L::Solid => (0, solid_draws),
                                L::Translucent => (solid_draws, translucent_draws),
                                L::ShadowSolid => (shadow_first, shadow_solid),
                                L::ShadowTranslucent => {
                                    (shadow_first + shadow_solid, shadow_translucent)
                                }
                            };
                            self.draw_sections(rp, &self.indirect, 0, first, count);
                        }
                    }
                };
            // Both strokes of a high contrast outline, the black one first.
            post.set_lines(pack_draws_outline.then(|| pack_post::Lines {
                edges: self.outline.clone(),
                count: outline_edges as u32 * if frame.outline_high_contrast { 2 } else { 1 },
            }));
            post.set_clouds(pack_draws_clouds.then(|| pack_post::Clouds {
                vertices: self.cloud_buffer.clone(),
                group: self.cloud_group.clone(),
                indices: self.indices.clone(),
                quads: cloud_quads,
            }));
            post.run(
                &self.device,
                &self.queue,
                &mut encoder,
                color,
                depth,
                &view_target,
                frame,
                &frame.pack,
                size,
                [self.config.width, self.config.height],
                pages,
                &mut terrain,
            );
            // Keep the capacity, not the buffers: a page the arena frees must not stay alive
            // here until the next shader-pack frame (or forever once the pack is off).
            self.pack_pages.clear();
            self.pack_post = Some((dimension, Some(post)));
        } else if let Some((color, depth)) = &scaled
            && native_post
            && let Some(native) = &mut self.native
            && let Some(post) = &mut native.post
        {
            let size = self.world_post.size().unwrap_or([1, 1]);
            let history_reset = native.frame.take_history_reset();
            post.run(
                &self.device,
                &mut encoder,
                (color, depth, &native.frame.surface_view),
                &view_target,
                size,
                history_reset,
                native.timing.as_mut(),
            );
        } else if let (Some((_, generated)), Some((_, depth))) = (&fg_frames, &scaled) {
            let world = self
                .frame_gen
                .world(&self.device, &self.world_post, &self.config);
            self.world_post
                .finish(&self.queue, &mut encoder, &world, self.own_timing.as_mut());
            let camera = frame_gen::Camera {
                jittered,
                view_proj,
                depth: depth.clone(),
                render_size: self.world_post.size().unwrap_or(size),
                jitter: self.world_post.jitter_pixels(),
                reproject: self.frame_gen.camera_motion(view_proj, frame.camera),
                near: NEAR_PLANE,
                far: frame.depth_far,
                fov_y: frame.fov_degrees.to_radians(),
            };
            let generated_view = generated.create_view(&wgpu::TextureViewDescriptor {
                format: Some(self.config.format.remove_srgb_suffix()),
                ..Default::default()
            });
            fg_made = self.frame_gen.generate(
                &self.device,
                &self.queue,
                &mut encoder,
                &self.world_post,
                &view_target,
                &generated_view,
                &camera,
            );
            if fg_made {
                let overlay = Overlay {
                    hand_quads: hand_quads * u32::from(!pack_draws_hand),
                    first_hand_vertex: made_hand_vertex,
                    crosshair,
                    first_crosshair: debug_depth + debug_top,
                    world_depth: false,
                    real: false,
                    hand_drawn: !pack_draws_hand,
                    hand_view_proj,
                };
                self.draw_overlay(&mut encoder, frame, &generated_view, generated, &overlay);
            }
        } else if scaled.is_some() {
            self.world_post.finish(
                &self.queue,
                &mut encoder,
                &view_target,
                self.own_timing.as_mut(),
            );
        }
        if !frame.title {
            let (depth, depth_size) = match &scaled {
                Some((_, depth)) => (depth, self.world_post.size().unwrap_or(size)),
                None => (&self.depth, size),
            };
            native_render::native_world_drawn(&mut native_render::WorldDrawn {
                device: &self.device,
                queue: &self.queue,
                encoder: &mut encoder,
                color: &target_texture,
                color_view: &view_target,
                color_format: self.output.frame.format,
                size,
                depth,
                depth_size,
                view_proj,
                jittered,
                camera: frame.camera,
                entities: frame.entities,
                spans: frame.entity_spans,
                motion: frame.entity_motion,
                atlas: self.hud.atlas_texture.as_ref(),
                quad_indices: &self.indices,
                block_sprites: native_render::BlockSpriteArrays {
                    views: &self.sprite_views,
                    split: self.sprite_split,
                    size: self.sprite_textures[0].width(),
                    nearest: &self.sprite_nearest,
                    filtered: &self.sprite_sampler,
                },
            });
        }
        // A 0.2 pack in SDR: its picture finishes on 8 bits before the hand, the camera overlays
        // and the HUD draw over it (`overlay_format`).
        let finished = (self.output.frame.overlay_format != self.output.frame.format)
            .then(|| {
                let image = surface_frame.as_ref().map(|f| &f.texture);
                image.or(self.offscreen.as_ref())
            })
            .flatten()
            .and_then(|image| {
                let screenshot = self.screenshot.is_some();
                self.readback
                    .finish(&self.device, &mut encoder, image, screenshot)
            });
        let (target_texture, view_target, overlay_format) = match finished {
            Some(texture) => {
                let view = texture.create_view(&wgpu::TextureViewDescriptor {
                    format: Some(self.output.frame.overlay_format),
                    ..Default::default()
                });
                (texture, view, self.output.frame.overlay_format)
            }
            None => (target_texture, view_target, self.output.frame.format),
        };
        let overlay = Overlay {
            hand_quads: hand_quads * u32::from(!pack_draws_hand && !hand_in_world),
            first_hand_vertex: ((world_quads + additive_quads) * 4) as i32,
            crosshair,
            first_crosshair: debug_depth + debug_top,
            world_depth: scaled.is_none(),
            real: true,
            hand_drawn: !pack_draws_hand,
            hand_view_proj,
        };
        let readback =
            self.draw_overlay(&mut encoder, frame, &view_target, &target_texture, &overlay);
        if culling && let Some(cull) = &mut self.cull {
            cull.end(&self.device, &mut encoder);
        }
        // GPU culling's counts once its world passes are done (phase 2's too).
        if gpu_frame && let Some(gpu) = &mut self.gpu_culling {
            gpu.copy_stats(&mut encoder, serial, cpu);
        }
        if let Some(timer) = &mut self.gpu_timer {
            timer.end(&self.device, &mut encoder, (&view_target, overlay_format));
        }
        laps.lap("r_passes");
        // The clipped picture for offscreen SDR presentation. Layered screenshots reconstruct
        // their SDR picture in take_readback without changing this or the HDR display targets.
        let sdr_texture = self
            .hdr
            .as_ref()
            .and_then(|h| h.hud())
            .cloned()
            .unwrap_or_else(|| target_texture.clone());
        // Under `HORIZON_FRAMEGEN_DEBUG=generated` a screenshot is the made frame.
        let shown_texture = match &fg_frames {
            Some((_, generated)) if fg_made && frame_gen::generated_only() => generated,
            _ => &sdr_texture,
        };
        let readback = readback.or_else(|| {
            self.screenshot
                .take()
                .and_then(|request| self.take_readback(request, &mut encoder, shown_texture))
        });
        self.uploads.finish(&encoder);
        if has_section_copies {
            self.section_copy_bytes = 0;
            self.section_uploads.finish(&encoder);
        }
        if let Some(image) = surface_frame
            .as_ref()
            .map(|f| &f.texture)
            .or(self.offscreen.as_ref())
        {
            self.readback.present(&self.device, &mut encoder, image);
        }
        // A 0.2 pack is shown at paper white: HDR output lifts nothing of its world.
        let lifts_world = !frame.title
            && self.output.frame.format != LINEAR_FRAME
            && self
                .native
                .as_ref()
                .is_none_or(|n| n.frame.api == horizon_shader_api::Api::V0_1);
        match (&self.hdr, &surface_frame) {
            (Some(hdr), Some(f)) => hdr.present(
                &self.queue,
                &mut encoder,
                &f.texture,
                lifts_world,
                self.own_timing.as_mut(),
            ),
            // Offscreen nothing shows HDR: the target gets the frame as SDR shows it, for
            // splitscreen's compositor.
            (Some(_), None) => {
                if let Some(image) = &self.offscreen {
                    self.readback
                        .draw_onto(&self.device, &mut encoder, &sdr_texture, image);
                }
            }
            (None, _) => {}
        }
        // With HDR on, a full-frame screenshot gets an HDR10 copy beside it.
        let hdr_readback = readback
            .as_ref()
            .filter(|(request, _)| request.size.is_none())
            .zip(self.hdr.as_ref())
            .and_then(|(_, hdr)| {
                let texture = hdr.capture(&self.device, &self.queue, &mut encoder, lifts_world)?;
                let size = [self.config.width, self.config.height];
                Some(copy_frame(&self.device, &mut encoder, &texture, size))
            });
        let first_frame = self.first_frame == FirstFrame::Submit;
        // Finished, submitted and presented on the submit thread when nothing after the
        // submission needs this one: screenshots' readbacks, the error scopes (one per
        // thread), the first frame's startup log, VSync's frame callback, a reconfigure.
        // Frame generation presents its own frames (`frame_gen.present` below).
        // A `screenshot-sequence` frame is submitted here, so it can wait for it to
        // finish.
        let (encoder, surface_frame, after) = if readback.is_none()
            && frame.pack.frame_timing.is_none()
            && fg_frames.is_none()
            && hdr_readback.is_none()
            && checks.is_none()
            && !first_frame
            && !reconfigure
            && self.hdr_marker.is_none()
            && (surface_frame.is_none() || self.config.present_mode != wgpu::PresentMode::Fifo)
        {
            let offscreen = surface_frame.is_none();
            // Occlusion culling's results, GPU culling's statistics and a shader pack's centre
            // depth are read back as soon as the frame is submitted, as here.
            let culling = self.after_submit();
            let center = match &mut self.pack_post {
                Some((_, Some(post))) => post.submitted(),
                _ => None,
            };
            let after = (culling.is_some() || center.is_some()).then(|| {
                Box::new(move || {
                    if let Some(read) = culling {
                        read();
                    }
                    if let Some(read) = center {
                        read();
                    }
                }) as submit::AfterSubmit
            });
            let job = submit::Job {
                encoder,
                surface: surface_frame,
                after,
            };
            match self.queue.submit_later(job) {
                Ok(()) => {
                    self.deferred = Some(offscreen);
                    // The frame's waits for the previous one on the submit thread.
                    self.last_wait += self.queue.take_waited();
                    self.last_encode = started.elapsed().saturating_sub(self.last_wait);
                    self.visible = visible;
                    self.visible_lods = visible_lods;
                    self.shadow_visible = shadow_visible;
                    return Ok(true);
                }
                Err(back) => (back.encoder, back.surface, back.after),
            }
        } else {
            (encoder, surface_frame, None)
        };
        let commands = crate::spans::time("encode", || encoder.finish());
        if first_frame {
            startup_step(format_args!("submitting the first frame"));
            self.first_frame = FirstFrame::Drawn;
        }
        let submission = crate::spans::time("submit", || self.queue.submit([commands]));
        if let Some(read) = after.or_else(|| self.after_submit()) {
            read();
        }
        if let Some(timer) = &mut self.gpu_timer {
            timer.submitted(&self.queue);
            timer.collect(&self.device, &mut self.gpu_times);
        }
        if let Some(Some(timer)) = Self::pass_timer(&mut self.pack_post, &mut self.native) {
            timer.submitted(&self.queue);
        }
        if let Some(timer) = &mut self.own_timing {
            timer.submitted(&self.queue);
        }
        if let Some((_, Some(post))) = &mut self.pack_post
            && let Some(read) = post.submitted()
        {
            read();
        }
        let failed = checks.and_then(GpuChecks::pop);
        if let Some((request, buffer)) = readback {
            // Out of memory on the way: the PNG would be black; taken again at half the size.
            if let Some(e) = failed {
                log::warn!(
                    "Horizon renderer: a {}x{} screenshot failed ({e}); trying half the size",
                    self.config.width,
                    self.config.height
                );
                self.screenshot_halvings += 1;
                self.screenshot = Some(request);
            } else {
                self.screenshot_halvings = 0;
                self.screenshot_hdr = None;
                if let Some(hdr_buffer) = hdr_readback {
                    let hdr_path = hdr_screenshot_path(&request.path);
                    match self.write_hdr_screenshot(&hdr_path, &hdr_buffer) {
                        Ok(()) => {
                            log::info!("Saved HDR screenshot to {}", hdr_path.display());
                            self.screenshot_hdr = Some(hdr_path);
                        }
                        Err(e) => log::warn!(
                            "Horizon renderer: HDR screenshot {}: {e}",
                            hdr_path.display()
                        ),
                    }
                }
                self.screenshot_result = Some(
                    self.write_screenshot(&request, &buffer)
                        .map_err(|e| e.to_string()),
                );
            }
        }
        // The frame's own waits for the submit thread (its queue writes and passes).
        self.last_wait += self.queue.take_waited();
        let present = Instant::now();
        if let Some(surface_frame) = surface_frame {
            // On Wayland this ties RedrawRequested to the compositor's frame
            // callbacks, one per refresh. Only VSync (Fifo) waits for the
            // display: with it off, vanilla's window draws uncapped.
            if self.config.present_mode == wgpu::PresentMode::Fifo {
                self.window.pre_present_notify();
            }
            if first_frame {
                startup_step(format_args!("presenting the first frame"));
            }
            self.queue.present(surface_frame);
            if self.hdr.is_some()
                && let Some(marker) = self.hdr_marker.take()
            {
                let _ = std::fs::remove_file(marker);
                log::info!("Horizon renderer: HDR output presented its first frame");
            }
            if reconfigure {
                self.configure_surface();
            }
        } else if let Some((real, generated)) = fg_frames {
            self.frame_gen.present(real, generated, fg_made);
        }
        if self.surface.is_none() {
            // No swapchain paces offscreen frames: keep two on the GPU at most.
            self.in_flight.push_back(submission);
            if !self.paced_elsewhere {
                self.wait_in_flight();
            }
        }
        self.last_wait += present.elapsed();
        // Inside the present's time already.
        self.queue.take_waited();
        self.last_encode = started.elapsed().saturating_sub(self.last_wait);
        let _ = self.started;
        self.visible = visible;
        self.visible_lods = visible_lods;
        self.shadow_visible = shadow_visible;
        Ok(true)
    }
}
impl Renderer {
    /// What draws over the world, on the real frame and frame generation's made one:
    /// the hand, entity outlines, post effects, screen effects, the debug crosshair and
    /// the HUD. Returns the world icon's readback, taken on the real frame.
    fn draw_overlay(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        frame: &Frame,
        view_target: &wgpu::TextureView,
        target_texture: &wgpu::Texture,
        overlay: &Overlay,
    ) -> Option<(ScreenshotRequest, wgpu::Buffer)> {
        // Horizon's own pass timings (#5887) time the real frame's overlay only: a made frame's
        // would count the same passes twice.
        let mut timing = if overlay.real {
            self.own_timing.take()
        } else {
            None
        };
        // HDR output's layered frame keeps the world after the hand and the camera overlays,
        // as light (`Hdr::layered`); else it is kept here, before them.
        let layered = self.hdr.as_ref().is_some_and(|h| h.layered());
        if let Some(hdr) = &self.hdr {
            if layered {
                hdr.begin(&self.queue, !frame.title);
            } else {
                hdr.keep_world(encoder, timing.as_mut());
            }
        }
        if overlay.hand_quads > 0 {
            let attachments = [Some(wgpu::RenderPassColorAttachment {
                view: view_target,
                resolve_target: None,
                depth_slice: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Load,
                    store: wgpu::StoreOp::Store,
                },
            })];
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("First-person hand"),
                timestamp_writes: timing.as_mut().and_then(|t| t.writes("Hand")),
                color_attachments: &attachments,
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &self.depth,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(0.0),
                        // HDR output's hand marks test against it (below).
                        store: if self.hdr.is_some() {
                            wgpu::StoreOp::Store
                        } else {
                            wgpu::StoreOp::Discard
                        },
                    }),
                    stencil_ops: None,
                }),
                ..Default::default()
            });
            pass.set_pipeline(self.overlay_hand.as_ref().unwrap_or(&self.entity_pipeline));
            pass.set_bind_group(0, &self.hand_group, &[]);
            pass.set_index_buffer(self.indices.slice(..), wgpu::IndexFormat::Uint32);
            pass.set_vertex_buffer(0, self.entity_buffer.slice(..));
            pass.draw_indexed(0..overlay.hand_quads * 6, overlay.first_hand_vertex, 0..1);
            drop(pass);
            if let Some(hdr) = &self.hdr
                && !no_pack_hdr_gain(layered, self.shader_pack_kind().is_some(), !frame.title)
            {
                hdr.mark_hand(encoder, timing.as_mut());
                // The non-layered 0.2 hand keeps its UI-brightness sentinel, not light marks.
                if light_marks(self.output.frame.format, true)
                    && let Some(view) = hdr.hand_light_view()
                {
                    // The light sources in the hand may shine, the rest of it stays at the
                    // world's brightness: nothing the world kept here shows through it.
                    let attachments = [Some(wgpu::RenderPassColorAttachment {
                        view: &view,
                        resolve_target: None,
                        depth_slice: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Load,
                            store: wgpu::StoreOp::Store,
                        },
                    })];
                    let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                        label: Some("HDR hand marks"),
                        timestamp_writes: timing.as_mut().and_then(|t| t.writes("HDR hand marks")),
                        color_attachments: &attachments,
                        depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                            view: &self.depth,
                            depth_ops: Some(wgpu::Operations {
                                load: wgpu::LoadOp::Load,
                                store: wgpu::StoreOp::Discard,
                            }),
                            stencil_ops: None,
                        }),
                        ..Default::default()
                    });
                    pass.set_bind_group(0, &self.hand_group, &[]);
                    pass.set_index_buffer(self.indices.slice(..), wgpu::IndexFormat::Uint32);
                    pass.set_vertex_buffer(0, self.entity_buffer.slice(..));
                    let quads = 0..overlay.hand_quads * 6;
                    pass.set_pipeline(&self.hdr_hand_unmark_pipeline);
                    pass.draw_indexed(quads.clone(), overlay.first_hand_vertex, 0..1);
                    pass.set_pipeline(&self.hdr_entity_mark_pipeline);
                    pass.draw_indexed(quads, overlay.first_hand_vertex, 0..1);
                }
            }
        }
        // A native mod's turn after the hand, on the real frame only.
        if overlay.real && !frame.title {
            native_render::native_hand_drawn(&mut native_render::HandDrawn {
                device: &self.device,
                queue: &self.queue,
                encoder: &mut *encoder,
                color: target_texture,
                color_view: view_target,
                color_format: self.output.frame.overlay_format,
                size: [self.config.width, self.config.height],
                hand: if overlay.hand_drawn { frame.hand } else { &[] },
                view_proj: overlay.hand_view_proj,
                camera: frame.camera,
                atlas: self.hud.atlas_texture.as_ref(),
                quad_indices: &self.indices,
            });
        }
        // GameRenderer.render: LevelRenderer.blitEntityOutline after the level and the hand.
        self.entity_outline.draw(
            encoder,
            view_target,
            &self.entity_group,
            &self.indices,
            &self.entity_buffer,
            timing.as_mut(),
        );
        // The world icon grabs here: after the level and the hand, before
        // the effects and the HUD draw over them (`tryTakeScreenshotIfNeeded`
        // runs right after `renderLevel`).
        let readback = self
            .screenshot
            .take_if(|request| overlay.real && request.size.is_some())
            .and_then(|request| self.take_readback(request, encoder, target_texture));
        // GameRenderer.render: applyPostEffects, after the level and before the GUI.
        if !frame.title
            && !frame.post_effects.effects.is_empty()
            && target_texture
                .usage()
                .contains(wgpu::TextureUsages::COPY_SRC)
        {
            let device = &self.device;
            let post = self
                .post_effects
                .get_or_insert_with(|| crate::post_chain::PostEffects::new(device));
            // The chains a spectated mob applies compile in the background from the
            // first frame in a world, not in the frame that first needs one.
            post.preload(
                &self.device,
                &self.queue,
                frame.post_effects.root,
                &crate::post_chain::SPECTATOR_CHAINS,
                target_texture.format(),
            );
            post.apply(
                &self.device,
                &self.queue,
                encoder,
                target_texture,
                view_target,
                [self.config.width, self.config.height],
                // The scaled world pass drew its own depth buffer.
                overlay.world_depth.then_some(&self.depth),
                &frame.post_effects,
                timing.as_mut(),
            );
        }
        // The layered frame's light eases into the peak, before the camera overlays draw on it,
        // unless a 0.2 pack's own `[tonemap]` did.
        let pack_tonemap = self
            .native
            .as_ref()
            .and_then(|n| n.post.as_ref())
            .is_some_and(native_post::NativePost::tonemaps);
        if horizon_tone_curve(layered, !frame.title, pack_tonemap)
            && let Some(hdr) = &self.hdr
        {
            hdr.tonemap(encoder, timing.as_mut());
        }
        if !frame.screen.is_empty() {
            let attachments = [Some(wgpu::RenderPassColorAttachment {
                view: view_target,
                resolve_target: None,
                depth_slice: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Load,
                    store: wgpu::StoreOp::Store,
                },
            })];
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Screen effects"),
                timestamp_writes: timing.as_mut().and_then(|t| t.writes("Screen effects")),
                color_attachments: &attachments,
                ..Default::default()
            });
            pass.set_bind_group(0, &self.entity_group, &[]);
            pass.set_index_buffer(self.indices.slice(..), wgpu::IndexFormat::Uint32);
            pass.set_vertex_buffer(0, self.screen_buffer.slice(..));
            for (i, quad) in frame.screen.iter().enumerate() {
                let blend = match quad.blend {
                    crate::screen_effects::Blend::Alpha => 0,
                    crate::screen_effects::Blend::Vignette => 4,
                };
                let tint = if quad.world_tint && self.hdr.is_some() {
                    8
                } else {
                    0
                };
                pass.set_pipeline(
                    &self.screen_pipelines[tint + blend + (quad.sampling & 3) as usize],
                );
                pass.draw_indexed(0..6, (i * 4) as i32, 0..1);
            }
            drop(pass);
            // A tint leaves the frame's alpha and goes over the kept world too: light sources
            // under water still show the world, and still shine.
            if let Some(view) = self.hdr.as_ref().and_then(|h| h.hand_light_view())
                && frame.screen.iter().any(|quad| quad.world_tint)
            {
                let attachments = [Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    },
                })];
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("Screen tints over the kept world"),
                    timestamp_writes: None,
                    color_attachments: &attachments,
                    ..Default::default()
                });
                pass.set_bind_group(0, &self.entity_group, &[]);
                pass.set_index_buffer(self.indices.slice(..), wgpu::IndexFormat::Uint32);
                pass.set_vertex_buffer(0, self.screen_buffer.slice(..));
                for (i, quad) in frame.screen.iter().enumerate() {
                    if quad.world_tint {
                        pass.set_pipeline(&self.screen_pipelines[8 + (quad.sampling & 3) as usize]);
                        pass.draw_indexed(0..6, (i * 4) as i32, 0..1);
                    }
                }
            }
        }
        if layered && let Some(hdr) = &self.hdr {
            hdr.keep_world(encoder, timing.as_mut());
        }
        // What the HUD and menus draw on: the layered frame's HUD target, else the frame.
        let hud = self
            .hdr
            .as_ref()
            .and_then(|h| h.hud())
            .map(|t| t.create_view(&Default::default()));
        let hud_target = hud.as_ref().unwrap_or(view_target);
        let coverage = self.hdr.as_ref().and_then(|h| h.coverage_view());
        if let Some(view) = &coverage {
            hud_coverage::clear(encoder, view);
        }
        // GameRenderer.render3dHud: the debug crosshair after the hand and
        // the screen effects.
        if overlay.crosshair > 0 {
            let mut attachments = vec![Some(wgpu::RenderPassColorAttachment {
                view: hud_target,
                resolve_target: None,
                depth_slice: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Load,
                    store: wgpu::StoreOp::Store,
                },
            })];
            if let Some(view) = &coverage {
                attachments.push(Some(hud_coverage::attachment(view)));
            }
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("3d crosshair"),
                timestamp_writes: timing.as_mut().and_then(|t| t.writes("Crosshair")),
                color_attachments: &attachments,
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &self.depth,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(0.0),
                        store: wgpu::StoreOp::Discard,
                    }),
                    stencil_ops: None,
                }),
                ..Default::default()
            });
            pass.set_pipeline(match &coverage {
                Some(_) => self.debug_crosshair.as_ref().unwrap(),
                None => self
                    .overlay_crosshair
                    .as_ref()
                    .unwrap_or(&self.debug_lines_on_top),
            });
            pass.set_bind_group(0, &self.crosshair_group, &[]);
            pass.set_vertex_buffer(0, self.debug_line_buffer.slice(..));
            let first = overlay.first_crosshair;
            pass.draw(0..4, first..first + overlay.crosshair);
        }
        let menu_blur = frame
            .menu_blur
            .filter(|_| self.config.usage.contains(wgpu::TextureUsages::COPY_SRC));
        let blur_at = menu_blur.map_or(0, |_| frame.hud.blur_at);
        self.hud.draw_range(
            encoder,
            hud_target,
            coverage.as_ref(),
            &self.depth,
            frame.hud,
            [self.config.width, self.config.height],
            0..blur_at,
            timing.as_mut(),
        );
        if let Some(radius) = menu_blur {
            let format = self.output.frame.overlay_format;
            let device = &self.device;
            let software = is_software(self.info.device_type);
            let blur = self
                .menu_blur
                .get_or_insert_with(|| crate::menu_blur::MenuBlur::new(device, format, software));
            let size = [self.config.width, self.config.height];
            match self.hdr.as_ref().filter(|_| layered) {
                // The blur keeps light above white: it blurs the picture so far, the HUD over
                // the world's light, which becomes the world the menu draws over.
                Some(hdr) => {
                    let (_, source) = blur.source(device, size);
                    hdr.composite(encoder, &source, timing.as_mut());
                    blur.blur(
                        device,
                        &self.queue,
                        encoder,
                        view_target,
                        size,
                        radius,
                        timing.as_mut(),
                    );
                    hdr.keep_world(encoder, timing.as_mut());
                }
                None => blur.apply(
                    device,
                    &self.queue,
                    encoder,
                    target_texture,
                    view_target,
                    size,
                    radius,
                    timing.as_mut(),
                ),
            }
            if let Some(view) = &coverage {
                hud_coverage::clear(encoder, view);
            }
        } else if let Some(blur) = &mut self.menu_blur {
            blur.release();
        }
        self.hud.draw_range(
            encoder,
            hud_target,
            coverage.as_ref(),
            &self.depth,
            frame.hud,
            [self.config.width, self.config.height],
            blur_at..frame.hud.vertices.len() as u32,
            timing.as_mut(),
        );
        if overlay.real {
            self.own_timing = timing;
        }
        readback
    }
}
/// Out-of-memory and validation error scopes, popped together: the first error either caught.
struct GpuChecks {
    memory: wgpu::ErrorScopeGuard,
    validation: wgpu::ErrorScopeGuard,
}
impl GpuChecks {
    fn push(device: &wgpu::Device) -> Self {
        let memory = device.push_error_scope(wgpu::ErrorFilter::OutOfMemory);
        let validation = device.push_error_scope(wgpu::ErrorFilter::Validation);
        Self { memory, validation }
    }
    fn pop(self) -> Option<wgpu::Error> {
        let validation = pollster::block_on(self.validation.pop());
        let memory = pollster::block_on(self.memory.pop());
        memory.or(validation)
    }
}
/// How many times the window's size a scaled screenshot is: the Resolution Scale
/// `world_scale` when above 1, times `quality`, as far as a side fits in
/// `max_side` pixels and the readback in `max_buffer` bytes.
fn scaled_screenshot_factor(
    [width, height]: [u32; 2],
    world_scale: f32,
    quality: u32,
    max_side: u32,
    max_buffer: u64,
) -> f32 {
    let wanted = world_scale.max(1.0) * quality.max(1) as f32;
    let (w, h) = (f64::from(width.max(1)), f64::from(height.max(1)));
    let side = f64::from(max_side) / w.max(h);
    // padded_row(w * f) * h * f <= (4 * w * f + 256) * h * f <= max_buffer.
    let (a, b) = (4.0 * w * h, 256.0 * h);
    let buffer = ((b * b + 4.0 * a * max_buffer as f64).sqrt() - b) / (2.0 * a);
    wanted.min(side.min(buffer) as f32)
}
/// Bytes per row of a `width`-pixel frame read back at 4 bytes a pixel.
fn padded_row(width: u32) -> u32 {
    (width * 4).next_multiple_of(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT)
}
/// Encodes the copy of `texture`'s `size` pixels into a mappable buffer.
pub(crate) fn copy_frame(
    device: &wgpu::Device,
    encoder: &mut wgpu::CommandEncoder,
    texture: &wgpu::Texture,
    [width, height]: [u32; 2],
) -> wgpu::Buffer {
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("Horizon screenshot"),
        size: u64::from(padded_row(width)) * u64::from(height),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    encoder.copy_texture_to_buffer(
        texture.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(padded_row(width)),
                rows_per_image: None,
            },
        },
        wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
    );
    buffer
}
impl Renderer {
    /// The frame's readbacks to start right after its submission (on the submit thread when
    /// the frame goes there): occlusion culling's results (`cull`) and GPU culling's
    /// statistics. GPU frames leave occlusion culling off, so a frame has one at most.
    fn after_submit(&mut self) -> Option<submit::AfterSubmit> {
        let cull = self.cull.as_mut().and_then(|c| c.submitted());
        let gpu = self.gpu_culling.as_mut().and_then(|g| g.submitted());
        match (cull, gpu) {
            (Some(c), Some(g)) => Some(Box::new(move || {
                c();
                g();
            })),
            (Some(c), None) => Some(Box::new(c)),
            (None, Some(g)) => Some(Box::new(g)),
            (None, None) => None,
        }
    }
    /// Frame `frame`'s statistics shown, unless a newer frame's are: a CPU frame's as it is
    /// drawn, a GPU frame's when its counts come back (`GpuCulling::collect`).
    fn show_stats(&mut self, frame: u64, [sections, draws, calls]: [usize; 3]) {
        if frame > self.stats_frame {
            self.stats_frame = frame;
            (
                self.drawn_sections,
                self.section_draws,
                self.section_draw_calls,
            ) = (sections, draws, calls);
        }
    }
    /// Frames between the last one drawn and the one whose statistics are shown: 0 after a
    /// CPU frame, one to three in GPU frames, while their counts are on their way back.
    pub fn stats_lag(&self) -> u64 {
        self.frame_serial - self.stats_frame
    }
    /// Take in the frame the submit thread submitted: start its readbacks and, offscreen,
    /// keep at most two frames ahead of the GPU, as a frame submitted in place does.
    fn land_frame(&mut self) {
        let Some(offscreen) = self.deferred.take() else {
            return;
        };
        let submission = self.queue.landed();
        if let Some(timer) = &mut self.gpu_timer {
            timer.submitted(&self.queue);
            timer.collect(&self.device, &mut self.gpu_times);
        }
        if let Some(Some(timer)) = Self::pass_timer(&mut self.pack_post, &mut self.native) {
            timer.submitted(&self.queue);
        }
        if offscreen && let Some(submission) = submission {
            self.in_flight.push_back(submission);
            if !self.paced_elsewhere {
                self.wait_frames();
            }
        }
    }
    /// No swapchain paces offscreen frames: keep two on the GPU at most.
    pub fn wait_in_flight(&mut self) {
        self.land_frame();
        self.wait_frames();
    }
    /// The frame on the submit thread submitted: a surface reconfigure waits for the
    /// device and breaks if a frame is submitted while it waits, and the compositor
    /// copies views after them. (The end of a frame already lands the frames of the
    /// turn before; this is the turn's own.)
    pub fn wait_submissions(&mut self) {
        self.queue.wait();
    }
    fn wait_frames(&mut self) {
        while self.in_flight.len() > 2 {
            let index = self.in_flight.pop_front();
            let _ = self.device.poll(wgpu::PollType::Wait {
                submission_index: index,
                timeout: Some(std::time::Duration::from_secs(5)),
            });
        }
    }
    /// Take a screenshot request: queue the frame's copy, or report why the
    /// frame cannot be read back.
    fn take_readback(
        &mut self,
        request: ScreenshotRequest,
        encoder: &mut wgpu::CommandEncoder,
        texture: &wgpu::Texture,
    ) -> Option<(ScreenshotRequest, wgpu::Buffer)> {
        // The copy and PNG writer both assume 8-bit pixels; a float surface would make
        // the row stride too small for the copy and abort in validation.
        if self.config.format.block_copy_size(None) == Some(4) {
            let size = [self.config.width, self.config.height];
            let format = self.config.format.remove_srgb_suffix();
            // The layered float frame maps the world to SDR, then adds the HUD as drawn.
            // Other 16-bit frames keep their clipped 8-bit copy.
            let copy = self
                .hdr
                .as_mut()
                .and_then(|hdr| {
                    hdr.sdr_screenshot(&self.device, encoder, format, request.size.is_some())
                })
                .or_else(|| {
                    (texture.format() != format && texture.format() == self.output.frame.format)
                        .then(|| {
                            self.readback
                                .unorm_copy(&self.device, encoder, texture, format)
                        })
                });
            let texture = copy.as_ref().unwrap_or(texture);
            Some((request, copy_frame(&self.device, encoder, texture, size)))
        } else {
            self.screenshot_result = Some(Err(format!(
                "screenshots need a 32-bit surface format, not {:?}",
                self.config.format
            )));
            None
        }
    }
    /// The pixels `copy_frame` read back into `buffer`, 4 bytes each, row after row.
    fn read_frame(&self, buffer: &wgpu::Buffer) -> Result<Vec<[u8; 4]>> {
        let (sender, receiver) = std::sync::mpsc::channel();
        buffer.map_async(wgpu::MapMode::Read, .., move |r| {
            let _ = sender.send(r);
        });
        self.device
            .poll(wgpu::PollType::wait_indefinitely())
            .map_err(|e| e.to_string())?;
        receiver
            .recv()
            .map_err(|e| e.to_string())?
            .map_err(|e| e.to_string())?;
        let (width, height, row) = (
            self.config.width as usize,
            self.config.height as usize,
            padded_row(self.config.width) as usize,
        );
        let mut pixels = Vec::with_capacity(width * height);
        {
            let data = buffer.get_mapped_range(..).map_err(|e| e.to_string())?;
            for y in 0..height {
                pixels.extend_from_slice(data[y * row..y * row + width * 4].as_chunks::<4>().0);
            }
        }
        buffer.unmap();
        Ok(pixels)
    }
    fn write_screenshot(
        &self,
        request: &ScreenshotRequest,
        buffer: &wgpu::Buffer,
    ) -> Result<[u32; 2]> {
        let bgra = matches!(
            self.config.format.remove_srgb_suffix(),
            wgpu::TextureFormat::Bgra8Unorm
        );
        let mut pixels = Vec::with_capacity((self.config.width * self.config.height * 4) as usize);
        for px in self.read_frame(buffer)? {
            let (r, b) = if bgra { (px[2], px[0]) } else { (px[0], px[2]) };
            pixels.extend_from_slice(&[r, px[1], b, 255]);
        }
        // NativeImage.resizeSubRectTo on takeAutoScreenshot's grab: the icon
        // is the frame's centre square averaged down to its size.
        let (width, height, pixels) = match request.size {
            Some(size) => (
                size[0],
                size[1],
                square_downscale(&pixels, self.config.width, self.config.height, size),
            ),
            None => (self.config.width, self.config.height, pixels),
        };
        let mut encoder = png::Encoder::new(
            std::io::BufWriter::new(std::fs::File::create(&request.path)?),
            width,
            height,
        );
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder.write_header()?.write_image_data(&pixels)?;
        Ok([width, height])
    }
    /// The HDR10 copy (`Hdr::capture`) as a 16-bit PNG tagged BT.2020 with PQ (cICP, PNG's
    /// third edition), with the content light levels its pixels hold (cLLI), and
    /// an ICC profile for the programs that don't read cICP (`hdr_icc`).
    fn write_hdr_screenshot(&self, path: &std::path::Path, buffer: &wgpu::Buffer) -> Result<()> {
        if self.hdr.is_none() {
            return Ok(());
        }
        let pixels = hdr10_to_rgb16(&self.read_frame(buffer)?);
        let mut info = png::Info::with_size(self.config.width, self.config.height);
        info.color_type = png::ColorType::Rgb;
        info.bit_depth = png::BitDepth::Sixteen;
        info.icc_profile = Some(hdr_icc::profile().into());
        let encoder =
            png::Encoder::with_info(std::io::BufWriter::new(std::fs::File::create(path)?), info)?;
        let mut writer = encoder.write_header()?;
        // BT.2020 primaries, PQ, RGB, full range.
        writer.write_chunk(png::chunk::cICP, &[9, 16, 0, 1])?;
        writer.write_chunk(png::chunk::cLLI, &hdr_icc::content_light_levels(&pixels))?;
        writer.write_image_data(&pixels)?;
        Ok(())
    }
}
/// `screenshots/2026-10-28_12.00.00.png` -> `screenshots/2026-10-28_12.00.00-hdr.png`.
fn hdr_screenshot_path(path: &std::path::Path) -> PathBuf {
    let stem = path.file_stem().unwrap_or_default().to_string_lossy();
    path.with_file_name(format!("{stem}-hdr.png"))
}
/// `Rgb10a2Unorm` pixels as 16-bit RGB, big-endian as PNG stores it: each 10-bit value
/// with its top bits repeated below, so 0 stays 0 and 1023 becomes 65535.
fn hdr10_to_rgb16(pixels: &[[u8; 4]]) -> Vec<u8> {
    let mut out = Vec::with_capacity(pixels.len() * 6);
    for &px in pixels {
        let v = u32::from_le_bytes(px);
        for shift in [0, 10, 20] {
            let c = (v >> shift & 0x3ff) as u16;
            out.extend_from_slice(&(c << 6 | c >> 4).to_be_bytes());
        }
    }
    out
}
/// Centre-crop the frame to a square and average it down to `size`.
pub(crate) fn square_downscale(pixels: &[u8], width: u32, height: u32, size: [u32; 2]) -> Vec<u8> {
    let (side, cx, cy) = if width > height {
        (height, (width - height) / 2, 0)
    } else {
        (width, 0, (height - width) / 2)
    };
    let [tw, th] = size;
    let mut out = vec![0u8; (tw * th * 4) as usize];
    for ty in 0..th {
        let y0 = cy + ty * side / th;
        let y1 = cy + (ty + 1) * side / th;
        for tx in 0..tw {
            let x0 = cx + tx * side / tw;
            let x1 = cx + (tx + 1) * side / tw;
            let (mut r, mut g, mut b) = (0u32, 0u32, 0u32);
            for y in y0..y1 {
                for x in x0..x1 {
                    let p = ((y * width + x) * 4) as usize;
                    r += u32::from(pixels[p]);
                    g += u32::from(pixels[p + 1]);
                    b += u32::from(pixels[p + 2]);
                }
            }
            let count = (x1 - x0) * (y1 - y0);
            let o = ((ty * tw + tx) * 4) as usize;
            out[o..o + 4].copy_from_slice(&[
                (r / count) as u8,
                (g / count) as u8,
                (b / count) as u8,
                255,
            ]);
        }
    }
    out
}
/// The union of two frusta (the camera's and its water mirror's), for `Resident::visible`.
#[derive(Clone, Copy, Debug)]
pub struct Frusta(pub Frustum, pub Option<Frustum>);
/// What `Resident::visible` culls with: a frustum or a union of them.
pub trait Cull {
    fn intersects(&self, min: Vec3, max: Vec3) -> bool;
    fn contains(&self, min: Vec3, max: Vec3) -> bool;
    /// Whether a section is tested by the box around its geometry, not its whole 16 blocks.
    fn tight(&self) -> bool {
        true
    }
}
impl Cull for Frustum {
    fn intersects(&self, min: Vec3, max: Vec3) -> bool {
        Frustum::intersects(self, min, max)
    }
    fn contains(&self, min: Vec3, max: Vec3) -> bool {
        Frustum::contains(self, min, max)
    }
}
impl Cull for Frusta {
    fn intersects(&self, min: Vec3, max: Vec3) -> bool {
        self.0.intersects(min, max) || self.1.is_some_and(|f| f.intersects(min, max))
    }
    /// Entirely inside one of them is entirely inside the union.
    fn contains(&self, min: Vec3, max: Vec3) -> bool {
        self.0.contains(min, max) || self.1.is_some_and(|f| f.contains(min, max))
    }
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
/// The block entities of `sections` whose box, a block wider on every side, comes within the
/// 64 blocks block entities are drawn to (`Renderer::visible_block_entities_in_frustum`).
fn block_entities_within_reach<'a>(
    sections: impl Iterator<Item = SectionKey> + 'a,
    block_entities: &'a FxHashMap<SectionKey, Vec<([i32; 3], u32)>>,
    camera: DVec3,
    min_y: i32,
) -> impl Iterator<Item = &'a ([i32; 3], u32)> + 'a {
    sections
        .filter_map(move |key| {
            let min = (DVec3::new(
                f64::from(key.0 * 16 - 1),
                f64::from(min_y + key.1 * 16 - 1),
                f64::from(key.2 * 16 - 1),
            ) - camera)
                .as_vec3();
            let max = min + Vec3::splat(18.0);
            let near = Vec3::ZERO.max(min).min(max).length_squared() <= 64.0 * 64.0;
            near.then(|| block_entities.get(&key)).flatten()
        })
        .flatten()
}
/// The shadow camera's box that the shadow pass draws terrain in: the sides of `view.cull`, and
/// its near and far planes only with `depth_planes`.
fn shadow_box(view: &pack_post::ShadowView) -> Frustum {
    let mut planes = frustum(view.cull);
    if !view.depth_planes {
        // `frustum`'s faces at depth 0 and 1 take in everything.
        planes[4] = Vec4::W;
        planes[5] = Vec4::W;
    }
    Frustum { planes }
}
/// Camera-relative clip planes (`Frustum`).
#[derive(Clone, Copy, Debug)]
pub struct Frustum {
    planes: [Vec4; 6],
}
impl Frustum {
    /// Whether the camera-relative box `min..max` is at least partly inside.
    pub fn intersects(&self, min: Vec3, max: Vec3) -> bool {
        !self.planes.iter().any(|p| {
            let sign = p.truncate().cmpge(Vec3::ZERO);
            let v = Vec3::select(sign, max, min);
            p.truncate().dot(v) + p.w < 0.0
        })
    }
    /// Whether a camera-relative sphere is at least partly inside. Frustum
    /// planes are normalized, so each test needs only one dot product and a
    /// radius comparison.
    pub fn intersects_sphere(&self, center: Vec3, radius: f32) -> bool {
        if !center.is_finite() || !radius.is_finite() || radius < 0.0 {
            return true;
        }
        !self
            .planes
            .iter()
            .any(|plane| plane.truncate().dot(center) + plane.w < -radius)
    }
    /// Depth clip planes alone: X/Y projection jitter cannot change this test.
    /// Retaining side-of-screen geometry makes this safe before TAA selects a phase.
    pub fn intersects_depth(&self, min: Vec3, max: Vec3) -> bool {
        if !min.is_finite() || !max.is_finite() || !min.cmple(max).all() {
            return true;
        }
        !self.planes[4..].iter().any(|p| {
            let sign = p.truncate().cmpge(Vec3::ZERO);
            let v = Vec3::select(sign, max, min);
            p.truncate().dot(v) + p.w < 0.0
        })
    }
    /// Whether the camera-relative box `min..max` is entirely inside. Then
    /// `intersects` accepts every box within it: each plane's distance only
    /// grows from this box's nearest corner to the inner box's farthest, and
    /// rounding keeps that order.
    pub fn contains(&self, min: Vec3, max: Vec3) -> bool {
        self.planes.iter().all(|p| {
            let sign = p.truncate().cmpge(Vec3::ZERO);
            let v = Vec3::select(sign, min, max);
            p.truncate().dot(v) + p.w >= 0.0
        })
    }
}
/// Vanilla's `Projection`: reversed depth (near maps to 1, far to 0, tests
/// are GREATER_THAN_OR_EQUAL), which keeps far faces a hair apart, like
/// fire_side's 0.01 pixel in front of its block, from z-fighting.
/// The section the camera is in.
fn camera_section(camera: DVec3, min_y: i32) -> SectionKey {
    (
        (camera.x.floor() as i32) >> 4,
        (camera.y.floor() as i32 - min_y) >> 4,
        (camera.z.floor() as i32) >> 4,
    )
}

/// The near plane of the world's projection, in blocks.
const NEAR_PLANE: f32 = 0.05;

/// Counts and offsets `Renderer::draw_overlay` draws from this frame's buffers.
struct Overlay {
    /// Zero when a shader pack draws the hand.
    hand_quads: u32,
    first_hand_vertex: i32,
    crosshair: u32,
    first_crosshair: u32,
    /// The world drew into `depth` (post effects may read it), not a scaled target's.
    world_depth: bool,
    /// The real frame, not frame generation's made one: the world icon grabs it.
    real: bool,
    /// The game drew the first-person hand, in its own pass or at the end of the world's
    /// (`hand_in_world`), not a shader pack.
    hand_drawn: bool,
    /// Camera-relative world to clip space for the hand: its own field of view and the bob.
    hand_view_proj: Mat4,
}

pub(crate) fn world_projection(fov_degrees: f32, aspect: f32, far: f32) -> Mat4 {
    Mat4::perspective_rh(fov_degrees.to_radians(), aspect, far, NEAR_PLANE)
}

/// `view_proj` mirrored about the horizontal plane at the camera-relative height `plane`:
/// with F the mirror (y flips and moves to 2 * plane), `view_proj * F` negates column 1
/// and moves column 3 by 2 * plane times column 1.
///
/// `mirror_inverse_view` is the inverse's other side: F * `inverse_view`.
fn mirror_view_projection(view_proj: Mat4, plane: f32) -> Mat4 {
    let mut m = view_proj.to_cols_array_2d();
    let col1 = m[1];
    m[1] = col1.map(|v| -v);
    m[3] = std::array::from_fn(|i| m[3][i] + 2.0 * plane * col1[i]);
    Mat4::from_cols_array_2d(&m)
}

/// The mirror's `inverse_view` (the sky's rays): what the view sees, mirrored about the
/// horizontal plane at the camera-relative height `plane`, so F * `inverse_view`.
fn mirror_inverse_view(inverse_view: [[f32; 4]; 4], plane: f32) -> [[f32; 4]; 4] {
    inverse_view.map(|[x, y, z, w]| [x, 2.0 * plane * w - y, z, w])
}
/// Where a perspective `view_proj` sees from, in its own (camera-relative)
/// space: the point whose clip x, y and w are all 0. `None` for a
/// projection without one (orthographic).
fn projection_center(view_proj: Mat4) -> Option<Vec3> {
    let m = view_proj.as_dmat4();
    let rows = [m.row(0), m.row(1), m.row(3)];
    let a = glam::DMat3::from_cols(
        DVec3::new(rows[0].x, rows[1].x, rows[2].x),
        DVec3::new(rows[0].y, rows[1].y, rows[2].y),
        DVec3::new(rows[0].z, rows[1].z, rows[2].z),
    );
    let det = a.determinant();
    if !det.is_normal() {
        return None;
    }
    let b = DVec3::new(rows[0].w, rows[1].w, rows[2].w);
    Some((a.inverse() * -b).as_vec3())
}
/// The runs of a section's solid quads the shadow pass draws: every face within the unculled
/// distance, or everywhere for `every_face`; else the quads that can face the light.
fn shadow_runs(
    sides: &Sides,
    view: &pack_post::ShadowView,
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

/// Calls `draw(first, quads)` for each run of a mesh's solid quads that can
/// face `eye` (`projection_center`), the mesh at the camera-relative
/// `origin`; all of them when there is no eye.
fn facing_runs(sides: &Sides, eye: Option<Vec3>, origin: [f32; 3], draw: impl FnMut(u32, u32)) {
    facing_runs_from(sides, eye, None, origin, Quads::All, draw)
}
/// `facing_runs` for the eye and, with `low`, every eye straight below it down to that
/// camera-relative height (the water mirror's eye).
fn facing_runs_from(
    sides: &Sides,
    eye: Option<Vec3>,
    low: Option<f32>,
    origin: [f32; 3],
    part: Quads,
    draw: impl FnMut(u32, u32),
) {
    match eye {
        Some(eye) => {
            let eye = eye - Vec3::from(origin);
            let low = low.map_or(eye.y, |y| (y - origin[1]).min(eye.y));
            sides.visible_from(eye.to_array(), low, part, draw)
        }
        None => sides.every(part, draw),
    }
}
/// The camera's view `view_proj` (wgpu depth, reversed or not) stretched without end toward
/// `light` (a direction): the boxes whose shadow, cast away from the light, can fall into the
/// view. Its planes are the view's faces turned toward the light, and through each edge between
/// such a face and one turned away, the plane along the light; unused ones take in everything.
fn shadow_casters(view_proj: Mat4, light: Vec3) -> [Frustum; 2] {
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
fn taa_frustum(view_proj: Mat4, rendered_size: [u32; 2]) -> Frustum {
    // TAA shifts clip x/y by at most half a rendered pixel times clip w.
    // Widen x/y before extracting the planes: this includes every such shifted
    // view, including a posed camera, without changing the near/far planes.
    let [x, y] = rendered_size.map(|size| 1.0 / (1.0 + 2.0 / size.max(1) as f32));
    Frustum {
        planes: frustum(Mat4::from_scale(Vec3::new(x, y, 1.0)) * view_proj),
    }
}

fn frustum(m: Mat4) -> [Vec4; 6] {
    let r = |i| m.row(i);
    let planes = [
        r(3) + r(0),
        r(3) - r(0),
        r(3) + r(1),
        r(3) - r(1),
        r(2),
        r(3) - r(2),
    ];
    planes.map(|p| p / p.truncate().length())
}

#[cfg(test)]
mod frame_policy_tests;

#[cfg(test)]
mod tests {
    #[test]
    fn taa_culling_covers_jittered_view_edges_but_still_rejects_behind_the_camera() {
        use super::*;

        let mut outside_unjittered = 0;
        for size in [[1, 1], [320, 180], [1280, 720], [3840, 2160]] {
            for (yaw, pitch) in [(0.0, 0.0), (73.0, -35.0), (-142.0, 79.0)] {
                let pose = Mat4::from_translation(Vec3::new(0.03, -0.02, 0.01))
                    * Mat4::from_scale(Vec3::new(1.1, 0.9, 1.0))
                    * crate::camera::view_matrix(yaw, pitch);
                let matrix = world_projection(70.0, 16.0 / 9.0, 512.0) * pose;
                let guarded = taa_frustum(matrix, size);
                let ordinary = Frustum {
                    planes: frustum(matrix),
                };
                for x in [-1.0, 1.0] {
                    for y in [-1.0, 1.0] {
                        let jitter = Mat4::from_translation(Vec3::new(
                            x / size[0] as f32,
                            y / size[1] as f32,
                            0.0,
                        ));
                        let inverse = (jitter * matrix).inverse();
                        for clip_x in [-1.0, 0.0, 1.0] {
                            for clip_y in [-1.0, 0.0, 1.0] {
                                for depth in [0.001, 0.5, 0.999] {
                                    // Independent oracle: a point on an actual jittered
                                    // viewport edge, transformed back into world space.
                                    let point = inverse * Vec4::new(clip_x, clip_y, depth, 1.0);
                                    let point = point.truncate() / point.w;
                                    assert!(guarded.intersects_sphere(point, 1.0e-4));
                                    let slack = Vec3::splat(1.0e-4);
                                    assert!(guarded.intersects(point - slack, point + slack));
                                    if !ordinary.intersects_sphere(point, 0.0) {
                                        outside_unjittered += 1;
                                    }
                                }
                            }
                        }
                    }
                }
                let behind = pose.inverse().transform_point3(Vec3::new(0.0, 0.0, 10.0));
                assert!(!guarded.intersects_sphere(behind, 0.3));
            }
        }
        assert!(
            outside_unjittered > 0,
            "the test must need the jitter guard"
        );
    }

    #[test]
    fn depth_culling_keeps_jittered_edges_and_depth_crossing_models() {
        use super::*;
        for (yaw, pitch) in [(0.0, 0.0), (73.0, -35.0), (-142.0, 79.0)] {
            let pose = Mat4::from_translation(Vec3::new(0.03, -0.02, 0.01))
                * crate::camera::view_matrix(yaw, pitch);
            let matrix = world_projection(70.0, 16.0 / 9.0, 512.0) * pose;
            let view = Frustum {
                planes: frustum(matrix),
            };
            for size in [[1, 1], [320, 180], [1280, 720], [3840, 2160]] {
                for (x, y) in [(-1.0, 1.0), (1.0, -1.0)] {
                    let jitter = Mat4::from_translation(Vec3::new(
                        x / size[0] as f32,
                        y / size[1] as f32,
                        0.0,
                    ));
                    let inverse = (jitter * matrix).inverse();
                    for (x, y) in [(-1.0, -1.0), (1.0, 1.0), (-1.0, 1.0), (1.0, -1.0)] {
                        for z in [0.001, 0.5, 0.999] {
                            let p = inverse * Vec4::new(x, y, z, 1.0);
                            let p = p.truncate() / p.w;
                            assert!(view.intersects_depth(
                                p - Vec3::splat(1.0e-4),
                                p + Vec3::splat(1.0e-4)
                            ));
                        }
                    }
                }
            }
            let point = |p| pose.inverse().transform_point3(p);
            for z in [10.0, -600.0] {
                let p = point(Vec3::new(0.0, 0.0, z));
                assert!(!view.intersects_depth(p - Vec3::splat(0.1), p + Vec3::splat(0.1)));
            }
            // An oversized model straddling the eye must stay, even with its origin behind.
            let p = point(Vec3::ZERO);
            assert!(view.intersects_depth(p - Vec3::splat(10.0), p + Vec3::splat(10.0)));
            assert!(view.intersects_depth(Vec3::splat(f32::NAN), Vec3::ONE));
        }
    }
    #[test]
    fn opaque_gizmo_lines_upload_first_for_the_depth_writing_pipeline() {
        let line = |argb: u32| [0.0, 0.0, 0.0, 1.0, 0.0, 0.0, f32::from_bits(argb), 2.5];
        // A hitbox's white, the chunk border's outer red columns (alpha 0x7F),
        // and the chunk border's yellow.
        let lines = [line(0xFFFF_FFFF), line(0x7FFF_0000), line(0xFFFF_FF00)];
        assert!(super::debug_line_opaque(&lines[0]));
        assert!(!super::debug_line_opaque(&lines[1]));
        assert!(!super::debug_line_opaque(&line(0xFE00_0000)));
        let order: Vec<u32> = super::debug_lines_opaque_first(&lines)
            .map(|l| l[6].to_bits())
            .collect();
        assert_eq!(order, [0xFFFF_FFFF, 0xFFFF_FF00, 0x7FFF_0000]);
    }
    use crate::gfx::static_entity_shader;

    fn adapter(device_type: wgpu::DeviceType) -> wgpu::AdapterInfo {
        wgpu::AdapterInfo {
            name: "test adapter".into(),
            vendor: 0,
            device: 0,
            device_type,
            device_pci_bus_id: String::new(),
            driver: String::new(),
            driver_info: String::new(),
            backend: wgpu::Backend::Vulkan,
            subgroup_min_size: 0,
            subgroup_max_size: 0,
            transient_saves_memory: None,
            limit_bucket: None,
        }
    }

    #[test]
    fn software_occlusion_culling_is_opt_in() {
        let cpu = adapter(wgpu::DeviceType::Cpu);
        let discrete = adapter(wgpu::DeviceType::DiscreteGpu);
        assert!(!super::occlusion_culling_requested(&cpu, None));
        assert!(super::occlusion_culling_requested(&discrete, None));
        assert!(super::occlusion_culling_requested(&cpu, Some("all")));
        assert!(!super::occlusion_culling_requested(&discrete, Some("0")));
    }

    /// Translucent Batching is offered with indirect multi-draws that aren't OpenGL's, flat
    /// frames (Metal, Intel Gen11/Gen12 on Vulkan) included.
    #[test]
    fn translucent_batching_is_offered_with_indirect_multi_draws_except_on_opengl() {
        let vulkan = adapter(wgpu::DeviceType::DiscreteGpu);
        let mut metal = adapter(wgpu::DeviceType::IntegratedGpu);
        metal.backend = wgpu::Backend::Metal;
        let mut gl = adapter(wgpu::DeviceType::DiscreteGpu);
        gl.backend = wgpu::Backend::Gl;
        let batchable = super::translucent_batchable;
        assert!(batchable(&vulkan, true));
        assert!(batchable(&metal, true));
        assert!(!batchable(&vulkan, false));
        assert!(!batchable(&gl, true));
    }

    /// GPU culling is offered where the device draws counts the GPU wrote and runs compute
    /// shaders with storage buffers enough, where Translucent Batching is (indirect
    /// multi-draws that aren't OpenGL's) and vanilla frames are never drawn flat, and never
    /// on Intel Gen11/Gen12's Vulkan driver, whose draws read instance-rate origins from the
    /// wrong first instance, even with flat draws off there, nor on DX12 with flat page
    /// groups, whose count draws leave the base vertex out of the pulled pipelines'
    /// `vertex_index`.
    #[test]
    fn gpu_culling_is_offered_where_counts_are_drawn_and_vanilla_never_draws_flat() {
        let vulkan = adapter(wgpu::DeviceType::DiscreteGpu);
        let mut gl = adapter(wgpu::DeviceType::DiscreteGpu);
        gl.backend = wgpu::Backend::Gl;
        let mut dx12 = adapter(wgpu::DeviceType::DiscreteGpu);
        dx12.backend = wgpu::Backend::Dx12;
        let mut intel = adapter(wgpu::DeviceType::IntegratedGpu);
        (intel.vendor, intel.device) = (0x8086, 0x9a49); // Tiger Lake
        let counts = wgpu::Features::MULTI_DRAW_INDIRECT_COUNT;
        let compute = wgpu::DownlevelFlags::COMPUTE_SHADERS;
        let storage = |buffers| wgpu::Limits {
            max_storage_buffers_per_shader_stage: buffers,
            ..Default::default()
        };
        let offered = |info, features, downlevel, buffers, multi_draw, flat| {
            super::gpu_culling::supported(
                info,
                features,
                downlevel,
                &storage(buffers),
                multi_draw,
                flat,
            )
        };
        // No flat draws, or flat draws for a shader pack's terrain alone.
        assert!(offered(&vulkan, counts, compute, 8, true, None));
        assert!(offered(&vulkan, counts, compute, 8, true, Some(usize::MAX)));
        // Vanilla frames drawn flat past MIN_SECTIONS (Metal), or all of them.
        let min = super::flat::MIN_SECTIONS;
        assert!(!offered(&vulkan, counts, compute, 8, true, Some(min)));
        assert!(!offered(&vulkan, counts, compute, 8, true, Some(0)));
        let none = wgpu::Features::empty();
        assert!(!offered(&vulkan, none, compute, 8, true, None));
        let no_compute = wgpu::DownlevelFlags::empty();
        assert!(!offered(&vulkan, counts, no_compute, 8, true, None));
        assert!(!offered(&vulkan, counts, compute, 7, true, None));
        assert!(!offered(&vulkan, counts, compute, 8, false, None));
        assert!(!offered(&gl, counts, compute, 8, true, None));
        assert!(!offered(&intel, counts, compute, 8, true, None));
        assert!(offered(&dx12, counts, compute, 8, true, None));
        assert!(!offered(&dx12, counts, compute, 8, true, Some(usize::MAX)));
        // The settings offer it there, but not on a software adapter (WARP, lavapipe),
        // whose pass would run on the CPU, and they say why not on DX12 with flat page groups.
        use crate::graphics::GpuCullingOffer as O;
        let offer = super::gpu_culling::offer;
        assert_eq!(offer(&vulkan, true, true), O::Offered);
        assert_eq!(
            offer(&adapter(wgpu::DeviceType::IntegratedGpu), true, false),
            O::Offered
        );
        assert_eq!(
            offer(&adapter(wgpu::DeviceType::Cpu), true, false),
            O::Software
        );
        assert_eq!(offer(&vulkan, false, false), O::Unsupported);
        assert_eq!(
            offer(&adapter(wgpu::DeviceType::Cpu), false, false),
            O::Unsupported
        );
        assert_eq!(offer(&dx12, true, false), O::Offered);
        assert_eq!(offer(&dx12, false, true), O::Dx12);
    }

    /// The pack-flat rule: with Translucent Batching translucent quads have an arena of
    /// their own, unless a shader pack's terrain programs or its shadow pass alone can
    /// draw flat (a pack picked and not loaded yet counts as drawing terrain).
    #[test]
    fn translucent_quads_stay_in_the_section_arena_while_a_pack_can_draw_flat() {
        // (batching, flat draws, the pack's terrain, its shadow pass)
        let split = super::translucent_split;
        assert!(split(true, true, false, false));
        assert!(!split(true, true, true, false));
        assert!(!split(true, true, false, true));
        assert!(!split(true, true, true, true));
        // Without flat draws, or with them off, the pack draws per section, from either.
        assert!(split(true, false, true, true));
        assert!(!split(false, true, false, false));
        assert!(!split(false, false, false, false));
    }

    /// No large translucent page starts to shrink until `SHRINK_SETTLE` after terrain
    /// last arrived, nor in a frame `compact_arena` copied in; terrain arriving after
    /// a try that found no room does not cut its `SHRINK_RETRY` short. Only starts
    /// wait: a move under way finishes, or its page would stay closed while the new
    /// one fills.
    #[test]
    fn shrinking_starts_once_terrain_has_settled() {
        use super::{COMPACT_BUDGET, SHRINK_RETRY, SHRINK_SETTLE, ShrinkGate};
        use std::time::{Duration, Instant};
        let (now, ms) = (Instant::now(), Duration::from_millis(1));
        let mut gate = ShrinkGate::default();
        assert!(gate.may_start(now, COMPACT_BUDGET));
        assert!(!gate.may_start(now, COMPACT_BUDGET - 1));
        gate.wait(now, SHRINK_SETTLE);
        assert!(!gate.may_start(now + SHRINK_SETTLE - ms, COMPACT_BUDGET));
        assert!(gate.may_start(now + SHRINK_SETTLE, COMPACT_BUDGET));
        gate.wait(now, SHRINK_RETRY);
        gate.wait(now + ms, SHRINK_SETTLE);
        assert!(!gate.may_start(now + SHRINK_RETRY - ms, COMPACT_BUDGET));
        assert!(gate.may_start(now + SHRINK_RETRY, COMPACT_BUDGET));
    }

    #[test]
    fn the_mirror_sees_each_point_where_its_reflection_is() {
        use glam::{Mat4, Vec3, Vec4};
        let view_proj = super::world_projection(70.0, 1.5, 512.0)
            * Mat4::look_to_rh(Vec3::ZERO, Vec3::new(0.3, -0.4, -1.0), Vec3::Y);
        let plane = -3.25;
        let mirror = super::mirror_view_projection(view_proj, plane);
        for p in [Vec3::new(1.0, 2.0, -9.0), Vec3::new(-4.0, -1.5, -20.0)] {
            let reflected = Vec3::new(p.x, 2.0 * plane - p.y, p.z);
            let a = mirror * Vec4::from((p, 1.0));
            let b = view_proj * Vec4::from((reflected, 1.0));
            assert!((a - b).abs().max_element() < 1e-4, "{a} {b}");
        }
    }
    #[test]
    fn the_mirrors_sky_rays_are_the_cameras_reflected() {
        use glam::{Mat4, Vec3, Vec4};
        // A camera looking down: its mirror looks up, by as much.
        let forward = Vec3::new(0.3, -0.64, -1.0).normalize();
        let view = Mat4::look_to_rh(Vec3::ZERO, forward, Vec3::Y);
        let mirrored = super::mirror_inverse_view(view.inverse().to_cols_array_2d(), -3.25);
        let ray = Mat4::from_cols_array_2d(&mirrored) * Vec4::new(0.0, 0.0, -1.0, 0.0);
        let reflected = Vec3::new(forward.x, -forward.y, forward.z);
        assert!((ray.truncate() - reflected).length() < 1e-5, "{ray}");
        // The view's origin, the camera, goes to the mirror's eye.
        let eye = Mat4::from_cols_array_2d(&mirrored) * Vec4::W;
        assert!(
            (eye.truncate() - Vec3::new(0.0, -6.5, 0.0)).length() < 1e-5,
            "{eye}"
        );
    }
    #[test]
    fn scaled_screenshots_follow_resolution_scale_and_quality_within_limits() {
        use super::scaled_screenshot_factor as factor;
        let big = u64::MAX / 2;
        // The window's size at 100% and 1x; Resolution Scale below 100% stays the window's.
        assert_eq!(factor([1920, 1080], 1.0, 1, 16384, big), 1.0);
        assert_eq!(factor([1920, 1080], 0.5, 1, 16384, big), 1.0);
        // The game's resolution, times the quality.
        assert_eq!(factor([1920, 1080], 2.0, 1, 16384, big), 2.0);
        assert_eq!(factor([1920, 1080], 1.5, 4, 16384, big), 6.0);
        assert_eq!(factor([1920, 1080], 0.5, 8, 16384, big), 8.0);
        // Cut to the largest texture side: 3840 * 4.266 = 16384.
        let f = factor([3840, 2160], 1.0, 8, 16384, big);
        assert!(
            (3840.0 * f) as u32 <= 16384 && (3840.0 * f) as u32 > 16300,
            "{f}"
        );
        // Cut to the largest buffer: 256 MiB holds about 3.8x of 1080p's readback.
        let max = 256 << 20;
        let f = factor([1920, 1080], 1.0, 8, 16384, max);
        let [w, h] = [1920, 1080].map(|s| (s as f32 * f) as u32);
        assert!(u64::from(super::padded_row(w)) * u64::from(h) <= max, "{f}");
        assert!(f > 3.7, "{f}");
    }
    #[test]
    fn drawable_size_skips_minimized_and_oversized_windows() {
        use super::drawable_size;
        assert!(drawable_size(1280, 720, 8192));
        assert!(drawable_size(1, 1, 8192));
        assert!(drawable_size(8192, 8192, 8192));
        // Minimized.
        assert!(!drawable_size(0, 720, 8192));
        assert!(!drawable_size(1280, 0, 8192));
        assert!(!drawable_size(0, 0, 8192));
        // One past the device's largest texture.
        assert!(!drawable_size(8193, 720, 8192));
        assert!(!drawable_size(1280, 8193, 8192));
        // The crash: a window `SetWindowPos` made 65535 pixels tall, on an RTX 5080 (32768).
        assert!(!drawable_size(984, 65496, 32768));
        assert!(!drawable_size(984, u32::MAX, 32768));
    }

    #[test]
    fn os_luminances_become_auto_values() {
        use super::os_reported;
        use crate::graphics::Reported;
        let info = |max, white| wgpu::DisplayHdrInfo {
            luminance: Some(wgpu::DisplayLuminance {
                max_nits: max,
                sdr_white_nits: white,
                ..Default::default()
            }),
            ..Default::default()
        };
        // Windows' SDR content brightness slider runs 80..480 nits.
        assert_eq!(
            os_reported(&info(Some(998.4), Some(240.0))),
            Some(Reported {
                paper_white: 240,
                peak: 998,
            })
        );
        assert_eq!(
            os_reported(&info(Some(120.0), Some(1000.0))),
            Some(Reported {
                paper_white: 480,
                peak: 480,
            })
        );
        // White above the calibrated peak (a Samsung QD-OLED's 380, the slider at 456).
        assert_eq!(
            os_reported(&info(Some(380.0), Some(456.0))),
            Some(Reported {
                paper_white: 456,
                peak: 456,
            })
        );
        assert_eq!(os_reported(&info(Some(1000.0), None)), None);
        assert_eq!(os_reported(&info(Some(f32::NAN), Some(80.0))), None);
        assert_eq!(os_reported(&wgpu::DisplayHdrInfo::default()), None);
    }
    #[test]
    fn system_hdr_changes_are_kept_without_luminance_changes() {
        let reading = |hdr| {
            super::DisplayReading::os(&wgpu::DisplayHdrInfo {
                coarse: Some(wgpu::DisplayCoarseRange {
                    high_dynamic_range: hdr,
                    ..Default::default()
                }),
                headroom: Some(wgpu::DisplayHeadroom {
                    potential: hdr.map(|on| if on { 2.0 } else { 1.0 }),
                    ..Default::default()
                }),
                ..Default::default()
            })
        };
        let off = reading(Some(false));
        let on = reading(Some(true));
        let unknown = reading(None);
        assert_eq!(on.hdr, Some(true));
        assert_eq!(off.hdr, Some(false));
        assert_eq!(unknown.hdr, None);
        assert_eq!(on.luminance, None); // macOS reports EDR headroom, not absolute nits.
        assert_ne!(on, off);
        assert_ne!(off, unknown);
    }
    #[test]
    fn macos_hdr_uses_potential_instead_of_brightness_dependent_current() {
        let info = |current, potential| wgpu::DisplayHdrInfo {
            headroom: Some(wgpu::DisplayHeadroom {
                current: Some(current),
                potential,
                ..Default::default()
            }),
            coarse: Some(wgpu::DisplayCoarseRange {
                high_dynamic_range: Some(current > 1.0),
                ..Default::default()
            }),
            ..Default::default()
        };
        for current in [1.0, 2.0, 4.0] {
            assert_eq!(super::macos_hdr(&info(current, Some(2.0))), Some(true));
            assert_eq!(super::macos_hdr(&info(current, Some(1.0))), Some(false));
        }
        for potential in [
            None,
            Some(0.0),
            Some(-1.0),
            Some(f32::NAN),
            Some(f32::INFINITY),
        ] {
            assert_eq!(super::macos_hdr(&info(4.0, potential)), None);
        }
    }
    #[test]
    fn surface_is_8_bit_and_not_srgb() {
        use super::surface_format;
        use wgpu::TextureFormat::{Bgra8Unorm, Bgra8UnormSrgb, Rgba8Unorm, Rgba16Float};
        // RADV's Wayland surfaces list the linear fp16 format first; choosing it makes the
        // compositor read gamma-encoded values as linear light.
        assert_eq!(
            surface_format(&[Rgba16Float, Bgra8UnormSrgb, Bgra8Unorm], Bgra8UnormSrgb),
            Bgra8Unorm
        );
        assert_eq!(
            surface_format(&[Bgra8UnormSrgb, Rgba8Unorm], Bgra8UnormSrgb),
            Rgba8Unorm
        );
        assert_eq!(surface_format(&[Rgba16Float], Bgra8UnormSrgb), Rgba16Float);
    }
    /// A Wayland surface on a compositor with colour management, as wgpu lists it (Mesa 26.2
    /// ANV on an Intel HD 620 under weston 16): wide formats first. The pre-#1075 choice,
    /// the first non-sRGB format, took `Rgba16Float` and the window looked washed out, as if
    /// under a white overlay (#1007); XWayland lists no such format, which is why it looked fine.
    #[test]
    fn a_colour_managed_wayland_surface_still_gets_8_bit() {
        use super::surface_format;
        use wgpu::TextureFormat::*;
        let wayland = [
            Rgba16Float,
            Rgba16Unorm,
            Rgb10a2Unorm,
            Bgra8UnormSrgb,
            Bgra8Unorm,
            Rgba8UnormSrgb,
            Rgba8Unorm,
        ];
        assert_eq!(surface_format(&wayland, Bgra8UnormSrgb), Bgra8Unorm);
        // Without Bgra8: Rgba8, still not a 16- or 10-bit format.
        let without_bgra = [
            Rgba16Float,
            Rgba16Unorm,
            Rgb10a2Unorm,
            Rgba8UnormSrgb,
            Rgba8Unorm,
        ];
        assert_eq!(surface_format(&without_bgra, Rgba8UnormSrgb), Rgba8Unorm);
    }
    /// The 16-bit frame only with HDR on, Smooth Gradients on, an HDR surface and a device
    /// that draws it; in every other case the swapchain's 8-bit format, as before it existed.
    #[test]
    fn the_frame_is_16_bit_only_with_hdr_and_a_device_that_draws_it() {
        use super::frame_format;
        use wgpu::TextureFormat::{Bgra8Unorm, Bgra8UnormSrgb, Rgba8Unorm, Rgba16Unorm};
        // No float frame asked for (or a device without it) leaves it as before.
        for linear in [
            (false, false, false),
            (false, false, true),
            (true, false, false),
            (false, true, false),
            (true, true, false),
        ] {
            assert_eq!(
                frame_format(true, true, true, Bgra8Unorm, linear),
                Rgba16Unorm
            );
            for (wide_frame, hdr_offered, draws) in [
                (false, true, true),
                (true, false, true),
                (true, true, false),
                (false, false, false),
            ] {
                assert_eq!(
                    frame_format(wide_frame, hdr_offered, draws, Bgra8UnormSrgb, linear),
                    Bgra8Unorm
                );
                assert_eq!(
                    frame_format(wide_frame, hdr_offered, draws, Rgba8Unorm, linear),
                    Rgba8Unorm
                );
            }
        }
    }
    /// A shader API 0.2 pack picked under `HORIZON_HDR_FRAME=float` gets the float frame
    /// wherever the device draws it, HDR on or off.
    #[test]
    fn a_0_2_pack_gets_the_float_frame_where_the_device_draws_it() {
        use super::{LINEAR_FRAME, frame_format};
        use wgpu::TextureFormat::Bgra8UnormSrgb;
        for (wide_frame, hdr_offered, draws) in [(true, true, true), (false, false, false)] {
            assert_eq!(
                frame_format(
                    wide_frame,
                    hdr_offered,
                    draws,
                    Bgra8UnormSrgb,
                    (true, false, true)
                ),
                LINEAR_FRAME
            );
        }
    }
    /// HDR on without a pack under `HORIZON_HDR_FRAME=float` gets the float frame where the
    /// surface offers HDR output, Smooth Gradients or not; where it doesn't, the frame is as
    /// without the switch.
    #[test]
    fn hdr_without_a_pack_gets_the_float_frame_where_hdr_output_is_offered() {
        use super::{LINEAR_FRAME, frame_format};
        use wgpu::TextureFormat::{Bgra8Unorm, Bgra8UnormSrgb};
        for (wide_frame, draws) in [(true, true), (false, false)] {
            assert_eq!(
                frame_format(wide_frame, true, draws, Bgra8UnormSrgb, (false, true, true)),
                LINEAR_FRAME
            );
            assert_eq!(
                frame_format(
                    wide_frame,
                    false,
                    draws,
                    Bgra8UnormSrgb,
                    (false, true, true)
                ),
                Bgra8Unorm
            );
        }
    }
    /// What `draws_wide_frame` needs of a device: both features, and the usages and flags
    /// every pass takes of the frame. Missing any one of them keeps the 8-bit frame.
    #[test]
    fn the_16_bit_frame_needs_every_usage_and_flag_the_passes_take() {
        use super::draws_wide_frame;
        use wgpu::{Features as F, TextureFormatFeatureFlags as Fl, TextureUsages as U};
        let features = F::TEXTURE_FORMAT_16BIT_NORM | F::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES;
        let usages = U::RENDER_ATTACHMENT | U::TEXTURE_BINDING | U::COPY_SRC | U::COPY_DST;
        let flags = Fl::BLENDABLE | Fl::FILTERABLE;
        let format = |allowed_usages, flags| wgpu::TextureFormatFeatures {
            allowed_usages,
            flags,
        };
        // A desktop GPU: all of it, and more.
        assert!(draws_wide_frame(
            features | F::FLOAT32_FILTERABLE,
            format(usages | U::STORAGE_BINDING, flags | Fl::MULTISAMPLE_X4),
        ));
        assert!(!draws_wide_frame(
            F::TEXTURE_FORMAT_16BIT_NORM,
            format(usages, flags)
        ));
        assert!(!draws_wide_frame(
            F::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES,
            format(usages, flags)
        ));
        // A downlevel device reports no usages at all for a format it lacks.
        assert!(!draws_wide_frame(
            F::empty(),
            format(U::empty(), Fl::empty())
        ));
        for usage in [
            U::RENDER_ATTACHMENT,
            U::TEXTURE_BINDING,
            U::COPY_SRC,
            U::COPY_DST,
        ] {
            assert!(!draws_wide_frame(features, format(usages - usage, flags)));
        }
        for flag in [Fl::BLENDABLE, Fl::FILTERABLE] {
            assert!(!draws_wide_frame(features, format(usages, flags - flag)));
        }
    }
    #[test]
    fn special_material_batches_preserve_quad_order_and_offsets() {
        let quad = |layer, id| {
            [super::Vertex {
                pos: [0.0; 3],
                uv: [0.0; 2],
                layer,
                color: 0xffff_ffff,
                light: id,
                normal: [0; 4],
                glint_uv: [0.0, 0.0],
            }; 4]
        };
        let entities: Vec<_> = [
            (0, 0),
            (14, 9),
            (4, 1),
            (11, 2),
            (15, 10),
            (7, 3),
            (35, 11),
            (4, 4),
            (12, 7),
        ]
        .into_iter()
        .flat_map(|(layer, id)| quad(layer, id))
        .collect();
        let mut batch = super::EntityBatch::default();
        let counts = batch.update(&entities, &quad(3, 5), &quad(0, 6), &quad(0, 8), false);
        let vertices = &batch.combined;
        assert_eq!(counts, [4, 1, 1, 1, 0, 0, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0]);
        assert_eq!(
            vertices
                .as_chunks::<4>()
                .0
                .iter()
                .map(|q| q[0].light)
                .collect::<Vec<_>>(),
            [0, 1, 3, 4, 11, 2, 7, 9, 10, 5, 6, 8]
        );
    }
    #[test]
    fn software_adapters_are_last_resort() {
        use super::is_software;
        assert!(is_software(wgpu::DeviceType::Cpu));
        assert!(!is_software(wgpu::DeviceType::DiscreteGpu));
        assert!(!is_software(wgpu::DeviceType::IntegratedGpu));
        assert!(!is_software(wgpu::DeviceType::VirtualGpu));
    }
    #[test]
    fn auto_uses_directx_12_first_on_windows_arm64() {
        use super::{GraphicsApi, backend_order};
        use wgpu::Backends;
        let order = |api, os, arch| backend_order(api, os, arch, false);
        assert_eq!(
            order(GraphicsApi::Auto, "windows", "x86_64"),
            [Backends::VULKAN, Backends::DX12, Backends::GL]
        );
        assert_eq!(
            order(GraphicsApi::Auto, "windows", "aarch64"),
            [Backends::DX12, Backends::VULKAN, Backends::GL]
        );
        assert_eq!(
            order(GraphicsApi::Vulkan, "windows", "aarch64"),
            [Backends::VULKAN]
        );
        assert_eq!(
            order(GraphicsApi::Dx12, "windows", "aarch64"),
            [Backends::DX12]
        );
        assert_eq!(
            order(GraphicsApi::OpenGl, "windows", "aarch64"),
            [Backends::GL]
        );
        assert_eq!(
            order(GraphicsApi::Auto, "linux", "x86_64"),
            [Backends::VULKAN, Backends::GL]
        );
        assert_eq!(
            order(GraphicsApi::Dx12, "linux", "x86_64"),
            [Backends::VULKAN]
        );
        assert_eq!(
            order(GraphicsApi::OpenGl, "linux", "x86_64"),
            [Backends::GL]
        );
        // WGPU_BACKEND picks the backends itself; macOS has Metal only.
        for api in GraphicsApi::ALL {
            assert_eq!(
                backend_order(api, "windows", "aarch64", true),
                [Backends::default()]
            );
            assert_eq!(order(api, "macos", "aarch64"), [Backends::default()]);
        }
    }
    #[test]
    fn a_chosen_api_without_a_gpu_tries_the_others_before_software() {
        use super::{GraphicsApi, device_search};
        use wgpu::Backends;
        let (v, d, g) = (Backends::VULKAN, Backends::DX12, Backends::GL);
        let search = |api, os, arch| device_search(api, os, arch, false);
        // #5185: DirectX 12 chosen on an Intel HD 2500 (WARP only): OpenGL's GPU before WARP.
        assert_eq!(
            search(GraphicsApi::Dx12, "windows", "x86_64"),
            [(vec![d], false), (vec![g], false), (vec![d], true)]
        );
        assert_eq!(
            search(GraphicsApi::Vulkan, "windows", "x86_64"),
            [(vec![v], false), (vec![d, g], false), (vec![v], true)]
        );
        assert_eq!(
            search(GraphicsApi::Auto, "windows", "x86_64"),
            [(vec![v, d, g], false), (vec![v, d, g], true)]
        );
        assert_eq!(
            search(GraphicsApi::Auto, "windows", "aarch64"),
            [(vec![d, v, g], false), (vec![d, v, g], true)]
        );
        assert_eq!(
            search(GraphicsApi::OpenGl, "linux", "x86_64"),
            [(vec![g], false), (vec![v], false), (vec![g], true)]
        );
        // WGPU_BACKEND and macOS: one set of backends, nothing else to try.
        let all = Backends::default();
        for (os, env) in [("windows", true), ("macos", false)] {
            assert_eq!(
                device_search(GraphicsApi::Vulkan, os, "x86_64", env),
                [(vec![all], false), (vec![all], true)]
            );
        }
    }
    /// The second try at a backend shows the adapters wgpu hides for reporting
    /// no Vulkan conformance version (Mesa's hasvk, which drives Ivy Bridge and
    /// Haswell, reports 0.0.0.0 and works).
    #[test]
    fn the_second_try_at_a_backend_shows_nonconformant_adapters() {
        let shown = |allow| {
            super::instance_descriptor(wgpu::Backends::VULKAN, None, allow, wgpu::Dx12Compiler::Fxc)
                .flags
                .contains(wgpu::InstanceFlags::ALLOW_UNDERLYING_NONCOMPLIANT_ADAPTER)
        };
        assert!(shown(true));
        // The first try keeps wgpu's own filtering, unless `with_env` turns the
        // flag on from the environment (WGPU_ALLOW_UNDERLYING_NONCOMPLIANT_ADAPTER).
        if std::env::var_os("WGPU_ALLOW_UNDERLYING_NONCOMPLIANT_ADAPTER").is_none() {
            assert!(!shown(false));
        }
    }
    /// Every WGSL shader file translates to HLSL as wgpu's DirectX 12 backend
    /// writes it for FXC (shader model 5.1), entry point by entry point, and
    /// every shader the OpenGL backend runs also translates to GLSL.
    #[test]
    fn shaders_translate_to_hlsl_for_directx_12_and_glsl_for_opengl() {
        fn wgsl_files(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
            for entry in std::fs::read_dir(dir).unwrap() {
                let path = entry.unwrap().path();
                if path.is_dir() {
                    wgsl_files(&path, out);
                } else if path.extension().is_some_and(|e| e == "wgsl") {
                    out.push(path);
                }
            }
        }
        let mut files = Vec::new();
        wgsl_files(
            std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/src")),
            &mut files,
        );
        assert!(files.len() >= 10, "{files:?}");
        // Improved transparency's files are appended to terrain.wgsl and entity.wgsl.
        let mut sources: Vec<(std::path::PathBuf, String)> = Vec::new();
        for path in files {
            let text = std::fs::read_to_string(&path).unwrap();
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if name == "oit.wgsl" || name == "gpu_occlusion.wgsl" || name == "hdr_light.wgsl" {
                // The shared maths: it needs the file it is appended to. GPU culling's
                // occlusion test is appended to gpu_culling.wgsl, on Vulkan alone.
                continue;
            } else if name == "terrain.wgsl"
                || name == "entity.wgsl"
                || name == "stars.wgsl"
                || name == "custom_sky.wgsl"
                || name == "border.wgsl"
                || name == "world_primitives.wgsl"
            {
                let base = include_str!("hdr_light.wgsl").to_owned() + &text;
                sources.push((path, crate::overlay::wgsl_constants() + &base));
            } else if name == "hdr.wgsl" {
                sources.push((path, super::hdr::shader()));
            } else if name == "oit_world.wgsl" {
                let base = super::terrain_shader() + include_str!("oit.wgsl");
                sources.push((path, base + &text));
            } else if name == "oit_features.wgsl" {
                let base = super::entity_shader() + include_str!("oit.wgsl");
                sources.push((path, base + &text));
            } else if name == "static_entity.wgsl" {
                sources.push((path, static_entity_shader()));
            } else if name == "visbuffer.wgsl" {
                // Appended to terrain.wgsl (gfx/visbuffer.rs).
                sources.push((path, super::terrain_shader() + &text));
            } else if name == "oit_border.wgsl" {
                let base = super::border_shader() + include_str!("oit.wgsl");
                sources.push((path, base + &text));
            } else if name == "oit_clouds.wgsl" {
                let base = super::entity_shader() + include_str!("oit.wgsl");
                sources.push((path, base + &text));
            } else if name == "world_warp_fragment.wgsl" {
                // A world warp's copy of terrain.wgsl (gfx/world_warp.rs), with the plainest warp.
                let warp = super::world_warp::IDENTITY;
                sources.push((path, super::world_warp::terrain_shader(warp).unwrap()));
            } else if name == "world_warp.wgsl" {
                // And of entity.wgsl with static_entity.wgsl.
                let warp = super::world_warp::IDENTITY;
                sources.push((path, super::world_warp::entity_shader(warp).unwrap()));
            } else {
                sources.push((path, text));
            }
        }
        // The default override specializes written light away. Translate its active form too.
        let written: Vec<_> = sources
            .iter()
            .filter(|(path, _)| {
                matches!(
                    path.file_name().and_then(|n| n.to_str()),
                    Some("terrain.wgsl" | "entity.wgsl" | "stars.wgsl" | "custom_sky.wgsl")
                )
            })
            .map(|(path, source)| {
                (
                    path.clone(),
                    source.replace(
                        "override HDR_WRITE_LIGHT: bool = false;",
                        "override HDR_WRITE_LIGHT: bool = true;",
                    ),
                )
            })
            .collect();
        sources.extend(written);
        let mut glsl_checked = 0;
        // Shaders the OpenGL backend runs must translate: skipping the ones
        // that fail hides exactly the bug where a shader GL needs cannot
        // compile (#9448).
        let mut glsl_failed = Vec::new();
        let mut translated = std::collections::HashSet::new();
        // naga's GLSL writer gives an image one sampler, and the HUD's glint
        // samples one image through two; one failed entry does not hide the rest.
        let known_unsupported = [("hud.wgsl", "fs_cit_glint")];
        for (path, text) in sources {
            // entity.wgsl comes after the overlay's constants (entity_shader).
            let module = naga::front::wgsl::parse_str(&text)
                .or_else(|_| {
                    naga::front::wgsl::parse_str(&(crate::overlay::wgsl_constants() + &text))
                })
                .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
            let info = naga::valid::Validator::new(
                naga::valid::ValidationFlags::all(),
                naga::valid::Capabilities::all(),
            )
            .validate(&module)
            .unwrap_or_else(|e| panic!("{}: {e:?}", path.display()));
            // A select between integer vectors on a vector of bools becomes GLSL's
            // mix(uvec3, uvec3, bvec3) (GLSL 4.5 or EXT_shader_integer_mix). Old AMD OpenGL
            // drivers (Radeon HD 7400M, driver 15.301) pick the float overload and refuse to
            // compile the terrain shader (#9892). Scalar selects become `?:` and work everywhere.
            let functions = module
                .functions
                .iter()
                .map(|(handle, f)| (f, &info[handle]))
                .chain(
                    module
                        .entry_points
                        .iter()
                        .enumerate()
                        .map(|(i, e)| (&e.function, info.get_entry_point(i))),
                );
            for (function, function_info) in functions {
                for (handle, expression) in function.expressions.iter() {
                    let naga::Expression::Select {
                        condition, accept, ..
                    } = *expression
                    else {
                        continue;
                    };
                    let vector_of = |h: naga::Handle<naga::Expression>| match *function_info[h]
                        .ty
                        .inner_with(&module.types)
                    {
                        naga::TypeInner::Vector { scalar, .. } => Some(scalar.kind),
                        _ => None,
                    };
                    assert!(
                        vector_of(condition).is_none()
                            || !matches!(
                                vector_of(accept),
                                Some(naga::ScalarKind::Uint | naga::ScalarKind::Sint)
                            ),
                        "{} {:?}: select between integer vectors ({handle:?}): use scalar selects",
                        path.display(),
                        function.name
                    );
                }
            }
            for entry in &module.entry_points {
                let (module, info) = naga::back::pipeline_constants::process_overrides(
                    &module,
                    &info,
                    Some((entry.stage, &entry.name)),
                    &Default::default(),
                )
                .unwrap();
                let mut hlsl = String::new();
                naga::back::hlsl::Writer::new(
                    &mut hlsl,
                    &naga::back::hlsl::Options::default(),
                    &naga::back::hlsl::PipelineOptions {
                        entry_point: Some((entry.stage, entry.name.clone())),
                    },
                )
                .write(&module, &info, None)
                .unwrap_or_else(|e| panic!("{} {}: {e}", path.display(), entry.name));
                assert!(!hlsl.is_empty());
                // OpenGL below 4.3 lacks ARB_arrays_of_arrays (#6267): no nested array may reach it.
                // Compute and storage-buffer shaders (culling, frame generation) never run on GL.
                if entry.stage == naga::ShaderStage::Compute
                    || text.contains("var<storage")
                    || text.contains("var<workgroup>")
                {
                    continue;
                }
                for version in [330, 400] {
                    let mut glsl = String::new();
                    let glsl_result = naga::back::glsl::Writer::new(
                        &mut glsl,
                        &module,
                        &info,
                        &naga::back::glsl::Options {
                            version: naga::back::glsl::Version::Desktop(version),
                            ..Default::default()
                        },
                        &naga::back::glsl::PipelineOptions {
                            shader_stage: entry.stage,
                            entry_point: entry.name.clone(),
                            multiview: None,
                        },
                        Default::default(),
                    )
                    .and_then(|mut w| w.write());
                    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
                    if let Err(e) = glsl_result {
                        if !known_unsupported.contains(&(name, entry.name.as_str())) {
                            glsl_failed.push(format!(
                                "{} {} (GLSL {version}): {e}",
                                path.display(),
                                entry.name
                            ));
                        }
                        continue;
                    }
                    translated.insert((name.to_owned(), entry.name.clone()));
                    glsl_checked += 1;
                    assert!(
                        !glsl.contains("ARB_arrays_of_arrays"),
                        "{} {} needs ARB_arrays_of_arrays on GLSL {version}",
                        path.display(),
                        entry.name
                    );
                }
            }
        }
        assert!(
            glsl_failed.is_empty(),
            "shaders the OpenGL backend runs must translate:\n{}",
            glsl_failed.join("\n")
        );
        // These read the world's depth through `textureLoad`; before #9448 their
        // depth binding could not translate, so TAA and the blit never came up.
        for (file, entry) in [("world_post.wgsl", "fs_taa"), ("pack_post.wgsl", "fs_blit")] {
            assert!(
                translated.contains(&(file.to_owned(), entry.to_owned())),
                "{file} {entry} must translate to GLSL"
            );
        }
        assert!(glsl_checked >= 10, "{glsl_checked} GLSL translations");
    }
    #[test]
    fn gl_sprite_arrays_are_neither_2d_textures_nor_cube_maps() {
        for (count, gl) in [(1, 2), (2, 2), (5, 5), (6, 7), (12, 13), (2048, 2048)] {
            assert_eq!(super::array_layers(count, true), gl);
            assert_eq!(super::array_layers(count, false), count);
        }
    }
    #[test]
    fn plain_solid_shaders_translate_for_every_backend() {
        // `SolidPipeline::plain` builds terrain.wgsl's solid entries with `HDR_LIGHTING`
        // off on every backend: they must still translate (the other test translates them
        // with the constants' defaults).
        let module = naga::front::wgsl::parse_str(&super::terrain_shader()).unwrap();
        let info = naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::all(),
        )
        .validate(&module)
        .unwrap();
        let stage = naga::ShaderStage::Fragment;
        let constants = [("HDR_LIGHTING".to_owned(), 0.0)].into_iter().collect();
        for entry in ["fs_solid", "fs_opaque"] {
            let (module, info) = naga::back::pipeline_constants::process_overrides(
                &module,
                &info,
                Some((stage, entry)),
                &constants,
            )
            .unwrap();
            let msl = naga::back::msl::write_string(
                &module,
                &info,
                &naga::back::msl::Options {
                    fake_missing_bindings: true,
                    ..Default::default()
                },
                &naga::back::msl::PipelineOptions {
                    entry_point: Some((stage, entry.to_owned())),
                    ..Default::default()
                },
            );
            // Metal compiles the lighting out from this constant.
            assert!(
                msl.unwrap()
                    .0
                    .contains("constant bool HDR_LIGHTING = false;"),
                "{entry}"
            );
            let mut hlsl = String::new();
            naga::back::hlsl::Writer::new(
                &mut hlsl,
                &Default::default(),
                &naga::back::hlsl::PipelineOptions {
                    entry_point: Some((stage, entry.to_owned())),
                },
            )
            .write(&module, &info, None)
            .unwrap_or_else(|e| panic!("{entry}: {e}"));
            let mut glsl = String::new();
            naga::back::glsl::Writer::new(
                &mut glsl,
                &module,
                &info,
                &naga::back::glsl::Options {
                    version: naga::back::glsl::Version::Desktop(330),
                    ..Default::default()
                },
                &naga::back::glsl::PipelineOptions {
                    shader_stage: stage,
                    entry_point: entry.to_owned(),
                    multiview: None,
                },
                Default::default(),
            )
            .and_then(|mut w| w.write())
            .unwrap_or_else(|e| panic!("{entry}: {e}"));
        }
    }
    #[test]
    fn entity_shader_validates_with_its_overlay_constants() {
        let module = naga::front::wgsl::parse_str(&super::entity_shader()).unwrap();
        naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::all(),
        )
        .validate(&module)
        .unwrap();
    }
    #[test]
    fn chunk_fade_is_vanillas_get_visibility() {
        use super::{chunk_visibility, fade_millis};
        assert_eq!(fade_millis(0.75), 750);
        assert_eq!(fade_millis(0.0), 0);
        assert_eq!(chunk_visibility(0, 750), 0.0_f32);
        assert_eq!(chunk_visibility(375, 750), 0.5);
        assert_eq!(chunk_visibility(750, 750), 1.0);
        assert_eq!(chunk_visibility(0, 0), 1.0, "Off: whole at once");
        assert_eq!(chunk_visibility(u64::MAX, 750), 1.0);
    }
    /// Iris 1.11.7's terrain `gl_Vertex`, read back by a probe pack of ours, is each block's
    /// position minus the camera with its fraction rounded to 1/65536 block: these are the
    /// fractions (in 65536ths) it implied, for eyes at y 69.62 and 82.92, at x 100.3 and
    /// 40000.3, at z -13.7 and 20000.7, and off the grid by a fraction of a 65536th.
    #[test]
    fn a_shader_pack_s_terrain_is_drawn_from_the_camera_in_65536ths() {
        let eye = |x: f64, feet: f64, z: f64| glam::DVec3::new(x, feet + f64::from(1.62f32), z);
        let cases = [
            (eye(13.5, 68.0, -13.5), [32768.0, 40632.0, 32768.0]),
            (eye(13.5, 81.3, -13.5), [32768.0, 60293.0, 32768.0]),
            (eye(100.3, 90.0, -13.7), [19661.0, 40632.0, 19661.0]),
            (eye(40000.3, 120.0, 20000.7), [19661.0, 40632.0, 45875.0]),
            (
                eye(
                    13.0 + 0.7 / 8192.0,
                    81.0 + 0.3 / 8192.0,
                    -13.0 - 0.6 / 8192.0,
                ),
                [6.0, 40635.0, 65531.0],
            ),
        ];
        for (camera, sixty_fifths) in cases {
            let from = super::pack_terrain_camera(camera);
            let fraction = (from - camera.floor()) * 65536.0;
            assert_eq!(fraction.to_array(), sixty_fifths, "{camera}");
        }
    }
    #[test]
    fn section_instance_carries_the_fade_in_the_step() {
        use super::section_instance;
        let step = 1.0 / crate::mesher::UV_STEPS;
        // Fully visible (the option Off) is exactly the record from before the fade.
        assert_eq!(
            section_instance([1.0, 2.0, 3.0], 0, 1.0),
            [1.0, 2.0, 3.0, step]
        );
        for visibility in [0.0_f32, 0.25, 0.5, 0.999] {
            let w = section_instance([0.0; 3], 0, visibility)[3];
            let bits = w.to_bits();
            assert_eq!(f32::from_bits(bits & 0xff80_0000), step);
            let read = 1.0 - (bits & 0x7f_ffff) as f32 / 8_388_608.0;
            assert!((read - visibility).abs() < 1e-6, "{visibility}: {read}");
        }
        // A section fading in over far terrain keeps its dither code unchanged.
        assert_eq!(section_instance([0.0; 3], 7, 0.5)[3], -(7.0 + step));
    }
    /// A section as `Renderer::upload_mesh` makes one, with `solid` and
    /// `translucent` quads, fading in over far terrain since `fading`.
    fn section(
        solid: u32,
        translucent: u32,
        fading: Option<std::time::Instant>,
    ) -> super::SectionGpu {
        super::SectionGpu {
            page: 0,
            start: 0,
            translucent_at: (0, u64::from(solid) * 4),
            solid,
            solid_slots: u64::from(solid) * 4,
            packed_far: false,
            packed: false,
            sides: crate::mesher::Sides::unsorted(solid),
            translucent,
            sort: super::translucency::SortState::new(Vec::new()),
            sorted_indices: None,
            flat_id: None,
            gpu_id: None,
            gpu_fading: false,
            fading,
            leaving: None,
            animated_layers: Box::new([]),
        }
    }
    /// A section's fade (`SectionGpu::drawn_faded`) only ends as time goes on,
    /// never while it leaves or is held, and by its `fade_end`, at most 10 ms
    /// after it has (a fade-in over far terrain is whole 9.4 ms before
    /// `lod::FADE`): fading in over far terrain or not, first uploaded or not,
    /// both, at every chunk fade-in time, in 1 ms steps over 3 s and at its end.
    #[test]
    fn a_fade_ends_by_its_deadline_and_never_comes_back() {
        use super::FadeInputs;
        use crate::fast_hash::{FxHashMap, FxHashSet};
        use std::time::{Duration, Instant};
        let ms = Duration::from_millis;
        // The steps start at `zero`; what began `t` ms before it began off the steps.
        let zero = Instant::now() + Duration::from_secs(3);
        let ago = |t: u64| zero - ms(t) + Duration::from_micros(370);
        let key = (3, 5, -2);
        // Neither leaving nor held, leaving, held, both.
        let states = [(false, false), (true, false), (false, true), (true, true)];
        for fading in [None, Some(0), Some(300), Some(590), Some(700)] {
            for up in [None, Some(0), Some(1), Some(740), Some(2_500)] {
                let uploaded: FxHashMap<_, _> = up.map(|t| (key, ago(t))).into_iter().collect();
                for (leaving, held) in states {
                    let mut s = section(1, 0, fading.map(ago));
                    s.leaving = leaving.then(|| ago(250));
                    let retiring: FxHashSet<_> = held.then_some(key).into_iter().collect();
                    for fade_ms in [0, 1, 50, 300, 750, 2_000] {
                        let case = (fading, up, leaving, held, fade_ms);
                        let at = |now: Instant| FadeInputs {
                            now,
                            fade_ms,
                            uploaded: &uploaded,
                            retiring: &retiring,
                        };
                        let faded = |now: Instant| s.drawn_faded(key, at(now));
                        let end = s.fade_end(key, at(zero));
                        if leaving || held {
                            assert!(end.is_none(), "{case:?}: an end");
                        }
                        let mut was = true;
                        for t in 0..=3_000 {
                            let now = zero + ms(t);
                            let is = faded(now);
                            assert!(was || !is, "{case:?}: faded again at {t} ms");
                            was = is;
                            if leaving || held {
                                assert!(is, "{case:?}: whole at {t} ms");
                            } else if end.is_none_or(|end| now >= end) {
                                assert!(!is, "{case:?}: faded at {t} ms, past its end");
                            }
                        }
                        if let Some(end) = end {
                            assert!(!faded(end), "{case:?}: faded at its end");
                            assert!(faded(end - ms(10)), "{case:?}: whole 10 ms before it");
                        }
                    }
                }
            }
        }
    }
    /// GPU culling's fading sections over 5 s of 16 ms frames, with no device
    /// (`mark_fades`): some 2,000 sections come, fading in as vanilla does or
    /// over far terrain, are meshed again, leave over far terrain, are held while
    /// it comes in and let go, and go, while the chunk fade-in time goes 750 ms,
    /// 2 s, Off and 750 ms. After every frame the GPU keeps only whole sections,
    /// the walk lists every other one, each record's flag is its section's, and
    /// a flagged section that neither leaves nor is held still faded a frame
    /// before. Each kind of event flags sections, and thousands go back. Once
    /// GPU culling is switched off (`forget_records`), every mark is the one a
    /// section has while it is off.
    #[test]
    fn fading_sections_go_to_the_cpu_and_come_back() {
        use super::{
            FadeInputs, SectionGpu, SectionKey, decide_fade, forget_records, gpu_culling,
            hold_fading, mark_fades,
        };
        use crate::fast_hash::{FxHashMap, FxHashSet};
        use std::time::{Duration, Instant};
        /// The level's sections: those at y -1 and 14 have no record.
        const ROWS: usize = 14;
        const FRAME: Duration = Duration::from_millis(16);
        /// The renderer's sections and what their fades depend on, and GPU
        /// culling's records and ends.
        #[derive(Default)]
        struct Sim {
            sections: super::resident::Resident<SectionGpu>,
            records: gpu_culling::Records,
            fades: gpu_culling::Fades,
            uploaded: FxHashMap<SectionKey, Instant>,
            retiring: FxHashSet<SectionKey>,
        }
        impl Sim {
            /// `Renderer::upload` at `at`: a new section, fading in over far
            /// terrain if `far`, or one meshed again.
            fn upload(&mut self, key: SectionKey, quads: (u32, u32), far: bool, at: Instant) {
                self.retiring.remove(&key);
                let fading = match self.sections.find(key) {
                    Some(old) => old.fading,
                    None => far.then_some(at),
                };
                if fading.is_none() {
                    self.uploaded.entry(key).or_insert(at);
                }
                self.release(key);
                let mut s = section(quads.0, quads.1, fading);
                if s.gpu_recorded(key, ROWS) {
                    if let Some(fade_ms) = self.fades.fade_ms {
                        let f = FadeInputs {
                            now: at,
                            fade_ms,
                            uploaded: &self.uploaded,
                            retiring: &self.retiring,
                        };
                        s.gpu_fading = decide_fade(&s, key, &mut self.fades, f);
                    }
                    s.gpu_id = Some(self.records.insert(s.record(key, None)));
                }
                let marked = s.listed(key, ROWS);
                self.sections.insert_marked(key, s, None, marked);
            }
            /// `Renderer::release_section`.
            fn release(&mut self, key: SectionKey) {
                if let Some(id) = self.sections.remove(key).and_then(|s| s.gpu_id) {
                    self.records.remove(id);
                }
            }
            /// `Renderer::leave_section`: its column gone (`remove_columns`), or
            /// no longer left out by far terrain while held (`set_lod_cutout`).
            fn leave(&mut self, key: SectionKey, at: Instant) {
                self.uploaded.remove(&key);
                if let Some(s) = self.sections.find_mut(key)
                    && s.leaving.is_none()
                {
                    s.leaving = Some(at);
                    hold_fading(&mut self.sections, &mut self.records, key);
                }
            }
            /// `Renderer::remove_columns`' held section: far terrain comes in under it.
            fn hold(&mut self, key: SectionKey) {
                self.uploaded.remove(&key);
                self.retiring.insert(key);
                hold_fading(&mut self.sections, &mut self.records, key);
            }
            fn flagged(&self, key: SectionKey) -> bool {
                self.sections.find(key).is_some_and(|s| s.gpu_fading)
            }
        }
        let mut seed = 0x2545_f491_u32;
        let mut next = move || {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            seed
        };
        // 16 x 16 columns of sections at y -1 to 14.
        let key = |r: u32| {
            let [x, y, z] = [0, 4, 8].map(|shift| ((r >> shift) % 16) as i32);
            (x, y - 1, z)
        };
        // Solid quads alone, with translucent ones, or translucent ones alone.
        let kinds = [(64, 0), (64, 0), (64, 0), (64, 16), (64, 16), (0, 16)];
        let quads = |r: u32| kinds[r as usize % 6];
        let start = Instant::now();
        let mut sim = Sim::default();
        for r in 0..4_096 {
            if next().is_multiple_of(2) {
                sim.upload(key(r), quads(next()), next().is_multiple_of(2), start);
            }
        }
        // Flags set by an upload, `leave_section`, a held section and a fade-in
        // time (GPU culling switched on in the first frame), and dropped.
        let (mut set_by, mut handed_back) = ([0; 4], 0);
        let mut last = start;
        for frame in 0..=312 {
            let now = start + FRAME * frame;
            for event in 0..12 {
                let at = last + (now - last) * (event + 1) / 13;
                let r = next();
                let k = key(r);
                let was = sim.flagged(k);
                let kind = match (r >> 20) % 16 {
                    0..=7 => {
                        sim.upload(k, quads(r >> 12), (r >> 24).is_multiple_of(2), at);
                        0
                    }
                    8 | 9 => {
                        sim.leave(k, at);
                        1
                    }
                    10 | 11 => {
                        sim.hold(k);
                        2
                    }
                    12 => {
                        // Far terrain no longer leaves a held section's column out.
                        let held = sim.retiring.iter().next().copied();
                        if let Some(held) = held {
                            sim.retiring.remove(&held);
                            sim.leave(held, at);
                        }
                        continue;
                    }
                    _ => {
                        sim.uploaded.remove(&k);
                        sim.release(k);
                        continue;
                    }
                };
                if !was && sim.flagged(k) {
                    set_by[kind] += 1;
                }
            }
            // Those faded out go (`release_left`).
            let gone = |t: Instant| now.saturating_duration_since(t) >= super::lod::FADE;
            let left: Vec<SectionKey> = sim
                .sections
                .iter()
                .filter(|(_, s, _)| s.leaving.is_some_and(gone))
                .map(|(k, ..)| k)
                .collect();
            for k in left {
                sim.release(k);
            }
            let flagged_before: FxHashSet<SectionKey> = sim
                .sections
                .iter()
                .filter(|(_, s, _)| s.gpu_fading)
                .map(|(k, ..)| k)
                .collect();
            let fade_ms = [750, 2_000, 0, 750][frame as usize * 4 / 313];
            let f = FadeInputs {
                now,
                fade_ms,
                uploaded: &sim.uploaded,
                retiring: &sim.retiring,
            };
            mark_fades(&mut sim.sections, &mut sim.records, &mut sim.fades, ROWS, f);
            let a_frame_before = FadeInputs {
                now: now - FRAME,
                ..f
            };
            let mut flagged = 0;
            for (k, s, _) in sim.sections.iter() {
                let marked = sim.sections.is_marked(k);
                assert_eq!(marked, s.listed(k, ROWS), "frame {frame}, {k:?}: its mark");
                // The GPU draws its solid quads whole, or the walk lists it.
                assert!(
                    s.solid == 0 || s.gpu_draws_solid(true) || marked,
                    "frame {frame}, {k:?}: drawn by neither"
                );
                assert!(
                    !s.gpu_draws_solid(true) || !s.drawn_faded(k, f),
                    "frame {frame}, {k:?}: drawn whole by the GPU while it fades"
                );
                if let Some(id) = s.gpu_id {
                    let record = s.record(k, None);
                    assert_eq!(sim.records.at(id), Some(&record), "frame {frame}, {k:?}");
                }
                if s.gpu_fading {
                    flagged += 1;
                    set_by[3] += usize::from(!flagged_before.contains(&k));
                    if s.leaving.is_none() && !sim.retiring.contains(&k) {
                        assert!(
                            s.drawn_faded(k, a_frame_before),
                            "frame {frame}, {k:?}: flagged a frame after its fade"
                        );
                    }
                } else {
                    handed_back += usize::from(flagged_before.contains(&k));
                }
            }
            assert_eq!(sim.records.fading(), flagged, "frame {frame}");
            last = now;
        }
        assert!(set_by.iter().all(|&n| n > 20), "flags set {set_by:?}");
        // Over a thousand: the half of the uploads over far terrain come in whole.
        assert!(handed_back > 1_000, "{handed_back} handed back");
        // Switched off with sections still flagged: none keeps a record or a
        // flag, and each is marked as while GPU culling is off.
        assert!(sim.records.fading() > 0, "none flagged at the end");
        forget_records(&mut sim.sections, ROWS);
        for (k, s, _) in sim.sections.iter() {
            assert!(s.gpu_id.is_none() && !s.gpu_fading, "{k:?}: kept once off");
            let off = s.translucent > 0 || !s.gpu_recorded(k, ROWS);
            assert_eq!(sim.sections.is_marked(k), off, "{k:?}: its mark once off");
        }
    }
    #[test]
    fn terrain_attributes_follow_the_terrain_vertex() {
        use super::TerrainVertex;
        use std::mem::offset_of;
        let offsets = super::TERRAIN_ATTRIBUTES.map(|a| (a.shader_location, a.offset as usize));
        assert_eq!(
            offsets,
            [
                (0, offset_of!(TerrainVertex, pos)),
                (1, offset_of!(TerrainVertex, uv)),
                (2, offset_of!(TerrainVertex, layer)),
                (4, offset_of!(TerrainVertex, light)),
                (3, offset_of!(TerrainVertex, color)),
            ]
        );
        assert_eq!(std::mem::size_of::<TerrainVertex>(), 24);
    }
    #[test]
    fn cracks_multiply_instead_of_covering_the_block() {
        let blend = super::CRUMBLING_BLEND;
        assert_eq!(blend.color.src_factor, wgpu::BlendFactor::Dst);
        assert_eq!(blend.color.dst_factor, wgpu::BlendFactor::Src);
        assert_eq!(blend.alpha.src_factor, wgpu::BlendFactor::One);
        assert_eq!(blend.alpha.dst_factor, wgpu::BlendFactor::Zero);
    }
    #[test]
    fn hdr_screenshots_keep_every_10_bit_step() {
        // Rgb10a2Unorm: red in the low 10 bits, then green, then blue.
        let px = |r: u32, g: u32, b: u32| (r | g << 10 | b << 20 | 3 << 30).to_le_bytes();
        let out = super::hdr10_to_rgb16(&[px(0, 1023, 512)]);
        assert_eq!(out, [0, 0, 0xff, 0xff, 0x80, 0x20]);
        assert_eq!(
            super::hdr_screenshot_path(std::path::Path::new("screenshots/2026-10-28_12.00.00.png")),
            std::path::Path::new("screenshots/2026-10-28_12.00.00-hdr.png")
        );
    }
    #[test]
    fn texture_filtering_samplers() {
        use super::{sampler_anisotropy, sprite_sampler_descriptor};
        // LevelRenderer.chunkLayerSampler: linear both ways, clamped, in every mode.
        for aniso in [1, 8] {
            let d = sprite_sampler_descriptor(aniso);
            assert_eq!(d.mag_filter, wgpu::FilterMode::Linear);
            assert_eq!(d.min_filter, wgpu::FilterMode::Linear);
            assert_eq!(d.mipmap_filter, wgpu::MipmapFilterMode::Linear);
            assert_eq!(d.address_mode_v, wgpu::AddressMode::ClampToEdge);
            assert_eq!(d.anisotropy_clamp, aniso);
        }
        assert_eq!(
            [1, 2, 3].map(sampler_anisotropy),
            [2, 4, 8],
            "Options.maxAnisotropyValue is 1 << bit"
        );
    }
    #[test]
    fn distant_tiles_repeat_their_sprite_under_the_clamped_sampler() {
        // terrain.wgsl's tile_uv, evaluated by naga as a constant expression.
        let source = super::terrain_shader();
        let body = source.split("fn tile_uv(").nth(1).unwrap();
        let body = body.split("return ").nth(1).unwrap();
        let expr = &body[..body.find(';').unwrap()];
        let tile_uv = |u: f32, v: f32| -> [f32; 2] {
            let uv = format!("vec2<f32>({u:?}, {v:?})");
            let wgsl = format!("const UV: vec2<f32> = {};", expr.replace("uv", &uv));
            let module = naga::front::wgsl::parse_str(&wgsl).unwrap();
            let (_, constant) = module.constants.iter().next().unwrap();
            let naga::Expression::Compose { components, .. } =
                &module.global_expressions[constant.init]
            else {
                panic!("{wgsl}");
            };
            components
                .iter()
                .map(|&c| match module.global_expressions[c] {
                    naga::Expression::Literal(naga::Literal::F32(x)) => x,
                    ref e => panic!("{e:?}"),
                })
                .collect::<Vec<_>>()
                .try_into()
                .unwrap()
        };
        // A section's sprite UVs, edges included, stay where the clamped sampler reads them.
        for (u, v) in [(0.0, 1.0), (0.25, 0.75), (1.0, 0.0)] {
            assert_eq!(tile_uv(u, v), [u, v]);
        }
        // A merged face's UV counts blocks: each takes the sprite from its own corner.
        assert_eq!(tile_uv(1.25, 2.0), [0.25, 1.0]);
        assert_eq!(tile_uv(1023.5, 3.75), [0.5, 0.75]);
    }
    #[test]
    fn shader_globals_match_the_rust_layout() {
        // terrain.wgsl and entity.wgsl bind the same buffer: same fields, same order.
        let fields = |source: &str| -> Vec<String> {
            let body = source.split("struct Globals {").nth(1).unwrap();
            body[..body.find('}').unwrap()]
                .lines()
                .filter_map(|l| l.trim().split_once(':').map(|(name, _)| name.to_string()))
                .filter(|name| !name.starts_with("//"))
                .collect()
        };
        let terrain = fields(include_str!("terrain.wgsl"));
        assert_eq!(terrain, fields(include_str!("entity.wgsl")));
        let rust = fields(include_str!("gfx.rs"));
        assert_eq!(terrain, rust);
    }
    #[test]
    fn selection_outline_uses_vanilla_pixel_width() {
        for width in [640, 1280, 1920] {
            assert_eq!(super::outline_width(width), 2.5);
        }
        assert_eq!(super::outline_width(2560), 2560.0 / 1920.0 * 2.5);
        assert_eq!(super::outline_width(3840), 5.0);
    }
    #[test]
    fn the_projection_center_follows_the_view_bob() {
        use glam::{Mat4, Vec3};
        let projection = super::world_projection(70.0, 16.0 / 9.0, 512.0);
        let view = crate::camera::view_matrix(30.0, 20.0);
        let eye = super::projection_center(projection * view).unwrap();
        assert!(eye.length() < 1e-5, "{eye}");
        // A pose translated by the bob: the eye is where the pose's inverse
        // takes the view's origin.
        let bob = Mat4::from_rotation_z(0.02) * Mat4::from_translation(Vec3::new(0.03, -0.08, 0.0));
        let expected = (bob * view).inverse().transform_point3(Vec3::ZERO);
        let eye = super::projection_center(projection * bob * view).unwrap();
        assert!(eye.distance(expected) < 1e-5, "{eye} {expected}");
        assert!(expected.length() > 0.05);
        assert!(
            super::projection_center(Mat4::orthographic_rh(-1.0, 1.0, -1.0, 1.0, 0.0, 1.0))
                .is_none()
        );
    }
    #[test]
    fn indirect_draws_are_validated_only_in_debug_builds_and_on_directx_12() {
        let validated = |backends| {
            super::instance_descriptor(backends, None, false, wgpu::Dx12Compiler::Fxc)
                .flags
                .contains(wgpu::InstanceFlags::VALIDATION_INDIRECT_CALL)
        };
        if std::env::var_os("WGPU_VALIDATION_INDIRECT_CALL").is_none()
            && std::env::var_os("WGPU_BACKEND").is_none()
        {
            assert_eq!(validated(wgpu::Backends::default()), cfg!(debug_assertions));
            assert_eq!(validated(wgpu::Backends::VULKAN), cfg!(debug_assertions));
            assert_eq!(
                validated(wgpu::Backends::DX12),
                cfg!(debug_assertions) || cfg!(windows)
            );
        }
    }
    #[test]
    fn far_faces_a_hair_apart_keep_their_depth_order() {
        let m = super::world_projection(70.0, 16.0 / 9.0, 768.0);
        let depth = |distance: f32| {
            let clip = m * glam::Vec4::new(0.0, 0.0, -distance, 1.0);
            clip.z / clip.w
        };
        // fire_side at 150 blocks, 0.01 pixel in front of the block it burns on.
        let (block, fire) = (depth(150.0), depth(150.0 - 0.01 / 16.0));
        assert!(fire > block, "{fire} {block}");
        assert!((0.0..=1.0).contains(&block));
        assert!((depth(0.05) - 1.0).abs() < 1e-6);
    }
    #[test]
    fn selection_outline_has_vanilla_perspective_layering() {
        assert_eq!(
            super::outline_params(1280, 720),
            [1280.0, 720.0, 2.5, 0.99975586]
        );
        assert_eq!(
            super::outline_params(3840, 0),
            [3840.0, 1.0, 5.0, 0.99975586]
        );
    }
    #[test]
    fn selection_outline_preserves_shape_edges_without_inflation() {
        let edges = [[2.0, 97.5, 3.0, 3.0, 97.5, 3.0]];
        assert_eq!(
            super::outline_vertices(&edges, glam::DVec3::new(0.5, 98.62, 0.5)),
            vec![[
                1.5,
                (97.5 - 98.62) as f32,
                2.5,
                2.5,
                (97.5 - 98.62) as f32,
                2.5
            ]]
        );
        assert!(super::outline_vertices(&[], glam::DVec3::ZERO).is_empty());
    }
}

#[cfg(test)]
mod sprite_upload_tests {
    use super::*;
    #[test]
    fn real_rail_mips_do_not_change_with_larger_array_layers() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../.local/horizon-client")
            .join(horizon_core::VANILLA_VERSION);
        if !root
            .join("assets/minecraft/textures/block/rail.png")
            .exists()
        {
            return;
        }
        let mut sprites = crate::sprites::Sprites::default();
        let layers: Vec<_> = [
            "rail",
            "powered_rail",
            "powered_rail_on",
            "detector_rail",
            "activator_rail",
        ]
        .map(|name| {
            sprites
                .insert(&root, &format!("minecraft:block/{name}"))
                .unwrap()
        })
        .into();
        sprites.finish();
        assert_eq!(sprites.size, 16);
        let native: Vec<_> = layers
            .iter()
            .map(|&layer| {
                let start = layer as usize * 16 * 16 * 4;
                mip_chain(
                    &sprites.pixels[start..start + 16 * 16 * 4],
                    16,
                    &sprites.mips,
                    layer,
                    1,
                )
                .0
            })
            .collect();
        sprites.insert(&root, "minecraft:block/oak_sign").unwrap();
        sprites.finish();
        assert_eq!(sprites.size, 32);
        for (&layer, expected) in layers.iter().zip(native) {
            let start = layer as usize * 32 * 32 * 4;
            let (actual, _) = mip_chain(
                &sprites.pixels[start..start + 32 * 32 * 4],
                32,
                &sprites.mips,
                layer,
                1,
            );
            assert_eq!(&actual[32 * 32 * 4..], expected, "rail layer {layer}");
        }
    }
    #[test]
    fn prepared_texture_uploads_preserve_layer_order_and_settings() {
        let size = 4;
        let layers = 5;
        let pixels: Vec<u8> = (0..size * size * layers)
            .flat_map(|i| [i as u8, (i * 3) as u8, 99, if i % 3 == 0 { 0 } else { 255 }])
            .collect();
        let settings: Vec<_> = (0..layers)
            .map(|i| crate::mipmap::Settings {
                strategy: if i % 2 == 0 {
                    crate::mipmap::Strategy::Mean
                } else {
                    crate::mipmap::Strategy::StrictCutout
                },
                ..Default::default()
            })
            .collect();
        let border = [20, 30, 40, 128].repeat(16);
        let expected = mip_chain(&pixels, size, &settings, 0, layers);
        // A partial final array and an exact boundary exercise the global
        // layer offset used to select each sprite's mipmap settings.
        for split in [1, 2, 5, 8] {
            let prepared = PreparedTextures::with_split(
                &pixels,
                size,
                layers,
                &settings,
                (&border, size),
                split,
            );
            assert_eq!(prepared.sprites.len(), layers.div_ceil(split) as usize);
            let actual: Vec<_> = prepared
                .sprites
                .iter()
                .flat_map(|(bytes, levels)| {
                    assert_eq!(*levels, expected.1);
                    bytes.iter().copied()
                })
                .collect();
            assert_eq!(actual, expected.0);
            assert_eq!(prepared.forcefield, mip_chain(&border, size, &[], 0, 1));
            // Chains cut before the adapter was known, cut again for it.
            for to in [1, 2, 3, 5, 8] {
                let direct = PreparedTextures::with_split(
                    &pixels,
                    size,
                    layers,
                    &settings,
                    (&border, size),
                    to,
                );
                let resplit = PreparedTextures::with_split(
                    &pixels,
                    size,
                    layers,
                    &settings,
                    (&border, size),
                    split,
                )
                .resplit(layers, to);
                assert_eq!(resplit.split, to);
                assert_eq!(resplit.sprites, direct.sprites, "{split} -> {to}");
            }
        }
    }

    /// The chains are made a chunk of layers at a time, each on its own thread. Their
    /// bytes must be the ones one thread making every layer in turn gives, in layer
    /// order, whichever layer a chunk starts at.
    #[test]
    fn mip_chains_match_one_thread_making_every_layer() {
        let size: u32 = 8;
        let layers: u32 = 40;
        let pixels: Vec<u8> = (0..size * size * layers)
            .flat_map(|i| [i as u8, (i * 7) as u8, 33, if i % 5 == 0 { 0 } else { 255 }])
            .collect();
        let settings: Vec<_> = (0..layers)
            .map(|i| crate::mipmap::Settings {
                strategy: match i % 3 {
                    0 => crate::mipmap::Strategy::Mean,
                    1 => crate::mipmap::Strategy::Cutout,
                    _ => crate::mipmap::Strategy::DarkCutout,
                },
                alpha_cutoff_bias: (i % 4) as f32 / 4.0,
                item: i % 7 == 0,
                ..Default::default()
            })
            .collect();
        let layer_bytes = (size * size * 4) as usize;
        let levels = size.trailing_zeros();
        let chain_bytes: usize = (0..=levels).map(|l| layer_bytes >> (2 * l)).sum();
        let mut expected: Vec<u8> = Vec::new();
        for (layer, s) in settings.iter().copied().enumerate() {
            let base = crate::mipmap::Image {
                width: size,
                height: size,
                pixels: pixels[layer * layer_bytes..(layer + 1) * layer_bytes]
                    .as_chunks::<4>()
                    .0
                    .to_vec(),
            };
            let has_transparent = s.has_transparent || base.pixels.iter().any(|p| p[3] == 0);
            for image in crate::mipmap::generate(
                base,
                levels,
                crate::mipmap::Settings {
                    has_transparent,
                    ..s
                },
            ) {
                expected.extend(image.pixels.iter().flatten());
            }
        }
        assert_eq!(expected.len(), chain_bytes * layers as usize);
        assert_eq!(mip_chain(&pixels, size, &settings, 0, layers).0, expected);
        // From the middle: the chunk's settings still come from the global layer index.
        let from = (layers / 2) as usize;
        let chain = mip_chain(
            &pixels[from * layer_bytes..],
            size,
            &settings,
            from as u32,
            layers / 2,
        );
        assert_eq!(chain.0, expected[from * chain_bytes..]);
    }

    #[test]
    fn sprite_split_follows_the_adapters_layer_limit() {
        // The Discord report: 512 layers per array, 1291 block textures (3 arrays).
        assert_eq!(layer_split(512, false, None, 1291), Ok(512));
        assert_eq!(layer_split(512, true, None, 1291), Ok(512));
        assert_eq!(layer_split(2048, false, None, 1291), Ok(SPRITE_SPLIT));
        assert_eq!(layer_split(8192, false, None, 1291), Ok(SPRITE_SPLIT));
        // HORIZON_SPRITE_SPLIT only lowers it.
        assert_eq!(layer_split(2048, false, Some(512), 1291), Ok(512));
        assert_eq!(layer_split(256, false, Some(512), 1291), Ok(256));
        // OpenGL pads a multiple of 6 by a layer: a full array must still fit.
        assert_eq!(layer_split(600, true, None, 1291), Ok(599));
        assert_eq!(layer_split(600, false, None, 1291), Ok(600));
        assert_eq!(array_layers(599, true), 599);
        // Past the arrays terrain.wgsl binds.
        assert_eq!(layer_split(256, false, None, 256 * SPRITE_ARRAYS), Ok(256));
        let err = layer_split(128, false, None, 1291).unwrap_err();
        assert!(err.contains("128 texture layers per array"), "{err}");
        assert!(layer_split(512, false, None, 512 * SPRITE_ARRAYS + 1).is_err());
    }

    #[test]
    fn metal_uses_a_conservative_default_split_but_respects_overrides() {
        let mut info = wgpu::AdapterInfo {
            name: "test adapter".into(),
            vendor: 0,
            device: 0,
            device_type: wgpu::DeviceType::IntegratedGpu,
            device_pci_bus_id: String::new(),
            driver: String::new(),
            driver_info: String::new(),
            backend: wgpu::Backend::Vulkan,
            subgroup_min_size: 0,
            subgroup_max_size: 0,
            transient_saves_memory: None,
            limit_bucket: None,
        };
        info.backend = wgpu::Backend::Metal;
        assert_eq!(adapter_requested_split(&info, None), Some(512));
        assert_eq!(adapter_requested_split(&info, Some(1024)), Some(1024));

        info.backend = wgpu::Backend::Vulkan;
        assert_eq!(adapter_requested_split(&info, None), None);
    }

    #[test]
    fn broken_haswell_vulkan_detects_haswell_and_leaves_ivy_bridge() {
        let mut info = wgpu::AdapterInfo {
            name: "Intel(R) HD Graphics 4400 (HSW GT2)".into(),
            vendor: 0x8086,
            device: 0x0a16,
            device_type: wgpu::DeviceType::IntegratedGpu,
            device_pci_bus_id: String::new(),
            driver: "Mesa 26.2.5".into(),
            driver_info: "Mesa 26.2.5".into(),
            backend: wgpu::Backend::Vulkan,
            subgroup_min_size: 0,
            subgroup_max_size: 0,
            transient_saves_memory: None,
            limit_bucket: None,
        };
        assert!(is_broken_haswell_vulkan(&info));

        // Ivy Bridge keeps Vulkan (#4059)
        info.name = "Intel(R) HD Graphics 4000 (IVB GT2)".into();
        info.device = 0x0166;
        assert!(!is_broken_haswell_vulkan(&info));

        // Haswell on OpenGL is stable (crocus)
        info.name = "Intel(R) HD Graphics 4400 (HSW GT2)".into();
        info.device = 0x0a16;
        info.backend = wgpu::Backend::Gl;
        assert!(!is_broken_haswell_vulkan(&info));
    }

    #[test]
    fn mip_chain_allocates_exactly_its_size() {
        for size in [16, 64] {
            let animation = crate::sprites::test_animation(size, 0);
            let (chain, _) = mip_chain(animation.pixels(0), size, &[], 0, 1);
            assert_eq!(chain.capacity(), chain.len());
        }
    }

    #[test]
    fn collect_animated_layers_masks_opaque_layer() {
        use crate::mesher::{self, SectionMesh, TerrainVertex};
        let opaque_vertex =
            TerrainVertex::new([0.0, 0.0, 0.0], [0.0, 0.0], 7 | mesher::OPAQUE_LAYER, 0, 0);
        let translucent_vertex = TerrainVertex::new([0.0, 0.0, 0.0], [0.0, 0.0], 12, 0, 0);
        let mut mesh = SectionMesh {
            solid: vec![opaque_vertex; 4],
            translucent: vec![translucent_vertex; 4],
            ..Default::default()
        };
        mesh.note_sprite_layer(7 | mesher::OPAQUE_LAYER);
        mesh.note_sprite_layer(12);
        mesh.finish_sprite_layers();

        let animated = super::collect_animated_layers(&mesh, |layer| layer == 7 || layer == 12);
        assert_eq!(&*animated, &[7, 12]);
    }
}

/// What the system says about the window's display, kept up to date: on Wayland, the
/// compositor's preferred image description of the surface; elsewhere what wgpu reads
/// from the OS (on Windows, DXGI's luminances and the SDR white level).
#[derive(Default)]
struct DisplayWatch {
    #[cfg(target_os = "linux")]
    wayland: Option<horizon_hdr::wayland::Watch>,
    /// Without Wayland's event, when to read wgpu's values again: the OS sends no event for
    /// a moved window or a changed SDR brightness slider.
    polled: Option<std::time::Instant>,
}

impl DisplayReading {
    /// What wgpu reads from the OS for `surface`'s display; nothing without one.
    fn read(surface: Option<&wgpu::Surface>, adapter: &wgpu::Adapter) -> Self {
        surface.map_or_else(Self::default, |s| Self::os(&s.display_hdr_info(adapter)))
    }
    fn os(info: &wgpu::DisplayHdrInfo) -> Self {
        #[cfg(target_os = "macos")]
        let hdr = macos_hdr(info);
        #[cfg(not(target_os = "macos"))]
        let hdr = info.coarse.and_then(|coarse| coarse.high_dynamic_range);
        Self {
            luminance: os_reported(info),
            hdr,
        }
    }
}
/// Potential EDR is fixed for a screen; current EDR follows brightness and must not
/// repeatedly switch HDR output as the player adjusts the desktop's brightness.
#[cfg(any(target_os = "macos", test))]
fn macos_hdr(info: &wgpu::DisplayHdrInfo) -> Option<bool> {
    info.headroom?
        .potential
        .filter(|n| n.is_finite() && *n > 0.0)
        .map(|n| n > 1.0)
}
/// How often [`DisplayWatch`] reads the OS's values again where no event says they changed.
const DISPLAY_POLL: std::time::Duration = std::time::Duration::from_secs(2);
impl DisplayWatch {
    /// Starts watching, with what the system reports now.
    fn new(
        window: &Window,
        surface: Option<&wgpu::Surface>,
        adapter: &wgpu::Adapter,
    ) -> (Self, DisplayReading) {
        #[cfg(target_os = "linux")]
        if surface.is_some()
            && let Some((wayland, now)) = horizon_hdr::wayland::Watch::new(window)
        {
            let watch = Self {
                wayland: Some(wayland),
                polled: None,
            };
            return (watch, wayland_reading(now));
        }
        let _ = window;
        let now = DisplayReading::read(surface, adapter);
        log_os_reported(now.luminance);
        let watch = Self {
            #[cfg(target_os = "linux")]
            wayland: None,
            polled: Some(std::time::Instant::now() + DISPLAY_POLL),
        };
        (watch, now)
    }
    /// What the system reports at each poll that is due at `time` (`read`; on Wayland, at each
    /// event), whether or not it is `last` again: the surface's capabilities are read with it.
    fn poll(
        &mut self,
        last: DisplayReading,
        time: std::time::Instant,
        read: impl FnOnce() -> DisplayReading,
    ) -> Option<DisplayReading> {
        #[cfg(target_os = "linux")]
        if let Some(wayland) = &mut self.wayland {
            return wayland.changed().map(wayland_reading);
        }
        let next = self.polled.as_mut()?;
        if time < *next {
            return None;
        }
        *next = time + DISPLAY_POLL;
        let now = read();
        if now != last {
            log_os_reported(now.luminance);
        }
        Some(now)
    }
}

/// The log of what the system reports about the display: one line per change. The
/// renderer is made anew a few times while the game starts and each asks again, and a
/// slider dragged in the desktop's settings changes the values many times a second, so
/// a change within [`DISPLAY_LOG_QUIET`] of the last line waits until the values have
/// held that long, and only the one they settled on is logged.
struct DisplayLog {
    last: String,
    at: Option<std::time::Instant>,
    pending: Option<(String, std::time::Instant)>,
}
static DISPLAY_LOG: std::sync::Mutex<DisplayLog> = std::sync::Mutex::new(DisplayLog {
    last: String::new(),
    at: None,
    pending: None,
});
const DISPLAY_LOG_QUIET: std::time::Duration = std::time::Duration::from_secs(1);
fn log_display(line: String) {
    let now = std::time::Instant::now();
    let mut log = DISPLAY_LOG
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if line == log.last {
        log.pending = None;
    } else if log.at.is_none_or(|at| now - at >= DISPLAY_LOG_QUIET) {
        log::info!("{line}");
        (log.last, log.at, log.pending) = (line, Some(now), None);
    } else {
        log.pending = Some((line, now));
    }
}
/// Logs a change [`log_display`] held back, once the values have held for
/// [`DISPLAY_LOG_QUIET`].
fn flush_display_log() {
    let now = std::time::Instant::now();
    let mut log = DISPLAY_LOG
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some((_, since)) = &log.pending
        && now - *since >= DISPLAY_LOG_QUIET
        && let Some((line, _)) = log.pending.take()
    {
        log::info!("{line}");
        (log.last, log.at) = (line, Some(now));
    }
}
fn log_os_reported(reported: Option<crate::graphics::Reported>) {
    log_display(match reported {
        Some(r) => format!(
            "Horizon renderer: the system reports SDR white at {} nits and a peak of {} nits",
            r.paper_white, r.peak
        ),
        None => "Horizon renderer: the system doesn't report the display's luminance".into(),
    });
}
/// What the OS reports in absolute nits (Windows: DXGI's `MaxLuminance` and the SDR
/// white level the brightness slider sets), which is HDR output's scale there: SDR
/// white is Paper White, the display's peak is Peak Brightness. Both are kept within
/// the settings' ranges. The OS's figures often come from the display's EDID and may
/// run optimistic; the player's own value always wins.
fn os_reported(info: &wgpu::DisplayHdrInfo) -> Option<crate::graphics::Reported> {
    let luminance = info.luminance?;
    let usable = |n: f32| n.is_finite() && n > 0.0;
    let peak = luminance.max_nits.filter(|&n| usable(n))?;
    let white = luminance.sdr_white_nits.filter(|&n| usable(n))?;
    let paper_white = (white.round() as u32).clamp(80, 480);
    Some(crate::graphics::Reported {
        paper_white,
        // Windows' SDR brightness slider can set white above the reported peak (480 against
        // a calibrated 380): the highlights then stay at white instead of dropping below it.
        peak: (peak.round() as u32).clamp(200, 10_000).max(paper_white),
    })
}

/// What a compositor's luminances mean in HDR output's scale.
///
/// KWin maps content's reference white (203 nits in PQ, BT.2408) onto its own, so the
/// desktop's white is 203 there and the peak is the target's maximum over that ratio.
/// Checked on KWin. Hyprland (0.57) reports the standard 203, and shows HDR windows below
/// the SDR white of its desktop (which it doesn't report), so Auto can't match its desktop
/// there. Other compositors are unchecked. The peak is kept within the settings' lowest and
/// PQ's top, so the metadata never ends below the black level.
#[cfg(target_os = "linux")]
fn wayland_reading(now: Option<horizon_hdr::wayland::Luminances>) -> DisplayReading {
    now.map_or_else(DisplayReading::default, |l| DisplayReading {
        luminance: Some(reported(l)),
        // The preferred target's headroom is the compositor's live report, not the
        // swapchain's supported formats. Invalid or absent readings stay unknown.
        hdr: (l.reference.is_finite()
            && l.reference > 0.0
            && l.target_max.is_finite()
            && l.target_max > 0.0)
            .then_some(l.target_max > l.reference),
    })
}
#[cfg(target_os = "linux")]
fn reported(l: horizon_hdr::wayland::Luminances) -> crate::graphics::Reported {
    let peak = (l.target_max * 203.0 / l.reference.max(1.0)).round() as u32;
    log_display(format!(
        "Horizon renderer: the compositor shows this window with reference white {} nits, \
         {}..{} nits: peak {peak} nits in HDR output's scale",
        l.reference, l.target_min, l.target_max,
    ));
    crate::graphics::Reported {
        paper_white: 203,
        peak: peak.clamp(200, 10_000),
    }
}

#[cfg(test)]
mod shadow_face_tests {
    use super::*;

    /// The faces of one block at `at` (section-relative), each with its own layer:
    /// 0 +X, 1 -X, 2 +Y (up), 3 -Y (down), 4 +Z, 5 -Z; counter-clockwise from outside.
    fn block(at: [f32; 3]) -> Vec<TerrainVertex> {
        let [x, y, z] = at;
        let (a, b, c) = (x + 1.0, y + 1.0, z + 1.0);
        [
            [[a, y, z], [a, b, z], [a, b, c], [a, y, c]],
            [[x, y, z], [x, y, c], [x, b, c], [x, b, z]],
            [[x, b, z], [x, b, c], [a, b, c], [a, b, z]],
            [[x, y, z], [a, y, z], [a, y, c], [x, y, c]],
            [[x, y, c], [a, y, c], [a, b, c], [x, b, c]],
            [[x, y, z], [x, b, z], [a, b, z], [a, y, z]],
        ]
        .into_iter()
        .enumerate()
        .flat_map(|(layer, q)| q.map(|p| TerrainVertex::new(p, [0.0; 2], layer as u16, 0, 0)))
        .collect()
    }

    /// The layers (faces) of the quads the shadow pass draws of `solid`.
    fn drawn(view: &pack_post::ShadowView, origin: [f32; 3]) -> Vec<u16> {
        let mut solid = block([7.0, 7.0, 7.0]);
        let sides = crate::mesher::sort_tinted(&mut solid, &mut Vec::new());
        let mut layers = Vec::new();
        shadow_runs(&sides, view, origin, |first, quads| {
            for q in first..first + quads {
                layers.push(solid[q as usize * 4].layer);
            }
        });
        layers.sort_unstable();
        layers
    }

    fn view(every_face: bool) -> pack_post::ShadowView {
        pack_post::ShadowView {
            cull: Mat4::IDENTITY,
            light: Vec3::Y,
            distance: None,
            unculled: None,
            every_face,
            view_casters: true,
            depth_planes: false,
        }
    }

    /// Iris 1.11.7's shadow map holds faces turned away from the light (#9033): the block's
    /// bottom face, under a sun straight above, is drawn too.
    #[test]
    fn the_shadow_pass_draws_faces_turned_away_from_the_light() {
        // A section 32 blocks under the camera.
        let origin = [-8.0, -40.0, -8.0];
        assert_eq!(drawn(&view(true), origin), [0, 1, 2, 3, 4, 5]);
        // Horizon's own packs keep drawing only what can face the light: no bottom face.
        assert!(!drawn(&view(false), origin).contains(&3));
    }
}

#[cfg(test)]
mod shadow_caster_tests {
    use super::*;

    #[test]
    fn the_shadow_pass_takes_the_block_entities_of_its_sections_within_64_blocks() {
        // Sections 64 to 79 high (the world from -64): a chest in each of three. The shadow
        // pass drew `near` and `far`; `left_out` lies as near but the pass didn't draw it, and
        // `far`'s nearest block is 78.5 blocks away.
        let (near, left_out, far) = ((0, 8, 0), (1, 8, 0), (5, 8, 0));
        let mut block_entities = FxHashMap::default();
        block_entities.insert(near, vec![([3, 70, 3], 7)]);
        block_entities.insert(left_out, vec![([20, 70, 3], 7)]);
        block_entities.insert(far, vec![([85, 70, 3], 7)]);
        let camera = DVec3::new(0.5, 70.0, 0.5);
        let drawn = [near, far].into_iter();
        let taken: Vec<_> = block_entities_within_reach(drawn, &block_entities, camera, -64)
            .copied()
            .collect();
        assert_eq!(taken, vec![([3, 70, 3], 7)]);
    }

    /// What the shadow pass keeps of a camera-relative section at `min`, the camera at the
    /// origin looking along -X (yaw 90) with the scenes' 70 degree, 16:9 view, and the light in
    /// direction `light`; the light's own box takes in everything.
    fn kept(light: Vec3, min: [f32; 3]) -> bool {
        kept_looking(Vec3::NEG_X, light, min)
    }

    /// `kept` with the camera looking along `forward`.
    fn kept_looking(forward: Vec3, light: Vec3, min: [f32; 3]) -> bool {
        let camera = world_projection(70.0, 16.0 / 9.0, 192.0)
            * Mat4::look_to_rh(Vec3::ZERO, forward, Vec3::Y);
        let cull = ShadowCull {
            light: Frustum {
                planes: [Vec4::W; 6],
            },
            casters: Some(shadow_casters(camera, light)),
            reach: None,
        };
        let min = Vec3::from(min);
        cull.intersects(min, min + 16.0)
    }

    /// Our probes' Iris 1.11.7 shadow maps fit keeping only the terrain whose shadow can fall
    /// into the camera's view (#8464).
    #[test]
    fn the_shadow_pass_keeps_what_can_shade_the_view() {
        // A noon sun: a treetop high above the ground ahead shades it, though the camera doesn't
        // see it; behind the camera, or far off to the side, nothing does.
        assert!(kept(Vec3::Y, [-48.0, 64.0, -8.0]));
        assert!(!kept(Vec3::Y, [32.0, 0.0, -8.0]));
        assert!(!kept(Vec3::Y, [-32.0, 0.0, 96.0]));
        // Nor does what lies under the ground the camera looks along.
        assert!(!kept(Vec3::Y, [-48.0, -96.0, -8.0]));
        // A low sun ahead (west, -X) throws shadows toward the camera: what is behind it can't
        // reach the view.
        let west = Vec3::new(-1.0, 0.1, 0.0).normalize();
        assert!(kept(west, [-48.0, 0.0, -8.0]));
        assert!(!kept(west, [32.0, 0.0, -8.0]));
        // The same sun behind the camera throws the terrain behind it forward, into the view.
        let east = Vec3::new(1.0, 0.1, 0.0).normalize();
        assert!(kept(east, [32.0, 0.0, -8.0]));
        assert!(kept(east, [96.0, 0.0, -8.0]));
        // Looking 20 degrees down under a noon sun, the view's sides lean out below the camera:
        // a treetop above the ground ahead still shades it, one off to the side doesn't, though
        // it lies inside each of the view's planes that faces the light.
        let down = Vec3::new(-20f32.to_radians().cos(), -20f32.to_radians().sin(), 0.0);
        assert!(kept_looking(down, Vec3::Y, [-48.0, 16.0, -8.0]));
        assert!(!kept_looking(down, Vec3::Y, [-32.0, 16.0, -96.0]));
    }

    /// Our probes' Iris 1.11.7 maps keep the sections whose box, grown by a margin of 1.1224 to
    /// 1.1252 blocks, meets the shape (#8464): a camera at (13.5, 69.62, -13.5) under a noon sun with the scenes' flying
    /// field of view (77°).
    #[test]
    fn the_shadow_pass_keeps_a_margin_around_the_view() {
        let kept = |yaw: f32, column: [f32; 2]| {
            let camera =
                world_projection(77.0, 16.0 / 9.0, 1024.0) * crate::camera::view_matrix(yaw, 0.0);
            let cull = ShadowCull {
                light: Frustum {
                    planes: [Vec4::W; 6],
                },
                casters: Some(shadow_casters(camera, Vec3::Y)),
                reach: None,
            };
            let min = Vec3::new(
                column[0] * 16.0 - 13.5,
                64.0 - 69.62,
                column[1] * 16.0 + 13.5,
            );
            cull.intersects(min, min + 16.0)
        };
        // Looking west, the column at chunk (-2, -6) needs a margin of 0.9 blocks: Iris keeps it.
        assert!(kept(90.0, [-2.0, -6.0]));
        // Looking south (yaw 7.5), the one at (3, 0) needs a margin of 1.3 blocks: Iris drops it.
        assert!(!kept(7.5, [3.0, 0.0]));
    }

    /// With `shadowDistanceRenderMul`, our probe's Iris 1.11.7 maps keep the sections within
    /// `shadowDistance × mul` of the camera along each axis, out to the cube's corners, and with
    /// some slack (the camera at (13.5, 69.62, -13.5)):
    /// - at 100 blocks, the column at chunk (7, 5), 98.5 and 93.5 blocks out (136 diagonally), and
    ///   the one at (-6, -7) are kept; the one at (8, -4), 114.5 out on x, is not;
    /// - at 49.5 blocks, the column at (-2, -5), 50.5 out on z, is kept; at 49 it is not;
    /// - at 50 blocks from a camera at y 151.62, the ground's sections 70 blocks below are not.
    #[test]
    fn the_shadow_distance_render_mul_keeps_a_cube() {
        let kept = |camera: DVec3, reach: f32, keys: &[(i32, i32)]| {
            let y = (64 - crate::chunk::MIN_Y) / 16;
            let mut sections = resident::Resident::<()>::default();
            for &(x, z) in keys {
                sections.insert_with_bounds((x, y, z), (), None);
            }
            let cull = ShadowCull {
                light: Frustum {
                    planes: [Vec4::W; 6],
                },
                casters: None,
                reach: Some(reach),
            };
            let mut out = Vec::new();
            sections.visible(camera, 12, &cull, |_| true, |_, _| false, &mut out);
            let mut kept: Vec<_> = out.iter().map(|v| (v.key.0, v.key.2)).collect();
            kept.sort_unstable();
            kept
        };
        let ground = DVec3::new(13.5, 69.62, -13.5);
        let columns = [(7, 5), (8, -4), (-6, -7)];
        assert_eq!(kept(ground, 100.0, &columns), [(-6, -7), (7, 5)]);
        assert_eq!(kept(ground, 49.5, &[(-2, -5)]), [(-2, -5)]);
        assert_eq!(kept(ground, 49.0, &[(-2, -5)]), []);
        assert_eq!(kept(DVec3::new(13.5, 151.62, -13.5), 50.0, &[(0, 0)]), []);
    }

    /// With the camera looking straight up under a noon sun, our probe's Iris 1.11.7 map holds
    /// 7 columns there, which testing whole sections reproduces (a guess): the sections beside
    /// the camera whose 16 blocks reach above it are kept, even where their terrain lies below
    /// the camera.
    #[test]
    fn the_shadow_pass_tests_whole_sections() {
        let camera = DVec3::new(8.0, 6.0, 8.0);
        let up = world_projection(70.0, 16.0 / 9.0, 192.0)
            * Mat4::look_to_rh(Vec3::ZERO, Vec3::Y, Vec3::Z);
        let view = Frustum {
            planes: frustum(up),
        };
        // The section east of the camera's, y 0..16, with ground only in its lowest 2 blocks.
        let mut sections = resident::Resident::<()>::default();
        let key = (1, -crate::chunk::MIN_Y / 16, 0);
        sections.insert_with_bounds(key, (), Some([Vec3::ZERO, Vec3::new(16.0, 2.0, 16.0)]));
        let drawn = |cull: &dyn Fn(&mut Vec<resident::Visible>)| {
            let mut out = Vec::new();
            cull(&mut out);
            out.iter().map(|v| v.key).collect::<Vec<_>>()
        };
        let all = Frustum {
            planes: [Vec4::W; 6],
        };
        let shadow = ShadowCull {
            light: all,
            casters: Some(shadow_casters(up, Vec3::Y)),
            reach: None,
        };
        assert_eq!(
            drawn(&|out| sections.visible(camera, 4, &shadow, |_| true, |_, _| false, out)),
            [key]
        );
        // The camera's own view goes by the terrain's box, which lies below it.
        assert!(
            drawn(&|out| sections.visible(camera, 4, &view, |_| true, |_, _| false, out))
                .is_empty()
        );
    }

    /// Our probe pack's Iris 1.11.7 shadow maps, under a noon sun with `shadowDistance` 128, hold
    /// the terrain at every depth along the light whatever planes the pack gives (#10403): from
    /// a cave at y -7, the surface's sections ~70 blocks toward the light with `shadowNearPlane`
    /// 0.05 and `shadowFarPlane` 256; from y 600, the ground ~535 blocks away with no planes set;
    /// and both with the planes at 200 and 210. Only the box's sides cull.
    #[test]
    fn the_shadow_pass_draws_terrain_outside_the_pack_s_planes() {
        let kept = |near: Option<f32>, far: Option<f32>, y: f64, min: [f32; 3], depth: bool| {
            let shadow = horizon_shaders::Shadow {
                distance: 128.0,
                near,
                far,
                ..horizon_shaders::Shadow::default()
            };
            let camera = DVec3::new(13.5, y + 1.62, -13.5);
            let view = pack_post::ShadowView {
                cull: pack_uniforms::shadow_culling_at(Vec3::Y, camera, &shadow),
                light: Vec3::Y,
                distance: None,
                unculled: None,
                every_face: true,
                view_casters: false,
                depth_planes: depth,
            };
            let cull = ShadowCull {
                light: shadow_box(&view),
                casters: None,
                reach: None,
            };
            let min = Vec3::from(min);
            cull.intersects(min, min + 16.0)
        };
        // The section holding the surface (y 64..80) above the cave, and the ground under y 600.
        let above = [-8.0, 64.0 - (-7.0 + 1.62), -8.0];
        let below = [-8.0, 64.0 - (600.0 + 1.62), -8.0];
        assert!(kept(Some(0.05), Some(256.0), -7.0, above, false));
        assert!(kept(None, None, 600.0, below, false));
        assert!(kept(Some(200.0), Some(210.0), -7.0, above, false));
        assert!(kept(Some(200.0), Some(210.0), 600.0, below, false));
        // A section 200 blocks off to the side is still outside the box.
        let side = [200.0, 0.0, -8.0];
        assert!(!kept(Some(0.05), Some(256.0), -7.0, side, false));
        // Horizon's own packs keep culling by their planes.
        assert!(!kept(Some(0.05), Some(256.0), -7.0, above, true));
    }

    /// The margin doesn't follow the field of view: from the same camera under a noon sun, our
    /// probe's Iris 1.11.7 maps keep a section that needs a growth of 1.1224 blocks at a field of
    /// view of 70 (77° flying) and drop ones that need 1.1252 to 1.1422 at 50 (55° flying) and
    /// 70 (#8464).
    #[test]
    fn the_shadow_pass_keeps_the_same_margin_at_every_field_of_view() {
        let kept = |fov: f32, yaw: f32, pitch: f32, section: [f32; 3]| {
            let camera = world_projection(fov * 1.1, 16.0 / 9.0, 1024.0)
                * crate::camera::view_matrix(yaw, pitch);
            let cull = ShadowCull {
                light: Frustum {
                    planes: [Vec4::W; 6],
                },
                casters: Some(shadow_casters(camera, Vec3::Y)),
                reach: None,
            };
            let min = Vec3::new(
                section[0] * 16.0 - 13.5,
                section[1] * 16.0 - 69.62,
                section[2] * 16.0 + 13.5,
            );
            cull.intersects(min, min + 16.0)
        };
        // fov 70, yaw 159, pitch 20: (4, 4, -4) needs 1.1224 blocks; Iris keeps it.
        assert!(kept(70.0, 159.0, 20.0, [4.0, 4.0, -4.0]));
        // fov 70, yaw 247.5: (1, 4, 0) needs 1.1253; Iris drops it.
        assert!(!kept(70.0, 247.5, 0.0, [1.0, 4.0, 0.0]));
        // fov 50, yaw 339, pitch -20: (3, 4, -1) needs 1.1252; looking south, (-2, 3, 0) needs
        // 1.1422. Iris drops both.
        assert!(!kept(50.0, 339.0, -20.0, [3.0, 4.0, -1.0]));
        assert!(!kept(50.0, 0.0, 0.0, [-2.0, 3.0, 0.0]));
    }
}

/// A `shaders/` pack building on its own thread (`Renderer::load_shader_pack`): the dimension it
/// is for, the thread (a panic in it contained), and what the game named it with and when.
/// Dropping it (another pack, none, or another dimension) cancels the build: the thread stops at
/// the next program instead of compiling the rest for nothing.
struct PackLoading {
    dimension: i32,
    /// Held to be dropped with it: that cancels the build.
    _cancel: CancelOnDrop,
    #[allow(clippy::type_complexity)]
    thread: std::thread::JoinHandle<
        std::result::Result<std::result::Result<pack_post::PackPost, pack_post::Refused>, String>,
    >,
    label: String,
    started: std::time::Instant,
}

/// Sets its flag when dropped: `PackLoading`'s cancel, which `PackPost::new` checks per program.
struct CancelOnDrop(std::sync::Arc<std::sync::atomic::AtomicBool>);

impl Drop for CancelOnDrop {
    fn drop(&mut self) {
        self.0.store(true, std::sync::atomic::Ordering::Relaxed);
    }
}
