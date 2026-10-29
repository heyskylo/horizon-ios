//! A shader pack's post-processing over Horizon's own world (spec 2.1 rows 4, 6, 8, 9; 6.1; 6.2;
//! 6.4): the world renders into its own target as with a resolution scale, its colour becomes
//! colortex0 and its depth `depthtex0`, then the pack's `prepare`, `deferred`, `composite` and
//! `final` programs run in order, and `final` draws onto the screen under the hand and HUD.
//! Gbuffers programs come later; until then the pack's composites read Horizon's picture.
//!
//! Every colortex buffer a program reads or writes has two textures, A and B. Programs read
//! "main" and write "alt", and each buffer a program wrote flips afterwards (6.1.6). The
//! mapping carries over to the next frame, so a buffer that is not cleared reads back what was
//! written last (OQ-8). Buffers are cleared at the start of the frame, "main" only.
//! Pack buffers store rows as GL does (row 0 at the bottom: the translator negates clip y), so
//! the world is copied in upside down, and its reverse-Z depth becomes GL window depth.

use std::collections::HashMap;

use horizon_shaders::{Format, Group, Kind, Post, Val};

use super::pack_draw::DrawUniforms;
use super::pack_entities::{PROGRAMS, PackEntities};
use super::pack_sky::PackSky;
use super::pack_uniforms::{self, FrameUniforms, PackInputs};

const BUFFERS: usize = horizon_shaders::BUFFERS;

/// The texture behind a pack sampler name.
#[derive(Clone, Copy, PartialEq)]
enum Source {
    Color(usize),
    Image(usize),
    Snapshot(usize),
    Custom(usize),
    Depth,
    /// `depthtex1`/`depthtex2`: depth without translucent terrain (a copy taken before it).
    Depth1,
    Noise,
    /// A 1×1 stand-in for names this slice does not provide yet (custom textures...).
    Dummy,
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
    kind: SamplerKind,
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
    Comparison,
}

impl SamplerKind {
    fn of(source: Source, filtering: bool) -> Self {
        match source {
            Source::Custom(_)
            | Source::Snapshot(_)
            | Source::Image(_)
            | Source::Color(_)
            | Source::ShadowColor(_)
            | Source::Dummy
                if !filtering =>
            {
                Self::Nearest
            }
            Source::Depth | Source::Depth1 | Source::ShadowDepth(_, false) => Self::Nearest,
            Source::Noise => Self::Repeat,
            Source::Shadow | Source::ShadowDepth(_, true) => Self::Comparison,
            // Blocks look as the game draws them: nearest texels.
            Source::Atlas => Self::NearestRepeat,
            Source::Custom(_)
            | Source::Snapshot(_)
            | Source::Image(_)
            | Source::Color(_)
            | Source::ShadowColor(_)
            | Source::Dummy
            | Source::Lightmap => Self::Linear,
        }
    }

    fn binding_type(self) -> wgpu::SamplerBindingType {
        match self {
            Self::Linear | Self::Repeat => wgpu::SamplerBindingType::Filtering,
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

#[derive(Clone)]
struct Bindings {
    name: String,
    group: Group,
    layout: wgpu::BindGroupLayout,
    ubo: wgpu::Buffer,
    block: Vec<u8>,
    uniforms: Vec<(Option<usize>, String, u32)>,
    samplers: Vec<Binding>,
    images: Vec<(u32, usize)>,
    feedback: Vec<usize>,
    reads: u16,
    shadow_reads: u16,
    atlas: Option<wgpu::TextureView>,
    groups: HashMap<u64, wgpu::BindGroup>,
}
#[derive(Clone)]
struct Pass {
    bindings: Bindings,
    pipeline: wgpu::RenderPipeline,
    // (index into `pack_uniforms::NAMES`, declared type, offset).
    // (location, buffer) of each output that reaches a buffer.
    writes: Vec<(u32, usize)>,
    // Buffers flipped after the pass.
    flip: u16,
    // Buffers sampled, for the bind group cache key.
    // shadowcolor buffers sampled by a program of the other buffers.
    // Buffers written.
    written: u16,
    // Buffers whose mip chain is regenerated right before it (6.1.7).
    mipmaps: u16,
    // Reads the per-draw block (`DrawUniforms`).
    draw: bool,
    // What `texture` reads instead of the block atlas.
    // Reads `mc_Entity`'s table (`PackPost::block_ids`).
    ids: bool,
    // Bind groups by the sides read (bits 0–15), the buffers whose mip chain is current (bits
    // 16–31) and the shadowcolor sides read (bits 32–33).
}

impl std::ops::Deref for Pass {
    type Target = Bindings;
    fn deref(&self) -> &Bindings {
        &self.bindings
    }
}
impl std::ops::DerefMut for Pass {
    fn deref_mut(&mut self) -> &mut Bindings {
        &mut self.bindings
    }
}
struct ComputePass {
    base: String,
    group: horizon_shaders::ComputeGroup,
    dispatch: horizon_shaders::ComputeDispatch,
    pipeline: wgpu::ComputePipeline,
    ran: bool,
    bindings: Bindings,
}

/// One side of a buffer with mipmaps.
struct Mips {
    /// Every level, for sampling.
    all: wgpu::TextureView,
    /// From the second level down: its attachment, and a bind group over the level above.
    levels: Vec<(wgpu::TextureView, wgpu::BindGroup)>,
}

struct Targets {
    size: [u32; 2],
    /// A and B of each buffer used.
    color: [Option<[wgpu::TextureView; 2]>; BUFFERS],
    depth: wgpu::TextureView,
    depth_texture: wgpu::Texture,
    final_color: Option<wgpu::TextureView>,
    /// `depthtex1`, when a program after translucent terrain reads it.
    depth1: Option<(wgpu::Texture, wgpu::TextureView)>,
    /// Both sides' mip chains of the buffers with mipmaps (`color` is their first level).
    mips: [Option<[Mips; 2]>; BUFFERS],
}

/// What a pack's gbuffers programs read and draw from the game's world.
pub(super) struct World<'a> {
    pub adapter: &'a wgpu::Adapter,
    /// The block atlas and its size.
    pub atlas: Option<(wgpu::TextureView, [u32; 2])>,
    pub lightmap: wgpu::TextureView,
    /// Terrain's geometry, when the pack draws it.
    pub terrain: Option<Geometry<'a>>,
    /// Per-source-state model/fluid IDs, render types, and emitted light.
    pub block_ids: &'a [i32],
    /// The atlas of the sun and moon's sprites.
    pub celestial: Option<wgpu::TextureView>,
    /// Set 1 of the clouds' draw: their offset and palette.
    pub clouds: Option<&'a wgpu::BindGroupLayout>,
    /// The game can hand its rain and snow to the pack.
    pub weather: bool,
    /// The game can hand its first-person hand to the pack.
    pub hand: bool,
}

/// Owned inputs that can move to the compilation worker. Binding layouts and
/// texture views are shared wgpu handles; vertex attributes have static storage.
pub(super) struct OwnedWorld {
    pub adapter: wgpu::Adapter,
    pub atlas: Option<(wgpu::TextureView, [u32; 2])>,
    pub lightmap: wgpu::TextureView,
    pub buffers: Vec<Option<wgpu::VertexBufferLayout<'static>>>,
    pub terrain_table: Option<wgpu::BindGroupLayout>,
    pub has_terrain: bool,
    pub block_ids: Vec<i32>,
    pub celestial: Option<wgpu::TextureView>,
    pub clouds: Option<wgpu::BindGroupLayout>,
    pub weather: bool,
    pub hand: bool,
}
impl OwnedWorld {
    pub fn build(
        self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        post: Post,
        screen: wgpu::TextureFormat,
        cancel: &std::sync::atomic::AtomicBool,
    ) -> PackPost {
        let terrain = self.has_terrain.then(|| Geometry {
            buffers: &self.buffers,
            table: self.terrain_table.as_ref(),
            depth_write: true,
            atlas: None,
            ids: None,
            cull: true,
        });
        PackPost::new_cancellable(
            device,
            queue,
            post,
            screen,
            World {
                adapter: &self.adapter,
                atlas: self.atlas,
                lightmap: self.lightmap,
                terrain,
                block_ids: &self.block_ids,
                celestial: self.celestial,
                clouds: self.clouds.as_ref(),
                weather: self.weather,
                hand: self.hand,
            },
            Some(cancel),
        )
    }
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

pub struct PackPost {
    pub complete: bool,
    pub notes: Vec<String>,
    passes: Vec<Pass>,
    images: super::pack_images::Images,
    textures: super::pack_textures::Textures,
    compute: Vec<ComputePass>,
    compute_setup: bool,
    /// What draws terrain (`gbuffers_terrain` or its fallback), between `prepare` and `deferred`.
    terrain: Option<Pass>,
    /// What draws translucent terrain (`gbuffers_water` or its fallback), after `deferred`.
    water: Option<Pass>,
    /// What draws the sky (`gbuffers_skybasic` or its fallback), before terrain, and its vertices.
    sky: Option<Sky>,
    /// What draws entities and block entities, after terrain, and their vertices.
    entities: Option<Entities>,
    /// What draws the clouds, after entities, and this frame's clouds.
    clouds: Option<Pass>,
    cloud_draw: Option<Clouds>,
    /// The pack has `gbuffers_clouds`, even one that writes nothing (a pack drawing its own
    /// clouds discards vanilla's): the game's clouds are not drawn.
    hides_clouds: bool,
    /// What draws rain and snow (the same program, each with its kind's sprite as `texture`),
    /// after translucent terrain, this frame's weather and the sprites' generation bound.
    weather: Option<[Pass; 2]>,
    weather_draw: Option<Weather>,
    weather_generation: u64,
    /// What draws the first-person hand (`gbuffers_hand` or its fallback), after the opaque
    /// world, and its vertices.
    hand: Option<(Pass, super::pack_hand::PackHand)>,
    /// A program after translucent terrain reads `depthtex1`: it is copied before water.
    depth1: bool,
    /// The shadow buffers, when the shadow pass runs (2.5), and the `shadowcomp` programs.
    shadow_targets: Option<super::pack_shadow::ShadowTargets>,
    /// What draws terrain from the light (`shadow`), and translucent terrain (`shadow_water`).
    shadow_terrain: Option<Pass>,
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
    /// Set 3 of terrain (`mc_Entity`'s state table) and its bind group for each arena page,
    /// with the page buffer it was made for.
    block_ids: Option<(wgpu::BindGroupLayout, wgpu::Buffer)>,
    page_groups: HashMap<u32, (wgpu::Buffer, wgpu::Buffer, wgpu::BindGroup)>,
    /// GPU time of each pass, while the renderer asks for it (`pack_timing.rs`).
    pub timing: Option<super::pack_timing::PassTiming>,
    /// `shaders.properties`' custom uniforms, after the frame's in `uniforms.values`.
    custom: horizon_shaders::Custom,
    atlas: Option<wgpu::TextureView>,
    lightmap: wgpu::TextureView,
    deferred_pre: u16,
    composite_pre: u16,
    screen_format: wgpu::TextureFormat,
    formats: [wgpu::TextureFormat; BUFFERS],
    clears: [Option<[f32; 4]>; BUFFERS],
    used: u16,
    /// Bit n set: B is buffer n's "main".
    main: u16,
    targets: Option<Targets>,
    noise: wgpu::TextureView,
    dummy: wgpu::TextureView,
    /// The stand-in shadow map and its comparison sampler; cleared on the first frame.
    shadow: (wgpu::TextureView, wgpu::Sampler),
    shadow_cleared: bool,
    linear: wgpu::Sampler,
    nearest: wgpu::Sampler,
    repeat: wgpu::Sampler,
    nearest_repeat: wgpu::Sampler,
    blit: (wgpu::RenderPipeline, wgpu::BindGroupLayout),
    /// The blit also writes depthtex0 (some program reads it) and clears these buffers.
    blit_depth: bool,
    blit_clears: Vec<usize>,
    /// Buffers cleared in passes of their own (more than the blit's eight attachments).
    other_clears: Vec<usize>,
    /// The blit's bindings, for the world target of this size.
    blit_group: Option<([u32; 2], wgpu::BindGroup)>,
    /// Shows colortex0 when the pack has no `final` (OQ-5).
    copy: (wgpu::RenderPipeline, wgpu::BindGroupLayout),
    present: (wgpu::RenderPipeline, wgpu::BindGroupLayout),
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

/// The texture behind sampler `name` in a program of the shadow buffers (`shadow_space`: buffer
/// n is shadowcolor n) or of the others, when the shadow pass runs (`shadow_pass`). `shadow` is
/// `shadowtex1` when the program also declares `watershadow` (6.3).
fn source(name: &str, shadow_space: bool, shadow_pass: bool, watershadow: bool) -> Source {
    let shadow_color = match name {
        "shadowcolor" | "shadowcolor0" => Some(0),
        "shadowcolor1" => Some(1),
        _ => None,
    };
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
        // The hand is drawn after the pack, so `depthtex2` (without it) is `depthtex1`.
        "depthtex1" | "depthtex2" => Source::Depth1,
        "noisetex" => Source::Noise,
        "texture" | "gtexture" | "tex" => Source::Atlas,
        "lightmap" => Source::Lightmap,
        n => horizon_shaders::buffer_index(n).map_or(Source::Dummy, Source::Color),
    }
}

/// 256×256 RGB white noise (6.4), from a fixed seed so every run looks the same.
fn noise(device: &wgpu::Device, queue: &wgpu::Queue) -> wgpu::TextureView {
    let mut state = 0x2545_f491_4f6c_dd1du64;
    let texels: Vec<u8> = (0..256 * 256 * 4)
        .map(|i| {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            if i % 4 == 3 { 255 } else { (state >> 56) as u8 }
        })
        .collect();
    texture(device, queue, "Shader pack noisetex", 256, &texels)
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

/// Runs `create` and returns its validation error, if any. A panic while it runs is an error
/// too: naga's backends turn a module they cannot write into a panic as the driver gets the
/// pipeline (#3525), which a pack's program must not turn into a crash.
fn checked<T>(device: &wgpu::Device, create: impl FnOnce() -> T) -> Result<T, String> {
    let memory = device.push_error_scope(wgpu::ErrorFilter::OutOfMemory);
    let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
    let value = crate::crash::contain(create);
    let validation = pollster::block_on(scope.pop());
    let allocation = pollster::block_on(memory.pop());
    match validation.or(allocation) {
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
        visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
        ty,
        count: None,
    }
}

fn texture_entry(binding: u32, sample_type: wgpu::TextureSampleType) -> wgpu::BindGroupLayoutEntry {
    entry(
        binding,
        wgpu::BindingType::Texture {
            sample_type,
            view_dimension: wgpu::TextureViewDimension::D2,
            multisampled: false,
        },
    )
}

fn image_feedback(
    device: &wgpu::Device,
    images: &mut super::pack_images::Images,
    samplers: &mut [Binding],
    interface: &horizon_shaders::Interface,
) -> Vec<usize> {
    let writes: Vec<_> = interface
        .images
        .iter()
        .filter(|s| s.access.contains(naga::StorageAccess::STORE))
        .filter_map(|s| images.image(&s.name))
        .collect();
    let mut feedback = Vec::new();
    for sampler in samplers {
        if let Source::Image(i) = sampler.source
            && writes.contains(&i)
        {
            images.ensure_snapshot(device, i);
            sampler.source = Source::Snapshot(i);
            if !feedback.contains(&i) {
                feedback.push(i);
            }
        }
    }
    feedback
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

fn target(format: wgpu::TextureFormat) -> Option<wgpu::ColorTargetState> {
    Some(wgpu::ColorTargetState {
        format,
        blend: None,
        write_mask: wgpu::ColorWrites::ALL,
    })
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
pub(super) type TerrainDraw<'a> =
    dyn FnMut(&mut wgpu::RenderPass<'_>, TerrainList, &dyn Fn(&mut wgpu::RenderPass<'_>, u32)) + 'a;

/// What the shadow camera sees of the terrain, for the game's draw lists: its view and
/// projection in wgpu's clip space, the direction toward the light, and how far from the camera
/// terrain is drawn (`shadowDistanceRenderMul`).
pub(super) struct ShadowView {
    pub cull: glam::Mat4,
    pub light: glam::Vec3,
    pub distance: Option<f32>,
}

/// Set 3 of terrain: state-to-material data, page vertices, and each quad's source state/cell.
fn block_ids(device: &wgpu::Device, table: &[i32]) -> (wgpu::BindGroupLayout, wgpu::Buffer) {
    let empty = [-1, -1];
    let table = if table.is_empty() { &empty[..] } else { table };
    let buffer = wgpu::util::DeviceExt::create_buffer_init(
        device,
        &wgpu::util::BufferInitDescriptor {
            label: Some("Shader pack block ids"),
            contents: bytemuck::cast_slice(table),
            usage: wgpu::BufferUsages::STORAGE,
        },
    );
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
    (layout, buffer)
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
    /// `mc_Entity`'s table at set 3 (both terrain draw paths).
    pub ids: Option<&'a wgpu::BindGroupLayout>,
    /// Back faces are culled, as for the game's own terrain.
    pub cull: bool,
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
    shadow: Option<&[wgpu::TextureFormat; 2]>,
    screen: wgpu::TextureFormat,
    notes: &mut Vec<String>,
    geometry: Option<&Geometry<'_>>,
    custom: &[String],
    draws: &wgpu::BindGroupLayout,
    image_bank: &mut super::pack_images::Images,
    texture_bank: &super::pack_textures::Textures,
) -> Option<Pass> {
    let features = device.features();
    // The frame's uniforms, then the pack's custom ones.
    let index = |name: &str| {
        pack_uniforms::index(name).or_else(|| {
            let k = custom.iter().position(|c| c == name)?;
            Some(pack_uniforms::NAMES.len() + k)
        })
    };
    let mask = |flips: &[(u32, bool)]| flips.iter().filter(|f| f.1).fold(0u16, |m, f| m | 1 << f.0);
    let interface = &p.program.interface;
    let draw = interface.draw;
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
    // A gbuffers pass draws into its buffers and the depth buffer: reading them as well would
    // be a feedback loop (undefined in GL, refused by wgpu), so those read a blank texture.
    let in_place = match geometry {
        Some(_) => p.buffers.iter().fold(0u16, |m, &b| m | 1 << b),
        None => 0,
    };
    for s in &interface.samplers {
        let mut src = texture_bank.sampler(&s.name, p.group).map_or_else(
            || {
                image_bank.sampler(&s.name).map_or_else(
                    || source(&s.name, p.group.shadow(), shadow.is_some(), watershadow),
                    Source::Image,
                )
            },
            Source::Custom,
        );
        if matches!(src, Source::Dummy | Source::Shadow) {
            notes.push(format!(
                "{}: texture {} is unavailable; pack disabled",
                p.name, s.name
            ));
            return None;
        }
        let blank = match src {
            // The shadow pass writes the shadow map.
            Source::ShadowDepth(..) => p.group == Group::Shadow,
            Source::Atlas => geometry.is_none(),
            Source::Depth => geometry.is_some(),
            // Translucent terrain reads the copy; opaque terrain would read itself.
            Source::Depth1 => p.group == Group::Gbuffers,
            Source::Color(n) => in_place & 1 << n != 0,
            _ => false,
        };
        let float = wgpu::TextureSampleType::Float { filterable: true };
        let mut sample_type = match src {
            Source::Custom(_) => float,
            Source::Image(i) => image_bank.0[i]
                .format
                .sample_type(None, Some(features))
                .unwrap_or(float),
            Source::Color(n) => formats[n]
                .sample_type(None, Some(features))
                .unwrap_or(float),
            Source::ShadowColor(n) => shadow
                .and_then(|f| f[n].sample_type(None, Some(features)))
                .unwrap_or(float),
            Source::Depth | Source::Depth1 | Source::ShadowDepth(..) => {
                wgpu::TextureSampleType::Float { filterable: false }
            }
            // Unfilterable on WebGPU-level devices; the lightmap is sampled at texel centres.
            Source::Lightmap => float,
            _ => float,
        };
        let int = !matches!(sample_type, wgpu::TextureSampleType::Float { .. });
        if let Source::ShadowDepth(n, _) = src {
            let comparison = s.ty == "sampler2DShadow";
            src = Source::ShadowDepth(n, comparison);
            if comparison {
                sample_type = wgpu::TextureSampleType::Depth;
            }
        }
        let dimension = match src {
            Source::Image(i) => image_bank.0[i].dimension,
            _ => wgpu::TextureViewDimension::D2,
        };
        let wanted = super::pack_textures::sampler_matches(&s.ty, sample_type, dimension);
        if !wanted {
            notes.push(format!(
                "{}: `{} {}` is not provided yet",
                p.name, s.ty, s.name
            ));
            return None;
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
        let filtering = sample_type == float;
        match src {
            Source::Color(n) => reads |= 1 << n,
            Source::ShadowColor(n) => shadow_reads |= 1 << n,
            _ => {}
        }
        let mut texture_layout = texture_entry(s.binding, sample_type);
        if let wgpu::BindingType::Texture { view_dimension, .. } = &mut texture_layout.ty {
            *view_dimension = dimension;
        }
        entries.push(texture_layout);
        let kind = match src {
            Source::Custom(i) => match (texture_bank.0[i].linear, texture_bank.0[i].clamp) {
                (true, true) => SamplerKind::Linear,
                (false, true) => SamplerKind::Nearest,
                (true, false) => SamplerKind::Repeat,
                (false, false) => SamplerKind::NearestRepeat,
            },
            _ => SamplerKind::of(src, filtering),
        };
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
            kind,
            sampler,
        });
    }
    let mut images = Vec::new();
    for image in &interface.images {
        let Some(i) = image_bank.image(&image.name) else {
            notes.push(format!("{}: missing storage image {}", p.name, image.name));
            return None;
        };
        if !image_bank.0[i].supports_access(image.access) {
            notes.push(format!(
                "{}: storage image {} format {:?} does not support access {:?}",
                p.name, image.name, image_bank.0[i].format, image.access
            ));
            return None;
        }
        let access = match (
            image.access.contains(naga::StorageAccess::LOAD),
            image.access.contains(naga::StorageAccess::STORE),
        ) {
            (true, true) => wgpu::StorageTextureAccess::ReadWrite,
            (true, false) => wgpu::StorageTextureAccess::ReadOnly,
            _ => wgpu::StorageTextureAccess::WriteOnly,
        };
        entries.push(entry(
            image.binding,
            wgpu::BindingType::StorageTexture {
                access,
                format: image_bank.0[i].format,
                view_dimension: image_bank.0[i].dimension,
            },
        ));
        images.push((image.binding, i));
    }
    let feedback = image_feedback(device, image_bank, &mut samplers, interface);
    let final_pass = p.group == Group::Final;
    let mut targets = Vec::new();
    let mut writes = Vec::new();
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
            let mut state = target(format);
            if let (Some(t), Some(b)) = (&mut state, p.blend)
                && format
                    .guaranteed_format_features(features)
                    .flags
                    .contains(wgpu::TextureFormatFeatureFlags::BLENDABLE)
            {
                t.blend = Some(blend_state(b));
            }
            targets[l] = state;
            writes.push((location, buffer as usize));
        }
    }
    if final_pass && !interface.outputs.contains(&0) {
        notes.push(format!(
            "{}: has no window color output; pack disabled",
            p.name
        ));
        return None;
    }
    if !final_pass && writes.is_empty() && geometry.is_none() {
        notes.push(format!("{}: writes no buffer; skipped", p.name));
        return None;
    }
    let written = writes.iter().fold(0u16, |m, w| m | 1 << w.1);
    let kept = p
        .flips
        .iter()
        .filter(|f| !f.1)
        .fold(0u16, |m, f| m | 1 << f.0);
    // Gbuffers programs write in place (6.1.6).
    let flip = if final_pass || geometry.is_some() {
        0
    } else {
        (written & !kept) | mask(&p.flips)
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
            ],
            ..Default::default()
        });
        let module = |mut m: naga::Module| {
            share_samplers(&mut m, &slots);
            device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some(&p.name),
                source: wgpu::ShaderSource::Naga(std::borrow::Cow::Owned(m)),
            })
        };
        let (vs, fs) = (module(p.program.vertex), module(p.program.fragment));
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some(&p.name),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &vs,
                entry_point: Some("main"),
                compilation_options: Default::default(),
                buffers: geometry.map_or(&[], |g| g.buffers),
            },
            // Rows flipped to GL's turn front faces clockwise.
            primitive: wgpu::PrimitiveState {
                front_face: wgpu::FrontFace::Cw,
                cull_mode: geometry.filter(|g| g.cull).map(|_| wgpu::Face::Back),
                ..Default::default()
            },
            depth_stencil: geometry.map(|g| wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth32Float,
                depth_write_enabled: Some(g.depth_write),
                depth_compare: Some(wgpu::CompareFunction::LessEqual),
                stencil: Default::default(),
                bias: Default::default(),
            }),
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState {
                module: &fs,
                entry_point: Some("main"),
                compilation_options: Default::default(),
                targets: &targets,
            }),
            multiview_mask: None,
            cache: None,
        });
        (layout, pipeline)
    });
    let (layout, pipeline) = match created {
        Ok(created) => created,
        Err(e) => {
            notes.push(format!("{}: skipped: {e}", p.name));
            return None;
        }
    };
    let interface = &p.program.interface;
    let missing: Vec<&str> = interface
        .uniforms
        .iter()
        .filter(|u| !u.name.starts_with("hz_") && index(&u.name).is_none())
        .map(|u| u.name.as_str())
        .collect();
    if !missing.is_empty() {
        notes.push(format!("{}: zero for now: {}", p.name, missing.join(" ")));
    }
    Some(Pass {
        bindings: Bindings {
            ubo: device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(&p.name),
                size: u64::from(interface.uniform_size.max(16)),
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }),
            block: vec![0; interface.uniform_size as usize],
            uniforms: interface
                .uniforms
                .iter()
                .map(|u| (index(&u.name), u.ty.clone(), u.offset))
                .collect(),
            name: p.name,
            group: p.group,
            layout,
            samplers,
            images,
            feedback,
            reads,
            shadow_reads,
            atlas: geometry.and_then(|g| g.atlas.clone()),
            groups: HashMap::new(),
        },
        pipeline,
        writes,
        flip,
        written,
        mipmaps: p.mipmaps,
        draw,
        ids: geometry.is_some_and(|g| g.ids.is_some()),
    })
}

fn build_compute(
    device: &wgpu::Device,
    p: horizon_shaders::LoadedCompute,
    images: &mut super::pack_images::Images,
    textures: &super::pack_textures::Textures,
    formats: &[wgpu::TextureFormat; BUFFERS],
    shadow: Option<&[wgpu::TextureFormat; 2]>,
    custom: &[String],
) -> Result<ComputePass, String> {
    let limits = device.limits();
    let local = p.dispatch.local_size;
    if local[0] > limits.max_compute_workgroup_size_x
        || local[1] > limits.max_compute_workgroup_size_y
        || local[2] > limits.max_compute_workgroup_size_z
        || local
            .iter()
            .try_fold(1u32, |a, &b| a.checked_mul(b))
            .is_none_or(|v| v > limits.max_compute_invocations_per_workgroup)
    {
        return Err(format!(
            "{}: compute local size exceeds device limits",
            p.name
        ));
    }
    let interface = p.program.interface;
    let group = match p.group {
        horizon_shaders::ComputeGroup::ShadowComp => Group::ShadowComp,
        horizon_shaders::ComputeGroup::Deferred => Group::Deferred,
        horizon_shaders::ComputeGroup::Composite => Group::Composite,
        horizon_shaders::ComputeGroup::Final => Group::Final,
        _ => Group::Prepare,
    };
    let mut entries = vec![entry(
        0,
        wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Uniform,
            has_dynamic_offset: false,
            min_binding_size: None,
        },
    )];
    let mut samplers = Vec::new();
    let mut storage = Vec::new();
    let mut kinds = HashMap::new();
    let mut slots = HashMap::new();
    let mut reads = 0;
    let mut shadow_reads = 0;
    for s in &interface.samplers {
        let mut src = textures.compute_sampler(&s.name, p.group).map_or_else(
            || {
                images.sampler(&s.name).map_or_else(
                    || source(&s.name, group.shadow(), shadow.is_some(), false),
                    Source::Image,
                )
            },
            Source::Custom,
        );
        let float = wgpu::TextureSampleType::Float { filterable: true };
        let (sample_type, dimension) = match src {
            Source::Custom(_) => (float, wgpu::TextureViewDimension::D2),
            Source::Image(i) => (
                images.0[i]
                    .format
                    .sample_type(None, Some(device.features()))
                    .unwrap_or(float),
                images.0[i].dimension,
            ),
            Source::Color(i) => {
                reads |= 1 << i;
                (
                    formats[i]
                        .sample_type(None, Some(device.features()))
                        .unwrap_or(float),
                    wgpu::TextureViewDimension::D2,
                )
            }
            Source::ShadowColor(i) => {
                shadow_reads |= 1 << i;
                (
                    shadow.map_or(float, |f| {
                        f[i].sample_type(None, Some(device.features()))
                            .unwrap_or(float)
                    }),
                    wgpu::TextureViewDimension::D2,
                )
            }
            Source::ShadowDepth(n, _) if s.ty == "sampler2DShadow" => {
                src = Source::ShadowDepth(n, true);
                (
                    wgpu::TextureSampleType::Depth,
                    wgpu::TextureViewDimension::D2,
                )
            }
            Source::Depth | Source::Depth1 | Source::ShadowDepth(..) => {
                if let Source::ShadowDepth(n, _) = src {
                    src = Source::ShadowDepth(n, false);
                }
                (
                    wgpu::TextureSampleType::Float { filterable: false },
                    wgpu::TextureViewDimension::D2,
                )
            }
            Source::Noise | Source::Lightmap => (float, wgpu::TextureViewDimension::D2),
            _ => {
                return Err(format!(
                    "{}: unavailable compute sampler {}",
                    p.name, s.name
                ));
            }
        };
        if !super::pack_textures::sampler_matches(&s.ty, sample_type, dimension) {
            return Err(format!(
                "{}: `{} {}` does not match texture dimension or scalar type",
                p.name, s.ty, s.name
            ));
        }
        let filtering = sample_type == float;
        let mut layout = texture_entry(s.binding, sample_type);
        if let wgpu::BindingType::Texture { view_dimension, .. } = &mut layout.ty {
            *view_dimension = dimension;
        }
        entries.push(layout);
        let kind = match src {
            Source::Custom(i) => match (textures.0[i].linear, textures.0[i].clamp) {
                (true, true) => SamplerKind::Linear,
                (false, true) => SamplerKind::Nearest,
                (true, false) => SamplerKind::Repeat,
                (false, false) => SamplerKind::NearestRepeat,
            },
            _ => SamplerKind::of(src, filtering),
        };
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
            kind,
            sampler,
        });
    }
    for image in &interface.images {
        let i = images
            .image(&image.name)
            .ok_or_else(|| format!("{}: missing compute image {}", p.name, image.name))?;
        if !images.0[i].supports_access(image.access) {
            return Err(format!(
                "{}: storage image {} format {:?} does not support access {:?}",
                p.name, image.name, images.0[i].format, image.access
            ));
        }
        let access = match (
            image.access.contains(naga::StorageAccess::LOAD),
            image.access.contains(naga::StorageAccess::STORE),
        ) {
            (true, true) => wgpu::StorageTextureAccess::ReadWrite,
            (true, false) => wgpu::StorageTextureAccess::ReadOnly,
            _ => wgpu::StorageTextureAccess::WriteOnly,
        };
        entries.push(entry(
            image.binding,
            wgpu::BindingType::StorageTexture {
                access,
                format: images.0[i].format,
                view_dimension: images.0[i].dimension,
            },
        ));
        storage.push((image.binding, i));
    }
    let feedback = image_feedback(device, images, &mut samplers, &interface);
    for entry in &mut entries {
        entry.visibility = wgpu::ShaderStages::COMPUTE;
    }
    let (layout, pipeline) = checked(device, || {
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some(&p.name),
            entries: &entries,
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some(&p.name),
            bind_group_layouts: &[Some(&layout)],
            ..Default::default()
        });
        let mut module = p.program.module;
        share_samplers(&mut module, &slots);
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some(&p.name),
            source: wgpu::ShaderSource::Naga(std::borrow::Cow::Owned(module)),
        });
        let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some(&p.name),
            layout: Some(&pipeline_layout),
            module: &module,
            entry_point: Some("main"),
            compilation_options: Default::default(),
            cache: None,
        });
        (layout, pipeline)
    })?;
    let index = |name: &str| {
        pack_uniforms::index(name).or_else(|| {
            custom
                .iter()
                .position(|n| n == name)
                .map(|i| i + pack_uniforms::NAMES.len())
        })
    };
    Ok(ComputePass {
        base: p.base,
        group: p.group,
        dispatch: p.dispatch,
        pipeline,
        ran: false,
        bindings: Bindings {
            name: p.name.clone(),
            group,
            layout,
            ubo: device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(&p.name),
                size: u64::from(interface.uniform_size.max(16)),
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }),
            block: vec![0; interface.uniform_size as usize],
            uniforms: interface
                .uniforms
                .iter()
                .map(|u| (index(&u.name), u.ty.clone(), u.offset))
                .collect(),
            samplers,
            images: storage,
            feedback,
            reads,
            shadow_reads,
            atlas: None,
            groups: HashMap::new(),
        },
    })
}

impl PackPost {
    /// Builds the pipelines of `post` for a screen of `screen` format.
    #[cfg(test)]
    pub fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        post: Post,
        screen: wgpu::TextureFormat,
        world: World<'_>,
    ) -> Self {
        Self::new_cancellable(device, queue, post, screen, world, None)
    }
    fn new_cancellable(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        post: Post,
        screen: wgpu::TextureFormat,
        world: World<'_>,
        cancel: Option<&std::sync::atomic::AtomicBool>,
    ) -> Self {
        let features = device.features();
        let formats: [wgpu::TextureFormat; BUFFERS] =
            std::array::from_fn(|i| texture_format(post.config.buffers[i].format, features));
        let clears = std::array::from_fn(|i| {
            let b = &post.config.buffers[i];
            b.clear.then(|| {
                b.clear_color
                    .unwrap_or(if i == 1 { [1.0; 4] } else { [0.0; 4] })
            })
        });
        let mut complete = post.complete;
        let storage_names = super::pack_images::storage_names(&post);
        let mut notes = post.notes;
        let mut images = match checked(device, || {
            super::pack_images::Images::new(
                device,
                world.adapter,
                &post.images,
                &storage_names,
                [1, 1],
            )
        })
        .and_then(|v| v)
        {
            Ok(images) => images,
            Err(error) => {
                complete = false;
                notes.push(format!("Custom shader images: {error}"));
                super::pack_images::Images(Vec::new())
            }
        };
        let textures = match checked(device, || {
            super::pack_textures::Textures::new(device, queue, post.textures)
        })
        .and_then(|v| v)
        {
            Ok(textures) => textures,
            Err(error) => {
                complete = false;
                notes.push(error);
                super::pack_textures::Textures(Vec::new())
            }
        };
        notes.extend(post.config.notes);
        let shadow_config = post.config.shadow.clone();
        let mut custom = post.custom;
        let custom_names: Vec<String> = custom.uniforms().map(|u| u.0.to_owned()).collect();
        // A first run finds the names neither the frame nor the pack defines.
        custom.eval(0.0, &|n| pack_uniforms::index(n).map(|_| Val::scalar(0.0)));
        if !custom.notes.is_empty() {
            notes.push(format!("custom uniforms: {}", custom.notes.join("; ")));
            custom.notes.clear();
        }
        let mask =
            |flips: &[(u32, bool)]| flips.iter().filter(|f| f.1).fold(0u16, |m, f| m | 1 << f.0);
        let mut used = 1u16;
        let draws = DrawUniforms::new(device);
        // The shadow buffers' formats when the shadow pass runs; their programs see them as
        // buffers 0 and 1.
        let shadow_formats: Option<[wgpu::TextureFormat; 2]> = post.shadow_pass.then(|| {
            std::array::from_fn(|n| texture_format(shadow_config.colors[n].format, features))
        });
        let shadow_space: [wgpu::TextureFormat; BUFFERS] = std::array::from_fn(|i| {
            shadow_formats
                .filter(|_| i < 2)
                .map_or(formats[i], |f| f[i])
        });
        let mut compute = Vec::new();
        complete &= post.compute.complete;
        notes.extend(post.compute.notes);
        for program in post.compute.programs {
            let in_shadow = program.group == horizon_shaders::ComputeGroup::ShadowComp;
            match build_compute(
                device,
                program,
                &mut images,
                &textures,
                if in_shadow { &shadow_space } else { &formats },
                shadow_formats.as_ref(),
                &custom_names,
            ) {
                Ok(pass) => {
                    if !pass.bindings.group.shadow() {
                        used |= pass.bindings.reads;
                    }
                    compute.push(pass);
                }
                Err(error) => {
                    complete = false;
                    notes.push(error);
                }
            }
        }
        let mut build = |p: horizon_shaders::PostProgram, geometry: Option<&Geometry<'_>>| {
            if cancel.is_some_and(|cancel| cancel.load(std::sync::atomic::Ordering::Relaxed)) {
                complete = false;
                return None;
            }
            let in_shadow = p.group.shadow();
            let no_op = p.group != Group::Final
                && geometry.is_none()
                && p.program
                    .interface
                    .outputs
                    .iter()
                    .all(|location| p.buffers.get(*location as usize).is_none());
            let result = build_pass(
                device,
                p,
                if in_shadow { &shadow_space } else { &formats },
                shadow_formats.as_ref(),
                screen,
                &mut notes,
                geometry,
                &custom_names,
                &draws.layout,
                &mut images,
                &textures,
            );
            let Some(pass) = result else {
                if !no_op {
                    complete = false;
                }
                return None;
            };
            if !in_shadow {
                used |= pass.reads | pass.written | pass.flip;
            }
            Some(pass)
        };
        // Both terrain paths read exact per-quad source identity from arena sidecars.
        let block_ids = world
            .terrain
            .as_ref()
            .map(|_| block_ids(device, world.block_ids));
        let terrain_geometry = world.terrain.as_ref().map(|g| Geometry {
            buffers: g.buffers,
            table: g.table,
            depth_write: g.depth_write,
            atlas: g.atlas.clone(),
            ids: block_ids.as_ref().map(|i| &i.0),
            cull: g.cull,
        });
        let terrain = post
            .terrain
            .zip(terrain_geometry.as_ref())
            .and_then(|(p, g)| build(p, Some(g)));
        let water = post
            .water
            .zip(terrain_geometry.as_ref())
            .and_then(|(p, g)| build(p, Some(g)));
        let shadow_terrain = post
            .shadow
            .zip(terrain_geometry.as_ref())
            .and_then(|(p, g)| build(p, Some(g)));
        let shadow_water = post
            .shadow_water
            .zip(terrain_geometry.as_ref())
            .and_then(|(p, g)| build(p, Some(g)));
        let sky = (post.sky.is_some() || post.sky_textured.is_some()).then(|| {
            let geometry = PackSky::new(device);
            let basic = Geometry {
                buffers: &PackSky::buffers(),
                table: Some(&geometry.layout),
                depth_write: false,
                atlas: None,
                ids: None,
                cull: false,
            };
            let basic = post.sky.and_then(|p| build(p, Some(&basic)));
            let textured = Geometry {
                buffers: &PackSky::celestial_buffers(),
                table: None,
                depth_write: false,
                atlas: world.celestial.clone(),
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
        ];
        let late = [false, false, post.translucent_late, post.particles_late];
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
            ids: None,
            cull: false,
        });
        let entity_passes = programs.map(|p| {
            p.zip(entity_geometry.as_ref())
                .and_then(|(p, g)| build(p, Some(g)))
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
            ids: None,
            cull: false,
        });
        let hides_clouds = post.clouds.is_some() && cloud_geometry.is_some();
        let weather_geometry = world.weather.then(|| Geometry {
            buffers: &entity_buffers,
            table: None,
            depth_write: true,
            atlas: None,
            ids: None,
            cull: false,
        });
        let hand_geometry = world.hand.then(|| super::pack_hand::PackHand::new(device));
        let hand = post.hand.zip(hand_geometry).and_then(|(p, geometry)| {
            let g = Geometry {
                buffers: &entity_buffers,
                table: Some(&geometry.layout),
                depth_write: true,
                atlas: world.celestial.clone(),
                ids: None,
                cull: false,
            };
            build(p, Some(&g)).map(|pass| (pass, geometry))
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
        // Developers' aid: `HORIZON_SHADER_PACK_SKIP=composite,final` leaves programs out.
        let skip = std::env::var("HORIZON_SHADER_PACK_SKIP").unwrap_or_default();
        let passes: Vec<Pass> = post
            .programs
            .into_iter()
            .filter(|p| !skip.split(',').any(|s| s == p.name))
            .filter_map(|p| build(p, None))
            .collect();
        let shadow_comp: Vec<Pass> = post
            .shadow_comp
            .into_iter()
            .filter(|p| !skip.split(',').any(|s| s == p.name))
            .filter_map(|p| build(p, None))
            .collect();
        if let Some(t) = &terrain {
            notes.push(format!("terrain draws with {}", t.name));
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
        if let Some(c) = &clouds {
            notes.push(format!("clouds draw with {}", c.name));
        }
        if let Some((h, _)) = &hand {
            notes.push(format!("the hand draws with {}", h.name));
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
        let has_translucent = water.is_some()
            || weather.is_some()
            || entities.as_ref().is_some_and(|e| {
                e.passes
                    .iter()
                    .zip(e.late)
                    .any(|(p, late)| p.is_some() && late)
            });
        let depth1 = has_translucent
            && passes
                .iter()
                .map(|p| &p.bindings)
                .chain(water.iter().map(|p| &p.bindings))
                .chain(weather.iter().take(1).flatten().map(|p| &p.bindings))
                .chain(compute.iter().map(|p| &p.bindings))
                .any(|p| p.samplers.iter().any(|s| s.source == Source::Depth1));
        let every_pass = || {
            (passes
                .iter()
                .chain(&shadow_comp)
                .chain(&terrain)
                .chain(&water))
            .chain(shadow_terrain.iter().chain(&shadow_water))
            .chain(sky.iter().flat_map(|s| s.basic.iter().chain(&s.textured)))
            .chain(entities.iter().flat_map(|e| e.passes.iter().flatten()))
            .chain(entities.iter().flat_map(|e| e.shadow.iter().flatten()))
            .chain(&clouds)
            .chain(weather.iter().flatten())
            .chain(hand.iter().map(|h| &h.0))
        };
        let shadow_targets = shadow_formats.map(|f| {
            let colors = every_pass().fold(0u16, |m, p| {
                let own=if p.group.shadow(){p.reads|p.written|p.flip}else{0};
                m|own|p.shadow_reads
            }) | compute.iter().fold(0u16,|m,p|m|p.bindings.shadow_reads|if p.bindings.group.shadow(){p.bindings.reads}else{0});
            let depth1 = every_pass()
                .map(|p|&p.bindings).chain(compute.iter().map(|p|&p.bindings))
                .flat_map(|p| &p.samplers)
                .any(|s| matches!(s.source, Source::ShadowDepth(1, _)));
            let t =
                super::pack_shadow::ShadowTargets::new(device, &shadow_config, f, colors, depth1);
            notes.push(format!(
                "shadow map {0}x{0} over {2} blocks, sunPathRotation {3}, shadowcolor buffers {colors:#b}{1}",
                t.size,
                if depth1 { ", shadowtex1" } else { "" },
                shadow_config.distance,
                shadow_config.sun_path_rotation,
            ));
            t
        });
        // A clear only matters when a program reads the buffer before one writes all of it
        // (composite passes cover their whole target, terrain does not); the others are skipped.
        let mut cleared = Vec::new();
        for b in 1..BUFFERS {
            let bit = 1u16 << b;
            let touched = |p: &&Pass| (p.reads | p.flip | p.written) & bit != 0;
            // Gbuffers passes cover part of their targets: a clear is needed if they touch it.
            let first = sky
                .iter()
                .flat_map(|s| s.basic.iter().chain(&s.textured))
                .chain(&terrain)
                .chain(entities.iter().flat_map(|e| e.passes.iter().flatten()))
                .chain(&clouds)
                .chain(&water)
                .chain(weather.iter().take(1).flatten())
                .chain(hand.iter().map(|h| &h.0))
                .find(touched)
                .map(|_| true)
                .or_else(|| {
                    let p = passes.iter().find(touched)?;
                    Some(p.reads & bit != 0 || p.written & bit == 0)
                });
            let compute_reads = compute
                .iter()
                .any(|p| !p.bindings.group.shadow() && p.bindings.reads & bit != 0);
            if clears[b].is_some() && (first == Some(true) || compute_reads) {
                cleared.push(b);
            }
        }
        // Terrain tests against the world's depth (entities...), so the blit copies it then.
        let blit_depth = terrain.is_some()
            || water.is_some()
            || sky.is_some()
            || entities.as_ref().is_some_and(Entities::drawn)
            || clouds.is_some()
            || weather.is_some()
            || hand.is_some()
            || passes
                .iter()
                .map(|p| &p.bindings)
                .chain(compute.iter().map(|p| &p.bindings))
                .any(|p| {
                    p.samplers
                        .iter()
                        .any(|s| matches!(s.source, Source::Depth | Source::Depth1))
                });
        let room = 8 - 1;
        let other_clears = cleared.split_off(room.min(cleared.len()));
        let float = wgpu::TextureSampleType::Float { filterable: true };
        let blit_targets = std::iter::once(target(formats[0]));
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
        let blit = helper(
            device,
            "vs_full",
            if blit_depth {
                "fs_blit"
            } else {
                "fs_blit_color"
            },
            &[
                texture_entry(0, float),
                texture_entry(1, wgpu::TextureSampleType::Depth),
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
        );
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
        let present = helper(
            device,
            "vs_present",
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
        Self {
            images,
            textures,
            compute,
            compute_setup: false,
            complete,
            notes,
            passes,
            terrain,
            water,
            sky,
            entities,
            clouds,
            cloud_draw: None,
            hides_clouds,
            weather,
            weather_draw: None,
            weather_generation: 0,
            hand,
            depth1,
            shadow_targets,
            shadow_terrain,
            shadow_water,
            shadow_comp,
            mipped,
            mips_valid: 0,
            down,
            draws,
            block_ids,
            page_groups: HashMap::new(),
            timing: None,
            custom,
            atlas: world.atlas.as_ref().map(|a| a.0.clone()),
            lightmap: world.lightmap,
            deferred_pre: mask(&post.deferred_pre),
            composite_pre: mask(&post.composite_pre),
            formats,
            screen_format: screen,
            clears,
            used,
            blit_depth,
            blit_clears: cleared,
            other_clears,
            blit_group: None,
            main: 0,
            targets: None,
            noise: noise(device, queue),
            dummy: texture(device, queue, "Shader pack stand-in", 1, &white),
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
            blit,
            copy,
            present,
            uniforms: {
                let mut u = FrameUniforms::default();
                u.atlas = world.atlas.map_or([0; 2], |a| a.1);
                u.shadow = shadow_config;
                u
            },
        }
    }

    /// Writes `pass`'s uniforms for a target of `out` pixels; its bind group for the buffers'
    /// current mapping.
    fn bind(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        pass: &mut Bindings,
        out: [u32; 2],
    ) -> wgpu::BindGroup {
        self.images.snapshot(encoder, &pass.feedback);
        let fog = self.uniforms.fog();
        // Our block members first (`hz_target`, `hz_fog_color`, `hz_fog`), then the pack's.
        let ours = [
            [
                out[0] as f32,
                out[1] as f32,
                1.0 / out[0] as f32,
                1.0 / out[1] as f32,
            ],
            fog[0].to_array(),
            fog[1].to_array(),
        ];
        let head: &[u8] = bytemuck::cast_slice(&ours);
        let n = head.len().min(pass.block.len());
        pass.block[..n].copy_from_slice(&head[..n]);
        for (index, ty, offset) in &pass.uniforms {
            if let Some(v) = index.and_then(|i| self.uniforms.values.get(i)) {
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
                                Group::Composite | Group::Final | Group::Translucent
                            ) =>
                        {
                            &d.1
                        }
                        _ => &targets.depth,
                    },
                    Source::Noise => &self.noise,
                    Source::Dummy => &self.dummy,
                    Source::Shadow => &self.shadow.0,
                    Source::Atlas => pass
                        .atlas
                        .as_ref()
                        .or(self.atlas.as_ref())
                        .unwrap_or(&self.dummy),
                    Source::Lightmap => &self.lightmap,
                    Source::Image(i) => &self.images.0[i].view,
                    Source::Snapshot(i) => {
                        &self.images.0[i]
                            .snapshot
                            .as_ref()
                            .expect("prepared snapshot")
                            .1
                    }
                    Source::Custom(i) => &self.textures.0[i].view,
                };
                let sampler = match s.kind {
                    SamplerKind::Linear => &self.linear,
                    SamplerKind::Nearest => &self.nearest,
                    SamplerKind::Repeat => &self.repeat,
                    SamplerKind::NearestRepeat => &self.nearest_repeat,
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
            for &(binding, i) in &pass.images {
                entries.push(wgpu::BindGroupEntry {
                    binding,
                    resource: wgpu::BindingResource::TextureView(&self.images.0[i].view),
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
            _ => (super::pack_shadow::load(), [load; 2]),
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
    /// the game's draws.
    fn draw_terrain(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        size: [u32; 2],
        list: TerrainList,
        draw: &mut TerrainDraw<'_>,
    ) {
        let slot = match list {
            TerrainList::Solid => &mut self.terrain,
            TerrainList::Translucent => &mut self.water,
            TerrainList::ShadowSolid => &mut self.shadow_terrain,
            TerrainList::ShadowTranslucent => &mut self.shadow_water,
        };
        let Some(mut pass) = slot.take() else {
            return;
        };
        let translucent = list.translucent();
        let group = self.bind(device, queue, encoder, &mut pass, size);
        {
            let mut rp = self.begin(encoder, &pass);
            rp.set_pipeline(&pass.pipeline);
            rp.set_bind_group(0, &group, &[]);
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
                if let Some((_, _, group)) = groups.get(&page).filter(|_| ids) {
                    rp.set_bind_group(3, group, &[]);
                }
            };
            draw(&mut rp, list, &page);
        }
        if !pass.group.shadow() {
            self.mips_valid &= !pass.written;
        }
        *match list {
            TerrainList::Solid => &mut self.terrain,
            TerrainList::Translucent => &mut self.water,
            TerrainList::ShadowSolid => &mut self.shadow_terrain,
            TerrainList::ShadowTranslucent => &mut self.shadow_water,
        } = Some(pass);
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
                rp.set_pipeline(&pass.pipeline);
                rp.set_bind_group(0, group, &[]);
                sky.geometry.bind(rp, part.textured);
                open.as_mut().expect("open").1 = i;
            }
            let rp = &mut open.as_mut().expect("open").0;
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
            {
                let mut rp = self.begin(encoder, pass);
                rp.set_pipeline(&pass.pipeline);
                rp.set_bind_group(0, &group, &[]);
                rp.set_bind_group(1, &geometry.table_group, &[]);
                rp.set_vertex_buffer(0, geometry.vertices());
                rp.set_index_buffer(geometry.indices.slice(..), wgpu::IndexFormat::Uint32);
                for d in draws {
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
            let mut rp = self.begin(encoder, pass);
            rp.set_pipeline(&pass.pipeline);
            rp.set_bind_group(0, &group, &[]);
            rp.set_bind_group(1, &geometry.table_group, &[]);
            rp.set_vertex_buffer(0, geometry.vertices());
            rp.set_index_buffer(geometry.indices.slice(..), wgpu::IndexFormat::Uint32);
            for d in draws {
                if pass.draw {
                    rp.set_bind_group(2, &geometry.group, &[geometry.offset(d.slot)]);
                }
                rp.draw_indexed(0..d.quads * 6, d.base as i32, 0..1);
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
        rest: &mut Vec<super::Vertex>,
    ) {
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
            shadow,
            rest,
        );
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
        self.draw_clouds(device, queue, encoder, size);
        self.draw_hand(device, queue, encoder, size, frame);
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
        let projection = super::pack_hand::projection(
            frame.hand_fov_degrees,
            size[0] as f32 / size[1].max(1) as f32,
            frame.depth_far,
        );
        geometry.prepare(
            device,
            queue,
            frame.hand,
            frame.hand_spans,
            &self.draws.layout,
            projection,
        );
        if geometry.quads > 0 {
            let group = self.bind(device, queue, encoder, &mut pass, size);
            {
                let mut rp = self.begin(encoder, &pass);
                rp.set_pipeline(&pass.pipeline);
                rp.set_bind_group(0, &group, &[]);
                rp.set_bind_group(1, &geometry.group, &[]);

                rp.set_vertex_buffer(0, geometry.vertices());
                rp.set_index_buffer(geometry.indices.slice(..), wgpu::IndexFormat::Uint32);
                if pass.draw {
                    for draw in &geometry.draws {
                        rp.set_bind_group(
                            2,
                            geometry.draw_group.as_ref().expect("hand draw metadata"),
                            &[draw.offset],
                        );
                        rp.draw_indexed(draw.indices.clone(), 0, 0..1);
                    }
                } else {
                    rp.draw_indexed(0..geometry.quads * 6, 0, 0..1);
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
        let (Some(mut pass), Some(clouds)) = (self.clouds.take(), self.cloud_draw.take()) else {
            return;
        };
        let group = self.bind(device, queue, encoder, &mut pass, size);
        {
            let mut rp = self.begin(encoder, &pass);
            rp.set_pipeline(&pass.pipeline);
            rp.set_bind_group(0, &group, &[]);
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
        if let Some(t) = &self.targets
            && let Some((copy, _)) = &t.depth1
        {
            encoder.copy_texture_to_texture(
                t.depth_texture.as_image_copy(),
                copy.as_image_copy(),
                t.depth_texture.size(),
            );
        }
        self.draw_terrain(
            device,
            queue,
            encoder,
            size,
            TerrainList::Translucent,
            terrain,
        );
        self.draw_entities(device, queue, encoder, size, true);
        self.draw_weather(device, queue, encoder, size);
    }

    /// The rain, then the snow, `set_weather` gave for this frame, with `gbuffers_weather`.
    fn draw_weather(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        size: [u32; 2],
    ) {
        let (Some(mut passes), Some(weather)) = (self.weather.take(), self.weather_draw.take())
        else {
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
                rp.set_pipeline(&pass.pipeline);
                rp.set_bind_group(0, &group, &[]);
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
    fn mipmap(&self, encoder: &mut wgpu::CommandEncoder, b: usize) {
        let (Some(targets), Some((pipeline, _))) = (&self.targets, self.down.get(&self.formats[b]))
        else {
            return;
        };
        let Some(mips) = &targets.mips[b] else { return };
        let side = &mips[usize::from(self.main & 1 << b != 0)];
        for (attachment, group) in &side.levels {
            let mut rp = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Shader pack mipmap"),
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
        self.shadow_targets.as_ref()?;
        if self.shadow_terrain.is_none() && self.shadow_water.is_none() {
            return None;
        }
        let shadow = &self.uniforms.shadow;
        Some(ShadowView {
            cull: pack_uniforms::shadow_culling(frame, shadow),
            light: pack_uniforms::shadow_light(frame, shadow),
            distance: (shadow.render_mul > 0.0).then_some(shadow.distance * shadow.render_mul),
        })
    }

    /// Whether the pack draws terrain itself.
    pub fn has_terrain(&self) -> bool {
        self.terrain.is_some()
    }

    /// Whether the pack draws the sky (disc, sunset, stars, void) itself.
    pub fn has_sky(&self) -> bool {
        self.sky.as_ref().is_some_and(|s| s.basic.is_some())
    }

    /// Whether the pack draws the sun and moon itself.
    pub fn has_celestial(&self) -> bool {
        self.sky.as_ref().is_some_and(|s| s.textured.is_some())
    }

    /// Whether the pack draws translucent terrain itself.
    pub fn has_water(&self) -> bool {
        self.water.is_some()
    }

    /// The names of the programs that run, in order.
    pub fn programs(&self) -> Vec<&str> {
        let mut names: Vec<&str> = self.passes.iter().map(|p| p.name.as_str()).collect();
        for pass in [
            &self.terrain,
            &self.water,
            &self.clouds,
            &self.shadow_terrain,
            &self.shadow_water,
        ]
        .into_iter()
        .flatten()
        {
            names.push(&pass.name);
        }
        if let Some(sky) = &self.sky {
            for pass in [&sky.basic, &sky.textured].into_iter().flatten() {
                names.push(&pass.name);
            }
        }
        if let Some(entities) = &self.entities {
            for pass in entities.passes.iter().chain(&entities.shadow).flatten() {
                names.push(&pass.name);
            }
        }
        if let Some(weather) = &self.weather {
            names.push(&weather[0].name);
        }
        if let Some((hand, _)) = &self.hand {
            names.push(&hand.name);
        }
        names.extend(self.shadow_comp.iter().map(|p| p.name.as_str()));
        names.extend(self.compute.iter().map(|p| p.bindings.name.as_str()));
        names
    }

    pub(super) fn resize(&mut self, device: &wgpu::Device, size: [u32; 2]) {
        if self.targets.as_ref().is_some_and(|t| t.size == size) {
            return;
        }
        if let Err(error) = checked(device, || self.resize_targets(device, size)) {
            self.complete = false;
            self.notes
                .push(format!("Shader targets could not resize: {error}"));
        }
    }
    fn resize_targets(&mut self, device: &wgpu::Device, size: [u32; 2]) {
        if self.targets.as_ref().is_some_and(|t| t.size == size) {
            return;
        }
        self.compute_setup = false;
        match checked(device, || self.images.resize(device, size)).and_then(|v| v) {
            Ok(_) => {}
            Err(error) => {
                self.complete = false;
                self.notes.push(error);
                return;
            }
        }
        let full_chain = 32 - size[0].max(size[1]).max(1).leading_zeros();
        let texture_levels = |label: &str, format, usage, levels| {
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
        let texture = |label: &str, format, usage| texture_levels(label, format, usage, 1);
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
                let levels = if mipped { full_chain } else { 1 };
                let attachment = wgpu::TextureUsages::RENDER_ATTACHMENT;
                let sides =
                    [0, 1].map(|_| texture_levels(&label, self.formats[i], attachment, levels));
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
        self.targets = Some(Targets {
            size,
            color,
            depth: depth_texture.create_view(&Default::default()),
            depth_texture,
            final_color: None,
            depth1,
            mips,
        });
        let gbuffers = self.terrain.iter_mut().chain(&mut self.water);
        for pass in self
            .passes
            .iter_mut()
            .chain(gbuffers)
            .chain(&mut self.clouds)
            .chain(&mut self.shadow_terrain)
            .chain(&mut self.shadow_water)
            .chain(&mut self.shadow_comp)
        {
            pass.groups.clear();
        }
        if let Some(sky) = &mut self.sky {
            for pass in sky.basic.iter_mut().chain(&mut sky.textured) {
                pass.groups.clear();
            }
        }
        if let Some(entities) = &mut self.entities {
            for pass in entities
                .passes
                .iter_mut()
                .chain(&mut entities.shadow)
                .flatten()
            {
                pass.groups.clear();
            }
        }
        if let Some(weather) = &mut self.weather {
            for pass in weather {
                pass.groups.clear();
            }
        }
        if let Some((hand, _)) = &mut self.hand {
            hand.groups.clear();
        }
        for pass in &mut self.compute {
            pass.bindings.groups.clear();
        }
    }

    fn run_compute(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        group: horizon_shaders::ComputeGroup,
        before: Option<&str>,
        size: [u32; 2],
    ) {
        let ordinal = |name: &str| {
            name.trim_start_matches(|c: char| !c.is_ascii_digit())
                .parse::<u32>()
                .unwrap_or(0)
        };
        let mut compute = std::mem::take(&mut self.compute);
        for pass in &mut compute {
            if pass.ran
                || pass.group != group
                || before.is_some_and(|name| ordinal(&pass.base) > ordinal(name))
            {
                continue;
            }
            let Some(groups) = pass.dispatch.groups(size) else {
                log::error!("shader compute {}: dispatch overflow", pass.bindings.name);
                continue;
            };
            if groups
                .iter()
                .any(|&n| n > device.limits().max_compute_workgroups_per_dimension)
            {
                log::error!(
                    "shader compute {}: dispatch exceeds device limits",
                    pass.bindings.name
                );
                continue;
            }
            let bindings = self.bind(device, queue, encoder, &mut pass.bindings, size);
            let mut compute_pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some(&pass.bindings.name),
                timestamp_writes: None,
            });
            compute_pass.set_pipeline(&pass.pipeline);
            compute_pass.set_bind_group(0, &bindings, &[]);
            compute_pass.dispatch_workgroups(groups[0], groups[1], groups[2]);
            pass.ran = true;
        }
        self.compute = compute;
    }

    /// The shadow pass (2.1, passes 2 and 3): the shadow buffers cleared, then the `shadowcomp`
    /// programs, each drawing into the "alt" side of what it writes and flipping it after.
    fn run_shadow(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        terrain: &mut TerrainDraw<'_>,
    ) {
        let Some(t) = &mut self.shadow_targets else {
            self.run_compute(
                device,
                queue,
                encoder,
                horizon_shaders::ComputeGroup::ShadowComp,
                None,
                self.targets.as_ref().map_or([1, 1], |t| t.size),
            );
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
            self.run_compute(
                device,
                queue,
                encoder,
                horizon_shaders::ComputeGroup::ShadowComp,
                Some(&pass.name),
                self.targets.as_ref().map_or(size, |t| t.size),
            );
            let group = self.bind(device, queue, encoder, pass, size);
            let t = self.shadow_targets.as_ref().expect("shadow pass");
            let mut attachments: Vec<Option<wgpu::RenderPassColorAttachment>> = Vec::new();
            for &(location, buffer) in &pass.writes {
                let l = location as usize;
                if attachments.len() <= l {
                    attachments.resize(l + 1, None);
                }
                // Overwritten whole.
                let ops = wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
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
                rp.set_pipeline(&pass.pipeline);
                rp.set_bind_group(0, &group, &[]);
                rp.draw(0..6, 0..1);
            }
            if let Some(t) = &mut self.shadow_targets {
                t.main ^= pass.flip;
            }
        }
        self.run_compute(
            device,
            queue,
            encoder,
            horizon_shaders::ComputeGroup::ShadowComp,
            None,
            self.targets.as_ref().map_or(size, |t| t.size),
        );
        self.shadow_comp = comp;
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
        pages: &[(u32, wgpu::Buffer, wgpu::Buffer)],
        terrain: &mut TerrainDraw<'_>,
    ) {
        self.resize(device, size);
        if !self.complete {
            return;
        }
        if let Some(targets) = &mut self.targets {
            if size == window {
                targets.final_color = None;
            } else if targets.final_color.is_none() {
                targets.final_color = Some(
                    device
                        .create_texture(&wgpu::TextureDescriptor {
                            label: Some("Shader final at render resolution"),
                            size: wgpu::Extent3d {
                                width: size[0],
                                height: size[1],
                                depth_or_array_layers: 1,
                            },
                            mip_level_count: 1,
                            sample_count: 1,
                            dimension: wgpu::TextureDimension::D2,
                            format: self.screen_format,
                            usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                                | wgpu::TextureUsages::TEXTURE_BINDING,
                            view_formats: &[],
                        })
                        .create_view(&Default::default()),
                );
            }
        }
        if let Some((layout, ids)) = &self.block_ids {
            // A page that grew has a new buffer.
            self.page_groups
                .retain(|p, (b, m, _)| pages.iter().any(|(q, c, n)| q == p && c == b && n == m));
            for (page, buffer, metadata) in pages {
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
                                resource: metadata.as_entire_binding(),
                            },
                        ],
                    });
                    (buffer.clone(), metadata.clone(), group)
                });
            }
        }
        if let Some(t) = &mut self.timing {
            t.begin(device, queue);
        }
        self.uniforms.update(frame, inputs, size);
        self.uniforms.eval_custom(&mut self.custom);
        if self.targets.is_none() {
            return;
        }
        self.images.clear(encoder);
        for pass in &mut self.compute {
            pass.ran = false;
        }
        if !self.compute_setup {
            self.run_compute(
                device,
                queue,
                encoder,
                horizon_shaders::ComputeGroup::Setup,
                None,
                size,
            );
            self.compute_setup = true;
        }
        self.run_compute(
            device,
            queue,
            encoder,
            horizon_shaders::ComputeGroup::Begin,
            None,
            size,
        );
        self.run_shadow(device, queue, encoder, terrain);
        let Some(targets) = &self.targets else { return };
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
        for chunk in self.other_clears.chunks(8) {
            let attachments: Vec<_> = chunk
                .iter()
                .map(|&b| attach(view(self.main, b, false).expect("used buffer"), clear_op(b)))
                .collect();
            encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Shader pack clear"),
                color_attachments: &attachments,
                timestamp_writes: self.timing.as_mut().and_then(|t| t.writes("clear")),
                ..Default::default()
            });
        }

        // The world into colortex0 and depthtex0, flipped to GL's rows, and the clears.
        if self.blit_group.as_ref().is_none_or(|g| g.0 != size) {
            let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("Shader pack world"),
                layout: &self.blit.1,
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
            self.blit_group = Some((size, group));
        }
        {
            let mut attachments = vec![attach(
                view(self.main, 0, false).expect("colortex0"),
                overwrite,
            )];
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
            pass.set_pipeline(&self.blit.0);
            pass.set_bind_group(0, self.blit_group.as_ref().map(|g| &g.1), &[]);
            pass.draw(0..3, 0..1);
        }

        // The world and the clears just rewrote the buffers' first levels.
        self.mips_valid = 0;
        let mut passes = std::mem::take(&mut self.passes);
        let mut seen = (false, false);
        let mut shown = false;
        let mut terrain_drawn = self.terrain.is_none()
            && self.sky.is_none()
            && !self.entities.as_ref().is_some_and(Entities::drawn)
            && self.clouds.is_none()
            && self.hand.is_none();
        let late_entities = self.entities.as_ref().is_some_and(|e| {
            e.passes
                .iter()
                .zip(e.late)
                .any(|(p, late)| p.is_some() && late)
        });
        let mut water_drawn = self.water.is_none() && self.weather.is_none() && !late_entities;
        for pass in &mut passes {
            use horizon_shaders::ComputeGroup as C;
            if pass.group != Group::Prepare {
                self.run_compute(device, queue, encoder, C::Prepare, None, size);
                if !terrain_drawn {
                    terrain_drawn = true;
                    self.draw_opaque(device, queue, encoder, size, frame, terrain);
                }
            }
            if matches!(pass.group, Group::Composite | Group::Final) {
                if !seen.0 {
                    seen.0 = true;
                    self.main ^= self.deferred_pre;
                }
                self.run_compute(device, queue, encoder, C::Deferred, None, size);
                if !water_drawn {
                    water_drawn = true;
                    self.draw_translucent(device, queue, encoder, size, terrain);
                }
            }
            if pass.group == Group::Final {
                if !seen.1 {
                    seen.1 = true;
                    self.main ^= self.composite_pre;
                }
                self.run_compute(device, queue, encoder, C::Composite, None, size);
            }
            if pass.group == Group::Deferred && !seen.0 {
                seen.0 = true;
                self.main ^= self.deferred_pre;
            }
            if pass.group == Group::Composite && !seen.1 {
                seen.1 = true;
                self.main ^= self.composite_pre;
            }
            let compute_group = match pass.group {
                Group::Prepare => C::Prepare,
                Group::Deferred => C::Deferred,
                Group::Composite => C::Composite,
                Group::Final => C::Final,
                _ => unreachable!(),
            };
            self.run_compute(
                device,
                queue,
                encoder,
                compute_group,
                Some(&pass.name),
                size,
            );
            let stale = pass.mipmaps & self.mipped & !self.mips_valid;
            for b in (0..BUFFERS).filter(|b| stale & 1 << b != 0) {
                self.mipmap(encoder, b);
            }
            self.mips_valid |= stale;
            let final_pass = pass.group == Group::Final;
            let group = self.bind(device, queue, encoder, pass, size);
            let targets = self.targets.as_ref().expect("resized");
            let view = |i: usize| {
                targets.color[i]
                    .as_ref()
                    .map(|t| &t[usize::from(self.main & 1 << i == 0)])
            };
            let mut attachments: Vec<Option<wgpu::RenderPassColorAttachment>> = Vec::new();
            if final_pass {
                attachments.push(attach(
                    targets.final_color.as_ref().unwrap_or(screen),
                    overwrite,
                ));
                shown = true;
            } else {
                for &(location, buffer) in &pass.writes {
                    let l = location as usize;
                    if attachments.len() <= l {
                        attachments.resize(l + 1, None);
                    }
                    attachments[l] = view(buffer).and_then(|v| attach(v, overwrite));
                }
            }
            {
                let mut rp = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some(&pass.name),
                    color_attachments: &attachments,
                    timestamp_writes: self.timing.as_mut().and_then(|t| t.writes(&pass.name)),
                    ..Default::default()
                });
                rp.set_pipeline(&pass.pipeline);
                rp.set_bind_group(0, &group, &[]);
                rp.draw(0..6, 0..1);
            }
            self.main ^= pass.flip;
            self.mips_valid &= !pass.written;
        }
        self.passes = passes;
        use horizon_shaders::ComputeGroup as C;
        self.run_compute(device, queue, encoder, C::Prepare, None, size);
        if !terrain_drawn {
            self.draw_opaque(device, queue, encoder, size, frame, terrain);
        }
        if !seen.0 {
            self.main ^= self.deferred_pre;
        }
        self.run_compute(device, queue, encoder, C::Deferred, None, size);
        if !water_drawn {
            self.draw_translucent(device, queue, encoder, size, terrain);
        }
        if !seen.1 {
            self.main ^= self.composite_pre;
        }
        self.run_compute(device, queue, encoder, C::Composite, None, size);
        self.run_compute(device, queue, encoder, C::Final, None, size);
        let targets = self.targets.as_ref().expect("resized");
        let view = |m: u16, i: usize, alt: bool| {
            targets.color[i]
                .as_ref()
                .map(|t| &t[main(m, i) ^ usize::from(alt)])
        };
        if !shown || size != window {
            let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("Shader pack colortex0"),
                layout: if shown { &self.present.1 } else { &self.copy.1 },
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(if shown {
                            targets.final_color.as_ref().expect("scaled final")
                        } else {
                            view(self.main, 0, false).expect("colortex0")
                        }),
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
            rp.set_pipeline(if shown { &self.present.0 } else { &self.copy.0 });
            rp.set_bind_group(0, &group, &[]);
            rp.draw(0..3, 0..1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wgpu::TextureFormat as T;

    /// Pipeline construction against private real-pack inputs. This classifies
    /// unsupported inputs; it does not claim a visual or iOS runtime pass.
    #[test]
    #[ignore = "requires HORIZON_TEST_SHADER_PACKS with private archives"]
    fn real_packs_build_or_report_unsupported_inputs_on_metal() {
        use horizon_shaders::{GameInputs, Macros, Pack, Properties, dimension_folder, load_post};
        let directory =
            std::path::PathBuf::from(std::env::var_os("HORIZON_TEST_SHADER_PACKS").unwrap());
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::METAL,
            ..wgpu::InstanceDescriptor::new_without_display_handle()
        });
        let adapter = pollster::block_on(instance.request_adapter(&Default::default()))
            .expect("Metal adapter");
        let features = adapter.features()
            & (wgpu::Features::TEXTURE_FORMAT_16BIT_NORM
                | wgpu::Features::RG11B10UFLOAT_RENDERABLE
                | wgpu::Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES
                | wgpu::Features::VERTEX_WRITABLE_STORAGE
                | wgpu::Features::CLEAR_TEXTURE);
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            required_features: features,
            required_limits: adapter.limits(),
            ..Default::default()
        }))
        .unwrap();
        let array = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("compatibility atlas"),
            size: wgpu::Extent3d {
                width: 2,
                height: 2,
                depth_or_array_layers: 2,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: T::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::COPY_SRC
                | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let mut encoder = device.create_command_encoder(&Default::default());
        let atlas =
            super::super::pack_atlas::PackAtlas::new(&device, &mut encoder, &[array], 2, 2, 2)
                .unwrap();
        queue.submit([encoder.finish()]);
        let inputs = GameInputs {
            terrain: Some(atlas.terrain_inputs(false)),
            sky: Some(super::super::pack_sky::inputs()),
            celestial: Some(super::super::pack_sky::celestial_inputs()),
            entities: Some(super::super::pack_entities::inputs()),
            clouds: Some(super::super::pack_entities::cloud_inputs()),
            weather: Some(super::super::pack_weather::inputs()),
            hand: Some(super::super::pack_hand::inputs()),
        };
        let instance_attributes = wgpu::vertex_attr_array![5=>Float32x4];
        let terrain_buffers = [
            Some(wgpu::VertexBufferLayout {
                array_stride: std::mem::size_of::<super::super::TerrainVertex>() as u64,
                step_mode: wgpu::VertexStepMode::Vertex,
                attributes: &super::super::TERRAIN_ATTRIBUTES,
            }),
            Some(wgpu::VertexBufferLayout {
                array_stride: 16,
                step_mode: wgpu::VertexStepMode::Instance,
                attributes: &instance_attributes,
            }),
        ];
        let cloud_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: None,
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let mut reports = Vec::new();
        for found in horizon_shaders::discover(&directory)
            .into_iter()
            .filter(|p| p.problem.is_none())
        {
            let pack = Pack::open(&found.path).unwrap();
            let mut macros = Macros::standard();
            macros.undef("MC_OS_MAC");
            macros.define("MC_OS_OTHER", "");
            let properties = Properties::from_pack(&pack, "", &macros);
            for dimension in [0, -1, 1] {
                let post = load_post(
                    &pack,
                    &dimension_folder(&pack, dimension),
                    &properties,
                    &macros,
                    &inputs,
                );
                let parsed = post.complete;
                let world = World {
                    adapter: &adapter,
                    atlas: Some((atlas.view.clone(), atlas.size())),
                    lightmap: texture(&device, &queue, "test lightmap", 1, &[255; 4]),
                    terrain: Some(Geometry {
                        buffers: &terrain_buffers,
                        table: None,
                        depth_write: true,
                        atlas: None,
                        ids: None,
                        cull: true,
                    }),
                    block_ids: &[-1, -1, -1, -1],
                    celestial: Some(atlas.view.clone()),
                    clouds: Some(&cloud_layout),
                    weather: true,
                    hand: true,
                };
                let mut result = PackPost::new(&device, &queue, post, T::Rgba8Unorm, world);
                if result.complete && !result.compute.is_empty() {
                    result.resize(&device, [16, 16]);
                    checked(&device, || {
                        let mut encoder = device.create_command_encoder(&Default::default());
                        result.images.clear(&mut encoder);
                        result.run_compute(
                            &device,
                            &queue,
                            &mut encoder,
                            horizon_shaders::ComputeGroup::ShadowComp,
                            None,
                            [16, 16],
                        );
                        queue.submit([encoder.finish()]);
                        device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
                    })
                    .expect("real pack compute must execute without resource hazards");
                    assert!(
                        result
                            .compute
                            .iter()
                            .filter(|p| p.group == horizon_shaders::ComputeGroup::ShadowComp)
                            .all(|p| p.ran)
                    );
                }
                let programs = result.programs();
                println!(
                    "{} dimension {}: parsed={} pipelines_complete={} programs={} notes={:?}",
                    found.name,
                    dimension,
                    parsed,
                    result.complete,
                    programs.len(),
                    result.notes
                );
                reports.push(serde_json::json!({"pack":found.name,"dimension":dimension,"parsed":parsed,"complete":result.complete,"programs":programs,"notes":result.notes}));
            }
        }
        assert!(!reports.is_empty());
        if std::env::var_os("HORIZON_TEST_SHADER_REQUIRE_COMPLETE").is_some() {
            assert!(
                reports
                    .iter()
                    .all(|r| r["parsed"] == true && r["complete"] == true),
                "Real shader pack stage/pipeline failed; inspect report: {reports:?}"
            );
        }
        if let Some(out) = std::env::var_os("HORIZON_SHADER_TEST_REPORT") {
            std::fs::write(out, serde_json::to_vec_pretty(&reports).unwrap()).unwrap();
        }
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
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
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
    fn sampler_names() {
        let main = |n| source(n, false, false, false);
        assert!(main("gaux1") == Source::Color(4));
        assert!(main("gdepthtex") == Source::Depth);
        assert!(main("depthtex2") == Source::Depth1);
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
#[path = "pack_storage_tests.rs"]
mod pack_storage_tests;

#[cfg(test)]
#[path = "pack_frame_tests.rs"]
mod pack_frame_tests;

#[cfg(test)]
#[path = "pack_terrain_tests.rs"]
mod pack_terrain_tests;
