//! A shader pack's post-processing over Horizon's own world (spec 2.1 rows 4, 6, 8, 9; 6.1; 6.2;
//! 6.4): the world renders into its own target as with a resolution scale, its colour becomes
//! colortex0 and its depth `depthtex0`, then the pack's `prepare`, `deferred`, `composite` and
//! `final` programs run in order, and `final` draws onto the screen under the hand and HUD.
//! Gbuffers programs come later; until then the pack's composites read Horizon's picture.
//!
//! Every colortex buffer a program reads or writes has two textures, A and B. Programs read
//! "main" and write "alt", and each buffer a program wrote flips afterwards (6.1.6). Every
//! frame starts with A as "main". A cleared buffer has both textures cleared then (Iris 1.11.7
//! reads and blends onto the clear value where a program leaves pixels, probed with a pack of our
//! own). A buffer that is not cleared reads back what was written last (OQ-8): when the last
//! frame ended on B, B is copied to A, and B keeps what was last written into it, which is what
//! a write leaves alone or blends onto there (a model that fits Iris 1.11.7's values in probe
//! packs of our own).
//! Pack buffers store rows as GL does (row 0 at the bottom: the translator negates clip y), so
//! the world is copied in upside down, and its reverse-Z depth becomes GL window depth.

use std::collections::HashMap;

use glam::{DMat4, DVec3};
use horizon_shaders::{Format, Group, Kind, Post, SHADOW_COLORS};

use super::pack_draw::DrawUniforms;
use super::pack_entities::{PROGRAMS, PackEntities};
use super::pack_images;
use super::pack_sky::PackSky;
use super::pack_textures;
use super::pack_uniforms::{self, FrameUniforms, PackInputs};

const BUFFERS: usize = horizon_shaders::BUFFERS;

mod packed;

#[cfg(test)]
mod world_target_tests;

#[cfg(test)]
mod celestial_blend_tests;

#[cfg(test)]
mod higher_shadowcolor_tests;

#[cfg(test)]
mod single_side_tests;

#[cfg(test)]
mod mixed_size_tests;

#[cfg(test)]
mod sky_disc_blend_tests;

#[cfg(test)]
mod undrawn_terrain_tests;

#[cfg(test)]
mod shadow_faces_tests;

#[cfg(test)]
mod r11f_image_tests;

#[cfg(test)]
mod forced_flip_tests;

#[cfg(test)]
mod weather_depth_tests;

#[cfg(test)]
mod float_into_integer_tests;

#[cfg(test)]
mod custom_type_tests;

#[cfg(test)]
mod sky_depth_tests;

#[cfg(test)]
mod runtime_length_tests;

#[cfg(test)]
mod translucent_color_tests;

#[cfg(test)]
mod noise_resolution_tests;

/// The texture behind a pack sampler name.
#[derive(Clone, Copy, PartialEq)]
enum Source {
    Color(usize),
    Depth,
    /// `depthtex1`: depth without translucent terrain (a copy taken before it).
    Depth1,
    /// `depthtex2`: depth without translucent terrain and without the first-person hand.
    Depth2,
    Noise,
    /// A 1×1 stand-in for names this slice does not provide yet.
    Dummy,
    /// A 1×1 flat normal (128, 128, 255) stand-in when no normal map is present.
    Normals,
    /// A 1×1 black (0, 0, 0, 0) stand-in when no specular map is present.
    Specular,
    /// `texture.<stage>.<name>`: the pack's own texture, filtered and wrapped as its
    /// `.mcmeta` says.
    Custom {
        index: usize,
        blur: bool,
        clamp: bool,
    },
    /// `image.<name>` through its sampler name.
    Image(usize),
    /// A 1×1 depth of 1.0 behind a comparison sampler, until shadow maps exist: no shadow.
    Shadow,
    /// The block atlas (pack_atlas.rs), in gbuffers programs.
    Atlas,
    Lightmap,
    /// `shadowtex0`/`shadowtex1` (6.3), with depth comparison (`sampler2DShadow`) or not.
    ShadowDepth(usize, bool),
    /// `shadowcolor0/1` read outside the shadow programs (in them they are `Color`).
    ShadowColor(usize),
}

#[derive(Clone)]
struct Binding {
    source: Source,
    binding: u32,
    /// A filtering sampler (the texture is filterable).
    filtering: bool,
    /// The view's dimension: 2D, or 1D and 3D for custom textures and their stand-ins.
    dimension: wgpu::TextureViewDimension,
    /// Where this texture's sampler object is bound: the first texture of each kind binds it
    /// (`binding + 1`), the others share it (devices have as few as 16 sampler slots).
    sampler: u32,
}

/// The sampler objects a pass binds, one slot per kind it uses.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum SamplerKind {
    Linear,
    Nearest,
    Repeat,
    NearestRepeat,
    /// The block atlas: nearest texels, blended between the two nearest levels.
    Atlas,
    /// colortex buffers: linear, blended between the two nearest levels.
    Buffer,
    Comparison,
}

impl SamplerKind {
    fn of(source: Source, filtering: bool) -> Self {
        match source {
            Source::Color(_) | Source::ShadowColor(_) | Source::Dummy if !filtering => {
                Self::Nearest
            }
            Source::Depth | Source::Depth1 | Source::Depth2 | Source::ShadowDepth(_, false) => {
                Self::Nearest
            }
            Source::Noise => Self::Repeat,
            Source::Shadow | Source::ShadowDepth(_, true) => Self::Comparison,
            // Blocks look as the game draws them: nearest texels. Between levels, Iris 1.11.7
            // blends `gtexture`'s two nearest (our probe packs, #9307: a read at level 1.25 is
            // levels 1 and 2 mixed 3:1).
            Source::Atlas if filtering => Self::Atlas,
            Source::Atlas | Source::Normals | Source::Specular => Self::NearestRepeat,
            Source::Image(_) if filtering => Self::Linear,
            Source::Image(_) => Self::Nearest,
            Source::Custom { blur, clamp, .. } => match (blur && filtering, clamp) {
                (true, true) => Self::Linear,
                (false, true) => Self::Nearest,
                (true, false) => Self::Repeat,
                (false, false) => Self::NearestRepeat,
            },
            // Iris 1.11.7 blends a buffer's two nearest mipmap levels (our probe packs, #9307:
            // every colortex read at levels 1.25 and 2.5 in composite and final).
            Source::Color(_) => Self::Buffer,
            Source::ShadowColor(_) | Source::Dummy | Source::Lightmap => Self::Linear,
        }
    }

    fn binding_type(self) -> wgpu::SamplerBindingType {
        match self {
            Self::Linear | Self::Repeat | Self::Atlas | Self::Buffer => {
                wgpu::SamplerBindingType::Filtering
            }
            Self::Nearest | Self::NearestRepeat => wgpu::SamplerBindingType::NonFiltering,
            Self::Comparison => wgpu::SamplerBindingType::Comparison,
        }
    }
}

/// Points every sampler object of `module` at the slot `slot` gives its binding, so that
/// samplers sharing a slot become one variable (two variables may not share a binding).
fn share_samplers(module: &mut naga::Module, slot: &HashMap<u32, u32>) {
    let mut first = HashMap::new();
    let mut same = HashMap::new();
    for (h, g) in module.global_variables.iter_mut() {
        let Some(b) = g.binding.as_mut() else {
            continue;
        };
        if !matches!(module.types[g.ty].inner, naga::TypeInner::Sampler { .. }) {
            continue;
        }
        let Some(&to) = slot.get(&b.binding) else {
            continue;
        };
        b.binding = to;
        match first.entry(to) {
            std::collections::hash_map::Entry::Occupied(e) => _ = same.insert(h, *e.get()),
            std::collections::hash_map::Entry::Vacant(e) => _ = e.insert(h),
        }
    }
    if same.is_empty() {
        return;
    }
    let fix = |expressions: &mut naga::Arena<naga::Expression>| {
        for (_, e) in expressions.iter_mut() {
            if let naga::Expression::GlobalVariable(g) = e
                && let Some(&to) = same.get(g)
            {
                *g = to;
            }
        }
    };
    for (_, f) in module.functions.iter_mut() {
        fix(&mut f.expressions);
    }
    for ep in &mut module.entry_points {
        fix(&mut ep.function.expressions);
    }
}

/// The sky's geometry and the programs that draw it: `gbuffers_skybasic` (disc, sunset, stars,
/// void) and `gbuffers_skytextured` (sun, moon), each with its fallbacks. A part without a
/// program is Horizon's own.
struct Sky {
    geometry: PackSky,
    basic: Option<Pass>,
    textured: Option<Pass>,
}

/// Entities, block entities, translucent entity parts and particles a pack draws
/// (`pack_entities::ENTITY`... order), after terrain or, `late`, after deferred; Horizon draws
/// the others.
struct Entities {
    geometry: PackEntities,
    passes: [Option<Pass>; PROGRAMS],
    late: [bool; PROGRAMS],
    /// What draws entities and block entities into the shadow map (2.4).
    shadow: [Option<Pass>; 2],
    /// Only the player, only block entities that emit light (3.2).
    only: (bool, bool),
}

impl Entities {
    /// Whether a gbuffers program draws some of them (else only the shadow pass does).
    fn drawn(&self) -> bool {
        self.passes.iter().any(Option::is_some)
    }
}

/// What an image binds: a buffer (`colorimgN`, the side its sampler reads) or one of the pack's.
#[derive(Clone, Copy, PartialEq)]
enum Image {
    Color(usize),
    Custom(usize),
    /// The copy of a buffer or a pack image taken before a dispatch, which its loads read.
    ColorBefore(usize),
    CustomBefore(usize),
    /// An image no `image.*` gives: its stand-in of the declared format and dimension
    /// (`pack_images::StandIn`).
    StandIn(wgpu::TextureFormat, wgpu::TextureViewDimension),
}

impl Image {
    fn buffer(self) -> Option<usize> {
        match self {
            Image::Color(b) => Some(b),
            _ => None,
        }
    }
}

/// The buffers and the pack images (masks) `images` read as copies taken before a dispatch.
fn befores(images: &[(u32, Image)]) -> (u16, u64) {
    images
        .iter()
        .fold((0, 0), |(colors, customs), i| match i.1 {
            Image::ColorBefore(b) => (colors | 1 << b, customs),
            Image::CustomBefore(k) => (colors, customs | 1 << k),
            _ => (colors, customs),
        })
}

/// The color buffers of `images`, as a mask.
fn image_buffers(images: &[(u32, Image)]) -> u16 {
    images
        .iter()
        .filter_map(|i| i.1.buffer())
        .fold(0, |m, b| m | 1 << b)
}

/// The format of the sink of passes that write only images.
const SINK: wgpu::TextureFormat = wgpu::TextureFormat::R8Unorm;

/// What packs use of the device beyond drawing. OpenGL ES and older drivers lack some of it: a
/// program that needs what is missing is skipped with a note, and the game runs on without it.
#[derive(Clone, Copy)]
struct Gpu {
    compute: bool,
    /// Images and storage buffers written by fragment shaders.
    fragment_storage: bool,
    /// Storage read and written by vertex shaders (a geometry stage).
    vertex_storage: bool,
    /// `clear_texture`; without it images are cleared by copying zeros.
    clear: bool,
    adapter_formats: bool,
}

impl Gpu {
    fn of(adapter: &wgpu::Adapter, features: wgpu::Features) -> Self {
        use wgpu::DownlevelFlags as D;
        let flags = adapter.get_downlevel_capabilities().flags;
        let mut gpu = Gpu {
            compute: flags.contains(D::COMPUTE_SHADERS),
            fragment_storage: flags.contains(D::FRAGMENT_WRITABLE_STORAGE),
            vertex_storage: flags.contains(D::VERTEX_STORAGE)
                && features.contains(wgpu::Features::VERTEX_WRITABLE_STORAGE),
            clear: features.contains(wgpu::Features::CLEAR_TEXTURE),
            adapter_formats: features
                .contains(wgpu::Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES),
        };
        // `HORIZON_SHADER_PACK_WITHOUT=compute,fragment-storage,vertex-storage,clear` tries the
        // fallbacks on a device that has it all.
        let without = std::env::var("HORIZON_SHADER_PACK_WITHOUT").unwrap_or_default();
        for w in without.split(',') {
            match w.trim() {
                "compute" => gpu.compute = false,
                "fragment-storage" => gpu.fragment_storage = false,
                "vertex-storage" => gpu.vertex_storage = false,
                "clear" => gpu.clear = false,
                _ => {}
            }
        }
        gpu
    }

    /// Whether textures of `format` can be bound as images.
    fn storable(
        self,
        adapter: &wgpu::Adapter,
        format: wgpu::TextureFormat,
        features: wgpu::Features,
    ) -> bool {
        let usages = if self.adapter_formats {
            adapter.get_texture_format_features(format).allowed_usages
        } else {
            format.guaranteed_format_features(features).allowed_usages
        };
        (self.compute || self.fragment_storage)
            && usages.contains(wgpu::TextureUsages::STORAGE_BINDING)
    }

    /// The stages that may bind images and storage buffers.
    fn storage_stages(self) -> wgpu::ShaderStages {
        let mut stages = wgpu::ShaderStages::NONE;
        if self.compute {
            stages |= wgpu::ShaderStages::COMPUTE;
        }
        if self.fragment_storage {
            stages |= wgpu::ShaderStages::FRAGMENT;
        }
        if self.vertex_storage {
            stages |= wgpu::ShaderStages::VERTEX;
        }
        stages
    }
}

/// A pass's pipeline: a draw, or a compute dispatch.
#[derive(Clone)]
enum Pipeline {
    Render(wgpu::RenderPipeline),
    Compute {
        pipeline: wgpu::ComputePipeline,
        /// `workGroups`, else `workGroupsRender` and the local size.
        groups: Option<[u32; 3]>,
        render: [f32; 2],
        local: [u32; 3],
    },
}

#[derive(Clone)]
struct Pass {
    name: String,
    group: Group,
    pipeline: Pipeline,
    /// The same program as vanilla blends the sky's parts (`PostProgram::vanilla_sky_blends`):
    /// without blending for the disc and the void, then with `STARS_BLEND` for the stars.
    sky_parts: Option<[wgpu::RenderPipeline; 2]>,
    layout: wgpu::BindGroupLayout,
    ubo: wgpu::Buffer,
    block: Vec<u8>,
    /// (index into `pack_uniforms::NAMES`, declared type, offset).
    uniforms: Vec<(Option<usize>, String, u32)>,
    samplers: Vec<Binding>,
    /// (location, buffer) of each output that reaches a buffer.
    writes: Vec<(u32, usize)>,
    /// Buffers flipped after the pass.
    flip: u16,
    /// `hz_depth_linear`: which of the depth samplers a plain sampler reads filter.
    depth_linear: [f32; 4],
    /// Buffers sampled, for the bind group cache key.
    reads: u16,
    /// shadowcolor buffers sampled by a program of the other buffers.
    shadow_reads: u16,
    /// Buffers written.
    written: u16,
    /// Buffers whose mip chain is regenerated right before it (6.1.7).
    mipmaps: u16,
    /// Reads the per-draw block (`DrawUniforms`).
    draw: bool,
    /// What `texture` reads instead of the block atlas.
    atlas: Option<wgpu::TextureView>,
    material_maps: bool,
    /// Reads `mc_Entity`'s table (`PackPost::block_ids`).
    ids: bool,
    /// Bind groups by the sides read (bits 0–15), the buffers whose mip chain is current (bits
    /// 16–31) and the shadowcolor sides read (bits 32–33).
    groups: HashMap<u64, wgpu::BindGroup>,
    /// The same for End portal draws, with `end_portal.png` bound as the atlas.
    portal_groups: HashMap<u64, wgpu::BindGroup>,
    /// (binding, target) of each image: for a buffer, the side its sampler reads.
    images: Vec<(u32, Image)>,
    /// Binds the pack's storage buffers in set 4.
    buffers: bool,
    /// Buffers both sampled and bound as images: sampled from a copy taken first, as wgpu binds
    /// a texture one way per pass.
    copies: u16,
    /// Compute programs dispatched before this pass.
    compute: Vec<Pass>,
    /// Writes no buffer, only images: draws into `Targets::sink`.
    sink: bool,
    /// Instances to draw for a geometry stage folded into the vertex stage; 0 without one.
    geometry: u32,
    /// The pack's images (bit n: `image.*` n) this pass samples and stores: sampled from a copy
    /// taken just before it.
    image_copies: u64,
    /// The pack's images a compute pass's loads read as they were before it: copied first.
    snapshots: u64,
    /// `scale.<program>` (scale, offset x, offset y): the pass draws only that part of its
    /// buffers and keeps the rest.
    viewport: Option<[f32; 3]>,
}

impl Pass {
    fn render(&self) -> &wgpu::RenderPipeline {
        match &self.pipeline {
            Pipeline::Render(p) => p,
            Pipeline::Compute { .. } => unreachable!("a compute pass draws nothing"),
        }
    }
}

/// One side of a buffer with mipmaps.
struct Mips {
    /// Every level, for sampling.
    all: wgpu::TextureView,
    /// From the second level down: its attachment, and a bind group over the level above.
    levels: Vec<(wgpu::TextureView, wgpu::BindGroup)>,
}

/// The picture a pack's `final` draws at the world's size while the window is another: it is
/// copied onto the window once, after the pack's passes, as the game's own post pass does
/// without a pack.
struct FinalTarget {
    size: [u32; 2],
    view: wgpu::TextureView,
}

struct Targets {
    size: [u32; 2],
    /// A and B of each buffer used.
    color: [Option<[wgpu::TextureView; 2]>; BUFFERS],
    depth: wgpu::TextureView,
    depth_texture: wgpu::Texture,
    /// Pre-translucent depth for `depthtex1`, also serving `depthtex2` without a shader hand.
    depth1: Option<(wgpu::Texture, wgpu::TextureView)>,
    /// `depthtex2`, when a program after the hand reads it without the hand.
    depth2: Option<(wgpu::Texture, wgpu::TextureView)>,
    /// Both sides' mip chains of the buffers with mipmaps (`color` is their first level).
    mips: [Option<[Mips; 2]>; BUFFERS],
    /// Where a pass that both samples and stores a buffer samples it from.
    scratch: [Option<wgpu::TextureView>; BUFFERS],
    /// Each buffer's size: the render size, or its `size.buffer`.
    sizes: [[u32; 2]; BUFFERS],
    /// The attachment of passes that write only images.
    sink: wgpu::TextureView,
}

/// What a pack's gbuffers programs read and draw from the game's world.
pub(super) struct World<'a> {
    /// The block atlas and its size.
    pub atlas: Option<(wgpu::TextureView, [u32; 2])>,
    /// Of a stitched block atlas, each sprite layer's place (`PackAtlas::rects_table`); empty
    /// for a grid.
    pub sprite_rects: &'a [u32],
    pub material_maps: Option<[wgpu::TextureView; 2]>,
    pub lightmap: wgpu::TextureView,
    /// Terrain's geometry, when the pack draws it.
    pub terrain: Option<Geometry<'a>>,
    /// Of each block sprite layer, `mc_Entity` (id, then 1 for fluids or 0) and `at_midBlock.w`
    /// (its block's light): three values a layer.
    pub block_ids: &'a [i32],
    /// The atlas of the sun and moon's sprites.
    pub celestial: Option<wgpu::TextureView>,
    pub gui_size: [u32; 2],
    /// Set 1 of the clouds' draw: their offset and palette.
    pub clouds: Option<&'a wgpu::BindGroupLayout>,
    /// The game can hand its rain and snow to the pack.
    pub weather: bool,
    /// The game can hand its first-person hand to the pack.
    pub hand: bool,
}

/// This frame's block outline, when the pack draws it: the outline buffer (one instance per
/// edge, pack_lines.rs) and the edges in it.
pub(super) struct Lines {
    pub edges: wgpu::Buffer,
    pub count: u32,
}

/// This frame's clouds, when the pack draws them: their vertices, uniform, quad indices and
/// quad count.
pub(super) struct Clouds {
    pub vertices: wgpu::Buffer,
    pub group: wgpu::BindGroup,
    pub indices: wgpu::Buffer,
    pub quads: u32,
}

/// This frame's rain and snow, when the pack draws them: their vertices, the quad indices, the
/// quads of each kind (rain, snow), each kind's sprite, and a number that changes with them.
pub(super) struct Weather {
    pub vertices: wgpu::Buffer,
    pub indices: wgpu::Buffer,
    pub quads: [std::ops::Range<u32>; 2],
    pub sprites: [Option<wgpu::TextureView>; 2],
    pub generation: u64,
}

/// Shadow cache: the key of the shadow map the shadow targets hold. Only the
/// shadow passes write those targets, so while the key matches they still hold
/// it and the pass is skipped.
#[derive(Default)]
struct ShadowCache {
    key: Option<ShadowCacheKey>,
}

/// Key for validating the shadow cache.
/// Must match the cache state exactly to be considered valid.
#[derive(Clone, Copy, Debug, PartialEq)]
struct ShadowCacheKey {
    sun_angle: f32,
    moon_angle: f32,
    snapped_camera: Option<DVec3>,
    shadow_settings_hash: u64,
    shadow_scene_revision: (u64, u64),
    ticks: u64,
    day_time: i64,
    shadow_uniforms_hash: u64,
}

pub struct PackPost {
    setup_pending: bool,
    pub notes: Vec<String>,
    passes: Vec<Pass>,
    /// Shadow cache for temporal reprojection.
    shadow_cache: ShadowCache,
    /// What draws terrain (`gbuffers_terrain` or its fallback), between `prepare` and `deferred`;
    /// when the pack draws terrain in two parts, solid terrain only (`gbuffers_terrain_solid` or
    /// its fallback).
    terrain: Option<Pass>,
    /// What draws cutout terrain (`gbuffers_terrain_cutout` or its fallback), after `terrain`,
    /// when the pack draws terrain in two parts. A pack with no program for one part has only
    /// the other.
    terrain_cutout: Option<Pass>,
    /// What draws translucent terrain (`gbuffers_water` or its fallback), after `deferred`.
    water: Option<Pass>,
    /// The pack has no program for solid and cutout terrain, or for translucent terrain:
    /// nothing draws it (`horizon_shaders::Post::terrain_undrawn`).
    terrain_undrawn: bool,
    water_undrawn: bool,
    /// What draws the sky (`gbuffers_skybasic` or its fallback), before terrain, and its vertices.
    sky: Option<Sky>,
    /// What draws entities and block entities, after terrain, and their vertices.
    entities: Option<Entities>,
    /// What draws the clouds, after entities, and this frame's clouds.
    clouds: Option<Pass>,
    /// What draws the block outline (`gbuffers_line`, else `gbuffers_basic`), and this frame's.
    lines: Option<Pass>,
    line_draw: Option<Lines>,
    cloud_draw: Option<Clouds>,
    /// The pack draws the clouds (`gbuffers_clouds` or its fallback), even with a program that
    /// writes nothing (a pack drawing its own clouds discards vanilla's), or `clouds=off`: the
    /// game's clouds are not drawn.
    hides_clouds: bool,
    /// The pack has a sky (`gbuffers_skybasic`) or sun and moon (`gbuffers_skytextured`)
    /// program: vanilla's stays hidden even when the program writes nothing.
    hides_sky: bool,
    hides_celestial: bool,
    /// `colortex0ClearColor`, when the pack sets it.
    sky_clear: Option<[f32; 4]>,
    /// What draws rain and snow (the same program, each with its kind's sprite as `texture`),
    /// after translucent terrain, this frame's weather and the sprites' generation bound.
    weather: Option<[Pass; 2]>,
    weather_draw: Option<Weather>,
    weather_generation: u64,
    /// What draws the first-person hand (`gbuffers_hand` or its fallback), after the opaque
    /// world, and its vertices.
    hand: Option<(Pass, super::pack_hand::PackHand)>,
    /// `gbuffers_hand_water` (or its fallback): the hand's translucent quads, after deferred.
    hand_water: Option<Pass>,
    /// The pre-translucent copy: `depthtex1`, or `depthtex2` without a shader hand.
    depth1: bool,
    /// A program after the hand reads `depthtex2`: it is copied before the hand.
    depth2: bool,
    /// The shadow buffers, when the shadow pass runs (2.5), and the `shadowcomp` programs.
    shadow_targets: Option<super::pack_shadow::ShadowTargets>,
    /// What draws terrain from the light (`shadow`), and translucent terrain (`shadow_water`).
    shadow_terrain: Option<Pass>,
    /// `end_portal.png`'s atlas rect this frame, and a texture of its own copied from it that
    /// End portal draws bind as their atlas.
    portal_rect: [f32; 4],
    portal_texture: Option<([f32; 4], wgpu::TextureView)>,
    shadow_water: Option<Pass>,
    shadow_comp: Vec<Pass>,
    /// Buffers with mipmaps (filterable ones a program asks them for).
    mipped: u16,
    /// Buffers whose mip chain matches their first level, this frame.
    mips_valid: u16,
    /// The downsampling pipeline of each format with mipmaps.
    down: HashMap<wgpu::TextureFormat, (wgpu::RenderPipeline, wgpu::BindGroupLayout)>,
    /// The per-draw uniforms of gbuffers programs.
    draws: DrawUniforms,
    /// Set 3 of flat terrain (`mc_Entity`'s table, then the sprites' places) and its bind group
    /// for each arena page, with the page buffer it was made for.
    block_ids: Option<(wgpu::BindGroupLayout, wgpu::Buffer, wgpu::Buffer)>,
    page_groups: HashMap<u32, (wgpu::Buffer, wgpu::BindGroup)>,
    gui_size: [u32; 2],
    /// GPU time of each pass, while the renderer asks for it (`pack_timing.rs`).
    pub timing: Option<super::pack_timing::PassTiming>,
    /// `depthtex0` at the screen's centre, read back for `centerDepthSmooth`.
    center_depth: Option<super::pack_center_depth::CenterDepth>,
    /// `shaders.properties`' custom uniforms, after the frame's in `uniforms.values`.
    custom: horizon_shaders::Custom,
    atlas: Option<wgpu::TextureView>,
    material_maps: Option<[wgpu::TextureView; 2]>,
    lightmap: wgpu::TextureView,
    deferred_pre: u16,
    composite_pre: u16,
    formats: [wgpu::TextureFormat; BUFFERS],
    clears: [Option<[f32; 4]>; BUFFERS],
    /// Buffers of a three-channel format, and whether their textures are new: then they get
    /// alpha 1 before the next frame.
    rgb: u16,
    rgb_unset: bool,
    used: u16,
    /// Buffers some pass draws into the "alt" side of or flips; the others' B is their A.
    doubled: u16,
    /// `size.buffer.*`.
    sizes: [Option<horizon_shaders::BufferSize>; BUFFERS],
    /// Buffers bound as images, and those also copied for a pass that samples them.
    storage: u16,
    copied: u16,
    /// Bit n set: B is buffer n's "main".
    main: u16,
    targets: Option<Targets>,
    noise: wgpu::TextureView,
    dummy: wgpu::TextureView,
    dummy_black: wgpu::TextureView,
    dummy_normal: wgpu::TextureView,
    dummy_3d: wgpu::TextureView,
    dummy_1d: wgpu::TextureView,
    /// `image.*` and `bufferObject.*`.
    images: Vec<pack_images::PackImage>,
    /// Set 5 of programs with a geometry stage.
    indices: Option<wgpu::BindGroupLayout>,
    shadow_culling: horizon_shaders::ShadowCulling,
    gpu: Gpu,
    /// Zeros to clear images by copying, where the device has no `clear_texture`.
    zeros: Option<wgpu::Buffer>,
    /// What the images no `image.*` gives bind.
    stand_ins: Vec<pack_images::StandIn>,
    dump: Option<pack_images::Dump>,
    buffers: pack_images::Buffers,
    textures: Vec<pack_textures::PackTexture>,
    /// The stand-in shadow map and its comparison sampler; cleared on the first frame.
    shadow: (wgpu::TextureView, wgpu::Sampler),
    shadow_cleared: bool,
    linear: wgpu::Sampler,
    nearest: wgpu::Sampler,
    repeat: wgpu::Sampler,
    nearest_repeat: wgpu::Sampler,
    atlas_sampler: wgpu::Sampler,
    /// colortex buffers' (`SamplerKind::Buffer`).
    buffer_sampler: wgpu::Sampler,
    /// None when it would write nothing: colortex0 holds integers and there is no depth to copy
    /// nor buffer to clear.
    blit: Option<(wgpu::RenderPipeline, wgpu::BindGroupLayout)>,
    /// The blit also writes depthtex0 (some program reads it) and clears these buffers.
    blit_depth: bool,
    blit_clears: Vec<usize>,
    /// Buffers cleared in passes of their own (more than the blit's eight attachments).
    other_clears: Vec<usize>,
    /// Source views identify the world target, including replacements at the same size.
    blit_group: Option<([wgpu::TextureView; 2], wgpu::BindGroup)>,
    /// Shows colortex0 when the pack has no `final` (OQ-5).
    copy: (wgpu::RenderPipeline, wgpu::BindGroupLayout),
    /// Copies the picture `final` drew at the world's size onto the window: no flip, it is
    /// already in the screen's rows.
    upscale: (wgpu::RenderPipeline, wgpu::BindGroupLayout),
    /// The picture `final` draws while the world's size is not the window's.
    final_target: Option<FinalTarget>,
    screen: wgpu::TextureFormat,
    uniforms: FrameUniforms,
}

/// The wgpu format of a pack format (6.7), the nearest one wgpu can render to.
fn texture_format(f: Format, features: wgpu::Features) -> wgpu::TextureFormat {
    use wgpu::TextureFormat as T;
    let pick = |r, rg, rgba| match f.channels {
        1 => r,
        2 => rg,
        _ => rgba,
    };
    let half = pick(T::R16Float, T::Rg16Float, T::Rgba16Float);
    match (f.kind, f.bits) {
        (Kind::Unorm, 10) => T::Rgb10a2Unorm,
        (Kind::Unorm, 16)
            if features.contains(
                wgpu::Features::TEXTURE_FORMAT_16BIT_NORM
                    | wgpu::Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES,
            ) =>
        {
            pick(T::R16Unorm, T::Rg16Unorm, T::Rgba16Unorm)
        }
        (Kind::Unorm, b) if b <= 8 => pick(T::R8Unorm, T::Rg8Unorm, T::Rgba8Unorm),
        (Kind::Float, 32) => pick(T::R32Float, T::Rg32Float, T::Rgba32Float),
        (Kind::Float, 11) if features.contains(wgpu::Features::RG11B10UFLOAT_RENDERABLE) => {
            T::Rg11b10Ufloat
        }
        (Kind::Int, 8) => pick(T::R8Sint, T::Rg8Sint, T::Rgba8Sint),
        (Kind::Int, 16) => pick(T::R16Sint, T::Rg16Sint, T::Rgba16Sint),
        (Kind::Int, _) => pick(T::R32Sint, T::Rg32Sint, T::Rgba32Sint),
        (Kind::Uint, 8) => pick(T::R8Uint, T::Rg8Uint, T::Rgba8Uint),
        (Kind::Uint, 10) => T::Rgb10a2Uint,
        (Kind::Uint, 16) => pick(T::R16Uint, T::Rg16Uint, T::Rgba16Uint),
        (Kind::Uint, _) => pick(T::R32Uint, T::Rg32Uint, T::Rgba32Uint),
        // Snorm, 16-bit norm without the feature, RGB9_E5: half floats hold them.
        _ => half,
    }
}

/// `format` where `renders` says it can be a colour target, else the nearest float format that can:
/// the same channels at 16 bits, then `Rgba8Unorm`. On OpenGL the 32-bit and 16-bit float formats
/// render only with `GL_EXT_color_buffer_float`/`half_float`, and wgpu aborts on creating a target
/// without. Other formats pass through. Float colour buffers are only sampled (never blended), so
/// RENDER_ATTACHMENT is the only usage that decides.
fn renderable_float(
    format: wgpu::TextureFormat,
    renders: impl Fn(wgpu::TextureFormat) -> bool,
) -> wgpu::TextureFormat {
    use wgpu::TextureFormat as T;
    let half = match format {
        T::R32Float | T::R16Float => T::R16Float,
        T::Rg32Float | T::Rg16Float => T::Rg16Float,
        T::Rgba32Float | T::Rgba16Float => T::Rgba16Float,
        _ => return format,
    };
    if renders(format) {
        format
    } else if renders(half) {
        half
    } else {
        T::Rgba8Unorm
    }
}

/// [`texture_format`] made renderable on this adapter (see [`renderable_float`]).
fn target_format(
    f: Format,
    features: wgpu::Features,
    adapter: &wgpu::Adapter,
) -> wgpu::TextureFormat {
    renderable_float(texture_format(f, features), |format| {
        (adapter.get_texture_format_features(format).allowed_usages)
            .contains(wgpu::TextureUsages::RENDER_ATTACHMENT)
    })
}

/// The SNORM wgpu format of SNORM pack format `f`, where this adapter can render to it. Iris
/// stores such a buffer clamped to [-1, 1] and quantised to its bits; elsewhere the half floats of
/// [`texture_format`] stand in.
fn snorm_format(
    f: Format,
    features: wgpu::Features,
    adapter: &wgpu::Adapter,
) -> Option<wgpu::TextureFormat> {
    use wgpu::TextureFormat as T;
    let format = match (f.kind, f.bits, f.channels) {
        (Kind::Snorm, 8, 1) => T::R8Snorm,
        (Kind::Snorm, 8, 2) => T::Rg8Snorm,
        (Kind::Snorm, 8, _) => T::Rgba8Snorm,
        (Kind::Snorm, 16, 1) => T::R16Snorm,
        (Kind::Snorm, 16, 2) => T::Rg16Snorm,
        (Kind::Snorm, 16, _) => T::Rgba16Snorm,
        _ => return None,
    };
    let mut needs = wgpu::Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES;
    if f.bits == 16 {
        needs |= wgpu::Features::TEXTURE_FORMAT_16BIT_NORM;
    }
    let renders = (adapter.get_texture_format_features(format).allowed_usages)
        .contains(wgpu::TextureUsages::RENDER_ATTACHMENT);
    (features.contains(needs) && renders).then_some(format)
}

/// The texture behind sampler `name` in a program of the shadow buffers (`shadow_space`: buffer
/// n is shadowcolor n) or of the others, when the shadow pass runs (`shadow_pass`). `shadow` is
/// `shadowtex1` when the program also declares `watershadow` (6.3).
/// Whether a `ty` sampler can read a custom texture of `sample_type`: custom textures hold
/// colours, never depth to compare against.
fn fits(ty: &str, sample_type: wgpu::TextureSampleType) -> bool {
    use wgpu::TextureSampleType as T;
    !ty.contains("Shadow")
        && match sample_type {
            T::Sint => ty.starts_with('i'),
            T::Uint => ty.starts_with('u'),
            _ => ty.starts_with('s'),
        }
}

/// `hz_depth_linear` for a program: 1 for each of shadowtex0, shadowtex1, `shadow` and
/// `watershadow` whose depth filters (`linear`, bit n: shadowtex n). `shadow` is shadowtex1 in a
/// program that also declares `watershadow` (6.3).
fn depth_linear(interface: &horizon_shaders::Interface, linear: u8) -> [f32; 4] {
    let watershadow = interface.samplers.iter().any(|s| s.name == "watershadow");
    [0, 1, usize::from(watershadow), 0].map(|n| f32::from(linear >> n & 1))
}

fn source(name: &str, shadow_space: bool, shadow_pass: bool, watershadow: bool) -> Source {
    let shadow_color = horizon_shaders::shadow_color(name);
    let shadow_depth = match name {
        "shadowtex0" | "watershadow" | "shadowtex0HW" => Some(0),
        "shadowtex1" | "shadowtex1HW" => Some(1),
        "shadow" => Some(usize::from(watershadow)),
        _ => None,
    };
    if shadow_pass {
        if let Some(n) = shadow_depth {
            return Source::ShadowDepth(n, name.ends_with("HW"));
        }
        if let Some(n) = shadow_color {
            return if shadow_space {
                Source::Color(n)
            } else {
                Source::ShadowColor(n)
            };
        }
    }
    if shadow_space && horizon_shaders::buffer_index(name).is_some() {
        // colortex buffers are not this pass's.
        return Source::Dummy;
    }
    match name {
        "depthtex0" | "gdepthtex" => Source::Depth,
        "depthtex1" => Source::Depth1,
        "depthtex2" => Source::Depth2,
        "noisetex" => Source::Noise,
        "texture" | "gtexture" | "tex" => Source::Atlas,
        "lightmap" => Source::Lightmap,
        "normals" => Source::Normals,
        "specular" => Source::Specular,
        n => horizon_shaders::buffer_index(n).map_or(Source::Dummy, Source::Color),
    }
}

/// `size`×`size` RGB white noise (6.4), from a fixed seed so every run looks the same.
fn noise(device: &wgpu::Device, queue: &wgpu::Queue, size: u32) -> wgpu::TextureView {
    let size = size.min(device.limits().max_texture_dimension_2d);
    let mut state = 0x2545_f491_4f6c_dd1du64;
    let texels: Vec<u8> = (0..size as usize * size as usize * 4)
        .map(|i| {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            if i % 4 == 3 { 255 } else { (state >> 56) as u8 }
        })
        .collect();
    texture(device, queue, "Shader pack noisetex", size, &texels)
}

fn texture(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    label: &str,
    side: u32,
    texels: &[u8],
) -> wgpu::TextureView {
    let size = wgpu::Extent3d {
        width: side,
        height: side,
        depth_or_array_layers: 1,
    };
    let tex = device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    queue.write_texture(
        tex.as_image_copy(),
        texels,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(side * 4),
            rows_per_image: None,
        },
        size,
    );
    tex.create_view(&Default::default())
}

fn sampler(
    device: &wgpu::Device,
    filter: wgpu::FilterMode,
    address: wgpu::AddressMode,
) -> wgpu::Sampler {
    device.create_sampler(&wgpu::SamplerDescriptor {
        label: Some("Shader pack"),
        address_mode_u: address,
        address_mode_v: address,
        address_mode_w: address,
        mag_filter: filter,
        min_filter: filter,
        ..Default::default()
    })
}

/// Runs `create` and returns its validation error, if any, or the backend's (a shader the
/// driver's compiler refuses is an internal error). A panic while it runs is an error too:
/// naga's backends turn a module they cannot write into a panic as the driver gets the pipeline
/// (#3525), which a pack's program must not turn into a crash.
fn checked<T>(device: &wgpu::Device, create: impl FnOnce() -> T) -> Result<T, String> {
    let validation = device.push_error_scope(wgpu::ErrorFilter::Validation);
    let internal = device.push_error_scope(wgpu::ErrorFilter::Internal);
    let value = crate::crash::contain(create);
    let internal = pollster::block_on(internal.pop());
    match pollster::block_on(validation.pop()).or(internal) {
        Some(e) => {
            // The details are in the error's description or its sources.
            let mut message = match &e {
                wgpu::Error::Validation { source, .. } => format!("{source:?}"),
                e => e.to_string(),
            };
            let mut source = std::error::Error::source(&e);
            while let Some(s) = source {
                message = format!("{message}: {s}");
                source = s.source();
            }
            Err(message)
        }
        None => value.map_err(|panic| format!("the shader compiler failed: {panic}")),
    }
}

/// A full-screen helper pipeline of ours (`pack_post.wgsl`).
fn helper(
    device: &wgpu::Device,
    vertex: &str,
    entry: &str,
    entries: &[wgpu::BindGroupLayoutEntry],
    targets: &[Option<wgpu::ColorTargetState>],
    depth: Option<wgpu::DepthStencilState>,
) -> (wgpu::RenderPipeline, wgpu::BindGroupLayout) {
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("Shader pack helpers"),
        source: wgpu::ShaderSource::Wgsl(include_str!("pack_post.wgsl").into()),
    });
    let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some(entry),
        entries,
    });
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some(entry),
        bind_group_layouts: &[Some(&layout)],
        ..Default::default()
    });
    let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(entry),
        layout: Some(&pipeline_layout),
        vertex: wgpu::VertexState {
            module: &module,
            entry_point: Some(vertex),
            compilation_options: Default::default(),
            buffers: &[],
        },
        primitive: Default::default(),
        depth_stencil: depth,
        multisample: Default::default(),
        fragment: Some(wgpu::FragmentState {
            module: &module,
            entry_point: Some(entry),
            compilation_options: Default::default(),
            targets,
        }),
        multiview_mask: None,
        cache: None,
    });
    (pipeline, layout)
}

fn entry(binding: u32, ty: wgpu::BindingType) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::VERTEX_FRAGMENT | wgpu::ShaderStages::COMPUTE,
        ty,
        count: None,
    }
}

fn texture_entry(binding: u32, sample_type: wgpu::TextureSampleType) -> wgpu::BindGroupLayoutEntry {
    texture_entry_of(binding, sample_type, wgpu::TextureViewDimension::D2)
}

fn texture_entry_of(
    binding: u32,
    sample_type: wgpu::TextureSampleType,
    view_dimension: wgpu::TextureViewDimension,
) -> wgpu::BindGroupLayoutEntry {
    entry(
        binding,
        wgpu::BindingType::Texture {
            sample_type,
            view_dimension,
            multisampled: false,
        },
    )
}

/// `blend.<program>`'s factors as wgpu's, adding both terms.
fn blend_state(b: horizon_shaders::Blend) -> wgpu::BlendState {
    use horizon_shaders::Factor as F;
    use wgpu::BlendFactor as W;
    let f = |f| match f {
        F::Zero => W::Zero,
        F::One => W::One,
        F::SrcColor => W::Src,
        F::OneMinusSrcColor => W::OneMinusSrc,
        F::DstColor => W::Dst,
        F::OneMinusDstColor => W::OneMinusDst,
        F::SrcAlpha => W::SrcAlpha,
        F::OneMinusSrcAlpha => W::OneMinusSrcAlpha,
        F::DstAlpha => W::DstAlpha,
        F::OneMinusDstAlpha => W::OneMinusDstAlpha,
        F::SrcAlphaSaturate => W::SrcAlphaSaturated,
    };
    let component = |src, dst| wgpu::BlendComponent {
        src_factor: f(src),
        dst_factor: f(dst),
        operation: wgpu::BlendOperation::Add,
    };
    wgpu::BlendState {
        color: component(b[0], b[1]),
        alpha: component(b[2], b[3]),
    }
}

/// A buffer whose texels are floats (normalized or not), not integers.
fn holds_floats(format: wgpu::TextureFormat) -> bool {
    matches!(
        format.sample_type(None, None),
        Some(wgpu::TextureSampleType::Float { .. })
    )
}

fn target(format: wgpu::TextureFormat) -> Option<wgpu::ColorTargetState> {
    Some(wgpu::ColorTargetState {
        format,
        blend: None,
        write_mask: wgpu::ColorWrites::ALL,
    })
}

/// Color-attachment cost includes component alignment, not only the copied texel size.
fn attachment_bytes(bytes: u32, format: wgpu::TextureFormat) -> u32 {
    let alignment = format
        .target_component_alignment()
        .expect("color attachment");
    bytes.next_multiple_of(alignment) + format.target_pixel_byte_cost().expect("color attachment")
}

/// The viewport of `scale.<program>` (scale, offset x, offset y) on buffers of `size`, in whole
/// pixels, at least one and inside them. Buffer rows are GL's, so the offsets count from GL's
/// bottom-left corner as they are.
fn viewport(size: [u32; 2], [scale, x, y]: [f32; 3]) -> [f32; 4] {
    let [w, h] = size.map(|n| n as f32);
    let (x, y) = ((x * w).floor().min(w - 1.0), (y * h).floor().min(h - 1.0));
    [
        x,
        y,
        (scale * w).floor().clamp(1.0, w - x),
        (scale * h).floor().clamp(1.0, h - y),
    ]
}

fn attach(
    view: &wgpu::TextureView,
    ops: wgpu::Operations<wgpu::Color>,
) -> Option<wgpu::RenderPassColorAttachment<'_>> {
    Some(wgpu::RenderPassColorAttachment {
        view,
        resolve_target: None,
        depth_slice: None,
        ops,
    })
}

/// Which of the frame's terrain draws: the camera's solid or translucent ones, or the shadow
/// camera's.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum TerrainList {
    Solid,
    Translucent,
    ShadowSolid,
    ShadowTranslucent,
}

impl TerrainList {
    fn translucent(self) -> bool {
        matches!(self, Self::Translucent | Self::ShadowTranslucent)
    }
}

/// Draws the game's terrain of one list into a pass; calls its third argument with each arena
/// page before drawing from it.
/// Draws a list's terrain: indexed, or for a program with a geometry stage, without indices and
/// with that many instances (and its index buffer bound in set 5 by this layout).
pub(super) type TerrainDraw<'a> = dyn FnMut(
        &mut wgpu::RenderPass<'_>,
        TerrainList,
        &dyn Fn(&mut wgpu::RenderPass<'_>, u32),
        Option<(u32, &wgpu::BindGroupLayout)>,
    ) + 'a;

/// What the shadow camera sees of the terrain, for the game's draw lists: its view and
/// projection in wgpu's clip space, the direction toward the light, and how far from the camera
/// terrain is drawn (`shadowDistanceRenderMul`).
pub(super) struct ShadowView {
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

/// Set 3 of flat terrain, read by the vertex stage: the sprite layers' table (`mc_Entity`'s id
/// and fluid, `at_midBlock.w`; -1, -1, 0 when empty), per arena page, the page's vertices
/// (a vertex reads its quad's corners for its normal), and each sprite's place in a stitched
/// atlas (`rects`; zeros for a grid, which doesn't read them).
fn block_ids(
    device: &wgpu::Device,
    table: &[i32],
    rects: &[u32],
) -> (wgpu::BindGroupLayout, wgpu::Buffer, wgpu::Buffer) {
    let empty = [-1, -1, 0];
    let table = if table.is_empty() { &empty[..] } else { table };
    let buffer = |label, contents: &[u8]| {
        wgpu::util::DeviceExt::create_buffer_init(
            device,
            &wgpu::util::BufferInitDescriptor {
                label: Some(label),
                contents,
                usage: wgpu::BufferUsages::STORAGE,
            },
        )
    };
    let rects = if rects.is_empty() { &[0, 0][..] } else { rects };
    let ids = buffer("Shader pack block ids", bytemuck::cast_slice(table));
    let rects = buffer("Shader pack sprite rects", bytemuck::cast_slice(rects));
    let storage = |binding| wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::VERTEX,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Storage { read_only: true },
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        count: None,
    };
    let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("Shader pack terrain"),
        entries: &[storage(0), storage(1), storage(2)],
    });
    (layout, ids, rects)
}

/// The game's geometry a gbuffers program draws: its vertex buffers and, for flat draws
/// (flat.rs), the layout of their bind group 1.
pub(super) struct Geometry<'a> {
    pub buffers: &'a [Option<wgpu::VertexBufferLayout<'a>>],
    pub table: Option<&'a wgpu::BindGroupLayout>,
    /// Writes depth (the sky does not).
    pub depth_write: bool,
    /// What `texture` reads instead of the block atlas.
    pub atlas: Option<wgpu::TextureView>,
    /// This draw uses the block atlas's surface identities.
    pub material_maps: bool,
    /// `mc_Entity`'s table at set 3 (flat terrain).
    pub ids: Option<&'a wgpu::BindGroupLayout>,
    /// Back faces are culled, as for the game's own terrain.
    pub cull: bool,
}

/// What a pack brings besides its programs: custom textures, images and storage buffers.
struct Resources<'a> {
    textures: &'a [pack_textures::PackTexture],
    images: &'a [pack_images::PackImage],
    /// The images this device can't make.
    left_out: &'a [horizon_shaders::CustomImage],
    buffers: &'a pack_images::Buffers,
    /// Set 5 of a program with a geometry stage: the draw's indices.
    indices: Option<&'a wgpu::BindGroupLayout>,
    gpu: Gpu,
    /// The buffers whose format can be bound as an image.
    storable: u16,
    /// The buffers of a three-channel format (`rgb`): they have no alpha to write, and read 1.
    rgb: u16,
    /// The bits per channel a buffer of a small packed format keeps ([`packed::bits`]).
    packed: [Option<[u8; 4]>; BUFFERS],
    /// The stage (`texture.<stage>`) of the programs built so far, and the buffers one of them
    /// flipped: a `texture.<stage>.colortexN` no longer stands in for buffer N in the stage's
    /// later programs, which read what was written, as in Iris. `final` continues
    /// `composite`'s. (Super Duper Vanilla's `texture.composite.colortex0` is its clouds, read
    /// before `composite4` writes bloom there; Bliss's `colortex6` is blue noise until
    /// `composite5` writes it.)
    released: std::cell::Cell<(&'static str, u16)>,
    /// The formats and dimensions of the images no `image.*` gives, for their stand-ins.
    stand_ins: &'a std::cell::RefCell<Vec<(wgpu::TextureFormat, wgpu::TextureViewDimension)>>,
    /// Bit n: shadowtex n read through a plain sampler filters linearly (`shadowtexNNearest`
    /// unset): the translator's helpers blend a gather of it (`hz_depth_linear`).
    shadow_linear: u8,
}

/// A program's bind group layout (its uniform block, samplers and images) and how each sampler
/// and image binds. `None` (with a note) for an image the game can't give it.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn bindings(
    features: wgpu::Features,
    interface: &horizon_shaders::Interface,
    name: &str,
    group: Group,
    buffers: &[u32],
    geometry: Option<&Geometry<'_>>,
    formats: &[wgpu::TextureFormat; BUFFERS],
    shadow: Option<&[wgpu::TextureFormat; SHADOW_COLORS]>,
    notes: &mut Vec<String>,
    pack: &Resources<'_>,
) -> Option<(
    Vec<wgpu::BindGroupLayoutEntry>,
    Vec<Binding>,
    HashMap<u32, u32>,
    u16,
    u16,
    Vec<(u32, Image)>,
)> {
    let textures = pack.textures;
    if let Some(b) = interface.buffers.iter().find(|&&b| !pack.buffers.has(b)) {
        notes.push(format!("{name}: buffer {b} has no `bufferObject.{b}`"));
        return None;
    }
    let mut entries = vec![entry(
        0,
        wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Uniform,
            has_dynamic_offset: false,
            min_binding_size: None,
        },
    )];
    let mut samplers = Vec::new();
    let (mut kinds, mut slots) = (HashMap::new(), HashMap::new());
    let mut reads = 0u16;
    let mut shadow_reads = 0u16;
    let watershadow = interface.samplers.iter().any(|s| s.name == "watershadow");
    // Opaque gbuffers draws cannot sample their own attachments. Translucent draws can
    // read colortex4 and above from the preceding deferred stage; those reads use a copy
    // taken before the draw, so the sampled texture is never also an attachment.
    let in_place = match geometry {
        Some(_) => buffers.iter().fold(0u16, |m, &b| m | 1 << b),
        None => 0,
    };
    for s in &interface.samplers {
        let float = wgpu::TextureSampleType::Float { filterable: true };
        // A stand-in of the sampler's own dimension.
        let stand_in = if s.ty.contains("1D") {
            wgpu::TextureViewDimension::D1
        } else if s.ty.contains("3D") {
            wgpu::TextureViewDimension::D3
        } else {
            wgpu::TextureViewDimension::D2
        };
        if let Some(def) = pack
            .left_out
            .iter()
            .find(|d| d.sampler.as_deref() == Some(s.name.as_str()))
        {
            notes.push(format!(
                "{name}: samples image `{}`, which is left out",
                def.name
            ));
            return None;
        }
        let released = pack.released.get();
        let custom = pack_textures::find(textures, group, &s.ty, &s.name).filter(|&i| {
            let t = &textures[i];
            // A PNG under `texture.<stage>.<name>` whose name is no sampler of the game's
            // isn't bound: the sampler reads as if it had no texture. Iris 1.11.7 with a probe
            // pack of ours (#9064) bound it over `gaux2` and `noisetex` but not over names of its
            // own, where a raw texture was bound; this rule is a fit to those cases.
            let unbound = t.png
                && t.stage != horizon_shaders::EVERY_STAGE
                && source(&s.name, false, true, false) == Source::Dummy;
            !unbound
                && (t.stage != pack_textures::stage(group)
                    || released.0 != t.stage
                    || horizon_shaders::buffer_index(&t.name)
                        .is_none_or(|b| released.1 & 1 << b == 0))
        });
        let image = pack
            .images
            .iter()
            .position(|i| i.def.sampler.as_deref() == Some(s.name.as_str()));
        let (src, sample_type, dimension) = match custom {
            _ if let Some(k) = image => {
                let i = &pack.images[k];
                (Source::Image(k), i.sample_type(features), i.dimension)
            }
            Some(i) if !fits(&s.ty, textures[i].sample_type) => {
                notes.push(format!(
                    "{name}: `{} {}` can't read texture.{}.{}",
                    s.ty, s.name, textures[i].stage, textures[i].name
                ));
                (Source::Dummy, float, stand_in)
            }
            Some(i) => {
                let t = &textures[i];
                let src = match t.image {
                    pack_textures::Image::Lightmap => Source::Lightmap,
                    pack_textures::Image::Atlas => Source::Atlas,
                    pack_textures::Image::View(_) => Source::Custom {
                        index: i,
                        blur: t.blur,
                        clamp: t.clamp,
                    },
                };
                (src, t.sample_type, t.dimension)
            }
            None => {
                let mut src = source(&s.name, group.shadow(), shadow.is_some(), watershadow);
                // A sampler the game gives no texture reads texture unit 0, as do the atlas's
                // names: the atlas in gbuffers programs, `colortex0` in deferred, composite and
                // final (OptiFine's shaders.txt). Iris 1.11.7 read the atlas through a name of
                // ours in gbuffers_terrain, and the scene in `colortex0` through such a name and
                // through `gtexture`, `tex` and `texture` in those three, with or without
                // `colortex0` declared (probe packs of ours, #9307). Translucent programs and
                // the other groups weren't measured and keep the stand-in.
                let unit_0 = match group {
                    Group::Gbuffers => Some(Source::Atlas),
                    Group::Deferred | Group::Composite | Group::Final => Some(Source::Color(0)),
                    _ => None,
                };
                // A name of the game's that has no texture here (a shadow sampler with no
                // shadow pass) stays blank; so does a sampler that isn't 2D, and one under a
                // `texture.<stage>.<name>` left unbound (#9064's probe read a 16-texel-wide
                // texture there in final, undiagnosed).
                let unknown = src == Source::Dummy
                    && source(&s.name, false, true, false) == Source::Dummy
                    && s.ty == "sampler2D"
                    && !textures.iter().any(|t| t.name == s.name);
                // colortex0 to colortex3 (gcolor, gdepth, gnormal, composite) are no gbuffers
                // samplers in shaders.txt's table: Iris 1.11.7 read the atlas through each of
                // colortex0-3 and gnormal in gbuffers_terrain (our probe, #9307).
                let not_gbuffers =
                    group == Group::Gbuffers && matches!(src, Source::Color(n) if n < 4);
                if let Some(unit_0) = unit_0
                    && (src == Source::Atlas || unknown || not_gbuffers)
                {
                    src = unit_0;
                }
                let blank = match src {
                    // The shadow pass writes the shadow map.
                    Source::ShadowDepth(..) => group == Group::Shadow,
                    Source::Atlas => geometry.is_none(),
                    Source::Depth => geometry.is_some(),
                    // Translucent terrain reads the copy; opaque terrain would read itself.
                    Source::Depth1 | Source::Depth2 => group == Group::Gbuffers,
                    Source::Color(n) => {
                        (group != Group::Translucent || n < 4) && in_place & 1 << n != 0
                    }
                    _ => false,
                };
                let mut sample_type = match src {
                    Source::Color(n) => formats[n]
                        .sample_type(None, Some(features))
                        .unwrap_or(float),
                    Source::ShadowColor(n) => shadow
                        .and_then(|f| f[n].sample_type(None, Some(features)))
                        .unwrap_or(float),
                    Source::Depth | Source::Depth1 | Source::Depth2 | Source::ShadowDepth(..) => {
                        wgpu::TextureSampleType::Float { filterable: false }
                    }
                    // Unfilterable on WebGPU-level devices; the lightmap is sampled at texel centres.
                    Source::Lightmap => float,
                    _ => float,
                };
                let int = !matches!(sample_type, wgpu::TextureSampleType::Float { .. });
                if s.ty == "sampler2DShadow" {
                    src = match src {
                        Source::ShadowDepth(n, _) => Source::ShadowDepth(n, true),
                        _ => Source::Shadow,
                    };
                    sample_type = wgpu::TextureSampleType::Depth;
                }
                let wanted = match s.ty.as_str() {
                    "sampler2DShadow" => true,
                    "sampler2D" => !int,
                    "isampler2D" => sample_type == wgpu::TextureSampleType::Sint,
                    "usampler2D" => sample_type == wgpu::TextureSampleType::Uint,
                    _ => false,
                };
                if !wanted {
                    notes.push(format!(
                        "{}: `{} {}` is not provided yet",
                        name, s.ty, s.name
                    ));
                    (src, sample_type) = (Source::Dummy, float);
                }
                if blank && sample_type == wgpu::TextureSampleType::Depth {
                    src = Source::Shadow;
                } else if blank {
                    // The blank texture is float: an integer sampler then fails the pipeline (a note).
                    src = Source::Dummy;
                    if int {
                        sample_type = float;
                    }
                }
                let dimension = match src {
                    Source::Dummy => stand_in,
                    _ => wgpu::TextureViewDimension::D2,
                };
                (src, sample_type, dimension)
            }
        };
        let filtering = sample_type == float;
        match src {
            Source::Color(n) => reads |= 1 << n,
            Source::ShadowColor(n) => shadow_reads |= 1 << n,
            _ => {}
        }
        entries.push(texture_entry_of(s.binding, sample_type, dimension));
        let kind = SamplerKind::of(src, filtering);
        let sampler = *kinds.entry(kind).or_insert_with(|| {
            entries.push(entry(
                s.binding + 1,
                wgpu::BindingType::Sampler(kind.binding_type()),
            ));
            s.binding + 1
        });
        slots.insert(s.binding + 1, sampler);
        samplers.push(Binding {
            source: src,
            binding: s.binding,
            filtering,
            sampler,
            dimension,
        });
    }
    let mut images = Vec::new();
    for i in &interface.images {
        let buffer = i
            .name
            .strip_prefix("colorimg")
            .and_then(|n| n.parse::<usize>().ok())
            .filter(|&n| n < BUFFERS && !group.shadow());
        let own = pack.images.iter().position(|c| c.def.name == i.name);
        let Some(format) = image_format(&i.format) else {
            notes.push(format!("{name}: `{}` has format {}", i.name, i.format));
            return None;
        };
        let dimension = if i.ty.ends_with("1D") {
            wgpu::TextureViewDimension::D1
        } else if i.ty.ends_with("3D") {
            wgpu::TextureViewDimension::D3
        } else {
            wgpu::TextureViewDimension::D2
        };
        // An image no `image.*` gives reads zero, also after the program stored into it
        // (Iris 1.11.7, observed with a probe pack).
        let stand_in = |of: &str, notes: &mut Vec<String>| {
            notes.push(format!(
                "{name}: image `{of}` has no `image.{of}`; it reads zero"
            ));
            let mut stand_ins = pack.stand_ins.borrow_mut();
            if !stand_ins.contains(&(format, dimension)) {
                stand_ins.push((format, dimension));
            }
            (Image::StandIn(format, dimension), format)
        };
        let (target, actual) = match (buffer, own) {
            (_, Some(k)) => (Image::Custom(k), pack.images[k].format),
            (Some(b), None) => (Image::Color(b), formats[b]),
            (None, None) if i.before.is_some() => {
                // Bound as the image it copies.
                let of = i.before.as_deref().unwrap_or_default();
                let buffer = of
                    .strip_prefix("colorimg")
                    .and_then(|n| n.parse::<usize>().ok())
                    .filter(|&n| n < BUFFERS && !group.shadow());
                match (buffer, pack.images.iter().position(|c| c.def.name == of)) {
                    (_, Some(k)) => (Image::CustomBefore(k), pack.images[k].format),
                    (Some(b), None) => (Image::ColorBefore(b), formats[b]),
                    (None, None) => stand_in(of, notes),
                }
            }
            (None, None) => stand_in(&i.name, notes),
        };
        // An `R11F_G11F_B10F` image or buffer this device makes `RGBA16F` (a float stand-in
        // that neither clamps nor drops NaN) takes `layout(r11f_g11f_b10f)` as `rgba16f`
        // (`storage_formats_of`): Iris 1.11.7 stored into and read back such an image with a
        // probe pack of ours (#10490), where Horizon refused the program.
        let stand_in = format == wgpu::TextureFormat::Rg11b10Ufloat
            && actual == wgpu::TextureFormat::Rgba16Float;
        if actual != format && !stand_in {
            notes.push(format!(
                "{name}: `{}` is {} but its texture is {actual:?}",
                i.name, i.format
            ));
            return None;
        }
        let format = actual;
        let access = match i.access.as_str() {
            "writeonly" => wgpu::StorageTextureAccess::WriteOnly,
            "readonly" => wgpu::StorageTextureAccess::ReadOnly,
            "atomic" => wgpu::StorageTextureAccess::Atomic,
            _ => wgpu::StorageTextureAccess::ReadWrite,
        };
        if let Image::Color(b) | Image::ColorBefore(b) = target
            && pack.storable & 1 << b == 0
        {
            notes.push(format!(
                "{name}: `{}` can't be an image on this GPU; skipped",
                i.name
            ));
            return None;
        }
        entries.push(wgpu::BindGroupLayoutEntry {
            binding: i.binding,
            visibility: pack.gpu.storage_stages(),
            ty: wgpu::BindingType::StorageTexture {
                access,
                format,
                view_dimension: dimension,
            },
            count: None,
        });
        images.push((i.binding, target));
    }
    Some((entries, samplers, slots, reads, shadow_reads, images))
}

/// Makes each storage image of `module` in group 0 declare the format its layout entry binds:
/// a `layout(r11f_g11f_b10f)` image whose texture is the `RGBA16F` stand-in (`bindings`).
fn storage_formats_of(module: &mut naga::Module, entries: &[wgpu::BindGroupLayoutEntry]) {
    let globals: Vec<_> = module.global_variables.iter().map(|(h, _)| h).collect();
    for handle in globals {
        let var = &module.global_variables[handle];
        let Some(naga::ResourceBinding { group: 0, binding }) = var.binding else {
            continue;
        };
        let naga::TypeInner::Image {
            dim,
            arrayed,
            class: naga::ImageClass::Storage { format, access },
        } = module.types[var.ty].inner
        else {
            continue;
        };
        let bound = entries.iter().find_map(|e| match e.ty {
            wgpu::BindingType::StorageTexture { format, .. } if e.binding == binding => {
                Some(format)
            }
            _ => None,
        });
        if format == naga::StorageFormat::Rg11b10Ufloat
            && bound == Some(wgpu::TextureFormat::Rgba16Float)
        {
            let ty = module.types.insert(
                naga::Type {
                    name: module.types[var.ty].name.clone(),
                    inner: naga::TypeInner::Image {
                        dim,
                        arrayed,
                        class: naga::ImageClass::Storage {
                            format: naga::StorageFormat::Rgba16Float,
                            access,
                        },
                    },
                },
                naga::Span::UNDEFINED,
            );
            module.global_variables[handle].ty = ty;
        }
    }
}

/// The wgpu format of an image's format qualifier.
fn image_format(qualifier: &str) -> Option<wgpu::TextureFormat> {
    use wgpu::TextureFormat as T;
    Some(match qualifier {
        "rgba32f" => T::Rgba32Float,
        "rgba16f" => T::Rgba16Float,
        "rg32f" => T::Rg32Float,
        "rg16f" => T::Rg16Float,
        "r11f_g11f_b10f" => T::Rg11b10Ufloat,
        "r32f" => T::R32Float,
        "r16f" => T::R16Float,
        "rgba16" => T::Rgba16Unorm,
        "rgb10_a2" => T::Rgb10a2Unorm,
        "rgba8" => T::Rgba8Unorm,
        "rg16" => T::Rg16Unorm,
        "rg8" => T::Rg8Unorm,
        "r16" => T::R16Unorm,
        "r8" => T::R8Unorm,
        "rgba16_snorm" => T::Rgba16Snorm,
        "rgba8_snorm" => T::Rgba8Snorm,
        "rg16_snorm" => T::Rg16Snorm,
        "rg8_snorm" => T::Rg8Snorm,
        "r16_snorm" => T::R16Snorm,
        "r8_snorm" => T::R8Snorm,
        "rgba32i" => T::Rgba32Sint,
        "rgba16i" => T::Rgba16Sint,
        "rgba8i" => T::Rgba8Sint,
        "rg32i" => T::Rg32Sint,
        "rg16i" => T::Rg16Sint,
        "rg8i" => T::Rg8Sint,
        "r32i" => T::R32Sint,
        "r16i" => T::R16Sint,
        "r8i" => T::R8Sint,
        "rgba32ui" => T::Rgba32Uint,
        "rgba16ui" => T::Rgba16Uint,
        "rgb10_a2ui" => T::Rgb10a2Uint,
        "rgba8ui" => T::Rgba8Uint,
        "rg32ui" => T::Rg32Uint,
        "rg16ui" => T::Rg16Uint,
        "rg8ui" => T::Rg8Uint,
        "r32ui" => T::R32Uint,
        "r16ui" => T::R16Uint,
        "r8ui" => T::R8Uint,
        _ => return None,
    })
}

/// The value slot of the pack's custom uniform `name` for a program declaring it as GLSL type
/// `ty`: after the frame's uniforms, in the order of `custom` (each with the type
/// `shaders.properties` gives it). A `float`, `int`, `vec2`, `vec3` or `vec4` custom uniform
/// declared as another of these types gets none, and reads 0: Iris 1.11.7 logs "Wrong uniform
/// type ... Disabling that uniform" and our probes read 0 there (float as int, int as float,
/// vec3 as vec2, vec2 as vec3; the vec4 cases were not probed). Other pairs (a `bool` on either
/// side) keep the conversion `Value::write` makes: our probe read a float of 2.5 declared
/// `bool` as true in both games.
fn custom_index(custom: &[(String, String)], name: &str, ty: &str) -> Option<usize> {
    const STRICT: [&str; 5] = ["float", "int", "vec2", "vec3", "vec4"];
    let k = custom.iter().position(|(c, _)| c == name)?;
    let declared = custom[k].1.as_str();
    let disabled = declared != ty && STRICT.contains(&declared) && STRICT.contains(&ty);
    (!disabled).then_some(pack_uniforms::NAMES.len() + k)
}

/// The pass of program `p`: a quad, or `geometry` drawn by the game. `None` (with a note) when
/// wgpu refuses it or it writes nothing. `formats` are its buffers' (for a program of the shadow
/// buffers, buffer n is shadowcolor n); `shadow` the shadowcolor formats when the shadow pass
/// runs.
#[allow(clippy::too_many_arguments)]
fn build_pass(
    device: &wgpu::Device,
    p: horizon_shaders::PostProgram,
    formats: &[wgpu::TextureFormat; BUFFERS],
    shadow: Option<&[wgpu::TextureFormat; SHADOW_COLORS]>,
    screen: wgpu::TextureFormat,
    notes: &mut Vec<String>,
    geometry: Option<&Geometry<'_>>,
    custom: &[(String, String)],
    draws: &wgpu::BindGroupLayout,
    pack: &Resources<'_>,
) -> Option<Pass> {
    let features = device.features();
    // The frame's uniforms, then the pack's custom ones.
    let index = |name: &str, ty: &str| {
        pack_uniforms::index_in(name, geometry.is_some()).or_else(|| custom_index(custom, name, ty))
    };
    let mask = |flips: &[(u32, bool)]| flips.iter().filter(|f| f.1).fold(0u16, |m, f| m | 1 << f.0);
    let mut p = p;
    let program = p.program.take()?;
    let interface = &program.interface;
    let draw = interface.draw;
    let storage = !interface.images.is_empty() || !interface.buffers.is_empty();
    if storage && !pack.gpu.fragment_storage {
        notes.push(format!(
            "{}: this GPU can't write images from a fragment shader; skipped",
            p.name
        ));
        return None;
    }
    // Terrain's geometry stage reads the draw's indices at set 5; entities' walk their quads.
    let reads_indices = p.geometry_triangles > 0
        && (program.vertex.global_variables.iter())
            .any(|(_, v)| v.binding.as_ref().is_some_and(|b| b.group == 5));
    if p.geometry_triangles > 0
        && ((reads_indices && pack.indices.is_none()) || !pack.gpu.vertex_storage)
    {
        notes.push(format!(
            "{}: this GPU can't run a geometry stage; skipped",
            p.name
        ));
        return None;
    }
    let (entries, samplers, slots, reads, shadow_reads, images) = bindings(
        features, interface, &p.name, p.group, &p.buffers, geometry, formats, shadow, notes, pack,
    )?;
    let final_pass = p.group == Group::Final;
    // Outputs into an integer buffer, and its integer kind (`packed::bitcast_outputs`).
    let mut int_outputs = Vec::new();
    let mut targets = Vec::new();
    let mut writes = Vec::new();
    // Outputs into a buffer of a small packed format, and the bits it keeps.
    let mut packed_outputs = Vec::new();
    // Each output's own `blend.<program>.<buffer>`, and whether its buffer blends at all: what
    // the sky's part pipelines keep (`PostProgram::vanilla_sky_blends`).
    let mut own_blends: Vec<(Option<wgpu::BlendState>, bool)> = Vec::new();
    for &location in &interface.outputs {
        let l = location as usize;
        if targets.len() <= l {
            targets.resize(l + 1, None);
        }
        if final_pass {
            if location == 0 {
                targets[0] = target(screen);
            }
        } else if let Some(&buffer) = p.buffers.get(l) {
            let format = formats[buffer as usize];
            match format.sample_type(None, Some(features)) {
                Some(wgpu::TextureSampleType::Uint) => {
                    int_outputs.push((location, naga::ScalarKind::Uint));
                }
                Some(wgpu::TextureSampleType::Sint) => {
                    int_outputs.push((location, naga::ScalarKind::Sint));
                }
                _ => {}
            }
            let mut state = target(format);
            if !p.group.shadow()
                && pack.rgb & 1 << buffer != 0
                && let Some(t) = &mut state
            {
                t.write_mask = wgpu::ColorWrites::COLOR;
            }
            let own = p.buffer_blends.iter().find(|(n, _)| *n == buffer);
            let blendable = format
                .guaranteed_format_features(features)
                .flags
                .contains(wgpu::TextureFormatFeatureFlags::BLENDABLE);
            if let (Some(t), Some(b)) = (&mut state, own.map_or(p.blend, |(_, b)| *b))
                && blendable
            {
                t.blend = Some(blend_state(b));
            }
            if own_blends.len() <= l {
                own_blends.resize(l + 1, (None, false));
            }
            let own = own.and_then(|(_, b)| *b).filter(|_| blendable);
            own_blends[l] = (own.map(blend_state), blendable);
            if !p.group.shadow()
                && let Some(bits) = pack.packed[buffer as usize]
            {
                packed_outputs.push((location, bits));
            }
            targets[l] = state;
            writes.push((location, buffer as usize));
        }
    }
    // Outputs past the last buffer the program writes go nowhere, as in GL; a pass has no
    // attachment for them.
    while targets.last().is_some_and(Option::is_none) {
        targets.pop();
    }
    // A program that only writes images draws into a sink that keeps nothing; one drawing
    // geometry has the depth buffer, and draws with no colour target at all.
    let sink = !final_pass && writes.is_empty() && !program.interface.images.is_empty();
    if sink && geometry.is_none() {
        targets = vec![Some(wgpu::ColorTargetState {
            format: SINK,
            blend: None,
            write_mask: wgpu::ColorWrites::empty(),
        })];
    } else if !final_pass && writes.is_empty() && !sink {
        notes.push(format!("{}: writes no buffer; skipped", p.name));
        return None;
    }
    let written = writes.iter().fold(0u16, |m, w| m | 1 << w.1);
    let kept = p
        .flips
        .iter()
        .filter(|f| !f.1)
        .fold(0u16, |m, f| m | 1 << f.0);
    // Gbuffers programs write in place (6.1.6). A forced flip of a buffer the program writes
    // cancels the write's own flip, as in Iris 1.11.7 (probed with packs of our own).
    let flip = if final_pass || geometry.is_some() {
        0
    } else {
        (written & !kept) ^ mask(&p.flips)
    };
    let created = checked(device, || {
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some(&p.name),
            entries: &entries,
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some(&p.name),
            bind_group_layouts: &[
                Some(&layout),
                geometry.and_then(|g| g.table),
                draw.then_some(draws),
                geometry.and_then(|g| g.ids),
                (!interface.buffers.is_empty()).then_some(&pack.buffers.layout),
                pack.indices.filter(|_| reads_indices),
            ],
            ..Default::default()
        });
        let module = |mut m: naga::Module| {
            share_samplers(&mut m, &slots);
            storage_formats_of(&mut m, &entries);
            device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some(&p.name),
                source: wgpu::ShaderSource::Naga(std::borrow::Cow::Owned(m)),
            })
        };
        let mut fragment = program.fragment;
        packed::quantise_outputs(&mut fragment, &packed_outputs);
        packed::bitcast_outputs(&mut fragment, &int_outputs);
        let (vs, fs) = (module(program.vertex), module(fragment));
        let descriptor = |targets: &[Option<wgpu::ColorTargetState>]| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(&p.name),
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &vs,
                    entry_point: Some("main"),
                    compilation_options: Default::default(),
                    // A geometry stage reads its vertices itself.
                    buffers: geometry
                        .filter(|_| p.geometry_triangles == 0)
                        .map_or(&[], |g| g.buffers),
                },
                // Rows flipped to GL's turn front faces clockwise.
                primitive: wgpu::PrimitiveState {
                    front_face: wgpu::FrontFace::Cw,
                    cull_mode: geometry
                        .filter(|g| g.cull && p.geometry_triangles == 0)
                        .map(|_| wgpu::Face::Back),
                    ..Default::default()
                },
                depth_stencil: geometry.map(|g| wgpu::DepthStencilState {
                    format: wgpu::TextureFormat::Depth32Float,
                    depth_write_enabled: Some(g.depth_write),
                    depth_compare: Some(wgpu::CompareFunction::LessEqual),
                    stencil: Default::default(),
                    bias: if p.name == "gbuffers_damagedblock" {
                        wgpu::DepthBiasState {
                            constant: -10,
                            slope_scale: -1.0,
                            clamp: 0.0,
                        }
                    } else {
                        Default::default()
                    },
                }),
                multisample: Default::default(),
                fragment: Some(wgpu::FragmentState {
                    module: &fs,
                    entry_point: Some("main"),
                    compilation_options: Default::default(),
                    targets,
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        let pipeline = descriptor(&targets);
        // The program's own blend replaced by vanilla's for the part; a buffer's own
        // `blend.<program>.<buffer>` still applies (Iris 1.11.7 blended the sky disc into that
        // buffer only, with a probe pack of ours).
        let sky_parts = p.vanilla_sky_blends.then(|| {
            [None, Some(blend_state(horizon_shaders::STARS_BLEND))].map(|part| {
                let targets: Vec<_> = (targets.iter().enumerate())
                    .map(|(l, t)| {
                        let (own, blendable) = own_blends.get(l).copied().unwrap_or((None, false));
                        t.clone().map(|t| wgpu::ColorTargetState {
                            blend: own.or(part.filter(|_| blendable)),
                            ..t
                        })
                    })
                    .collect();
                descriptor(&targets)
            })
        });
        (layout, pipeline, sky_parts)
    });
    let (layout, pipeline, sky_parts) = match created {
        Ok(created) => created,
        Err(e) => {
            notes.push(format!("{}: skipped: {e}", p.name));
            return None;
        }
    };
    let interface = &program.interface;
    let missing: Vec<&str> = interface
        .uniforms
        .iter()
        .filter(|u| !u.name.starts_with("hz_") && index(&u.name, &u.ty).is_none())
        .map(|u| u.name.as_str())
        .collect();
    if !missing.is_empty() {
        notes.push(format!(
            "{}: shader defaults or zero: {}",
            p.name,
            missing.join(" ")
        ));
    }
    let image_copies = image_copies(&samplers, &images);
    Some(Pass {
        ubo: device.create_buffer(&wgpu::BufferDescriptor {
            label: Some(&p.name),
            size: u64::from(interface.uniform_size.max(16)),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        }),
        block: interface.initial_values.clone(),
        uniforms: interface
            .uniforms
            .iter()
            .map(|u| (index(&u.name, &u.ty), u.ty.clone(), u.offset))
            .collect(),
        name: p.name,
        group: p.group,
        pipeline: Pipeline::Render(pipeline),
        sky_parts,
        layout,
        samplers,
        writes,
        flip,
        depth_linear: depth_linear(interface, pack.shadow_linear),
        shadow_reads,
        written,
        mipmaps: p.mipmaps,
        draw,
        atlas: geometry.and_then(|g| g.atlas.clone()),
        material_maps: geometry.is_some_and(|g| g.material_maps),
        ids: geometry.is_some_and(|g| g.ids.is_some()),
        groups: HashMap::new(),
        portal_groups: HashMap::new(),
        copies: (image_buffers(&images)
            | if p.group == Group::Translucent {
                written
            } else {
                0
            })
            & reads,
        reads: reads | image_buffers(&images),
        buffers: !interface.buffers.is_empty(),
        images,
        compute: Vec::new(),
        sink,
        geometry: p.geometry_triangles,
        viewport: (!final_pass && p.scale != [1.0, 0.0, 0.0]).then_some(p.scale),
        image_copies,
        snapshots: 0,
    })
}

/// The pack's images a pass both samples and stores.
fn image_copies(samplers: &[Binding], images: &[(u32, Image)]) -> u64 {
    samplers
        .iter()
        .filter_map(|s| match s.source {
            Source::Image(k) if images.iter().any(|i| i.1 == Image::Custom(k)) => Some(k),
            _ => None,
        })
        .fold(0, |m, k| m | 1 << k)
}

/// The pass of compute program `c` of a `group` program: `None` (with a note) when wgpu refuses
/// it or it needs what the game can't give.
#[allow(clippy::too_many_arguments)]
fn build_compute(
    device: &wgpu::Device,
    name: String,
    group: Group,
    c: horizon_shaders::ComputeProgram,
    formats: &[wgpu::TextureFormat; BUFFERS],
    shadow: Option<&[wgpu::TextureFormat; SHADOW_COLORS]>,
    notes: &mut Vec<String>,
    custom: &[(String, String)],
    pack: &Resources<'_>,
) -> Option<Pass> {
    let index = |name: &str, ty: &str| {
        pack_uniforms::index(name).or_else(|| custom_index(custom, name, ty))
    };
    let interface = &c.interface;
    let (entries, samplers, slots, reads, shadow_reads, images) = bindings(
        device.features(),
        interface,
        &name,
        group,
        &[],
        None,
        formats,
        shadow,
        notes,
        pack,
    )?;
    let local = c.module.entry_points[0].workgroup_size;
    let created = checked(device, || {
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some(&name),
            entries: &entries,
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some(&name),
            bind_group_layouts: &[
                Some(&layout),
                None,
                None,
                None,
                (!interface.buffers.is_empty()).then_some(&pack.buffers.layout),
            ],
            ..Default::default()
        });
        let mut module = c.module;
        share_samplers(&mut module, &slots);
        storage_formats_of(&mut module, &entries);
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some(&name),
            source: wgpu::ShaderSource::Naga(std::borrow::Cow::Owned(module)),
        });
        let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some(&name),
            layout: Some(&pipeline_layout),
            module: &module,
            entry_point: Some("main"),
            compilation_options: Default::default(),
            cache: None,
        });
        (layout, pipeline)
    });
    let (layout, pipeline) = match created {
        Ok(created) => created,
        Err(e) => {
            notes.push(format!("{name}: skipped: {e}"));
            return None;
        }
    };
    let written = image_buffers(&images);
    let image_copies = image_copies(&samplers, &images);
    let (before_colors, snapshots) = befores(&images);
    Some(Pass {
        ubo: device.create_buffer(&wgpu::BufferDescriptor {
            label: Some(&name),
            size: u64::from(interface.uniform_size.max(16)),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        }),
        block: interface.initial_values.clone(),
        uniforms: interface
            .uniforms
            .iter()
            .map(|u| (index(&u.name, &u.ty), u.ty.clone(), u.offset))
            .collect(),
        name,
        group,
        pipeline: Pipeline::Compute {
            pipeline,
            groups: c.work_groups,
            render: c.work_groups_render,
            local,
        },
        sky_parts: None,
        layout,
        samplers,
        writes: Vec::new(),
        flip: 0,
        depth_linear: depth_linear(interface, pack.shadow_linear),
        copies: written & reads | before_colors,
        reads: reads | written,
        shadow_reads,
        written,
        mipmaps: 0,
        draw: false,
        atlas: None,
        material_maps: false,
        ids: false,
        groups: HashMap::new(),
        portal_groups: HashMap::new(),
        buffers: !interface.buffers.is_empty(),
        images,
        compute: Vec::new(),
        sink: false,
        geometry: 0,
        image_copies,
        snapshots,
        viewport: None,
    })
}

/// Why a pack can't run, and the notes of loading it until then (why programs were skipped).
#[derive(Debug)]
pub struct Refused {
    pub reason: String,
    pub notes: Vec<String>,
}

impl PackPost {
    /// Builds the pipelines of `post` for a screen of `screen` format. An error when the pack
    /// can't show anything on the screen: the game turns it off, as a pack that fails to load.
    pub fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        adapter: &wgpu::Adapter,
        mut post: Post,
        screen: wgpu::TextureFormat,
        world: World<'_>,
    ) -> Result<Self, Refused> {
        // What the pack's shaders need of its storage buffers, before its parts move out.
        let minimums = post.buffer_minimums();
        let features = device.features();
        let formats: [wgpu::TextureFormat; BUFFERS] = std::array::from_fn(|i| {
            let f = post.config.buffers[i].format;
            snorm_format(f, features, adapter)
                .unwrap_or_else(|| target_format(f, features, adapter))
        });
        let sky_clear = Some(&post.config.buffers[0])
            .filter(|b| b.clear)
            .and_then(|b| b.clear_color);
        // A three-channel buffer reads alpha 1 whatever is written or cleared, as in Iris: its
        // passes leave alpha alone, and its clears and first contents set it to 1.
        let rgb = (0..BUFFERS)
            .filter(|&i| post.config.buffers[i].format.channels == 3)
            .fold(0u16, |m, i| m | 1 << i);
        // A small packed format's buffer reads back fewer bits, as in Iris: its clears and
        // what its passes write keep only those.
        let packed: [Option<[u8; 4]>; BUFFERS] =
            std::array::from_fn(|i| packed::bits(post.config.buffers[i].format));
        let clears = std::array::from_fn(|i| {
            let b = &post.config.buffers[i];
            b.clear.then(|| {
                let mut c = (b.clear_color).unwrap_or(if i == 1 { [1.0; 4] } else { [0.0; 4] });
                if rgb & 1 << i != 0 {
                    c[3] = 1.0;
                }
                packed[i].map_or(c, |bits| packed::quantise(c, bits))
            })
        });
        let mut notes = post.notes;
        notes.extend(post.config.notes);
        let mut sizes = post.sizes;
        // The blit writes colortex0 beside the depth buffer, at the render size.
        if sizes[0].take().is_some() {
            notes.push("size.buffer.colortex0: colortex0 keeps the render size".into());
        }
        let textures = pack_textures::upload(device, queue, post.textures, &mut notes);
        let shadow_config = post.config.shadow.clone();
        let noise_size = post.config.noise_resolution.unwrap_or(256);
        // shadowcolor2 to 7 only for a pack that names `HIGHER_SHADOWCOLOR`.
        let shadow_colors = (1u16 << post.shadow_colors) - 1;
        let mut custom = post.custom;
        let custom_names: Vec<(String, String)> = (custom.uniforms())
            .map(|(name, ty)| (name.to_owned(), ty.to_owned()))
            .collect();
        // A first run finds the names neither the frame nor the pack defines.
        custom.eval(0.0, &pack_uniforms::zero);
        // Its zeros are no start for `smooth`: the first frame's values are, as in Iris.
        custom.restart_smoothing();
        if !custom.notes.is_empty() {
            notes.push(format!("custom uniforms: {}", custom.notes.join("; ")));
            custom.notes.clear();
        }
        let mask =
            |flips: &[(u32, bool)]| flips.iter().filter(|f| f.1).fold(0u16, |m, f| m | 1 << f.0);
        let mut used = 1u16;
        // Buffers some pass draws into the "alt" side of, or flips: only these get a B texture.
        let mut doubled = mask(&post.deferred_pre) | mask(&post.composite_pre);
        let mut shadow_doubled = 0u16;
        let draws = DrawUniforms::new(device);
        // The shadow buffers' formats when the shadow pass runs; their programs see them as
        // buffers 0 to 7.
        let shadow_formats: Option<[wgpu::TextureFormat; SHADOW_COLORS]> =
            post.shadow_pass.then(|| {
                std::array::from_fn(|n| {
                    target_format(shadow_config.colors[n].format, features, adapter)
                })
            });
        let shadow_space: [wgpu::TextureFormat; BUFFERS] = std::array::from_fn(|i| {
            shadow_formats
                .filter(|_| i < SHADOW_COLORS)
                .map_or(formats[i], |f| f[i])
        });
        let (mut storage, mut copied, mut image_copied, mut snapshotted) = (0u16, 0u16, 0u64, 0u64);
        let gpu = Gpu::of(adapter, features);
        if !gpu.compute {
            notes.push("this GPU has no compute shaders: compute programs are skipped".into());
        }
        // An image this device can't make or store into: the programs using it are skipped.
        let mut image_defs = std::mem::take(&mut post.images);
        let mut left_out = Vec::new();
        let packed_float = gpu.storable(adapter, wgpu::TextureFormat::Rg11b10Ufloat, features);
        image_defs.retain(|def| {
            let made = pack_images::makeable(device, def, packed_float).and_then(|()| {
                if gpu.storable(
                    adapter,
                    pack_images::format(def.format, packed_float),
                    features,
                ) {
                    Ok(())
                } else {
                    Err("this GPU can't store into its format".into())
                }
            });
            if let Err(e) = &made {
                notes.push(format!("image `{}`: {e}", def.name));
                left_out.push(def.clone());
            }
            made.is_ok()
        });
        // A mask of a pass's images holds this many.
        let kept = image_defs.len().min(pack_images::IMAGES);
        for def in image_defs.drain(kept..) {
            notes.push(format!(
                "image `{}`: past the {} images a pack may have",
                def.name,
                pack_images::IMAGES
            ));
            left_out.push(def);
        }
        let images = pack_images::images(image_defs, packed_float);
        let mut objects = std::mem::take(&mut post.buffer_objects);
        if gpu.storage_stages().is_empty() {
            objects.clear();
        }
        let buffers = pack_images::Buffers::new(device, objects, minimums, gpu.storage_stages());
        let storable = (0..BUFFERS)
            .filter(|&i| gpu.storable(adapter, formats[i], features))
            .fold(0u16, |m, i| m | 1 << i);
        let indices = gpu.vertex_storage.then(|| {
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("Shader pack geometry indices"),
                entries: &[wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                }],
            })
        });
        let stand_ins = std::cell::RefCell::new(Vec::new());
        let pack = Resources {
            textures: &textures,
            images: &images,
            left_out: &left_out,
            buffers: &buffers,
            indices: indices.as_ref(),
            gpu,
            storable,
            rgb,
            packed,
            released: std::cell::Cell::new(("", 0)),
            stand_ins: &stand_ins,
            shadow_linear: (0..2)
                .filter(|&n| !shadow_config.nearest[n])
                .fold(0, |m, n| m | 1 << n),
        };
        let mut build = |mut p: horizon_shaders::PostProgram, geometry: Option<&Geometry<'_>>| {
            let in_shadow = p.group.shadow();
            let mut compute = std::mem::take(&mut p.compute);
            if !gpu.compute {
                compute.clear();
                p.program.as_ref()?;
            }
            let group = p.group;
            let stage = pack_textures::stage(group);
            if pack.released.get().0 != stage {
                pack.released.set((stage, 0));
            }
            let mut computes: Vec<Pass> = compute
                .into_iter()
                .filter_map(|(name, c)| {
                    build_compute(
                        device,
                        name,
                        group,
                        c,
                        if in_shadow { &shadow_space } else { &formats },
                        shadow_formats.as_ref(),
                        &mut notes,
                        &custom_names,
                        &pack,
                    )
                })
                .collect();
            let mut pass = if p.program.is_none() {
                // Compute stages only: the last is the pass, the others run before it.
                let mut last = computes.pop()?;
                last.compute = computes;
                computes = Vec::new();
                last
            } else {
                build_pass(
                    device,
                    p,
                    if in_shadow { &shadow_space } else { &formats },
                    shadow_formats.as_ref(),
                    screen,
                    &mut notes,
                    geometry,
                    &custom_names,
                    &draws.layout,
                    &pack,
                )?
            };
            pass.compute.append(&mut computes);
            // A pass's attachments share a size: the depth buffer's, for the passes that draw
            // the world.
            let size =
                |b: usize| sizes[b].unwrap_or(horizon_shaders::BufferSize::Relative([1.0; 2]));
            let depth = matches!(group, Group::Gbuffers | Group::Translucent);
            let mut written = pass.writes.iter().map(|w| size(w.1));
            let first = if depth {
                Some(horizon_shaders::BufferSize::Relative([1.0; 2]))
            } else {
                written.next()
            };
            // Sizes of one kind that differ never meet. A size relative to the render size and
            // one in pixels can: the pass then draws while they resolve to one size
            // (`PackPost::sizes_differ`). Iris 1.11.7 drew a composite program writing
            // colortex2 at `0.25 0.25` and colortex3 at `320 180` at 1280×720 (#10454).
            let apart = |s: horizon_shaders::BufferSize| {
                first.is_some_and(|f| {
                    f != s && (depth || std::mem::discriminant(&f) == std::mem::discriminant(&s))
                })
            };
            if !in_shadow && group != Group::Final && written.any(apart) {
                notes.push(format!(
                    "{}: writes buffers of different sizes (size.buffer); skipped",
                    pass.name
                ));
                return None;
            }
            // Composite-style programs draw into the "alt" side of what they write; gbuffers,
            // shadow and `final` programs write in place, compute programs through images.
            let alt = if geometry.is_none()
                && group != Group::Final
                && matches!(pass.pipeline, Pipeline::Render(_))
            {
                pass.written
            } else {
                0
            };
            if in_shadow {
                shadow_doubled |= alt;
            } else {
                doubled |= alt;
            }
            for p in std::iter::once(&pass).chain(&pass.compute) {
                let (stage, flipped) = pack.released.get();
                pack.released.set((stage, flipped | p.flip));
                if in_shadow {
                    shadow_doubled |= p.flip;
                } else {
                    used |= p.reads | p.written | p.flip;
                    doubled |= p.flip;
                }
                storage |= image_buffers(&p.images);
                image_copied |= p.image_copies;
                snapshotted |= p.snapshots;
                copied |= p.copies;
            }
            Some(pass)
        };
        // Flat terrain reads `mc_Entity` from a table of the sprite layers.
        let block_ids = world
            .terrain
            .as_ref()
            .filter(|g| g.table.is_some())
            .map(|_| block_ids(device, world.block_ids, world.sprite_rects));
        let terrain_geometry = world.terrain.as_ref().map(|g| Geometry {
            buffers: g.buffers,
            table: g.table,
            depth_write: g.depth_write,
            atlas: g.atlas.clone(),
            material_maps: g.material_maps,
            ids: block_ids.as_ref().map(|i| &i.0),
            cull: g.cull,
        });
        let split = post.terrain.is_some() && post.terrain_cutout.is_some();
        let (terrain_undrawn, water_undrawn) = (post.terrain_undrawn, post.water_undrawn);
        let mut terrain = post
            .terrain
            .zip(terrain_geometry.as_ref())
            .and_then(|(p, g)| build(p, Some(g)));
        let mut terrain_cutout = post
            .terrain_cutout
            .zip(terrain_geometry.as_ref())
            .and_then(|(p, g)| build(p, Some(g)));
        let water = post
            .water
            .zip(terrain_geometry.as_ref())
            .and_then(|(p, g)| build(p, Some(g)));
        // The shadow pass draws terrain faces from both sides, as Iris 1.11.7 does (#9033): a
        // cave ceiling faces away from the sun, and is often all that shadows the cave.
        let shadow_geometry = terrain_geometry.as_ref().map(|g| Geometry {
            buffers: g.buffers,
            table: g.table,
            depth_write: g.depth_write,
            atlas: g.atlas.clone(),
            material_maps: g.material_maps,
            ids: g.ids,
            cull: false,
        });
        let shadow_terrain = post
            .shadow
            .zip(shadow_geometry.as_ref())
            .and_then(|(p, g)| build(p, Some(g)));
        let shadow_water = post
            .shadow_water
            .zip(shadow_geometry.as_ref())
            .and_then(|(p, g)| build(p, Some(g)));
        let (hides_sky, hides_celestial) = (post.sky.is_some(), post.sky_textured.is_some());
        let sky = (post.sky.is_some() || post.sky_textured.is_some()).then(|| {
            let geometry = PackSky::new(device);
            let basic = Geometry {
                buffers: &PackSky::buffers(),
                table: Some(&geometry.layout),
                depth_write: false,
                atlas: None,
                material_maps: false,
                ids: None,
                cull: false,
            };
            let basic = post.sky.and_then(|p| build(p, Some(&basic)));
            let textured = Geometry {
                buffers: &PackSky::celestial_buffers(),
                table: None,
                depth_write: false,
                atlas: world.celestial.clone(),
                material_maps: false,
                ids: None,
                cull: false,
            };
            let textured = post.sky_textured.and_then(|p| build(p, Some(&textured)));
            Sky {
                geometry,
                basic,
                textured,
            }
        });
        let sky = sky.filter(|s| s.basic.is_some() || s.textured.is_some());
        let programs = [
            post.entities,
            post.block,
            post.entities_translucent,
            post.particles,
            post.damaged_block,
            post.beacon_beam,
        ];
        let late = [
            false,
            false,
            post.translucent_late,
            post.particles_late,
            false,
            true,
        ];
        let shadow_only = (post.shadow_only_player, post.shadow_only_lit_blocks);
        let shadow_programs = [post.shadow_entities, post.shadow_block];
        let entity_vertices = (programs.iter().chain(&shadow_programs))
            .any(Option::is_some)
            .then(|| PackEntities::new(device, &draws.layout));
        let entity_buffers = super::pack_entities::buffers();
        let entity_geometry = entity_vertices.as_ref().map(|e| Geometry {
            buffers: &entity_buffers,
            table: Some(&e.table),
            depth_write: true,
            atlas: world.celestial.clone(),
            material_maps: false,
            ids: None,
            cull: false,
        });
        let entity_passes = programs.map(|p| {
            p.zip(entity_geometry.as_ref()).and_then(|(p, g)| {
                if p.name == "gbuffers_damagedblock" {
                    let decal = Geometry {
                        buffers: g.buffers,
                        table: g.table,
                        depth_write: false,
                        atlas: g.atlas.clone(),
                        material_maps: g.material_maps,
                        ids: g.ids,
                        cull: false,
                    };
                    build(p, Some(&decal))
                } else {
                    build(p, Some(g))
                }
            })
        });
        let shadow_entities = shadow_programs.map(|p| {
            p.zip(entity_geometry.as_ref())
                .and_then(|(p, g)| build(p, Some(g)))
        });
        let entities = entity_vertices
            .filter(|_| (entity_passes.iter().chain(&shadow_entities)).any(Option::is_some))
            .map(|geometry| Entities {
                geometry,
                passes: entity_passes,
                late,
                shadow: shadow_entities,
                only: shadow_only,
            });
        let cloud_geometry = world.clouds.map(|layout| Geometry {
            buffers: &entity_buffers,
            table: Some(layout),
            depth_write: true,
            atlas: world.celestial.clone(),
            material_maps: false,
            ids: None,
            cull: false,
        });
        let hides_clouds = post.clouds_off || post.clouds.is_some() && cloud_geometry.is_some();
        let weather_geometry = world.weather.then(|| Geometry {
            buffers: &entity_buffers,
            table: None,
            depth_write: post.rain_depth,
            atlas: None,
            material_maps: false,
            ids: None,
            cull: false,
        });
        let hand_geometry = world
            .hand
            .then(|| super::pack_hand::PackHand::new(device, &draws.layout));
        let hand = post.hand.zip(hand_geometry).and_then(|(p, geometry)| {
            let g = Geometry {
                buffers: &entity_buffers,
                table: Some(&geometry.layout),
                depth_write: true,
                atlas: world.celestial.clone(),
                material_maps: false,
                ids: None,
                cull: false,
            };
            build(p, Some(&g)).map(|pass| (pass, geometry))
        });
        // Translucent held items, drawn from the hand's own buffer.
        let hand_water = post.hand_water.and_then(|p| {
            let (_, geometry) = hand.as_ref()?;
            let g = Geometry {
                buffers: &entity_buffers,
                table: Some(&geometry.layout),
                depth_write: true,
                atlas: world.celestial.clone(),
                material_maps: false,
                ids: None,
                cull: false,
            };
            build(p, Some(&g))
        });
        let weather = post
            .weather
            .zip(weather_geometry.as_ref())
            .and_then(|(p, g)| build(p, Some(g)))
            .map(|rain| {
                let mut snow = rain.clone();
                snow.name = format!("{} (snow)", rain.name);
                [rain, snow]
            });
        let clouds = post
            .clouds
            .zip(cloud_geometry.as_ref())
            .and_then(|(p, g)| build(p, Some(g)));
        let line_buffers = [Some(super::pack_lines::layout())];
        let line_geometry = Geometry {
            buffers: &line_buffers,
            table: None,
            depth_write: false,
            atlas: None,
            material_maps: false,
            ids: None,
            cull: false,
        };
        let lines = post.lines.and_then(|p| build(p, Some(&line_geometry)));
        // Developers' aid: `HORIZON_SHADER_PACK_SKIP=composite,final` leaves programs out.
        let skip = std::env::var("HORIZON_SHADER_PACK_SKIP").unwrap_or_default();
        let passes: Vec<Pass> = post
            .programs
            .into_iter()
            .filter(|p| !skip.split(',').any(|s| s == p.name))
            .filter_map(|p| build(p, None))
            .collect();
        // The screen gets colortex0 through `final`, or else through Horizon's copy, which reads
        // floats: an integer colortex0 (`RGBA32UI`...) with no `final` left to draw can't reach
        // it (#9165).
        let shows = passes
            .iter()
            .any(|p| p.group == Group::Final && matches!(p.pipeline, Pipeline::Render(_)));
        if !shows && !holds_floats(formats[0]) {
            return Err(Refused {
                reason: format!(
                    "colortex0 has an integer format ({:?}) and no `final` program draws it to \
                     the screen",
                    formats[0]
                ),
                notes,
            });
        }
        let shadow_comp: Vec<Pass> = post
            .shadow_comp
            .into_iter()
            .filter(|p| !skip.split(',').any(|s| s == p.name))
            .filter_map(|p| build(p, None))
            .collect();
        let stand_ins = pack_images::stand_ins(device, stand_ins.take());
        // Terrain in two parts draws both or neither: one alone would leave the other part's
        // terrain undrawn. Without either, the game draws all terrain, as when the terrain
        // program of a pack that doesn't split can't draw.
        if split
            && terrain.is_some() != terrain_cutout.is_some()
            && let Some(p) = terrain.take().or(terrain_cutout.take())
        {
            notes.push(format!(
                "{}: the other part of terrain can't draw; the game draws all terrain",
                p.name
            ));
        }
        match (&terrain, &mut terrain_cutout) {
            (Some(t), None) => notes.push(format!("terrain draws with {}", t.name)),
            (t, Some(c)) => {
                if let Some(t) = t {
                    notes.push(format!("solid terrain draws with {}", t.name));
                }
                notes.push(format!("cutout terrain draws with {}", c.name));
                // Timed (and labelled) apart from the solid part when it runs the same program.
                if t.as_ref().is_some_and(|t| t.name == c.name) {
                    c.name.push_str(" (cutout)");
                }
            }
            (None, None) => {}
        }
        for (part, pass) in [
            ("terrain", &shadow_terrain),
            ("translucent terrain", &shadow_water),
        ] {
            if let Some(p) = pass {
                notes.push(format!("{part} casts shadows with {}", p.name));
            }
        }
        if let Some(w) = &water {
            notes.push(format!("translucent terrain draws with {}", w.name));
        }
        // Timed (and labelled) apart from the solid shadow pass that runs the same program.
        let mut shadow_water = shadow_water;
        if let Some(p) = &mut shadow_water {
            p.name.push_str(" (translucent)");
        }
        for (part, pass) in sky
            .iter()
            .flat_map(|s| [("sky", &s.basic), ("sun and moon", &s.textured)])
        {
            if let Some(p) = pass {
                notes.push(format!("the {part} draw with {}", p.name));
            }
        }
        let parts = [
            "entities",
            "block entities",
            "translucent entities",
            "particles",
        ];
        if let Some(l) = &lines {
            notes.push(format!("the block outline draws with {}", l.name));
        }
        if let Some(c) = &clouds {
            notes.push(format!("clouds draw with {}", c.name));
        }
        if let Some((h, _)) = &hand {
            notes.push(format!("the hand draws with {}", h.name));
        }
        if let Some(h) = &hand_water {
            notes.push(format!("translucent held items draw with {}", h.name));
        }
        if let Some([w, _]) = &weather {
            notes.push(format!("rain and snow draw with {}", w.name));
        }
        for (part, pass) in entities.iter().flat_map(|e| parts.iter().zip(&e.passes)) {
            if let Some(p) = pass {
                notes.push(format!("{part} draw with {}", p.name));
            }
        }
        // Timed (and labelled) apart from the terrain's shadow pass, which may run the same
        // program.
        let mut entities = entities;
        for e in entities.iter_mut() {
            let only = [
                (e.only.0, " (only the player)"),
                (e.only.1, " (only those that emit light)"),
            ];
            for ((part, pass), (only, which)) in parts.iter().zip(&mut e.shadow).zip(only) {
                if let Some(p) = pass {
                    let which = if only { which } else { "" };
                    notes.push(format!("{part}{which} cast shadows with {}", p.name));
                    p.name.push_str(&format!(" ({part})"));
                }
            }
        }
        let reads_depth = |source| {
            passes
                .iter()
                .filter(|p| matches!(p.group, Group::Deferred | Group::Composite | Group::Final))
                .chain(&water)
                .chain(weather.iter().take(1).flatten())
                .flat_map(|p| std::iter::once(p).chain(&p.compute))
                .any(|p| p.samplers.iter().any(|s| s.source == source))
        };
        let reads_depth2 = reads_depth(Source::Depth2);
        // Without a shader hand, depth1 also serves depth2: both exclude only translucency.
        let depth1 = (water.is_some() || hand.is_some())
            && (reads_depth(Source::Depth1) || (hand.is_none() && reads_depth2));
        let depth2 = hand.is_some() && reads_depth2;
        let every_pass = || {
            (passes
                .iter()
                .chain(&shadow_comp)
                .chain(&terrain)
                .chain(&terrain_cutout)
                .chain(&water))
            .chain(shadow_terrain.iter().chain(&shadow_water))
            .chain(sky.iter().flat_map(|s| s.basic.iter().chain(&s.textured)))
            .chain(entities.iter().flat_map(|e| e.passes.iter().flatten()))
            .chain(entities.iter().flat_map(|e| e.shadow.iter().flatten()))
            .chain(&clouds)
            .chain(&lines)
            .chain(weather.iter().flatten())
            .chain(hand.iter().map(|h| &h.0))
            .chain(&hand_water)
        };
        let shadow_targets = shadow_formats.map(|f| {
            let colors = every_pass().fold(0u16, |m, p| {
                let own = if p.group.shadow() {
                    p.reads | p.written | p.flip
                } else {
                    0
                };
                m | own | p.shadow_reads
            }) & shadow_colors;
            let depth1 = every_pass()
                .flat_map(|p| &p.samplers)
                .any(|s| matches!(s.source, Source::ShadowDepth(1, _)));
            let t = super::pack_shadow::ShadowTargets::new(
                device,
                &shadow_config,
                f,
                colors,
                shadow_doubled & colors,
                depth1,
            );
            notes.push(format!(
                "shadow map {0}x{0} over {2} blocks, sunPathRotation {3}, shadowcolor buffers {colors:#b} (two textures: {4:#b}){1}",
                t.size,
                if depth1 { ", shadowtex1" } else { "" },
                shadow_config.distance,
                shadow_config.sun_path_rotation,
                shadow_doubled & colors,
            ));
            t
        });
        log::info!(
            "shader pack: colortex buffers {used:#b}, with two textures {:#b}",
            doubled & used
        );
        // Any program can leave pixels untouched. Composite-style passes also write the
        // alternate image, so their first write does not replace the main image's clear.
        let mut cleared = Vec::new();
        for b in 1..BUFFERS {
            let bit = 1u16 << b;
            let touched = |p: &&Pass| (p.reads | p.flip | p.written) & bit != 0;
            // Gbuffers passes cover part of their targets: a clear is needed if they touch it.
            let first = sky
                .iter()
                .flat_map(|s| s.basic.iter().chain(&s.textured))
                .chain(&terrain)
                .chain(&terrain_cutout)
                .chain(entities.iter().flat_map(|e| e.passes.iter().flatten()))
                .chain(&clouds)
                .chain(&lines)
                .chain(&water)
                .chain(weather.iter().take(1).flatten())
                .chain(hand.iter().map(|h| &h.0))
                .chain(&hand_water)
                .find(touched)
                .map(|_| true)
                .or_else(|| passes.iter().find(touched).map(|_| true));
            if clears[b].is_some() && first == Some(true) {
                cleared.push(b);
            }
        }
        // Terrain tests against the world's depth (entities...), so the blit copies it then.
        let blit_depth = terrain.is_some()
            || terrain_cutout.is_some()
            || water.is_some()
            || sky.is_some()
            || entities.as_ref().is_some_and(Entities::drawn)
            || clouds.is_some()
            || lines.is_some()
            || weather.is_some()
            || hand.is_some()
            || passes
                .iter()
                .flat_map(|p| std::iter::once(p).chain(&p.compute))
                .any(|p| {
                    p.samplers.iter().any(|s| {
                        matches!(s.source, Source::Depth | Source::Depth1 | Source::Depth2)
                    })
                });
        // Buffers of a size of their own clear apart from the blit, which writes colortex0.
        let (mut cleared, mut other_clears): (Vec<usize>, Vec<usize>) =
            cleared.into_iter().partition(|&b| sizes[b].is_none());
        let limits = device.limits();
        let max_attachments = (limits.max_color_attachments as usize).min(8);
        let mut bytes = attachment_bytes(0, formats[0]);
        let mut attachments = 1;
        cleared.retain(|&b| {
            let next = attachment_bytes(bytes, formats[b]);
            if attachments < max_attachments && next <= limits.max_color_attachment_bytes_per_sample
            {
                attachments += 1;
                bytes = next;
                true
            } else {
                other_clears.push(b);
                false
            }
        });
        let float = wgpu::TextureSampleType::Float { filterable: true };
        // The world's colour is floats: an integer colortex0 (`RGBA32UI`...) gets only what the
        // pack writes, and is cleared in a pass of its own.
        let world_color = holds_floats(formats[0]);
        if !world_color {
            other_clears.push(0);
        }
        let mut world_target = world_color.then(|| target(formats[0])).flatten();
        if rgb & 1 != 0
            && let Some(t) = &mut world_target
        {
            t.write_mask = wgpu::ColorWrites::COLOR;
        }
        let blit_targets = std::iter::once(world_target);
        // Cleared by the pass's load operation; the shader writes nothing there.
        let blit_targets: Vec<_> = blit_targets
            .chain(cleared.iter().map(|&b| {
                Some(wgpu::ColorTargetState {
                    format: formats[b],
                    blend: None,
                    write_mask: wgpu::ColorWrites::empty(),
                })
            }))
            .collect();
        let blit = (world_color || blit_depth || !cleared.is_empty()).then(|| {
            helper(
                device,
                "vs_full",
                if blit_depth {
                    "fs_blit"
                } else {
                    "fs_blit_color"
                },
                &[
                    texture_entry(0, float),
                    texture_entry(1, wgpu::TextureSampleType::Float { filterable: false }),
                ],
                &blit_targets,
                // GL's window depth, written for depthtex0 and the terrain's depth test.
                blit_depth.then_some(wgpu::DepthStencilState {
                    format: wgpu::TextureFormat::Depth32Float,
                    depth_write_enabled: Some(true),
                    depth_compare: Some(wgpu::CompareFunction::Always),
                    stencil: Default::default(),
                    bias: Default::default(),
                }),
            )
        });
        let copy = helper(
            device,
            "vs_uv",
            "fs_copy",
            &[
                texture_entry(0, float),
                entry(
                    2,
                    wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                ),
            ],
            &[target(screen)],
            None,
        );
        let upscale = helper(
            device,
            "vs_screen",
            "fs_copy",
            &[
                texture_entry(0, float),
                entry(
                    2,
                    wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                ),
            ],
            &[target(screen)],
            None,
        );
        let filterable = |b: usize| {
            formats[b].sample_type(None, Some(device.features()))
                == Some(wgpu::TextureSampleType::Float { filterable: true })
        };
        let asked = passes.iter().fold(0u16, |m, p| m | p.mipmaps);
        let mipped = (0..BUFFERS)
            .filter(|&b| asked & 1 << b != 0 && filterable(b))
            .fold(0u16, |m, b| m | 1 << b);
        if asked & !mipped != 0 {
            notes.push(format!(
                "mipmaps of unfilterable buffers are not generated (buffers {:#b})",
                asked & !mipped
            ));
        }
        let mut down = HashMap::new();
        for b in (0..BUFFERS).filter(|b| mipped & 1 << b != 0) {
            down.entry(formats[b]).or_insert_with(|| {
                helper(
                    device,
                    "vs_full",
                    "fs_down",
                    &[
                        texture_entry(0, float),
                        entry(
                            2,
                            wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                        ),
                    ],
                    &[target(formats[b])],
                    None,
                )
            });
        }
        let white = [255u8; 4];
        let black = [0u8; 4];
        let flat_normal = [128u8, 128, 255, 255];
        Ok(Self {
            notes,
            setup_pending: true,
            passes,
            terrain,
            terrain_cutout,
            water,
            terrain_undrawn,
            water_undrawn,
            sky,
            entities,
            clouds,
            cloud_draw: None,
            lines,
            line_draw: None,
            hides_clouds,
            hides_sky,
            hides_celestial,
            sky_clear,
            weather,
            weather_draw: None,
            weather_generation: 0,
            hand,
            hand_water,
            depth1,
            depth2,
            shadow_targets,
            shadow_terrain,
            portal_rect: [0.0; 4],
            portal_texture: None,
            shadow_water,
            shadow_comp,
            mipped,
            mips_valid: 0,
            down,
            draws,
            block_ids,
            page_groups: HashMap::new(),
            gui_size: world.gui_size,
            timing: None,
            center_depth: None,
            custom,
            atlas: world.atlas.as_ref().map(|a| a.0.clone()),
            material_maps: world.material_maps,
            lightmap: world.lightmap,
            deferred_pre: mask(&post.deferred_pre),
            composite_pre: mask(&post.composite_pre),
            formats,
            clears,
            rgb,
            rgb_unset: true,
            used,
            doubled: doubled & used,
            storage,
            copied,
            sizes,
            blit_depth,
            blit_clears: cleared,
            other_clears,
            blit_group: None,
            main: 0,
            targets: None,
            noise: pack_textures::noise(&textures)
                .cloned()
                .unwrap_or_else(|| noise(device, queue, noise_size)),
            dummy: texture(device, queue, "Shader pack stand-in", 1, &white),
            dummy_black: texture(device, queue, "Shader pack black stand-in", 1, &black),
            dummy_normal: texture(
                device,
                queue,
                "Shader pack normal stand-in",
                1,
                &flat_normal,
            ),
            images: {
                let mut images = images;
                for (k, image) in images.iter_mut().enumerate() {
                    image.copied = image_copied & 1 << k != 0;
                    image.snapshotted = snapshotted & 1 << k != 0;
                }
                images
            },
            buffers,
            indices,
            shadow_culling: post.shadow_culling,
            gpu,
            zeros: None,
            stand_ins,
            dump: pack_images::Dump::new(),
            dummy_3d: device
                .create_texture(&wgpu::TextureDescriptor {
                    label: Some("Shader pack 3D stand-in"),
                    size: wgpu::Extent3d {
                        width: 1,
                        height: 1,
                        depth_or_array_layers: 1,
                    },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D3,
                    format: wgpu::TextureFormat::Rgba8Unorm,
                    usage: wgpu::TextureUsages::TEXTURE_BINDING,
                    view_formats: &[],
                })
                .create_view(&Default::default()),
            dummy_1d: device
                .create_texture(&wgpu::TextureDescriptor {
                    label: Some("Shader pack 1D stand-in"),
                    size: wgpu::Extent3d {
                        width: 1,
                        height: 1,
                        depth_or_array_layers: 1,
                    },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D1,
                    format: wgpu::TextureFormat::Rgba8Unorm,
                    usage: wgpu::TextureUsages::TEXTURE_BINDING,
                    view_formats: &[],
                })
                .create_view(&Default::default()),
            textures,
            shadow: (
                device
                    .create_texture(&wgpu::TextureDescriptor {
                        label: Some("Shader pack shadow stand-in"),
                        size: wgpu::Extent3d {
                            width: 1,
                            height: 1,
                            depth_or_array_layers: 1,
                        },
                        mip_level_count: 1,
                        sample_count: 1,
                        dimension: wgpu::TextureDimension::D2,
                        format: wgpu::TextureFormat::Depth32Float,
                        usage: wgpu::TextureUsages::TEXTURE_BINDING
                            | wgpu::TextureUsages::RENDER_ATTACHMENT,
                        view_formats: &[],
                    })
                    .create_view(&Default::default()),
                device.create_sampler(&wgpu::SamplerDescriptor {
                    label: Some("Shader pack shadow"),
                    compare: Some(wgpu::CompareFunction::LessEqual),
                    ..Default::default()
                }),
            ),
            shadow_cleared: false,
            linear: sampler(
                device,
                wgpu::FilterMode::Linear,
                wgpu::AddressMode::ClampToEdge,
            ),
            nearest: sampler(
                device,
                wgpu::FilterMode::Nearest,
                wgpu::AddressMode::ClampToEdge,
            ),
            repeat: sampler(device, wgpu::FilterMode::Linear, wgpu::AddressMode::Repeat),
            nearest_repeat: sampler(device, wgpu::FilterMode::Nearest, wgpu::AddressMode::Repeat),
            atlas_sampler: device.create_sampler(&wgpu::SamplerDescriptor {
                label: Some("Shader pack atlas"),
                address_mode_u: wgpu::AddressMode::Repeat,
                address_mode_v: wgpu::AddressMode::Repeat,
                mipmap_filter: wgpu::MipmapFilterMode::Linear,
                ..Default::default()
            }),
            buffer_sampler: device.create_sampler(&wgpu::SamplerDescriptor {
                label: Some("Shader pack buffers"),
                mag_filter: wgpu::FilterMode::Linear,
                min_filter: wgpu::FilterMode::Linear,
                mipmap_filter: wgpu::MipmapFilterMode::Linear,
                ..Default::default()
            }),
            blit,
            copy,
            upscale,
            final_target: None,
            screen,
            uniforms: {
                let mut u = FrameUniforms::default();
                u.atlas = world.atlas.map_or([0; 2], |a| a.1);
                u.shadow = shadow_config;
                u
            },
            shadow_cache: Default::default(),
        })
    }

    /// Writes `pass`'s uniforms for a target of `out` pixels; its bind group for the buffers'
    /// current mapping. The buffers it both samples and stores to are copied first, and it
    /// samples the copies.
    fn bind(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        pass: &mut Pass,
        out: [u32; 2],
    ) -> wgpu::BindGroup {
        pack_images::copy(encoder, &self.images, pass.image_copies);
        for &(_, image) in &pass.images {
            if let Image::StandIn(format, dimension) = image
                && let Some(s) = pack_images::stand_in(&self.stand_ins, format, dimension)
            {
                s.clear(encoder);
            }
        }
        if let Some(targets) = &self.targets {
            for i in (0..BUFFERS).filter(|i| pass.copies & 1 << i != 0) {
                if let (Some(t), Some(copy)) = (&targets.color[i], &targets.scratch[i]) {
                    let side = usize::from(self.main & 1 << i != 0);
                    encoder.copy_texture_to_texture(
                        t[side].texture().as_image_copy(),
                        copy.texture().as_image_copy(),
                        t[side].texture().size(),
                    );
                }
            }
        }
        let fog = self.uniforms.fog();
        // Our block members first (`hz_target`, `hz_fog_color`, `hz_fog`, `hz_depth_linear`),
        // then the pack's.
        let ours = [
            [
                out[0] as f32,
                out[1] as f32,
                1.0 / out[0] as f32,
                1.0 / out[1] as f32,
            ],
            fog[0].to_array(),
            fog[1].to_array(),
            pass.depth_linear,
        ];
        let head: &[u8] = bytemuck::cast_slice(&ours);
        let n = head.len().min(pass.block.len());
        pass.block[..n].copy_from_slice(&head[..n]);
        for (index, ty, offset) in &pass.uniforms {
            let Some((i, v)) = index.and_then(|i| Some((i, self.uniforms.values.get(i)?))) else {
                continue;
            };
            // A built-in of another type keeps its initial value (0); custom uniforms were
            // checked when the pass was built.
            if pack_uniforms::NAMES
                .get(i)
                .is_none_or(|name| v.read_as(name, ty))
            {
                v.write(ty, &mut pass.block, *offset as usize);
            }
        }
        if !pass.block.is_empty() {
            queue.write_buffer(&pass.ubo, 0, &pass.block);
        }
        let targets = self.targets.as_ref().expect("resized");
        let shadow = self.shadow_targets.as_ref();
        let shadow_main = shadow.map_or(0, |s| s.main);
        let shadow_space = pass.group.shadow();
        // A chain is sampled only while it is current: a buffer written since its last chain
        // reads its first level, never stale levels (6.1.7). The shadow buffers have none.
        let (main, current) = if shadow_space {
            (shadow_main, 0)
        } else {
            (self.main, self.mips_valid & self.mipped & pass.reads)
        };
        let key = u64::from(main & pass.reads)
            | u64::from(current) << 16
            | u64::from(shadow_main & pass.shadow_reads) << 32;
        let group = pass.groups.entry(key).or_insert_with(|| {
            let mut entries = vec![wgpu::BindGroupEntry {
                binding: 0,
                resource: pass.ubo.as_entire_binding(),
            }];
            for s in &pass.samplers {
                let shadow_color = |i: usize, side: usize| {
                    shadow.and_then(|t| t.color[i].as_ref()).map(|c| &c[side])
                };
                let texture = match s.source {
                    Source::Color(i) if shadow_space => {
                        shadow_color(i, usize::from(key & 1 << i != 0)).unwrap_or(&self.dummy)
                    }
                    Source::ShadowColor(i) => {
                        shadow_color(i, usize::from(key & 1 << (32 + i) != 0))
                            .unwrap_or(&self.dummy)
                    }
                    Source::ShadowDepth(i, _) => match shadow {
                        Some(t) => match (i, &t.depth1) {
                            (1, Some(d)) => &d.1,
                            _ => &t.depth.1,
                        },
                        None => &self.shadow.0,
                    },
                    Source::Color(i) if pass.copies & 1 << i != 0 => {
                        targets.scratch[i].as_ref().unwrap_or(&self.dummy)
                    }
                    Source::Color(i) => {
                        let side = usize::from(key & 1 << i != 0);
                        match (&targets.mips[i], &targets.color[i]) {
                            (Some(m), _) if current & 1 << i != 0 => &m[side].all,
                            (_, Some(t)) => &t[side],
                            (_, None) => &self.dummy,
                        }
                    }
                    Source::Depth => &targets.depth,
                    // Before translucent terrain the two are the same.
                    Source::Depth1 => match &targets.depth1 {
                        Some(d)
                            if matches!(
                                pass.group,
                                Group::Deferred
                                    | Group::Composite
                                    | Group::Final
                                    | Group::Translucent
                            ) =>
                        {
                            &d.1
                        }
                        _ => &targets.depth,
                    },
                    Source::Depth2 => match &targets.depth2 {
                        Some(d)
                            if matches!(
                                pass.group,
                                Group::Deferred
                                    | Group::Composite
                                    | Group::Final
                                    | Group::Translucent
                            ) =>
                        {
                            &d.1
                        }
                        _ => match &targets.depth1 {
                            Some(d)
                                if matches!(
                                    pass.group,
                                    Group::Deferred
                                        | Group::Composite
                                        | Group::Final
                                        | Group::Translucent
                                ) =>
                            {
                                &d.1
                            }
                            _ => &targets.depth,
                        },
                    },
                    Source::Noise => &self.noise,
                    Source::Dummy if s.dimension == wgpu::TextureViewDimension::D3 => {
                        &self.dummy_3d
                    }
                    Source::Dummy if s.dimension == wgpu::TextureViewDimension::D1 => {
                        &self.dummy_1d
                    }
                    Source::Normals => self
                        .material_maps
                        .as_ref()
                        .filter(|_| pass.material_maps)
                        .map_or(&self.dummy_normal, |maps| &maps[0]),
                    Source::Specular => self
                        .material_maps
                        .as_ref()
                        .filter(|_| pass.material_maps)
                        .map_or(&self.dummy_black, |maps| &maps[1]),
                    Source::Dummy => &self.dummy,
                    Source::Image(k) if pass.image_copies & 1 << k != 0 => {
                        self.images[k].copy.as_ref().unwrap_or(&self.dummy)
                    }
                    Source::Image(k) => self.images[k].view.as_ref().unwrap_or(&self.dummy),
                    Source::Custom { index, .. } => match &self.textures[index].image {
                        pack_textures::Image::View(v) => v,
                        _ => &self.dummy,
                    },
                    Source::Shadow => &self.shadow.0,
                    Source::Atlas => pass
                        .atlas
                        .as_ref()
                        .or(self.atlas.as_ref())
                        .unwrap_or(&self.dummy),
                    Source::Lightmap => &self.lightmap,
                };
                let sampler = match SamplerKind::of(s.source, s.filtering) {
                    SamplerKind::Linear => &self.linear,
                    SamplerKind::Nearest => &self.nearest,
                    SamplerKind::Repeat => &self.repeat,
                    SamplerKind::NearestRepeat => &self.nearest_repeat,
                    SamplerKind::Atlas => &self.atlas_sampler,
                    SamplerKind::Buffer => &self.buffer_sampler,
                    SamplerKind::Comparison => shadow.map_or(&self.shadow.1, |t| &t.comparison),
                };
                entries.push(wgpu::BindGroupEntry {
                    binding: s.binding,
                    resource: wgpu::BindingResource::TextureView(texture),
                });
                if s.sampler == s.binding + 1 {
                    entries.push(wgpu::BindGroupEntry {
                        binding: s.sampler,
                        resource: wgpu::BindingResource::Sampler(sampler),
                    });
                }
            }
            // An image is the side its buffer's sampler reads.
            for &(binding, image) in &pass.images {
                let view = match image {
                    Image::Color(i) => {
                        let side = usize::from(key & 1 << i != 0);
                        targets.color[i].as_ref().map_or(&self.dummy, |t| &t[side])
                    }
                    Image::Custom(k) => self.images[k].view.as_ref().unwrap_or(&self.dummy),
                    Image::ColorBefore(i) => targets.scratch[i].as_ref().unwrap_or(&self.dummy),
                    Image::CustomBefore(k) => self.images[k].before.as_ref().unwrap_or(&self.dummy),
                    Image::StandIn(format, dimension) => {
                        pack_images::stand_in(&self.stand_ins, format, dimension)
                            .map_or(&self.dummy, |s| &s.view)
                    }
                };
                entries.push(wgpu::BindGroupEntry {
                    binding,
                    resource: wgpu::BindingResource::TextureView(view),
                });
            }
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some(&pass.name),
                layout: &pass.layout,
                entries: &entries,
            })
        });
        group.clone()
    }

    /// Dispatches compute pass `pass` over a target of `size`.
    fn dispatch(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        pass: &mut Pass,
        size: [u32; 2],
    ) {
        let Pipeline::Compute {
            pipeline,
            groups,
            render,
            local,
        } = &pass.pipeline
        else {
            return;
        };
        let groups = groups.unwrap_or_else(|| {
            let n = |k: usize| (size[k] as f32 * render[k] / local[k] as f32).ceil() as u32;
            [n(0), n(1), 1]
        });
        let (pipeline, groups) = (pipeline.clone(), groups);
        for image in (0..self.images.len()).filter(|k| pass.snapshots & 1 << k != 0) {
            pack_images::snapshot(encoder, &self.images[image]);
        }
        let group = self.bind(device, queue, encoder, pass, size);
        let mut cp = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some(&pass.name),
            timestamp_writes: self
                .timing
                .as_mut()
                .and_then(|t| t.compute_writes(&pass.name)),
        });
        cp.set_pipeline(&pipeline);
        cp.set_bind_group(0, &group, &[]);
        if pass.buffers
            && let Some(g) = &self.buffers.group
        {
            cp.set_bind_group(4, g, &[]);
        }
        cp.dispatch_workgroups(groups[0], groups[1], groups[2]);
        drop(cp);
        self.mips_valid &= !pass.written;
    }

    /// A render pass into the buffers `pass` writes, in place, tested against the world's depth.
    fn begin<'e>(
        &mut self,
        encoder: &'e mut wgpu::CommandEncoder,
        pass: &Pass,
    ) -> wgpu::RenderPass<'e> {
        let targets = self.targets.as_ref().expect("resized");
        let load = wgpu::Operations {
            load: wgpu::LoadOp::Load,
            store: wgpu::StoreOp::Store,
        };
        // The shadow pass draws into the shadow buffers and the shadow map, the frame's first
        // one clearing them.
        let (depth_ops, shadow_ops) = match &mut self.shadow_targets {
            Some(s) if pass.group.shadow() => s.clear_ops(pass.writes.iter().map(|w| w.1)),
            _ => (super::pack_shadow::load(), [load; SHADOW_COLORS]),
        };
        let shadow = self.shadow_targets.as_ref().filter(|_| pass.group.shadow());
        let mut attachments: Vec<Option<wgpu::RenderPassColorAttachment>> = Vec::new();
        for &(location, buffer) in &pass.writes {
            let l = location as usize;
            if attachments.len() <= l {
                attachments.resize(l + 1, None);
            }
            attachments[l] = match shadow {
                Some(s) => s
                    .color(buffer, false)
                    .and_then(|v| attach(v, shadow_ops.get(buffer).copied().unwrap_or(load))),
                None => targets.color[buffer]
                    .as_ref()
                    .and_then(|t| attach(&t[usize::from(self.main & 1 << buffer != 0)], load)),
            };
        }
        let timestamp_writes = self.timing.as_mut().and_then(|t| t.writes(&pass.name));
        encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some(&pass.name),
            color_attachments: &attachments,
            timestamp_writes,
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: shadow.map_or(&targets.depth, |s| &s.depth.1),
                depth_ops: Some(depth_ops),
                stencil_ops: None,
            }),
            ..Default::default()
        })
    }

    /// Terrain (or, `translucent`, translucent terrain) with the pack's program; `draw` issues
    /// the game's draws. Solid terrain in two parts has its cutout part drawn second, from the
    /// same draws.
    fn draw_terrain(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        size: [u32; 2],
        list: TerrainList,
        draw: &mut TerrainDraw<'_>,
    ) {
        self.draw_terrain_part(device, queue, encoder, size, list, false, draw);
        if list == TerrainList::Solid {
            self.draw_terrain_part(device, queue, encoder, size, list, true, draw);
        }
    }

    /// The pass that draws `list`, or the cutout part of the solid list.
    fn terrain_slot(&mut self, list: TerrainList, cutout: bool) -> &mut Option<Pass> {
        match list {
            TerrainList::Solid if cutout => &mut self.terrain_cutout,
            TerrainList::Solid => &mut self.terrain,
            TerrainList::Translucent => &mut self.water,
            TerrainList::ShadowSolid => &mut self.shadow_terrain,
            TerrainList::ShadowTranslucent => &mut self.shadow_water,
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_terrain_part(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        size: [u32; 2],
        list: TerrainList,
        cutout: bool,
        draw: &mut TerrainDraw<'_>,
    ) {
        let Some(mut pass) = self.terrain_slot(list, cutout).take() else {
            return;
        };
        let translucent = list.translucent();
        let group = self.bind(device, queue, encoder, &mut pass, size);
        {
            let mut rp = self.begin(encoder, &pass);
            rp.set_pipeline(pass.render());
            rp.set_bind_group(0, &group, &[]);
            if pass.buffers
                && let Some(g) = &self.buffers.group
            {
                rp.set_bind_group(4, g, &[]);
            }
            if pass.draw {
                let stage = if translucent {
                    "TERRAIN_TRANSLUCENT"
                } else {
                    "TERRAIN_SOLID"
                };
                self.draws.bind(&mut rp, stage);
            }
            let (groups, ids) = (&self.page_groups, pass.ids);
            let page = |rp: &mut wgpu::RenderPass<'_>, page: u32| {
                if let Some((_, group)) = groups.get(&page).filter(|_| ids) {
                    rp.set_bind_group(3, group, &[]);
                }
            };
            let geometry = self
                .indices
                .as_ref()
                .filter(|_| pass.geometry > 0)
                .map(|indices| (pass.geometry, indices));
            draw(&mut rp, list, &page, geometry);
        }
        if !pass.group.shadow() {
            self.mips_valid &= !pass.written;
        }
        *self.terrain_slot(list, cutout) = Some(pass);
    }

    /// The sky's parts in vanilla's order, each with the program that draws it; one render pass
    /// while consecutive programs write the same buffers.
    fn draw_sky(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        size: [u32; 2],
        frame: &super::Frame,
    ) {
        let Some(mut sky) = self.sky.take() else {
            return;
        };
        let parts = sky
            .geometry
            .prepare(queue, frame, self.uniforms.shadow.sun_path_rotation);
        let mut groups = [None, None];
        for (i, pass) in [&mut sky.basic, &mut sky.textured].into_iter().enumerate() {
            if let Some(pass) = pass {
                groups[i] = Some(self.bind(device, queue, encoder, pass, size));
                self.mips_valid &= !pass.written;
            }
        }
        let passes = [&sky.basic, &sky.textured];
        let mut open: Option<(wgpu::RenderPass<'_>, usize)> = None;
        for part in parts {
            let i = usize::from(part.textured);
            let (Some(pass), Some(group)) = (passes[i], &groups[i]) else {
                continue;
            };
            let current = open.as_ref().map(|o| o.1);
            if current != Some(i) {
                let same = current
                    .and_then(|c| passes[c].as_ref())
                    .is_some_and(|p| p.writes == pass.writes);
                if !same {
                    drop(open.take());
                    open = Some((self.begin(encoder, pass).forget_lifetime(), i));
                }
                let rp = &mut open.as_mut().expect("open").0;
                rp.set_pipeline(pass.render());
                rp.set_bind_group(0, group, &[]);
                // A sky program that reads the pack's SSBOs (Noble's `gbuffers_textured`, the
                // sun's fallback) binds them, as the other passes do.
                if pass.buffers
                    && let Some(g) = &self.buffers.group
                {
                    rp.set_bind_group(4, g, &[]);
                }
                sky.geometry.bind(rp, part.textured);
                open.as_mut().expect("open").1 = i;
            }
            let rp = &mut open.as_mut().expect("open").0;
            // Each part with vanilla's blend for it: one program, three pipelines.
            let part_pipeline =
                pass.sky_parts
                    .as_ref()
                    .and_then(|[plain, stars]| match part.stage {
                        "SKY" | "VOID" => Some(plain),
                        "STARS" => Some(stars),
                        _ => None,
                    });
            rp.set_pipeline(part_pipeline.unwrap_or(pass.render()));
            if pass.draw {
                self.draws.bind(rp, part.stage);
            }
            rp.draw(part.range, 0..1);
        }
        drop(open);
        self.sky = Some(sky);
    }

    /// Entities, block entities, translucent entity parts and particles with the pack's
    /// programs, those drawn before deferred or (`late`) after: each run `prepare_entities` kept
    /// is one draw with its own `entityId` or `blockEntityId` and `entityColor`.
    fn draw_entities(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        size: [u32; 2],
        late: bool,
    ) {
        let Some(mut entities) = self.entities.take() else {
            return;
        };
        let geometry = &entities.geometry;
        for (program, pass) in entities.passes.iter_mut().enumerate() {
            let Some(pass) = pass.as_mut().filter(|_| entities.late[program] == late) else {
                continue;
            };
            let mut draws = geometry
                .draws
                .iter()
                .filter(|d| d.pack && d.program == program)
                .peekable();
            if draws.peek().is_none() {
                continue;
            }
            let group = self.bind(device, queue, encoder, pass, size);
            let portal_group = self.portal_group(
                device,
                queue,
                encoder,
                pass,
                size,
                geometry
                    .draws
                    .iter()
                    .any(|d| d.pack && d.program == program && d.portal),
            );
            {
                let mut rp = self.begin(encoder, pass);
                rp.set_pipeline(pass.render());
                rp.set_bind_group(0, &group, &[]);
                if pass.buffers
                    && let Some(g) = &self.buffers.group
                {
                    rp.set_bind_group(4, g, &[]);
                }
                rp.set_bind_group(1, &geometry.table_group, &[]);
                rp.set_vertex_buffer(0, geometry.vertices());
                rp.set_index_buffer(geometry.indices.slice(..), wgpu::IndexFormat::Uint32);
                let mut on_portal = false;
                for d in draws {
                    if let Some(portal) = &portal_group
                        && d.portal != on_portal
                    {
                        rp.set_bind_group(0, if d.portal { portal } else { &group }, &[]);
                        on_portal = d.portal;
                    }
                    if pass.draw {
                        rp.set_bind_group(2, &geometry.group, &[geometry.offset(d.slot)]);
                    }
                    rp.draw_indexed(0..d.quads * 6, d.base as i32, 0..1);
                }
            }
            self.mips_valid &= !pass.written;
        }
        self.entities = Some(entities);
    }

    /// End portal faces bind `end_portal.png` as the atlas, a texture of its own, so packs that
    /// wrap it with their own coordinates sample the portal sprite only. `None` when no portal
    /// face draws in this pass.
    fn portal_group(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        pass: &mut Pass,
        size: [u32; 2],
        wanted: bool,
    ) -> Option<wgpu::BindGroup> {
        let atlas = pass.atlas.clone().or_else(|| self.atlas.clone())?;
        if !wanted || self.portal_rect[2] <= 0.0 {
            return None;
        }
        if self
            .portal_texture
            .as_ref()
            .is_none_or(|(rect, _)| *rect != self.portal_rect)
        {
            let view = super::pack_weather::copy_sprite(
                device,
                encoder,
                atlas.texture(),
                self.portal_rect,
            );
            self.portal_texture = Some((self.portal_rect, view));
            pass.portal_groups.clear();
        }
        // `bind` caches groups by a key without the atlas: the portal groups are kept in a cache
        // of their own, swapped in with the texture.
        let view = self.portal_texture.as_ref().map(|p| p.1.clone());
        let saved = std::mem::replace(&mut pass.atlas, view);
        let groups = std::mem::replace(&mut pass.groups, std::mem::take(&mut pass.portal_groups));
        let group = self.bind(device, queue, encoder, pass, size);
        pass.portal_groups = std::mem::replace(&mut pass.groups, groups);
        pass.atlas = saved;
        Some(group)
    }

    /// Entities and block entities into the shadow map, each run with the per-draw uniforms
    /// of its gbuffers draw (2.4): opaque, so before the `shadowtex1` copy.
    fn draw_shadow_entities(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        size: [u32; 2],
    ) {
        let Some(mut entities) = self.entities.take() else {
            return;
        };
        let geometry = &entities.geometry;
        for (program, pass) in entities.shadow.iter_mut().enumerate() {
            let Some(pass) = pass else {
                continue;
            };
            let mut draws = (geometry.draws.iter())
                .filter(|d| d.shadow && d.program == program)
                .peekable();
            if draws.peek().is_none() {
                continue;
            }
            let group = self.bind(device, queue, encoder, pass, size);
            let portal_group = self.portal_group(
                device,
                queue,
                encoder,
                pass,
                size,
                (geometry.draws.iter()).any(|d| d.shadow && d.program == program && d.portal),
            );
            let mut on_portal = false;
            let mut rp = self.begin(encoder, pass);
            rp.set_pipeline(pass.render());
            rp.set_bind_group(0, &group, &[]);
            if pass.buffers
                && let Some(g) = &self.buffers.group
            {
                rp.set_bind_group(4, g, &[]);
            }
            rp.set_bind_group(1, &geometry.table_group, &[]);
            rp.set_vertex_buffer(0, geometry.vertices());
            rp.set_index_buffer(geometry.indices.slice(..), wgpu::IndexFormat::Uint32);
            for d in draws {
                if let Some(portal) = &portal_group
                    && d.portal != on_portal
                {
                    rp.set_bind_group(0, if d.portal { portal } else { &group }, &[]);
                    on_portal = d.portal;
                }
                if pass.draw {
                    rp.set_bind_group(2, &geometry.group, &[geometry.offset(d.slot)]);
                }
                if pass.geometry > 0 {
                    // A geometry stage reads its vertices itself: corner `c` of the run is
                    // vertex `c` past the run's first quad's six (`geometry_inputs`), one
                    // instance per triangle the stage can emit.
                    let first = d.base / 4 * 6;
                    rp.draw(first..first + d.quads * 6, 0..pass.geometry);
                } else {
                    rp.draw_indexed(0..d.quads * 6, d.base as i32, 0..1);
                }
            }
        }
        self.entities = Some(entities);
    }

    /// Splits `entities` (`Frame::entities`) between the pack and Horizon by its runs
    /// (`spans`): the pack's part is uploaded for this frame's draw, Horizon's goes to `rest`.
    pub fn prepare_entities(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        entities: &[super::Vertex],
        spans: &[super::pack_draw::EntitySpan],
        crumbling: [f32; 4],
        portal: [f32; 4],
        rest: &mut Vec<super::Vertex>,
        skin: Option<&super::entity_skin::SkinFrame>,
        motion: (&[(u32, u64)], &[u32]),
    ) {
        self.portal_rect = portal;
        let shadow = self.shadow_casters();
        let Some(e) = &mut self.entities else {
            rest.clear();
            rest.extend_from_slice(entities);
            return;
        };
        let drawn = e.passes.each_ref().map(Option::is_some);
        e.geometry.prepare(
            device,
            queue,
            &self.draws.layout,
            entities,
            spans,
            drawn,
            (crumbling, self.gui_size),
            shadow,
            rest,
            skin,
            motion,
        );
    }

    /// Every stage a posable subject can use must have a compiled fallback.
    /// Special models, player and eye/tagged layers retain their CPU paths.
    pub fn skins_entities(&self) -> bool {
        self.entities
            .as_ref()
            .is_some_and(|e| e.passes[..3].iter().all(Option::is_some))
    }

    pub fn skin_target(&self) -> Option<&wgpu::Buffer> {
        self.entities.as_ref().map(|e| e.geometry.skin_target())
    }

    /// Which entity runs the shadow pass draws: none without it.
    pub fn shadow_casters(&self) -> super::pack_entities::Casters {
        let Some(e) = self
            .entities
            .as_ref()
            .filter(|_| self.shadow_targets.is_some())
        else {
            return Default::default();
        };
        super::pack_entities::Casters {
            entities: e.shadow[0].is_some(),
            only_player: e.only.0,
            blocks: e.shadow[1].is_some(),
            only_lit: e.only.1,
        }
    }

    /// The centre depth the frame copied: its readback, started by calling it right after the
    /// frame's submission.
    pub fn submitted(&mut self) -> Option<impl FnOnce() + Send + 'static> {
        self.center_depth.as_mut()?.submitted()
    }

    /// Whether the pack draws entities or block entities itself.
    pub fn has_entities(&self) -> bool {
        self.entities.is_some()
    }

    /// The sky, when it shows, then terrain, entities and block entities.
    fn draw_opaque(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        size: [u32; 2],
        frame: &super::Frame,
        terrain: &mut TerrainDraw<'_>,
    ) {
        if super::sky_visible(frame) {
            self.draw_sky(device, queue, encoder, size, frame);
        }
        self.draw_terrain(device, queue, encoder, size, TerrainList::Solid, terrain);
        self.draw_entities(device, queue, encoder, size, false);
        self.draw_lines(device, queue, encoder, size);
        self.draw_clouds(device, queue, encoder, size);
        // Only depth2 excludes the opaque first-person hand.
        if let Some(t) = &self.targets
            && let Some((copy, _)) = &t.depth2
        {
            encoder.copy_texture_to_texture(
                t.depth_texture.as_image_copy(),
                copy.as_image_copy(),
                t.depth_texture.size(),
            );
        }
        self.draw_hand(device, queue, encoder, size, frame);
        // Depth1 includes that hand, but still precedes translucent terrain.
        if let Some(t) = &self.targets
            && let Some((copy, _)) = &t.depth1
        {
            encoder.copy_texture_to_texture(
                t.depth_texture.as_image_copy(),
                copy.as_image_copy(),
                t.depth_texture.size(),
            );
        }
    }

    /// The first-person hand and held items, with `gbuffers_hand` at the hand's projection.
    fn draw_hand(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        size: [u32; 2],
        frame: &super::Frame,
    ) {
        let Some((mut pass, mut geometry)) = self.hand.take() else {
            return;
        };
        // The hand draws through `gbufferModelView`, which holds the nausea spin the world
        // turns with; the hand doesn't (as vanilla's), so its projection takes the spin out.
        let unspin = frame.bob * frame.spin.inverse() * frame.bob.inverse();
        let projection = super::pack_hand::projection(
            frame.hand_fov_degrees,
            size[0] as f32 / size[1].max(1) as f32,
            frame.depth_far,
        ) * unspin;
        geometry.prepare(device, queue, frame.hand, frame.hand_items, projection);
        if geometry.quads > 0 {
            let group = self.bind(device, queue, encoder, &mut pass, size);
            {
                let mut rp = self.begin(encoder, &pass);
                rp.set_pipeline(pass.render());
                rp.set_bind_group(0, &group, &[]);
                if pass.buffers
                    && let Some(g) = &self.buffers.group
                {
                    rp.set_bind_group(4, g, &[]);
                }
                rp.set_bind_group(1, &geometry.group, &[]);
                rp.set_vertex_buffer(0, geometry.vertices());
                rp.set_index_buffer(geometry.indices.slice(..), wgpu::IndexFormat::Uint32);
                // The arm and each held item: their own `currentRenderedItemId`. Translucent
                // quads wait for `gbuffers_hand_water` when the pack has one.
                let late = self.hand_water.is_some();
                let runs = geometry
                    .runs
                    .iter()
                    .chain(geometry.translucent_runs.iter().filter(|_| !late));
                for &(first, quads, offset) in runs {
                    if pass.draw {
                        rp.set_bind_group(2, &geometry.slot_group, &[offset]);
                    }
                    rp.draw_indexed(first * 6..(first + quads) * 6, 0, 0..1);
                }
            }
            self.mips_valid &= !pass.written;
        }
        self.hand = Some((pass, geometry));
    }

    /// Whether the pack draws the first-person hand itself.
    pub fn has_hand(&self) -> bool {
        self.hand.is_some()
    }

    /// The clouds `set_clouds` gave for this frame, with `gbuffers_clouds`.
    fn draw_clouds(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        size: [u32; 2],
    ) {
        // The pass stays for later frames when this one has no clouds.
        let Some(clouds) = self.cloud_draw.take() else {
            return;
        };
        let Some(mut pass) = self.clouds.take() else {
            return;
        };
        let group = self.bind(device, queue, encoder, &mut pass, size);
        {
            let mut rp = self.begin(encoder, &pass);
            rp.set_pipeline(pass.render());
            rp.set_bind_group(0, &group, &[]);
            if pass.buffers
                && let Some(g) = &self.buffers.group
            {
                rp.set_bind_group(4, g, &[]);
            }
            rp.set_bind_group(1, &clouds.group, &[]);
            if pass.draw {
                self.draws.bind(&mut rp, "CLOUDS");
            }
            rp.set_vertex_buffer(0, clouds.vertices.slice(..));
            rp.set_index_buffer(clouds.indices.slice(..), wgpu::IndexFormat::Uint32);
            rp.draw_indexed(0..clouds.quads * 6, 0, 0..1);
        }
        self.mips_valid &= !pass.written;
        self.clouds = Some(pass);
    }

    /// The block outline `set_lines` gave for this frame, with the pack's lines program.
    fn draw_lines(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        size: [u32; 2],
    ) {
        // The pass stays for later frames when this one has no outline.
        let Some(lines) = self.line_draw.take() else {
            return;
        };
        let Some(mut pass) = self.lines.take() else {
            return;
        };
        let group = self.bind(device, queue, encoder, &mut pass, size);
        {
            let mut rp = self.begin(encoder, &pass);
            rp.set_pipeline(pass.render());
            rp.set_bind_group(0, &group, &[]);
            if pass.buffers
                && let Some(g) = &self.buffers.group
            {
                rp.set_bind_group(4, g, &[]);
            }
            if pass.draw {
                self.draws.bind(&mut rp, "OUTLINE");
            }
            rp.set_vertex_buffer(0, lines.edges.slice(..));
            rp.draw(0..6, 0..lines.count);
        }
        self.mips_valid &= !pass.written;
        self.lines = Some(pass);
    }

    /// Whether the pack draws the block outline itself.
    pub fn has_lines(&self) -> bool {
        self.lines.is_some()
    }

    /// The block outline to draw this frame (`None`: none).
    pub fn set_lines(&mut self, lines: Option<Lines>) {
        self.line_draw = lines.filter(|l| l.count > 0);
    }

    /// Whether the pack draws the clouds itself (or hides them).
    pub fn has_clouds(&self) -> bool {
        self.hides_clouds
    }

    /// The clouds to draw this frame (`None`: none).
    pub fn set_clouds(&mut self, clouds: Option<Clouds>) {
        self.cloud_draw = clouds.filter(|c| c.quads > 0);
    }

    /// Translucent terrain, then the entity parts drawn after deferred.
    fn draw_translucent(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        size: [u32; 2],
        terrain: &mut TerrainDraw<'_>,
    ) {
        self.draw_terrain(
            device,
            queue,
            encoder,
            size,
            TerrainList::Translucent,
            terrain,
        );
        self.draw_entities(device, queue, encoder, size, true);
        self.draw_hand_water(device, queue, encoder, size);
        self.draw_weather(device, queue, encoder, size);
    }

    /// The hand's translucent quads (uploaded by `draw_hand` this frame), with
    /// `gbuffers_hand_water`.
    fn draw_hand_water(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        size: [u32; 2],
    ) {
        let Some(mut pass) = self.hand_water.take() else {
            return;
        };
        let hand = self.hand.take();
        if let Some((_, geometry)) = hand
            .as_ref()
            .filter(|(_, g)| g.quads > 0 && !g.translucent_runs.is_empty())
        {
            let group = self.bind(device, queue, encoder, &mut pass, size);
            {
                let mut rp = self.begin(encoder, &pass);
                rp.set_pipeline(pass.render());
                rp.set_bind_group(0, &group, &[]);
                if pass.buffers
                    && let Some(g) = &self.buffers.group
                {
                    rp.set_bind_group(4, g, &[]);
                }
                rp.set_bind_group(1, &geometry.group, &[]);
                rp.set_vertex_buffer(0, geometry.vertices());
                rp.set_index_buffer(geometry.indices.slice(..), wgpu::IndexFormat::Uint32);
                for &(first, quads, offset) in &geometry.translucent_runs {
                    if pass.draw {
                        rp.set_bind_group(2, &geometry.slot_group, &[offset]);
                    }
                    rp.draw_indexed(first * 6..(first + quads) * 6, 0, 0..1);
                }
            }
            self.mips_valid &= !pass.written;
        }
        self.hand = hand;
        self.hand_water = Some(pass);
    }

    /// The rain, then the snow, `set_weather` gave for this frame, with `gbuffers_weather`.
    fn draw_weather(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        size: [u32; 2],
    ) {
        // The passes stay for later frames when this one has no rain or snow.
        let Some(weather) = self.weather_draw.take() else {
            return;
        };
        let Some(mut passes) = self.weather.take() else {
            return;
        };
        if weather.generation != self.weather_generation {
            self.weather_generation = weather.generation;
            for (pass, sprite) in passes.iter_mut().zip(&weather.sprites) {
                pass.atlas = sprite.clone();
                pass.groups.clear();
            }
        }
        for (pass, quads) in passes.iter_mut().zip(weather.quads.clone()) {
            if quads.is_empty() {
                continue;
            }
            let group = self.bind(device, queue, encoder, pass, size);
            {
                let mut rp = self.begin(encoder, pass);
                rp.set_pipeline(pass.render());
                rp.set_bind_group(0, &group, &[]);
                if pass.buffers
                    && let Some(g) = &self.buffers.group
                {
                    rp.set_bind_group(4, g, &[]);
                }
                if pass.draw {
                    self.draws.bind(&mut rp, "RAIN_SNOW");
                }
                rp.set_vertex_buffer(0, weather.vertices.slice(..));
                rp.set_index_buffer(weather.indices.slice(..), wgpu::IndexFormat::Uint32);
                rp.draw_indexed(quads.start * 6..quads.end * 6, 0, 0..1);
            }
            self.mips_valid &= !pass.written;
        }
        self.weather = Some(passes);
    }

    /// Whether the pack draws rain and snow itself.
    pub fn has_weather(&self) -> bool {
        self.weather.is_some()
    }

    /// The rain and snow to draw this frame (`None`: none).
    pub fn set_weather(&mut self, weather: Option<Weather>) {
        self.weather_draw = weather;
    }

    /// Regenerates the mip chain of buffer `b`'s side that programs read.
    fn mipmap(&mut self, encoder: &mut wgpu::CommandEncoder, b: usize) {
        let (Some(targets), Some((pipeline, _))) = (&self.targets, self.down.get(&self.formats[b]))
        else {
            return;
        };
        let Some(mips) = &targets.mips[b] else { return };
        let side = &mips[usize::from(self.main & 1 << b != 0)];
        // Timed as one pass per buffer (its first level), so the pass timer shows the chains.
        let label = format!("mipmaps colortex{b}");
        let mut first = true;
        for (attachment, group) in &side.levels {
            let timestamp_writes = if std::mem::take(&mut first) {
                self.timing.as_mut().and_then(|t| t.writes(&label))
            } else {
                None
            };
            let mut rp = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Shader pack mipmap"),
                timestamp_writes,
                color_attachments: &[attach(
                    attachment,
                    wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                )],
                ..Default::default()
            });
            rp.set_pipeline(pipeline);
            rp.set_bind_group(0, group, &[]);
            rp.draw(0..3, 0..1);
        }
    }

    /// What the shadow camera draws of the terrain this frame, when the pack draws it.
    pub fn shadow_view(&self, frame: &super::Frame) -> Option<ShadowView> {
        if !self.has_shadow() {
            return None;
        }
        let shadow = &self.uniforms.shadow;
        Some(ShadowView {
            cull: pack_uniforms::shadow_culling(frame, shadow),
            light: pack_uniforms::shadow_light(frame, shadow),
            distance: (shadow.render_mul > 0.0).then_some(shadow.distance * shadow.render_mul),
            unculled: match self.shadow_culling {
                horizon_shaders::ShadowCulling::On => None,
                horizon_shaders::ShadowCulling::Off => Some(f32::INFINITY),
                horizon_shaders::ShadowCulling::Reversed => Some(shadow.voxel_distance),
            },
            every_face: true,
            view_casters: true,
            depth_planes: false,
        })
    }

    /// Whether the buffers `pass` writes have different sizes at the current render size: then
    /// it is skipped, as one whose `size.buffer` sizes never meet is when the pack loads. Only
    /// a pass writing buffers sized relative to the render size and in pixels can.
    fn sizes_differ(&self, pass: &Pass) -> bool {
        let Some(targets) = &self.targets else {
            return false;
        };
        let mut sizes = pass.writes.iter().map(|&(_, b)| targets.sizes[b]);
        let first = sizes.next();
        !pass.group.shadow() && pass.group != Group::Final && sizes.any(|s| Some(s) != first)
    }

    /// Whether the pack draws terrain itself, or has nothing draw it.
    pub fn has_terrain(&self) -> bool {
        self.terrain.is_some() || self.terrain_cutout.is_some() || self.terrain_undrawn
    }

    /// Whether the pack's shadow pass draws terrain (`shadow_view`).
    pub fn has_shadow(&self) -> bool {
        self.shadow_targets.is_some()
            && (self.shadow_terrain.is_some() || self.shadow_water.is_some())
    }

    /// Whether the pack draws the sky (disc, sunset, stars, void) itself.
    /// What the world's background is when the pack draws the sky: colortex0's clear colour
    /// (`None`: the fog colour, its default).
    pub fn sky_clear(&self) -> Option<[f32; 4]> {
        self.sky_clear.filter(|_| self.has_sky())
    }

    /// Whether the pack has a sky program, drawing or not: vanilla's sky is then not drawn.
    pub fn has_sky(&self) -> bool {
        self.hides_sky
    }

    /// Whether the pack has the sun and moon's program, drawing or not.
    pub fn has_celestial(&self) -> bool {
        self.hides_celestial
    }

    /// Whether the pack draws translucent terrain itself.
    pub fn has_water(&self) -> bool {
        self.water.is_some() || self.water_undrawn
    }

    /// The names of the programs that run, in order.
    pub fn programs(&self) -> Vec<&str> {
        self.passes.iter().map(|p| p.name.as_str()).collect()
    }

    /// Drops every pass's cached bind groups, compute stages included: they name views that
    /// were made again.
    fn forget_groups(&mut self) {
        let sky = self
            .sky
            .iter_mut()
            .flat_map(|s| s.basic.iter_mut().chain(&mut s.textured));
        let entities = self
            .entities
            .iter_mut()
            .flat_map(|e| e.passes.iter_mut().chain(&mut e.shadow).flatten());
        let passes = self
            .passes
            .iter_mut()
            .chain(&mut self.terrain)
            .chain(&mut self.terrain_cutout)
            .chain(&mut self.water)
            .chain(sky)
            .chain(entities)
            .chain(&mut self.clouds)
            .chain(&mut self.lines)
            .chain(self.weather.iter_mut().flatten())
            .chain(self.hand.iter_mut().map(|h| &mut h.0))
            .chain(&mut self.hand_water)
            .chain(&mut self.shadow_terrain)
            .chain(&mut self.shadow_water)
            .chain(&mut self.shadow_comp);
        for pass in passes {
            pass.groups.clear();
            for c in &mut pass.compute {
                c.groups.clear();
            }
        }
    }

    fn resize(&mut self, device: &wgpu::Device, size: [u32; 2]) {
        if pack_images::resize_images(device, &mut self.images, size) {
            self.forget_groups();
        }
        self.buffers.resize(device, size);
        if self.targets.as_ref().is_some_and(|t| t.size == size) {
            return;
        }
        self.setup_pending = true;
        self.rgb_unset = true;
        // Free the old targets before making the new ones: a pack with large buffers can't
        // hold both sets at once on a card it otherwise fits (a window resize ran out of video
        // memory). The bind groups hold the old views too, and the GPU may still be using
        // them, so wait for it; a resize is rare.
        if self.targets.take().is_some() {
            self.forget_groups();
            let _ = device.poll(wgpu::PollType::wait_indefinitely());
        }
        let sizes: [[u32; 2]; BUFFERS] =
            std::array::from_fn(|i| self.sizes[i].map_or(size, |s| s.resolve(size)));
        let full_chain = |size: [u32; 2]| 32 - size[0].max(size[1]).max(1).leading_zeros();
        let texture_sized = |label: &str, format, usage, levels, size: [u32; 2]| {
            device.create_texture(&wgpu::TextureDescriptor {
                label: Some(label),
                size: wgpu::Extent3d {
                    width: size[0],
                    height: size[1],
                    depth_or_array_layers: 1,
                },
                mip_level_count: levels,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage: usage | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            })
        };
        let texture = |label: &str, format, usage| texture_sized(label, format, usage, 1, size);
        let level = |t: &wgpu::Texture, level: u32| {
            t.create_view(&wgpu::TextureViewDescriptor {
                base_mip_level: level,
                mip_level_count: Some(1),
                ..Default::default()
            })
        };
        let mut mips: [Option<[Mips; 2]>; BUFFERS] = Default::default();
        let color = std::array::from_fn(|i| {
            (self.used & 1 << i != 0).then(|| {
                let label = format!("colortex{i}");
                let mipped = self.mipped & 1 << i != 0;
                let levels = if mipped { full_chain(sizes[i]) } else { 1 };
                let mut attachment = wgpu::TextureUsages::RENDER_ATTACHMENT;
                if self.storage & 1 << i != 0 {
                    attachment |= wgpu::TextureUsages::STORAGE_BINDING;
                    // Atomics on a texture need it made for them (Metal ignores them otherwise).
                    if device.features().contains(wgpu::Features::TEXTURE_ATOMIC)
                        && matches!(
                            self.formats[i],
                            wgpu::TextureFormat::R32Sint | wgpu::TextureFormat::R32Uint
                        )
                    {
                        attachment |= wgpu::TextureUsages::STORAGE_ATOMIC;
                    }
                }
                if self.copied & 1 << i != 0 {
                    attachment |= wgpu::TextureUsages::COPY_SRC;
                }
                // A buffer that isn't cleared starts each frame with B copied back to A.
                if i != 0 && self.clears[i].is_none() {
                    attachment |= wgpu::TextureUsages::COPY_SRC | wgpu::TextureUsages::COPY_DST;
                }
                let side = || texture_sized(&label, self.formats[i], attachment, levels, sizes[i]);
                let a = side();
                let sides = [
                    a.clone(),
                    if self.doubled & 1 << i != 0 {
                        side()
                    } else {
                        a
                    },
                ];
                if mipped && let Some((_, layout)) = self.down.get(&self.formats[i]) {
                    mips[i] = Some(sides.each_ref().map(|t| {
                        Mips {
                            all: t.create_view(&Default::default()),
                            levels: (1..levels)
                                .map(|l| {
                                    let group =
                                        device.create_bind_group(&wgpu::BindGroupDescriptor {
                                            label: Some(&label),
                                            layout,
                                            entries: &[
                                                wgpu::BindGroupEntry {
                                                    binding: 0,
                                                    resource: wgpu::BindingResource::TextureView(
                                                        &level(t, l - 1),
                                                    ),
                                                },
                                                wgpu::BindGroupEntry {
                                                    binding: 2,
                                                    resource: wgpu::BindingResource::Sampler(
                                                        &self.linear,
                                                    ),
                                                },
                                            ],
                                        });
                                    (level(t, l), group)
                                })
                                .collect(),
                        }
                    }));
                }
                sides.map(|t| level(&t, 0))
            })
        });
        let depth32 = wgpu::TextureFormat::Depth32Float;
        let depth_texture = texture(
            "depthtex0",
            depth32,
            wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        );
        let depth1 = self.depth1.then(|| {
            let t = texture("depthtex1", depth32, wgpu::TextureUsages::COPY_DST);
            let view = t.create_view(&Default::default());
            (t, view)
        });
        let depth2 = self.depth2.then(|| {
            let t = texture("depthtex2", depth32, wgpu::TextureUsages::COPY_DST);
            let view = t.create_view(&Default::default());
            (t, view)
        });
        let scratch = std::array::from_fn(|i| {
            (self.copied & 1 << i != 0).then(|| {
                // A copy also read as an image before a dispatch.
                let image = if self.storage & 1 << i != 0 {
                    wgpu::TextureUsages::STORAGE_BINDING
                } else {
                    wgpu::TextureUsages::empty()
                };
                texture_sized(
                    &format!("colortex{i} copy"),
                    self.formats[i],
                    wgpu::TextureUsages::COPY_DST | image,
                    1,
                    sizes[i],
                )
                .create_view(&Default::default())
            })
        });
        let sink = texture(
            "Shader pack sink",
            SINK,
            wgpu::TextureUsages::RENDER_ATTACHMENT,
        )
        .create_view(&Default::default());
        self.targets = Some(Targets {
            size,
            sink,
            sizes,
            scratch,
            color,
            depth: depth_texture.create_view(&Default::default()),
            depth_texture,
            depth1,
            depth2,
            mips,
        });
        self.forget_groups();
        for pass in self.passes.iter().filter(|p| self.sizes_differ(p)) {
            log::info!(
                "shader pack: {}: writes buffers of different sizes at {}x{} (size.buffer); skipped",
                pass.name,
                size[0],
                size[1]
            );
        }
    }

    /// The shadow pass (2.1, passes 2 and 3): the shadow buffers cleared, then the `shadowcomp`
    /// programs, each drawing into the "alt" side of what it writes and flipping it after.
    fn run_shadow(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        terrain: &mut TerrainDraw<'_>,
        frame: &super::Frame,
    ) {
        // Opt-in only (`horizon-graphics.txt` shadowCache). Off: always redraw, drop any cache.
        if !frame.shadow_cache {
            self.shadow_cache = ShadowCache::default();
            self.draw_shadow_pass(device, queue, encoder, terrain);
            return;
        }

        // Compute the current frame's cache key before borrowing shadow_targets mutably.
        let cache_key = self.compute_shadow_cache_key(frame);

        // Entity vertices are interpolated each frame, so their shadows cannot be reused.
        let has_shadow_entities = self
            .entities
            .as_ref()
            .is_some_and(|entities| entities.geometry.draws.iter().any(|draw| draw.shadow));
        // Custom images are cleared each frame (`pack_images::clear`) before the
        // shadow pass writes them, so a pack with any redraws it too.
        let cache_hit = !has_shadow_entities
            && self.images.is_empty()
            && self.shadow_cache.key == Some(cache_key)
            && self.shadow_targets.is_some();
        if cache_hit {
            // The targets still hold this map (and `main`, the A/B side
            // `shadowcomp` left current).
            return;
        }
        self.draw_shadow_pass(device, queue, encoder, terrain);
        self.shadow_cache.key = self.shadow_targets.is_some().then_some(cache_key);
    }

    /// The shadow pass body: terrain, entities, clears, and `shadowcomp`.
    fn draw_shadow_pass(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        terrain: &mut TerrainDraw<'_>,
    ) {
        let Some(t) = &mut self.shadow_targets else {
            return;
        };
        t.begin_frame();
        let size = [t.size; 2];
        self.draw_terrain(
            device,
            queue,
            encoder,
            size,
            TerrainList::ShadowSolid,
            terrain,
        );
        self.draw_shadow_entities(device, queue, encoder, size);
        let t = self.shadow_targets.as_mut().expect("shadow pass");
        t.clear_rest(encoder);
        t.copy_depth1(encoder);
        self.draw_terrain(
            device,
            queue,
            encoder,
            size,
            TerrainList::ShadowTranslucent,
            terrain,
        );
        let mut comp = std::mem::take(&mut self.shadow_comp);
        for pass in &mut comp {
            let mut compute = std::mem::take(&mut pass.compute);
            for c in &mut compute {
                self.dispatch(device, queue, encoder, c, size);
            }
            pass.compute = compute;
            if matches!(pass.pipeline, Pipeline::Compute { .. }) {
                self.dispatch(device, queue, encoder, pass, size);
                continue;
            }
            let group = self.bind(device, queue, encoder, pass, size);
            let t = self.shadow_targets.as_ref().expect("shadow pass");
            let mut attachments: Vec<Option<wgpu::RenderPassColorAttachment>> = Vec::new();
            for &(location, buffer) in &pass.writes {
                let l = location as usize;
                if attachments.len() <= l {
                    attachments.resize(l + 1, None);
                }
                // A shadowcomp shader can leave part of its destination untouched.
                let ops = wgpu::Operations {
                    load: wgpu::LoadOp::Load,
                    store: wgpu::StoreOp::Store,
                };
                attachments[l] = t.color(buffer, true).and_then(|v| attach(v, ops));
            }
            {
                let mut rp = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some(&pass.name),
                    color_attachments: &attachments,
                    timestamp_writes: self.timing.as_mut().and_then(|t| t.writes(&pass.name)),
                    ..Default::default()
                });
                rp.set_pipeline(pass.render());
                rp.set_bind_group(0, &group, &[]);
                if pass.buffers
                    && let Some(g) = &self.buffers.group
                {
                    rp.set_bind_group(4, g, &[]);
                }
                rp.draw(0..6, 0..1);
            }
            if let Some(t) = &mut self.shadow_targets {
                t.main ^= pass.flip;
            }
        }
        self.shadow_comp = comp;
    }

    /// Computes the cache key for the current frame.
    fn compute_shadow_cache_key(&self, frame: &super::Frame) -> ShadowCacheKey {
        let shadow = &self.uniforms.shadow;
        // Use the same logic as shadow_camera to compute the light direction and snapped camera.
        // Use the public shadow_light function which returns the normalized light direction.
        let light = super::pack_uniforms::shadow_light(frame, shadow);
        let light = light.as_dvec3();
        let up = if light.z.abs() > 0.99 {
            DVec3::Y
        } else {
            DVec3::Z
        };
        let rotation = DMat4::look_to_rh(DVec3::ZERO, -light, up);
        let at = rotation.transform_vector3(frame.camera);
        let step = f64::from(shadow.interval);
        let snapped_camera = if step > 0.0 {
            Some((at / step).floor() * step)
        } else {
            Some(at)
        };

        // Hash the shadow settings that affect the shadow map.
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        shadow.resolution.hash(&mut hasher);
        shadow.distance.to_bits().hash(&mut hasher);
        shadow.render_mul.to_bits().hash(&mut hasher);
        shadow.interval.to_bits().hash(&mut hasher);
        shadow.sun_path_rotation.to_bits().hash(&mut hasher);
        shadow.fov.map(|f| f.to_bits()).hash(&mut hasher);
        shadow.near.map(f32::to_bits).hash(&mut hasher);
        shadow.far.map(f32::to_bits).hash(&mut hasher);
        for hf in &shadow.hardware_filtering {
            hf.hash(&mut hasher);
        }
        for color in &shadow.colors {
            format!("{:?}", color.format).hash(&mut hasher);
            color.clear.hash(&mut hasher);
            if let Some(cc) = color.clear_color {
                for c in cc {
                    c.to_bits().hash(&mut hasher);
                }
            }
            color.mipmap.hash(&mut hasher);
        }
        let shadow_settings_hash = hasher.finish();
        let shadow_uniforms_hash = self.shadow_uniforms_hash();

        ShadowCacheKey {
            sun_angle: frame.sky.sun_angle,
            moon_angle: frame.sky.moon_angle,
            snapped_camera,
            shadow_settings_hash,
            shadow_scene_revision: frame.shadow_scene_revision,
            ticks: frame.ticks,
            day_time: frame.pack.day_time,
            shadow_uniforms_hash,
        }
    }

    fn shadow_uniforms_hash(&self) -> u64 {
        use std::hash::{Hash, Hasher};

        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        let passes = self
            .shadow_terrain
            .iter()
            .chain(self.shadow_water.iter())
            .chain(self.shadow_comp.iter())
            .chain(
                self.entities
                    .iter()
                    .flat_map(|entities| entities.shadow.iter().flatten()),
            );
        for pass in passes {
            pass.name.hash(&mut hasher);
            for (index, ty, offset) in &pass.uniforms {
                index.hash(&mut hasher);
                ty.hash(&mut hasher);
                offset.hash(&mut hasher);
                let Some(value) = index.and_then(|index| self.uniforms.values.get(index)) else {
                    continue;
                };
                match value {
                    pack_uniforms::Value::F(values) => {
                        0u8.hash(&mut hasher);
                        for value in values {
                            value.to_bits().hash(&mut hasher);
                        }
                    }
                    pack_uniforms::Value::I(values) => {
                        1u8.hash(&mut hasher);
                        values.hash(&mut hasher);
                    }
                }
            }
        }
        hasher.finish()
    }

    /// Runs the pack over the world drawn into `world` (colour) and `world_depth`, then draws
    /// the result onto `screen`.
    #[allow(clippy::too_many_arguments)]
    pub fn run(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        world: &wgpu::TextureView,
        world_depth: &wgpu::TextureView,
        screen: &wgpu::TextureView,
        frame: &super::Frame,
        inputs: &PackInputs,
        size: [u32; 2],
        window: [u32; 2],
        pages: &[(u32, wgpu::Buffer)],
        terrain: &mut TerrainDraw<'_>,
    ) {
        self.resize(device, size);
        // A resolution scale leaves the picture smaller (or larger) than the window: the pack's
        // `final` draws it at the picture's size, and one copy takes it to the window (#9291).
        let final_view = if size != window && self.passes.iter().any(|p| p.group == Group::Final) {
            if self.final_target.as_ref().is_none_or(|t| t.size != size) {
                let texture = device.create_texture(&wgpu::TextureDescriptor {
                    label: Some("Shader pack final"),
                    size: wgpu::Extent3d {
                        width: size[0],
                        height: size[1],
                        depth_or_array_layers: 1,
                    },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: self.screen,
                    usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                        | wgpu::TextureUsages::TEXTURE_BINDING,
                    view_formats: &[],
                });
                self.final_target = Some(FinalTarget {
                    size,
                    view: texture.create_view(&Default::default()),
                });
            }
            self.final_target.as_ref().map(|t| t.view.clone())
        } else {
            self.final_target = None;
            None
        };
        if let Some((layout, ids, rects)) = &self.block_ids {
            // A page that grew has a new buffer.
            self.page_groups
                .retain(|p, (b, _)| pages.iter().any(|(q, c)| q == p && c == b));
            for (page, buffer) in pages {
                self.page_groups.entry(*page).or_insert_with(|| {
                    let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                        label: Some("Shader pack terrain"),
                        layout,
                        entries: &[
                            wgpu::BindGroupEntry {
                                binding: 0,
                                resource: ids.as_entire_binding(),
                            },
                            wgpu::BindGroupEntry {
                                binding: 1,
                                resource: buffer.as_entire_binding(),
                            },
                            wgpu::BindGroupEntry {
                                binding: 2,
                                resource: rects.as_entire_binding(),
                            },
                        ],
                    });
                    (buffer.clone(), group)
                });
            }
        }
        if let Some(t) = &mut self.timing {
            t.begin(device, queue);
        }
        self.uniforms.center_depth_raw = self
            .center_depth
            .get_or_insert_with(|| super::pack_center_depth::CenterDepth::new(device))
            .collect(device);
        self.uniforms.update(frame, inputs, size);
        // `smooth` starts on the first frame that knows the camera's biome.
        self.custom.world_frame(inputs.biome_known);
        self.uniforms.eval_custom(&mut self.custom);
        if self.targets.is_none() {
            return;
        }
        // Images start the frame cleared, before the shadow pass writes them.
        if let Some(dump) = &mut self.dump {
            dump.read(device);
            dump.copy(device, encoder, &self.images);
        }
        pack_images::clear(
            device,
            encoder,
            &self.images,
            self.gpu.clear,
            &mut self.zeros,
        );
        let Some(targets) = &self.targets else { return };
        // Every frame starts on A. A buffer that isn't cleared and ended the last frame on B has B
        // copied back to A, and B keeps what was last written into it: a model that fits Iris 1.11.7's
        // values in probe packs of our own (#9009), not a documented Iris rule. colortex0's A is
        // overwritten by the world below.
        for b in (1..BUFFERS).filter(|&b| self.main & 1 << b != 0 && self.clears[b].is_none()) {
            if let Some([a, b_side]) = &targets.color[b] {
                encoder.copy_texture_to_texture(
                    b_side.texture().as_image_copy(),
                    a.texture().as_image_copy(),
                    a.texture().size(),
                );
            }
        }
        self.main = 0;
        let main = |m: u16, i: usize| usize::from(m & 1 << i != 0);
        let view = |m: u16, i: usize, alt: bool| {
            targets.color[i]
                .as_ref()
                .map(|t| &t[main(m, i) ^ usize::from(alt)])
        };

        if !self.shadow_cleared {
            self.shadow_cleared = true;
            encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Shader pack shadow stand-in"),
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &self.shadow.0,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                ..Default::default()
            });
        }
        // Three-channel buffers start out with alpha 1 on both sides: a pass writes the side the
        // frame's clear didn't touch.
        if self.rgb_unset {
            self.rgb_unset = false;
            let start = wgpu::Operations {
                load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                store: wgpu::StoreOp::Store,
            };
            for b in (0..BUFFERS).filter(|&b| self.rgb & 1 << b != 0) {
                for alt in [false, true]
                    .into_iter()
                    .take(1 + usize::from(self.doubled & 1 << b != 0))
                {
                    if let Some(v) = view(self.main, b, alt) {
                        encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                            label: Some("Shader pack three-channel start"),
                            color_attachments: &[attach(v, start)],
                            ..Default::default()
                        });
                    }
                }
            }
        }
        let clear_op = |b: usize| {
            let c = self.clears[b].unwrap_or_default();
            wgpu::Operations {
                load: wgpu::LoadOp::Clear(wgpu::Color {
                    r: c[0].into(),
                    g: c[1].into(),
                    b: c[2].into(),
                    a: c[3].into(),
                }),
                store: wgpu::StoreOp::Store,
            }
        };
        // Overwritten whole: clearing lets a tiled GPU skip loading the old content.
        let overwrite = wgpu::Operations {
            load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
            store: wgpu::StoreOp::Store,
        };
        // Attachments share a size and stay within the device's count and byte limits.
        let limits = device.limits();
        let max_attachments = (limits.max_color_attachments as usize).min(8);
        // The buffers the blit doesn't clear, and the other texture of every cleared buffer.
        let alt_clears = (0..BUFFERS)
            .filter(|&b| self.clears[b].is_some() && self.doubled & 1 << b != 0)
            .filter(|&b| view(self.main, b, true).is_some())
            .map(|b| (b, true));
        let mut by_size: Vec<([u32; 2], Vec<(usize, bool)>)> = Vec::new();
        for (b, alt) in self
            .other_clears
            .iter()
            .map(|&b| (b, false))
            .chain(alt_clears)
        {
            let at = targets.sizes[b];
            match by_size.iter_mut().find(|(s, buffers)| {
                let bytes = buffers
                    .iter()
                    .fold(0, |bytes, &(b, _)| attachment_bytes(bytes, self.formats[b]));
                *s == at
                    && buffers.len() < max_attachments
                    && attachment_bytes(bytes, self.formats[b])
                        <= limits.max_color_attachment_bytes_per_sample
            }) {
                Some((_, l)) => l.push((b, alt)),
                None => by_size.push((at, vec![(b, alt)])),
            }
        }
        for (_, chunk) in &by_size {
            let attachments: Vec<_> = chunk
                .iter()
                .map(|&(b, alt)| attach(view(self.main, b, alt).expect("used buffer"), clear_op(b)))
                .collect();
            encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Shader pack clear"),
                color_attachments: &attachments,
                timestamp_writes: self.timing.as_mut().and_then(|t| t.writes("clear")),
                ..Default::default()
            });
        }

        // The world into colortex0 and depthtex0, flipped to GL's rows, and the clears.
        if let Some(blit) = &self.blit
            && self
                .blit_group
                .as_ref()
                .is_none_or(|g| g.0[0] != *world || g.0[1] != *world_depth)
        {
            let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("Shader pack world"),
                layout: &blit.1,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(world),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::TextureView(world_depth),
                    },
                ],
            });
            self.blit_group = Some(([world.clone(), world_depth.clone()], group));
        }
        if let Some(blit) = &self.blit {
            // The blit leaves a three-channel colortex0's alpha at the 1 it starts with.
            let world_ops = if self.rgb & 1 != 0 {
                wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                    store: wgpu::StoreOp::Store,
                }
            } else {
                overwrite
            };
            let mut attachments = vec![if holds_floats(self.formats[0]) {
                attach(view(self.main, 0, false).expect("colortex0"), world_ops)
            } else {
                None
            }];
            for &b in &self.blit_clears {
                attachments.push(attach(
                    view(self.main, b, false).expect("used buffer"),
                    clear_op(b),
                ));
            }
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Shader pack world"),
                color_attachments: &attachments,
                timestamp_writes: self.timing.as_mut().and_then(|t| t.writes("world")),
                depth_stencil_attachment: self.blit_depth.then_some(
                    wgpu::RenderPassDepthStencilAttachment {
                        view: &targets.depth,
                        depth_ops: Some(wgpu::Operations {
                            load: wgpu::LoadOp::Clear(1.0),
                            store: wgpu::StoreOp::Store,
                        }),
                        stencil_ops: None,
                    },
                ),
                ..Default::default()
            });
            pass.set_pipeline(&blit.0);
            pass.set_bind_group(0, self.blit_group.as_ref().map(|g| &g.1), &[]);
            pass.draw(0..3, 0..1);
        }

        // The world and the clears just rewrote the buffers' first levels.
        self.mips_valid = 0;
        let mut passes = std::mem::take(&mut self.passes);
        let mut seen = (false, false);
        let mut shown = false;
        // A water-only pack still needs the opaque phase to populate requested snapshots.
        let mut terrain_drawn = self.terrain.is_none()
            && self.terrain_cutout.is_none()
            && self.sky.is_none()
            && !self.entities.as_ref().is_some_and(Entities::drawn)
            && self.clouds.is_none()
            && self.lines.is_none()
            && self.hand.is_none()
            && !self.depth1
            && !self.depth2;
        let late_entities = self.entities.as_ref().is_some_and(|e| {
            e.passes
                .iter()
                .zip(e.late)
                .any(|(p, late)| p.is_some() && late)
        });
        let mut water_drawn = self.water.is_none() && self.weather.is_none() && !late_entities;
        let mut shadow_drawn = false;
        for pass in &mut passes {
            if pass.group == Group::Setup && !self.setup_pending {
                continue;
            }
            let early = matches!(pass.group, Group::Setup | Group::Begin);
            if !early && !shadow_drawn {
                self.run_shadow(device, queue, encoder, terrain, frame);
                shadow_drawn = true;
            }
            if !terrain_drawn && !early && pass.group != Group::Prepare {
                terrain_drawn = true;
                self.draw_opaque(device, queue, encoder, size, frame, terrain);
            }
            // As if the pack didn't have it: no compute stages, flips or stage changes.
            if self.sizes_differ(pass) {
                continue;
            }
            if !water_drawn && matches!(pass.group, Group::Composite | Group::Final) {
                water_drawn = true;
                self.draw_translucent(device, queue, encoder, size, terrain);
            }
            if pass.group == Group::Deferred && !seen.0 {
                seen.0 = true;
                self.main ^= self.deferred_pre;
            }
            if pass.group == Group::Composite && !seen.1 {
                seen.1 = true;
                self.main ^= self.composite_pre;
            }
            let mut compute = std::mem::take(&mut pass.compute);
            for c in &mut compute {
                self.dispatch(device, queue, encoder, c, size);
            }
            pass.compute = compute;
            if matches!(pass.pipeline, Pipeline::Compute { .. }) {
                self.dispatch(device, queue, encoder, pass, size);
                continue;
            }
            let stale = pass.mipmaps & self.mipped & !self.mips_valid;
            for b in (0..BUFFERS).filter(|b| stale & 1 << b != 0) {
                self.mipmap(encoder, b);
            }
            self.mips_valid |= stale;
            let final_pass = pass.group == Group::Final;
            // A pass writing buffers of a size of their own draws at that size (they share it).
            // `final` draws the picture at the world's size; a copy takes it to the window.
            let out = match pass.writes.first() {
                Some(&(_, b)) => self.targets.as_ref().map_or(size, |t| t.sizes[b]),
                None => size,
            };
            let group = self.bind(device, queue, encoder, pass, out);
            let targets = self.targets.as_ref().expect("resized");
            let view = |i: usize| {
                targets.color[i]
                    .as_ref()
                    .map(|t| &t[usize::from(self.main & 1 << i == 0)])
            };
            let mut attachments: Vec<Option<wgpu::RenderPassColorAttachment>> = Vec::new();
            // Vertex coverage and fragment discard can leave alternate pixels untouched.
            // The unused sink can still skip loading its old contents.
            let ops = if !pass.sink || pass.viewport.is_some() {
                wgpu::Operations {
                    load: wgpu::LoadOp::Load,
                    store: wgpu::StoreOp::Store,
                }
            } else {
                overwrite
            };
            if final_pass {
                attachments.push(attach(final_view.as_ref().unwrap_or(screen), overwrite));
                shown = true;
            } else if pass.sink {
                attachments.push(attach(&targets.sink, ops));
            } else {
                for &(location, buffer) in &pass.writes {
                    let l = location as usize;
                    if attachments.len() <= l {
                        attachments.resize(l + 1, None);
                    }
                    attachments[l] = view(buffer).and_then(|v| attach(v, ops));
                }
            }
            {
                let mut rp = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some(&pass.name),
                    color_attachments: &attachments,
                    timestamp_writes: self.timing.as_mut().and_then(|t| t.writes(&pass.name)),
                    ..Default::default()
                });
                rp.set_pipeline(pass.render());
                rp.set_bind_group(0, &group, &[]);
                if pass.buffers
                    && let Some(g) = &self.buffers.group
                {
                    rp.set_bind_group(4, g, &[]);
                }
                if let Some(scale) = pass.viewport {
                    let [x, y, w, h] = viewport(out, scale);
                    rp.set_viewport(x, y, w, h, 0.0, 1.0);
                }
                rp.draw(0..6, 0..1);
            }
            self.main ^= pass.flip;
            self.mips_valid &= !pass.written;
        }
        self.passes = passes;
        self.setup_pending = false;
        if !shadow_drawn {
            self.run_shadow(device, queue, encoder, terrain, frame);
        }
        if !terrain_drawn {
            self.draw_opaque(device, queue, encoder, size, frame, terrain);
        }
        if !water_drawn {
            self.draw_translucent(device, queue, encoder, size, terrain);
        }
        let targets = self.targets.as_ref().expect("resized");
        // depthtex0 as the frame leaves it, for next frames' `centerDepthSmooth`.
        if let Some(center) = &mut self.center_depth {
            center.copy(device, encoder, &targets.depth_texture);
        }
        let view = |m: u16, i: usize, alt: bool| {
            targets.color[i]
                .as_ref()
                .map(|t| &t[main(m, i) ^ usize::from(alt)])
        };
        // Without `final`, colortex0 holds floats (`new` refuses an integer one).
        if !shown {
            let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("Shader pack colortex0"),
                layout: &self.copy.1,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(
                            view(self.main, 0, false).expect("colortex0"),
                        ),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: wgpu::BindingResource::Sampler(&self.linear),
                    },
                ],
            });
            let mut rp = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Shader pack colortex0"),
                color_attachments: &[attach(screen, overwrite)],
                ..Default::default()
            });
            rp.set_pipeline(&self.copy.0);
            rp.set_bind_group(0, &group, &[]);
            rp.draw(0..3, 0..1);
        } else if let Some(picture) = &final_view {
            // The picture at the world's size onto the window: one upscale, smoothly.
            let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("Shader pack final"),
                layout: &self.upscale.1,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(picture),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: wgpu::BindingResource::Sampler(&self.linear),
                    },
                ],
            });
            let mut rp = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Shader pack final"),
                color_attachments: &[attach(screen, overwrite)],
                timestamp_writes: self.timing.as_mut().and_then(|t| t.writes("final copy")),
                ..Default::default()
            });
            rp.set_pipeline(&self.upscale.0);
            rp.set_bind_group(0, &group, &[]);
            rp.draw(0..3, 0..1);
        }
    }
}

#[cfg(test)]
mod snorm_tests;

#[cfg(test)]
#[path = "pack_post/depth_test.rs"]
mod depth_test;

#[cfg(test)]
#[path = "pack_post/terrain_parts_tests.rs"]
mod terrain_parts_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use wgpu::TextureFormat as T;

    #[test]
    fn shadow_cache_key_tracks_world_and_time_changes() {
        let key = ShadowCacheKey {
            sun_angle: 0.0,
            moon_angle: 180.0,
            snapped_camera: Some(DVec3::ZERO),
            shadow_settings_hash: 1,
            shadow_scene_revision: (2, 3),
            ticks: 4,
            day_time: 5,
            shadow_uniforms_hash: 6,
        };

        assert_ne!(
            key,
            ShadowCacheKey {
                shadow_scene_revision: (8, 3),
                ..key
            }
        );
        assert_ne!(key, ShadowCacheKey { day_time: 9, ..key });
        assert_ne!(
            key,
            ShadowCacheKey {
                shadow_uniforms_hash: 10,
                ..key
            }
        );
    }

    /// Two textures with a sampler each become two textures sharing one sampler variable.
    #[test]
    fn samplers_of_a_kind_share_one_variable() {
        let mut module = naga::front::wgsl::parse_str(
            "@group(0) @binding(1) var a: texture_2d<f32>;\n\
             @group(0) @binding(2) var sa: sampler;\n\
             @group(0) @binding(3) var b: texture_2d<f32>;\n\
             @group(0) @binding(4) var sb: sampler;\n\
             @fragment fn main() -> @location(0) vec4<f32> {\n\
             return textureSample(a, sa, vec2(0.5)) + textureSample(b, sb, vec2(0.5));\n}\n",
        )
        .unwrap();
        share_samplers(&mut module, &HashMap::from([(2, 2), (4, 2)]));
        let info = naga::valid::Validator::new(Default::default(), Default::default())
            .validate(&module)
            .unwrap();
        let used: Vec<u32> = module
            .global_variables
            .iter()
            .filter(|(h, _)| !info.get_entry_point(0)[*h].is_empty())
            .filter_map(|(_, g)| g.binding.as_ref().map(|b| b.binding))
            .collect();
        assert_eq!(used, [1, 2, 3]);
    }

    /// #3525: naga's SPIR-V writer panicked on Photon's gbuffers programs as the pipeline was
    /// made. Such a panic skips the pass, as a validation error does, and the device goes on.
    #[test]
    fn a_shader_compiler_panic_skips_the_pass() {
        let instance = crate::gfx::test_gpu::instance();
        let Ok(adapter) = pollster::block_on(instance.request_adapter(&Default::default())) else {
            return;
        };
        let Ok((device, _queue)) = pollster::block_on(adapter.request_device(&Default::default()))
        else {
            return;
        };
        let failed = checked(&device, || -> wgpu::Buffer {
            panic!("internal error: entered unreachable code: Expression [89] is not cached!")
        });
        let error = failed.expect_err("the panic comes back as an error");
        assert!(error.contains("Expression [89] is not cached!"), "{error}");
        let buffer = |usage| {
            checked(&device, || {
                device.create_buffer(&wgpu::BufferDescriptor {
                    label: None,
                    size: 16,
                    usage,
                    mapped_at_creation: false,
                })
            })
        };
        // The device still makes things, and its errors still come back.
        assert!(buffer(wgpu::BufferUsages::UNIFORM).is_ok());
        assert!(buffer(wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::MAP_WRITE).is_err());
    }

    /// `texture.composite.colortex0` stands in for buffer 0 in composite programs until one of
    /// them flips that buffer; later ones, and `final`, read the buffer (Super Duper Vanilla,
    /// Bliss, SEUS PTGI HRR). Another stage's flips don't release it.
    #[test]
    fn a_custom_texture_over_a_buffer_ends_when_its_stage_flips_it() {
        let instance = crate::gfx::test_gpu::instance();
        let Ok(adapter) = pollster::block_on(instance.request_adapter(&Default::default())) else {
            return;
        };
        let Ok((device, _queue)) = pollster::block_on(adapter.request_device(&Default::default()))
        else {
            return;
        };
        let textures = [pack_textures::PackTexture {
            stage: "composite".into(),
            name: "colortex0".into(),
            dimension: wgpu::TextureViewDimension::D2,
            sample_type: wgpu::TextureSampleType::Float { filterable: true },
            blur: false,
            clamp: false,
            png: false,
            image: pack_textures::Image::Atlas,
        }];
        let interface = horizon_shaders::Interface {
            uniform_size: 48,
            initial_values: vec![0; 48],
            uniforms: Vec::new(),
            samplers: vec![horizon_shaders::Sampler {
                name: "colortex0".into(),
                ty: "sampler2D".into(),
                binding: 1,
            }],
            outputs: vec![0],
            draw: false,
            images: Vec::new(),
            buffers: Vec::new(),
        };
        let buffers = pack_images::Buffers::new(
            &device,
            Default::default(),
            Default::default(),
            wgpu::ShaderStages::FRAGMENT,
        );
        let stand_ins = std::cell::RefCell::new(Vec::new());
        let pack = Resources {
            textures: &textures,
            images: &[],
            left_out: &[],
            buffers: &buffers,
            indices: None,
            gpu: Gpu::of(&adapter, device.features()),
            storable: 0,
            rgb: 0,
            packed: [None; BUFFERS],
            released: std::cell::Cell::new(("", 0)),
            stand_ins: &stand_ins,
            shadow_linear: 0,
        };
        let reads = |group: Group| {
            let (_, samplers, ..) = bindings(
                device.features(),
                &interface,
                "composite",
                group,
                &[1],
                None,
                &[T::Rgba8Unorm; BUFFERS],
                None,
                &mut Vec::new(),
                &pack,
            )
            .expect("the pass is kept");
            samplers[0].source
        };
        assert!(reads(Group::Composite) == Source::Atlas);
        pack.released.set(("deferred", 1));
        assert!(reads(Group::Composite) == Source::Atlas);
        pack.released.set(("composite", 1));
        assert!(reads(Group::Composite) == Source::Color(0));
        assert!(reads(Group::Final) == Source::Color(0));
    }

    /// Iris 1.11.7 (a probe pack of ours, #9064) didn't bind a PNG under
    /// `texture.<stage>.<name>` when `name` was a name of the pack's own, no sampler of the
    /// game's; a raw texture there, a PNG over `gaux2` or `noisetex`, and a `customTexture` PNG
    /// were bound.
    #[test]
    fn a_png_under_a_name_of_its_own_is_not_bound() {
        let instance = crate::gfx::test_gpu::instance();
        let Ok(adapter) = pollster::block_on(instance.request_adapter(&Default::default())) else {
            return;
        };
        let Ok((device, _queue)) = pollster::block_on(adapter.request_device(&Default::default()))
        else {
            return;
        };
        let texture = |stage: &str, name: &str, png: bool| pack_textures::PackTexture {
            stage: stage.into(),
            name: name.into(),
            dimension: wgpu::TextureViewDimension::D2,
            sample_type: wgpu::TextureSampleType::Float { filterable: true },
            blur: !png,
            clamp: !png,
            png,
            image: pack_textures::Image::Atlas,
        };
        let textures = [
            texture("composite", "tp", true),
            texture("composite", "tr", false),
            texture("composite", "gaux2", true),
            texture("composite", "noisetex", true),
            texture(horizon_shaders::EVERY_STAGE, "cp", true),
        ];
        let names = ["tp", "tr", "gaux2", "noisetex", "cp"];
        let interface = horizon_shaders::Interface {
            uniform_size: 48,
            uniforms: Vec::new(),
            samplers: names
                .iter()
                .zip(1..)
                .map(|(name, k)| horizon_shaders::Sampler {
                    name: (*name).into(),
                    ty: "sampler2D".into(),
                    binding: 2 * k - 1,
                })
                .collect(),
            outputs: vec![0],
            draw: false,
            images: Vec::new(),
            buffers: Vec::new(),
            initial_values: vec![0; 48],
        };
        let buffers = pack_images::Buffers::new(
            &device,
            Default::default(),
            Default::default(),
            wgpu::ShaderStages::FRAGMENT,
        );
        let stand_ins = std::cell::RefCell::new(Vec::new());
        let pack = Resources {
            textures: &textures,
            images: &[],
            left_out: &[],
            buffers: &buffers,
            indices: None,
            gpu: Gpu::of(&adapter, device.features()),
            storable: 0,
            rgb: 0,
            released: std::cell::Cell::new(("", 0)),
            stand_ins: &stand_ins,
            shadow_linear: 0,
            packed: [None; BUFFERS],
        };
        let (_, samplers, ..) = bindings(
            device.features(),
            &interface,
            "final",
            Group::Final,
            &[0],
            None,
            &[T::Rgba8Unorm; BUFFERS],
            None,
            &mut Vec::new(),
            &pack,
        )
        .expect("the pass is kept");
        let read: Vec<Source> = samplers.iter().map(|b| b.source).collect();
        assert!(
            read == [
                Source::Dummy,
                Source::Atlas,
                Source::Atlas,
                Source::Atlas,
                Source::Atlas
            ]
        );
    }

    /// Iris 1.11.7 runs a program whose image no `image.*` gives, and its loads read zero
    /// (observed with a probe pack). Its pass is kept, the image bound to a stand-in of its
    /// declared format, zero again before each pass even after one stored into it.
    #[test]
    fn an_image_without_an_entry_reads_zero() {
        use wgpu::TextureViewDimension as D;
        let instance = crate::gfx::test_gpu::instance();
        let Ok(adapter) = pollster::block_on(instance.request_adapter(&Default::default())) else {
            return;
        };
        let Ok((device, queue)) = pollster::block_on(adapter.request_device(&Default::default()))
        else {
            return;
        };
        let image = |name: &str, ty: &str, format: &str, binding| horizon_shaders::Image {
            name: name.into(),
            ty: ty.into(),
            format: format.into(),
            access: String::new(),
            binding,
            before: None,
        };
        let interface = horizon_shaders::Interface {
            uniform_size: 48,
            initial_values: vec![0; 48],
            uniforms: Vec::new(),
            samplers: Vec::new(),
            outputs: vec![0],
            draw: false,
            images: vec![
                image("mask", "uimage3D", "r32ui", 1),
                image("other", "uimage3D", "r32ui", 2),
                image("flat", "image2D", "rgba8", 3),
            ],
            buffers: Vec::new(),
        };
        let buffers = pack_images::Buffers::new(
            &device,
            Default::default(),
            Default::default(),
            wgpu::ShaderStages::FRAGMENT,
        );
        let stand_ins = std::cell::RefCell::new(Vec::new());
        let pack = Resources {
            textures: &[],
            images: &[],
            left_out: &[],
            buffers: &buffers,
            indices: None,
            gpu: Gpu::of(&adapter, device.features()),
            storable: 0,
            rgb: 0,
            packed: [None; BUFFERS],
            released: std::cell::Cell::new(("", 0)),
            stand_ins: &stand_ins,
            shadow_linear: 0,
        };
        let mut notes = Vec::new();
        let (.., images) = bindings(
            device.features(),
            &interface,
            "composite",
            Group::Composite,
            &[0],
            None,
            &[T::Rgba8Unorm; BUFFERS],
            None,
            &mut notes,
            &pack,
        )
        .expect("the pass is kept");
        assert!(
            images
                == [
                    (1, Image::StandIn(T::R32Uint, D::D3)),
                    (2, Image::StandIn(T::R32Uint, D::D3)),
                    (3, Image::StandIn(T::Rgba8Unorm, D::D2)),
                ]
        );
        assert_eq!(notes.len(), 3, "{notes:?}");
        // One stand-in for each format and dimension.
        let made = pack_images::stand_ins(&device, stand_ins.take());
        assert_eq!(made.len(), 2);
        let mask = pack_images::stand_in(&made, T::R32Uint, D::D3).expect("made");
        let texture = mask.view.texture();
        queue.write_texture(
            texture.as_image_copy(),
            &7u32.to_le_bytes(),
            wgpu::TexelCopyBufferLayout::default(),
            texture.size(),
        );
        let read = device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: 4,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = device.create_command_encoder(&Default::default());
        mask.clear(&mut encoder);
        encoder.copy_texture_to_buffer(
            texture.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &read,
                layout: wgpu::TexelCopyBufferLayout::default(),
            },
            texture.size(),
        );
        queue.submit([encoder.finish()]);
        read.slice(..).map_async(wgpu::MapMode::Read, |_| {});
        device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
        assert_eq!(&read.slice(..).get_mapped_range().unwrap()[..], &[0; 4]);
    }

    /// `scale.composite2 = 0.25 0.5 0.25` on a 1280×720 picture: Iris 1.11.7 draws x 640..960
    /// and GL y 180..360 (a probe pack of our own). Rows are GL's here, so that is rows 180..360.
    #[test]
    fn a_scaled_pass_draws_its_part_from_gl_s_corner() {
        assert_eq!(
            viewport([1280, 720], [0.25, 0.5, 0.25]),
            [640.0, 180.0, 320.0, 180.0]
        );
        assert_eq!(
            viewport([1280, 720], [0.5, 0.0, 0.0]),
            [0.0, 0.0, 640.0, 360.0]
        );
        // Whole pixels, at least one, and never past the buffer's edge.
        assert_eq!(viewport([5, 3], [0.5, 0.0, 0.0]), [0.0, 0.0, 2.0, 1.0]);
        assert_eq!(viewport([4, 4], [0.01, 0.0, 0.0]), [0.0, 0.0, 1.0, 1.0]);
        assert_eq!(
            viewport([100, 100], [1.0, 0.5, 0.75]),
            [50.0, 75.0, 50.0, 25.0]
        );
    }

    /// `blend.<program>` of a composite program blends its output onto what the buffer's side
    /// holds (Iris 1.11.7, probed: 0.25 over 0.75 with SRC_ALPHA ONE_MINUS_SRC_ALPHA ONE ZERO and
    /// a source alpha of 0.25 reads 0.625).
    #[test]
    fn a_composite_program_blends_onto_its_buffer() {
        let instance = crate::gfx::test_gpu::instance();
        let Ok(adapter) = pollster::block_on(instance.request_adapter(&Default::default())) else {
            return;
        };
        // A pass's pipeline layout has six bind group slots.
        let desc = wgpu::DeviceDescriptor {
            required_limits: adapter.limits(),
            ..Default::default()
        };
        let Ok((device, queue)) = pollster::block_on(adapter.request_device(&desc)) else {
            return;
        };
        let dir = std::env::temp_dir().join(format!("hz-composite-blend-{}", std::process::id()));
        let shaders = dir.join("shaders");
        std::fs::create_dir_all(&shaders).unwrap();
        let file = |name: &str, text: &str| std::fs::write(shaders.join(name), text).unwrap();
        file(
            "composite.vsh",
            "#version 120\nvoid main() { gl_Position = ftransform(); }\n",
        );
        file(
            "composite.fsh",
            "#version 120\n/* DRAWBUFFERS:3 */\n\
             void main() { gl_FragData[0] = vec4(0.25, 0.25, 1.0, 0.25); }\n",
        );
        let pack = horizon_shaders::Pack::open(&dir).unwrap();
        let properties = horizon_shaders::Properties::parse(
            "blend.composite = SRC_ALPHA ONE_MINUS_SRC_ALPHA ONE ZERO\n",
            &horizon_shaders::Macros::default(),
        );
        let mut post = horizon_shaders::load_post(
            &pack,
            "",
            &properties,
            &horizon_shaders::Macros::standard(),
            &Default::default(),
        );
        std::fs::remove_dir_all(&dir).unwrap();
        let program = post.programs.remove(0);
        let buffers = pack_images::Buffers::new(
            &device,
            Default::default(),
            Default::default(),
            wgpu::ShaderStages::FRAGMENT,
        );
        let stand_ins = std::cell::RefCell::new(Vec::new());
        let pack = Resources {
            textures: &[],
            images: &[],
            left_out: &[],
            buffers: &buffers,
            indices: None,
            gpu: Gpu::of(&adapter, device.features()),
            storable: 0,
            rgb: 0,
            packed: [None; BUFFERS],
            released: std::cell::Cell::new(("", 0)),
            stand_ins: &stand_ins,
            shadow_linear: 0,
        };
        let draws = DrawUniforms::new(&device);
        let mut notes = Vec::new();
        let pass = build_pass(
            &device,
            program,
            &[T::Rgba8Unorm; BUFFERS],
            None,
            T::Rgba8Unorm,
            &mut notes,
            None,
            &[],
            &draws.layout,
            &pack,
        )
        .unwrap_or_else(|| panic!("the pass builds: {notes:?}"));
        // colortex3 holds (0.75, 0.5, 0.25, 0.5) before the pass.
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: None,
            size: wgpu::Extent3d {
                width: 64,
                height: 1,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: T::Rgba8Unorm,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = texture.create_view(&Default::default());
        let head = [64.0f32, 1.0, 1.0 / 64.0, 1.0];
        queue.write_buffer(&pass.ubo, 0, bytemuck::cast_slice(&head));
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &pass.layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: pass.ubo.as_entire_binding(),
            }],
        });
        let mut encoder = device.create_command_encoder(&Default::default());
        let before = wgpu::Color {
            r: 0.75,
            g: 0.5,
            b: 0.25,
            a: 0.5,
        };
        encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            color_attachments: &[attach(
                &view,
                wgpu::Operations {
                    load: wgpu::LoadOp::Clear(before),
                    store: wgpu::StoreOp::Store,
                },
            )],
            ..Default::default()
        });
        {
            // Loaded, as `PackPost::render` attaches a buffer a composite pass writes.
            let load = wgpu::Operations {
                load: wgpu::LoadOp::Load,
                store: wgpu::StoreOp::Store,
            };
            let mut rp = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                color_attachments: &[attach(&view, load)],
                ..Default::default()
            });
            rp.set_pipeline(pass.render());
            rp.set_bind_group(0, &group, &[]);
            rp.draw(0..6, 0..1);
        }
        let read = device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: 256,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        encoder.copy_texture_to_buffer(
            texture.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &read,
                layout: wgpu::TexelCopyBufferLayout::default(),
            },
            texture.size(),
        );
        queue.submit([encoder.finish()]);
        read.slice(..).map_async(wgpu::MapMode::Read, |_| {});
        device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
        let texel: Vec<u8> = read.slice(..).get_mapped_range().unwrap()[..4].to_vec();
        // rgb = 0.25 × src + 0.75 × dst, alpha = the source's.
        let expected = [0.625, 0.4375, 0.4375, 0.25].map(|v: f32| v * 255.0);
        for (got, want) in texel.iter().zip(expected) {
            assert!(
                (f32::from(*got) - want).abs() <= 1.0,
                "{texel:?} vs {expected:?}"
            );
        }
    }

    #[test]
    fn pack_formats_map_to_renderable_ones() {
        let f = |name| texture_format(Format::parse(name).unwrap(), wgpu::Features::empty());
        assert_eq!(f("RGBA"), T::Rgba8Unorm);
        assert_eq!(f("RGB8"), T::Rgba8Unorm, "no three-channel formats in wgpu");
        assert_eq!(f("RG16F"), T::Rg16Float);
        assert_eq!(f("R32F"), T::R32Float);
        assert_eq!(f("RGB10_A2"), T::Rgb10a2Unorm);
        assert_eq!(f("R32UI"), T::R32Uint);
        assert_eq!(f("RGBA16"), T::Rgba16Float, "16-bit norm needs a feature");
        assert_eq!(f("RGBA8_SNORM"), T::Rgba16Float, "snorm is not renderable");
        assert_eq!(f("R11F_G11F_B10F"), T::Rgba16Float);
        let all = wgpu::Features::TEXTURE_FORMAT_16BIT_NORM
            | wgpu::Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES
            | wgpu::Features::RG11B10UFLOAT_RENDERABLE;
        let g = |name| texture_format(Format::parse(name).unwrap(), all);
        assert_eq!(g("RGBA16"), T::Rgba16Unorm);
        assert_eq!(g("R11F_G11F_B10F"), T::Rg11b10Ufloat);
    }

    #[test]
    fn float_targets_fall_back_when_not_renderable() {
        let all = |_| true;
        assert_eq!(renderable_float(T::Rgba32Float, all), T::Rgba32Float);
        // OpenGL without GL_EXT_color_buffer_float: only half floats render.
        let half_only = |f: T| matches!(f, T::R16Float | T::Rg16Float | T::Rgba16Float);
        assert_eq!(renderable_float(T::Rgba32Float, half_only), T::Rgba16Float);
        assert_eq!(renderable_float(T::Rg32Float, half_only), T::Rg16Float);
        assert_eq!(renderable_float(T::R32Float, half_only), T::R16Float);
        assert_eq!(renderable_float(T::Rgba16Float, half_only), T::Rgba16Float);
        // Neither renders: the 8-bit last resort.
        assert_eq!(renderable_float(T::Rgba32Float, |_| false), T::Rgba8Unorm);
        assert_eq!(renderable_float(T::Rg16Float, |_| false), T::Rgba8Unorm);
        // Other formats are never replaced.
        assert_eq!(renderable_float(T::Rgba8Uint, |_| false), T::Rgba8Uint);
        assert_eq!(
            renderable_float(T::Rgb10a2Unorm, |_| false),
            T::Rgb10a2Unorm
        );
    }

    #[test]
    fn samplers_read_custom_textures_of_their_type() {
        use wgpu::TextureSampleType as T;
        let float = T::Float { filterable: true };
        assert!(fits("sampler2D", float));
        assert!(fits("sampler3D", float));
        assert!(fits("isampler2D", T::Sint));
        assert!(fits("usampler1D", T::Uint));
        assert!(!fits("sampler2D", T::Uint));
        assert!(!fits("isampler2D", float));
        assert!(!fits("sampler2DShadow", float));
    }

    #[test]
    fn sampler_names() {
        let main = |n| source(n, false, false, false);
        assert!(main("gaux1") == Source::Color(4));
        assert!(main("gdepthtex") == Source::Depth);
        assert!(main("depthtex1") == Source::Depth1);
        assert!(main("depthtex2") == Source::Depth2);
        assert!(main("normals") == Source::Normals);
        assert!(main("specular") == Source::Specular);
        assert!(main("noisetex") == Source::Noise);
        // Without the shadow pass, the shadow buffers are stand-ins.
        assert!(main("shadowcolor0") == Source::Dummy);
        assert!(main("shadowtex0") == Source::Dummy);
        // With it: shadowcolor n is buffer n of the shadow programs, `shadow` is shadowtex1
        // beside `watershadow` (6.3), and colortex is not theirs.
        assert!(source("shadowcolor", false, true, false) == Source::ShadowColor(0));
        assert!(source("shadowcolor1", true, true, false) == Source::Color(1));
        assert!(source("colortex1", true, true, false) == Source::Dummy);
        assert!(source("shadow", false, true, false) == Source::ShadowDepth(0, false));
        assert!(source("shadow", false, true, true) == Source::ShadowDepth(1, false));
        assert!(source("watershadow", false, true, true) == Source::ShadowDepth(0, false));
        assert!(source("shadowtex1HW", false, true, false) == Source::ShadowDepth(1, true));
    }
}

#[cfg(test)]
mod destination_tests;

#[cfg(test)]
#[path = "pack_material_tests.rs"]
mod material_tests;

#[cfg(test)]
mod rgb_alpha_tests;

#[cfg(test)]
mod packed_tests;

#[cfg(test)]
mod trinary_tests;

#[cfg(test)]
mod const_offset_tests;

#[cfg(test)]
mod integer_colortex0_tests;

#[cfg(test)]
mod atlas_mip_tests;

#[cfg(test)]
mod wrong_type_uniform_tests;

#[cfg(test)]
mod atlas_name_post_tests;

#[cfg(test)]
mod unknown_sampler_terrain_tests;

#[cfg(test)]
mod mipmap_box_tests;

#[cfg(test)]
mod colortex_mip_blend_tests;
