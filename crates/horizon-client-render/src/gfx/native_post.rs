//! The post stage of Horizon's own shader packs (docs/shaders.md): each `[[post]]` pass is a
//! full-screen pipeline built from its checked naga module (`horizon_shader_api::check`), with
//! the pack's options as pipeline constants. A pass reads bind group 3: the frame, the world's
//! colour and depth, the previous pass's output and two samplers. Passes before the last write
//! half-float targets of the world's size, so values over 1 carry to the next; the last draws
//! onto the screen. Every stage of a pack reads the same `frame`, written once a frame
//! (`NativeFrame`).

use glam::{Mat4, Vec3};
use horizon_shader_api::{
    Api, Frame as FrameInputs, StageKind, check::CheckedStage, stages::GROUP,
};

use super::Frame;

pub(super) const INTERMEDIATE: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;

/// What the renderer's own entries call where a stage's colours cross into and out of it: as
/// they are under 0.1; under 0.2 vanilla's 2.2 power, as the prelude's `horizon_decode` and
/// `horizon_encode` (the shadow pass has no prelude, so these stand alone).
pub fn line_helpers(api: Api) -> &'static str {
    match api {
        Api::V0_1 => {
            "fn horizon_line_in(c: vec3<f32>) -> vec3<f32> { return c; }
fn horizon_line_out(c: vec3<f32>) -> vec3<f32> { return c; }
"
        }
        Api::V0_2 => {
            "fn horizon_line_in(c: vec3<f32>) -> vec3<f32> {
    return pow(select(vec3<f32>(0.0), c, c > vec3<f32>(0.0)), vec3<f32>(2.2));
}
fn horizon_line_out(c: vec3<f32>) -> vec3<f32> {
    return pow(select(vec3<f32>(0.0), c, c > vec3<f32>(0.0)), vec3<f32>(1.0 / 2.2));
}
"
        }
    }
}

/// 0.2's decoding copy: a picture in the frame's gamma into linear light in `INTERMEDIATE`, of
/// the same size, alpha 1 (`scene_color`, `opaque_color`, the mirror).
#[derive(Clone)]
pub struct Decoder(wgpu::RenderPipeline);

impl Decoder {
    fn new(device: &wgpu::Device) -> Self {
        let source = line_helpers(Api::V0_2).to_owned()
            + "@group(0) @binding(0) var horizon_from: texture_2d<f32>;
@vertex fn horizon_vs(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
    return vec4<f32>(vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u)) * 2.0 - 1.0, 0.0, 1.0);
}
@fragment fn horizon_fs(@builtin(position) p: vec4<f32>) -> @location(0) vec4<f32> {
    return vec4<f32>(horizon_line_in(textureLoad(horizon_from, vec2<i32>(p.xy), 0).rgb), 1.0);
}
";
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Shader API 0.2 decode"),
            source: wgpu::ShaderSource::Wgsl(source.into()),
        });
        let entries = (None, "horizon_vs", "horizon_fs");
        Self(fullscreen_pipeline(
            device,
            "Shader API 0.2 decode",
            &module,
            entries,
            INTERMEDIATE,
            &[],
        ))
    }

    /// Draws `from` decoded onto `to`.
    pub fn run(
        &self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        from: &wgpu::TextureView,
        to: &wgpu::TextureView,
    ) {
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Shader API 0.2 decode"),
            layout: &self.0.get_bind_group_layout(0),
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(from),
            }],
        });
        fullscreen_pass(
            encoder,
            "Shader API 0.2 decode",
            to,
            &self.0,
            (0, &group),
            None,
        );
    }
}

/// A full-screen pipeline `label` of `module`'s entries `(vertex, fragment)`, unblended.
fn fullscreen_pipeline(
    device: &wgpu::Device,
    label: &str,
    module: &wgpu::ShaderModule,
    (layout, vertex, fragment): (Option<&wgpu::PipelineLayout>, &str, &str),
    format: wgpu::TextureFormat,
    constants: &[(&str, f64)],
) -> wgpu::RenderPipeline {
    let options = wgpu::PipelineCompilationOptions {
        constants,
        ..Default::default()
    };
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(label),
        layout,
        vertex: wgpu::VertexState {
            module,
            entry_point: Some(vertex),
            compilation_options: options.clone(),
            buffers: &[],
        },
        primitive: Default::default(),
        depth_stencil: None,
        multisample: Default::default(),
        fragment: Some(wgpu::FragmentState {
            module,
            entry_point: Some(fragment),
            compilation_options: options,
            targets: &[Some(wgpu::ColorTargetState {
                format,
                blend: None,
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        multiview_mask: None,
        cache: None,
    })
}

/// One full-screen triangle of `pipeline` onto `view`, cleared first.
fn fullscreen_pass(
    encoder: &mut wgpu::CommandEncoder,
    label: &str,
    view: &wgpu::TextureView,
    pipeline: &wgpu::RenderPipeline,
    (index, group): (u32, &wgpu::BindGroup),
    timing: Option<&mut super::pack_timing::PassTiming>,
) {
    let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some(label),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view,
            resolve_target: None,
            depth_slice: None,
            ops: wgpu::Operations {
                load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                store: wgpu::StoreOp::Store,
            },
        })],
        timestamp_writes: timing.and_then(|t| t.writes(label)),
        ..Default::default()
    });
    pass.set_pipeline(pipeline);
    pass.set_bind_group(index, group, &[]);
    pass.draw(0..3, 0..1);
}

/// The tonemap stage's entries as the renderer draws them: the check's, with the result
/// encoded into the frame's gamma, where the camera overlays and the HUD draw on it. A pack's
/// `[tonemap]` is held to `frame.headroom`. Horizon's own (`default`) is a clip at white in
/// SDR; in HDR it leaves the light as it is for Horizon's tone curve (`Hdr::tonemap`), the
/// one HDR without a pack uses.
fn tonemap_entries(default: bool) -> String {
    let held = if default {
        "select(min(horizon_sanitize(out), vec3<f32>(frame.headroom)), horizon_sanitize(out), frame.hdr == 1u)"
    } else {
        "min(horizon_sanitize(out), vec3<f32>(frame.headroom))"
    };
    StageKind::Tonemap.entries(Api::V0_2).replace(
        "return vec4<f32>(min(horizon_sanitize(out), vec3<f32>(frame.headroom)), 1.0);",
        &format!("return vec4<f32>(horizon_encode({held}), 1.0);"),
    )
}

/// The tonemap of a 0.2 pack without `[tonemap]`: the picture as it is, held as
/// `tonemap_entries` holds Horizon's.
const DEFAULT_TONEMAP: &str =
    "fn tonemap(input: TonemapInput) -> vec3<f32> { return input.color; }\n";

/// A colour target of `INTERMEDIATE` and `size`, drawn and read.
fn intermediate(device: &wgpu::Device, label: &str, size: [u32; 2]) -> wgpu::TextureView {
    device
        .create_texture(&wgpu::TextureDescriptor {
            label: Some(label),
            size: wgpu::Extent3d {
                width: size[0].max(1),
                height: size[1].max(1),
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: INTERMEDIATE,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        })
        .create_view(&Default::default())
}

/// A 0.2 pack's display (docs/api/shaders-reference-0.2.md): the world decoded into
/// `scene_color` (the first pass's `previous`), the chain ending in `picture`, and the tonemap
/// stage drawing that onto the screen.
struct Display {
    decoder: Decoder,
    /// The pack's `[tonemap]` label, `None` for Horizon's.
    label: Option<String>,
    tonemap: wgpu::RenderPipeline,
    /// `scene_color` at the world's size; `picture` at the window's (none without a chain:
    /// the tonemap reads `scene_color`).
    scene: Option<([u32; 2], wgpu::TextureView)>,
    picture: Option<([u32; 2], wgpu::TextureView)>,
}

/// The world's colour and depth views a set of bind groups was made for.
type WorldViews = (wgpu::TextureView, wgpu::TextureView, wgpu::TextureView);

/// Where a post pass draws.
#[derive(Clone, Copy, PartialEq)]
enum Writes {
    /// One of the chain's two targets.
    Chain(usize),
    /// A pack target (`[[post]] output`).
    Target(usize),
    /// The screen: the last pass.
    Screen,
}

/// A post pass's target, and the chain's picture it reads as `previous` (the world's for
/// none).
#[derive(Clone, Copy)]
struct PassPlan {
    writes: Writes,
    reads: Option<usize>,
}

/// The passes' plan for `outputs` (each pass's `[[post]] output`, by target index): the chain's
/// passes write its targets in turn, each reading what the one before wrote; a pass with an
/// output writes that and reads the chain's picture, which goes on past it.
fn plan(outputs: &[Option<usize>]) -> Vec<PassPlan> {
    let mut current = None;
    outputs
        .iter()
        .enumerate()
        .map(|(i, output)| {
            let reads = current;
            let writes = match output {
                _ if i + 1 == outputs.len() => Writes::Screen,
                Some(t) => Writes::Target(*t),
                None => {
                    let slot = usize::from(current == Some(0));
                    current = Some(slot);
                    Writes::Chain(slot)
                }
            };
            PassPlan { writes, reads }
        })
        .collect()
}

/// A pack target's size: `scale` of the world's `size`, at least a texel.
fn scaled(size: [u32; 2], scale: f32) -> [u32; 2] {
    size.map(|s| ((s as f32 * scale).ceil() as u32).max(1))
}

pub struct NativePost {
    passes: Vec<(String, wgpu::RenderPipeline)>,
    layout: wgpu::BindGroupLayout,
    /// The pack's textures.
    textures: Vec<(wgpu::TextureView, wgpu::Sampler)>,
    frame: wgpu::Buffer,
    linear: wgpu::Sampler,
    nearest: wgpu::Sampler,
    /// Two targets of the world's size, written in turn (none with a single pass).
    targets: Option<([u32; 2], [wgpu::TextureView; 2])>,
    /// The frame's shadow map, its comparison sampler (bindings 6 and 7), the translucent map
    /// and its colour (bindings 9 and 10).
    shadow: (
        wgpu::TextureView,
        wgpu::Sampler,
        wgpu::TextureView,
        wgpu::TextureView,
    ),
    /// Last frame's input to the last pass (`history`, binding 8), made with the targets,
    /// and a black 1×1 in its place before then.
    history: Option<wgpu::Texture>,
    history_blank: wgpu::Texture,
    /// One bind group per pass, for these world views and either the valid history or black.
    groups: Option<(WorldViews, bool, Vec<wgpu::BindGroup>)>,
    /// Whether `history` contains the last frame's input to the last pass.
    history_valid: bool,
    /// What each pass writes and reads (`[[post]] output`).
    plan: Vec<PassPlan>,
    /// The pack's targets' scales (`Manifest::post_targets`), and their views for the world's
    /// size.
    scales: Vec<f32>,
    named: Option<([u32; 2], Vec<wgpu::TextureView>)>,
    /// The screen's colour format, the world target's too.
    format: wgpu::TextureFormat,
    /// Under 0.2: the decode, the picture and the tonemap.
    display: Option<Display>,
}

/// The `frame` every stage of a pack reads (group 3, binding 0), written once a frame, and
/// what the material stage reads besides.
pub struct NativeFrame {
    /// The solid terrain's second target, the post stage's `scene_surface`.
    surface: wgpu::Texture,
    pub surface_view: wgpu::TextureView,
    pub(super) buffer: wgpu::Buffer,
    /// Group 3 of the sky and material stages.
    pub layout: wgpu::BindGroupLayout,
    /// The pack's `[[textures]]` and their samplers, bound for every stage.
    pub textures: Vec<(wgpu::TextureView, wgpu::Sampler)>,
    /// The colour of block light around the camera (`[[lights]]`), bindings 9 and 10.
    pub block_light: super::block_light::BlockLight,
    pub group: wgpu::BindGroup,
    /// The world's depth before translucent terrain (the material stage's `opaque_depth`,
    /// binding 6), copied each frame.
    pub opaque_depth: wgpu::Texture,
    /// The world's colour before translucent terrain (`opaque_color`, binding 11), copied
    /// with it; 1×1 while the world draws straight to the window.
    pub opaque_color: wgpu::Texture,
    /// The resource packs' LabPBR maps (bindings 12 and 13: normals, specular), a blank 1x1
    /// without them.
    pub(super) pbr: [wgpu::TextureView; 2],
    /// The material stage's `reflection` (binding 1): this frame's water mirror, else
    /// `blank`; and its sampler (binding 2).
    reflection: wgpu::TextureView,
    blank: wgpu::TextureView,
    reflection_sampler: wgpu::Sampler,
    /// The shadow map the stages read, and the camera that draws it when the pack asks for one.
    pub shadow_map: super::native_shadow::ShadowMap,
    shadow_camera: Option<horizon_shaders::Shadow>,
    /// Whether the pack's material reads `reflection` (`NativePack::new`): else no mirror
    /// is drawn and `has_reflection` stays 0.
    pub reflects: bool,
    /// Last frame's view-projection, camera and time (`write`).
    last: Option<([[f32; 4]; 4], glam::DVec3, std::time::Instant)>,
    /// The eased eye light (block, sky) and being in water and lava (`write`); NaN until the
    /// first frame sets them.
    eye: [f32; 4],
    /// A post stage must use black history after a long gap until it has rendered a frame.
    history_reset: bool,
    start: std::time::Instant,
    frames: u32,
    /// `frame.wetness`, and when it was last written.
    wetness: (f32, Option<std::time::Instant>),
    /// The shader API line of the pack, and under 0.2 the decoding copy and the mirror it
    /// decoded (`linear_mirror`).
    pub api: Api,
    pub decoder: Option<Decoder>,
    mirror: Option<wgpu::TextureView>,
}

/// A depth texture of `size` the world's depth is copied into.
fn depth_copy(device: &wgpu::Device, size: [u32; 2]) -> wgpu::Texture {
    copy_target(
        device,
        "Native shader pack opaque depth",
        size,
        wgpu::TextureFormat::Depth32Float,
    )
}

/// A colour texture of `size` and `format` the world's colour is copied into.
fn color_copy(device: &wgpu::Device, size: [u32; 2], format: wgpu::TextureFormat) -> wgpu::Texture {
    copy_target(device, "Native shader pack opaque colour", size, format)
}

fn copy_target(
    device: &wgpu::Device,
    label: &str,
    size: [u32; 2],
    format: wgpu::TextureFormat,
) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d {
            width: size[0].max(1),
            height: size[1].max(1),
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        // RENDER_ATTACHMENT: 0.2 draws `opaque_color` decoded rather than copying it.
        usage: wgpu::TextureUsages::TEXTURE_BINDING
            | wgpu::TextureUsages::COPY_DST
            | wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    })
}

/// The uniform `frame` at binding 0, read by `visibility`.
fn frame_entry(visibility: wgpu::ShaderStages) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding: 0,
        visibility,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Uniform,
            has_dynamic_offset: false,
            min_binding_size: wgpu::BufferSize::new(size_of::<FrameInputs>() as u64),
        },
        count: None,
    }
}

/// The size of the water mirror for a world of `size`: half of it.
pub fn reflection_size(size: [u32; 2]) -> [u32; 2] {
    size.map(|s| s.div_ceil(2).max(1))
}

/// Seconds between frames past which the last one is forgotten (`NativeFrame::write`).
const MAX_FRAME_GAP: f32 = 0.5;

/// `value` moved toward `target` over `dt` seconds by a half-life in seconds; a NaN `value`
/// (the first frame) starts at `target`.
fn eased(value: f32, target: f32, half_life: f32, dt: f32) -> f32 {
    if value.is_nan() {
        target
    } else {
        target + (value - target) * 0.5f32.powf(dt / half_life)
    }
}

impl NativeFrame {
    /// The frame of a pack with the `[shadow]` settings `shadow`, if any, and the
    /// block light colours `lights`.
    pub fn new(
        device: &wgpu::Device,
        shadow: Option<&horizon_shader_api::manifest::Shadow>,
        textures: Vec<(wgpu::TextureView, wgpu::Sampler)>,
        lights: super::block_light::LightTable,
        pbr: Option<&[wgpu::Texture; 2]>,
        api: Api,
    ) -> Self {
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Native shader pack frame"),
            size: size_of::<FrameInputs>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let mut entries = vec![
            frame_entry(wgpu::ShaderStages::VERTEX_FRAGMENT),
            texture_entry(1, wgpu::TextureSampleType::Float { filterable: true }),
            sampler_entry(2),
            texture_entry(3, wgpu::TextureSampleType::Depth),
            comparison_entry(4),
            texture_entry(6, wgpu::TextureSampleType::Depth),
            texture_entry(7, wgpu::TextureSampleType::Depth),
            texture_entry(8, wgpu::TextureSampleType::Float { filterable: true }),
            texture_entry(11, wgpu::TextureSampleType::Float { filterable: false }),
            pbr_entry(12),
            pbr_entry(13),
        ];
        let block_light_entries = [
            wgpu::BindGroupLayoutEntry {
                binding: 9,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    view_dimension: wgpu::TextureViewDimension::D3,
                    multisampled: false,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 10,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            },
        ];
        entries.extend(block_light_entries);
        entries.extend(pack_texture_entries(textures.len()));
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Native shader pack frame"),
            entries: &entries,
        });
        let surface = surface_target(device, [1, 1]);
        let surface_view = surface.create_view(&Default::default());
        let opaque_depth = depth_copy(device, [1, 1]);
        let opaque_color = color_copy(device, [1, 1], wgpu::TextureFormat::Rgba8Unorm);
        let blank = copy_target(
            device,
            "Native shader pack reflection blank",
            [1, 1],
            wgpu::TextureFormat::Rgba8Unorm,
        )
        .create_view(&Default::default());
        let reflection_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("Native shader pack reflection sampler"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let shadow_map = super::native_shadow::ShadowMap::new(device, shadow, api);
        let block_light = super::block_light::BlockLight::new(device, lights);
        let array_view = |texture: &wgpu::Texture| {
            texture.create_view(&wgpu::TextureViewDescriptor {
                dimension: Some(wgpu::TextureViewDimension::D2Array),
                ..Default::default()
            })
        };
        let pbr = match pbr {
            Some(maps) => maps.each_ref().map(array_view),
            None => {
                // Two layers: one would make a 2D texture on the OpenGL backend.
                let blank = device.create_texture(&wgpu::TextureDescriptor {
                    label: Some("Native shader pack LabPBR (none)"),
                    size: wgpu::Extent3d {
                        width: 1,
                        height: 1,
                        depth_or_array_layers: 2,
                    },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: wgpu::TextureFormat::Rgba8Unorm,
                    usage: wgpu::TextureUsages::TEXTURE_BINDING,
                    view_formats: &[],
                });
                [array_view(&blank), array_view(&blank)]
            }
        };
        let group = Self::group(
            device,
            &layout,
            &buffer,
            (&blank, &reflection_sampler),
            &shadow_map,
            (&opaque_depth, &opaque_color),
            &textures,
            &block_light,
            &pbr,
        );
        Self {
            textures,
            block_light,
            pbr,
            buffer,
            layout,
            group,
            surface,
            surface_view,
            opaque_depth,
            opaque_color,
            reflection: blank.clone(),
            blank,
            reflection_sampler,
            shadow_map,
            shadow_camera: shadow.map(super::native_shadow::camera),
            reflects: false,
            api,
            decoder: (api == Api::V0_2).then(|| Decoder::new(device)),
            mirror: None,
            last: None,
            eye: [f32::NAN; 4],
            history_reset: true,
            start: std::time::Instant::now(),
            frames: 0,
            wetness: (0.0, None),
        }
    }

    /// What the shadow camera sees this frame, for the renderer's shadow lists.
    pub fn shadow_view(&self, frame: &Frame) -> Option<super::pack_post::ShadowView> {
        let camera = self.shadow_camera.as_ref()?;
        Some(super::pack_post::ShadowView {
            cull: super::pack_uniforms::shadow_culling(frame, camera),
            light: super::pack_uniforms::shadow_light(frame, camera),
            // The light's box culls: on the ground it reaches further than `distance` along
            // the sun's azimuth while the sun is low.
            distance: None,
            unculled: None,
            every_face: false,
            view_casters: false,
            depth_planes: true,
        })
    }

    fn group(
        device: &wgpu::Device,
        layout: &wgpu::BindGroupLayout,
        buffer: &wgpu::Buffer,
        (reflection, sampler): (&wgpu::TextureView, &wgpu::Sampler),
        shadow: &super::native_shadow::ShadowMap,
        (opaque_depth, opaque_color): (&wgpu::Texture, &wgpu::Texture),
        textures: &[(wgpu::TextureView, wgpu::Sampler)],
        block_light: &super::block_light::BlockLight,
        pbr: &[wgpu::TextureView; 2],
    ) -> wgpu::BindGroup {
        let opaque_depth = opaque_depth.create_view(&Default::default());
        let opaque_color = opaque_color.create_view(&Default::default());
        let mut entries = vec![
            wgpu::BindGroupEntry {
                binding: 0,
                resource: buffer.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::TextureView(reflection),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: wgpu::BindingResource::Sampler(sampler),
            },
            wgpu::BindGroupEntry {
                binding: 3,
                resource: wgpu::BindingResource::TextureView(&shadow.view),
            },
            wgpu::BindGroupEntry {
                binding: 4,
                resource: wgpu::BindingResource::Sampler(&shadow.sampler),
            },
            wgpu::BindGroupEntry {
                binding: 7,
                resource: wgpu::BindingResource::TextureView(&shadow.translucent_view),
            },
            wgpu::BindGroupEntry {
                binding: 8,
                resource: wgpu::BindingResource::TextureView(&shadow.color_view),
            },
            wgpu::BindGroupEntry {
                binding: 6,
                resource: wgpu::BindingResource::TextureView(&opaque_depth),
            },
            wgpu::BindGroupEntry {
                binding: 11,
                resource: wgpu::BindingResource::TextureView(&opaque_color),
            },
            wgpu::BindGroupEntry {
                binding: 12,
                resource: wgpu::BindingResource::TextureView(&pbr[0]),
            },
            wgpu::BindGroupEntry {
                binding: 13,
                resource: wgpu::BindingResource::TextureView(&pbr[1]),
            },
        ];
        entries.extend([
            wgpu::BindGroupEntry {
                binding: 9,
                resource: wgpu::BindingResource::TextureView(&block_light.view),
            },
            wgpu::BindGroupEntry {
                binding: 10,
                resource: block_light.grid.as_entire_binding(),
            },
        ]);
        entries.extend(pack_texture_group_entries(textures));
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Native shader pack frame"),
            layout,
            entries: &entries,
        })
    }

    /// Keeps the opaque copies the size of the world's depth `depth` and, where the world's
    /// colour `color` can be copied (the scaled target, not the window's own), its size and
    /// format; the group is made again when they change.
    pub fn fit_opaque(
        &mut self,
        device: &wgpu::Device,
        depth: &wgpu::Texture,
        color: Option<&wgpu::Texture>,
    ) {
        let size = [depth.width(), depth.height()];
        let (color_size, format) = match color {
            Some(c) if self.decoder.is_some() => ([c.width(), c.height()], INTERMEDIATE),
            Some(c) => ([c.width(), c.height()], c.format()),
            None => ([1, 1], self.opaque_color.format()),
        };
        let color_fits = [self.opaque_color.width(), self.opaque_color.height()] == color_size
            && self.opaque_color.format() == format;
        let depth_fits = [self.opaque_depth.width(), self.opaque_depth.height()] == size;
        if depth_fits && color_fits {
            return;
        }
        if !color_fits {
            self.opaque_color = color_copy(device, color_size, format);
        }
        if !depth_fits {
            self.opaque_depth = depth_copy(device, size);
        }
        self.regroup(device);
    }

    /// Fills `opaque_color` from the world's colour `color`: a copy, under 0.2 decoded.
    pub fn copy_opaque(
        &self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        color: &wgpu::TextureView,
    ) {
        match &self.decoder {
            Some(d) => {
                let to = self.opaque_color.create_view(&Default::default());
                d.run(device, encoder, color, &to);
            }
            None => encoder.copy_texture_to_texture(
                color.texture().as_image_copy(),
                self.opaque_color.as_image_copy(),
                color.texture().size(),
            ),
        }
    }

    /// The water mirror `view` as the material reads it: under 0.2 decoded into a copy.
    pub fn linear_mirror(
        &mut self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        view: wgpu::TextureView,
    ) -> wgpu::TextureView {
        let Some(decoder) = &self.decoder else {
            return view;
        };
        let size = [view.texture().width(), view.texture().height()];
        let fits = |m: &wgpu::TextureView| [m.texture().width(), m.texture().height()] == size;
        if !self.mirror.as_ref().is_some_and(fits) {
            self.mirror = Some(intermediate(device, "Water mirror, linear", size));
        }
        let mirror = self.mirror.clone().unwrap();
        decoder.run(device, encoder, &view, &mirror);
        mirror
    }

    /// Sizes the surface target: the world's while a material stage writes it, else a blank.
    pub fn fit_surface(&mut self, device: &wgpu::Device, size: [u32; 2]) {
        if [self.surface.width(), self.surface.height()] != size {
            self.surface = surface_target(device, size);
            self.surface_view = self.surface.create_view(&Default::default());
        }
    }

    /// Binds `view` as this frame's `reflection` (`None`: the blank). The mirror is drawn
    /// before the stages read it.
    pub fn set_reflection(&mut self, device: &wgpu::Device, view: Option<&wgpu::TextureView>) {
        let view = view.unwrap_or(&self.blank);
        if *view != self.reflection {
            self.reflection = view.clone();
            self.regroup(device);
        }
    }

    fn regroup(&mut self, device: &wgpu::Device) {
        self.group = Self::group(
            device,
            &self.layout,
            &self.buffer,
            (&self.reflection, &self.reflection_sampler),
            &self.shadow_map,
            (&self.opaque_depth, &self.opaque_color),
            &self.textures,
            &self.block_light,
            &self.pbr,
        );
    }

    /// Writes this frame's inputs, for a world of `size` drawn for `display`
    /// (`EffectiveOutput::display`), which a 0.2 pack reads.
    pub(crate) fn write(
        &mut self,
        queue: &wgpu::Queue,
        frame: &Frame,
        size: [u32; 2],
        display: super::output::DisplayValues,
    ) {
        let inputs = self.next_inputs(frame, size, display);
        queue.write_buffer(&self.buffer, 0, bytemuck::bytes_of(&inputs));
    }

    /// This frame's inputs, moving the eased values and the frame count on.
    fn next_inputs(
        &mut self,
        frame: &Frame,
        size: [u32; 2],
        display: super::output::DisplayValues,
    ) -> FrameInputs {
        // Under `HORIZON_STILL` the world's ticks are the clock (`PackInputs::still_ticks`), and
        // eased values sit at their targets.
        // A scripted clock (`PackInputs::frame_timing`) comes first, and eased values
        // move by its delta.
        let now = std::time::Instant::now();
        let timing = frame.pack.frame_timing;
        // Seconds, frame index, and the seconds eased values move by when not the
        // measured ones.
        let (seconds, index, step) = match (timing, frame.pack.still_ticks) {
            (Some(t), _) => (t.elapsed, t.counter, Some(t.delta)),
            (None, Some(ticks)) => (
                (ticks as f32 / 20.0) % 3600.0,
                ticks as u32,
                Some(f32::INFINITY),
            ),
            (None, None) => (
                self.start.elapsed().as_secs_f32() % 3600.0,
                self.frames,
                None,
            ),
        };
        let since = step.unwrap_or_else(|| self.wetness.1.map_or(0.0, |t| (now - t).as_secs_f32()));
        self.wetness = (wetness(self.wetness.0, frame.pack.rain, since), Some(now));
        let mut inputs = frame_inputs(frame, size, seconds, index, self.shadow_camera.as_ref());
        inputs.wetness = self.wetness.0;
        if self.api == Api::V0_2 {
            inputs.sky_color = decoded(inputs.sky_color);
            inputs.fog_color = decoded(inputs.fog_color);
            if display.hdr {
                inputs.hdr = 1;
                inputs.headroom = display.headroom;
                inputs.paper_white = display.paper_white;
                inputs.peak = display.peak;
                inputs.black_level = display.black_level;
            }
        }
        if !self.reflects {
            inputs.has_reflection = 0;
        }
        let view_proj = (Mat4::from_cols_array_2d(&inputs.projection)
            * Mat4::from_cols_array_2d(&inputs.view))
        .to_cols_array_2d();
        inputs.previous_view_projection = view_proj;
        // `write` skips title frames, so a gap means a pause or another world: the frame
        // before says nothing about this one, and the eased values start over.
        // A scripted clock's frames follow each other however long one takes to draw.
        match self.last {
            Some((previous, camera, time))
                if timing.is_some() || (now - time).as_secs_f32() < MAX_FRAME_GAP =>
            {
                inputs.previous_view_projection = previous;
                inputs.camera_motion = (frame.camera - camera).as_vec3().to_array();
                inputs.delta_time = (now - time).as_secs_f32();
            }
            _ => {
                self.eye = [f32::NAN; 4];
                self.history_reset = true;
            }
        }
        self.last = Some((view_proj, frame.camera, now));
        // The eye's light and medium eased by half-lives in seconds, from where they are at
        // first.
        let targets = [
            (inputs.eye_light[0], 2.0),
            (inputs.eye_light[1], 2.0),
            (f32::from(u8::from(inputs.eye_in == 1)), 0.25),
            (f32::from(u8::from(inputs.eye_in == 2)), 0.25),
        ];
        let dt = step.unwrap_or(inputs.delta_time);
        inputs.delta_time = match (timing, step) {
            (Some(t), _) => t.delta,
            (None, Some(_)) => 0.05,
            (None, None) => inputs.delta_time,
        };
        for (value, (target, half_life)) in self.eye.iter_mut().zip(targets) {
            *value = eased(*value, target, half_life, dt);
        }
        let [block, sky, water, lava] = self.eye;
        inputs.eye_light_smooth = [block, sky];
        inputs.in_water = water;
        inputs.in_lava = lava;
        self.frames = self.frames.wrapping_add(1);
        inputs
    }

    /// Forgets the frames before, as a freshly loaded pack: the post stage's next `history` is
    /// black, eased values start at their targets, wetness dry, the frame count at 0.
    pub(super) fn reset_history(&mut self) {
        self.last = None;
        self.frames = 0;
        self.wetness = (0.0, None);
        self.start = std::time::Instant::now();
    }

    /// Takes the post stage's pending history reset. It stays pending if post rendering is
    /// skipped for a frame, so the first frame it does render still sees black history.
    pub(super) fn take_history_reset(&mut self) -> bool {
        let reset = self.history_reset;
        self.history_reset = false;
        reset
    }
}

/// The option values of `constants` that `module` declares as overrides (wgpu refuses others).
pub fn present_constants<'a>(
    module: &naga::Module,
    constants: &'a [(String, f64)],
) -> Vec<(&'a str, f64)> {
    constants
        .iter()
        .filter(|(id, _)| {
            module
                .overrides
                .iter()
                .any(|(_, o)| o.name.as_ref() == Some(id))
        })
        .map(|(id, v)| (id.as_str(), *v))
        .collect()
}

/// The solid terrain's surface target of `size`, drawn and read.
fn surface_target(device: &wgpu::Device, size: [u32; 2]) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some("Native shader pack surface"),
        size: wgpu::Extent3d {
            width: size[0],
            height: size[1],
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: super::native_material::SURFACE_FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    })
}

fn texture_entry(binding: u32, sample_type: wgpu::TextureSampleType) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Texture {
            sample_type,
            view_dimension: wgpu::TextureViewDimension::D2,
            multisampled: false,
        },
        count: None,
    }
}

/// A LabPBR map's array, read by the material entries (`native_material.rs`).
fn pbr_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        // `[displacement]`'s grid moves its corners by the heights (gfx/displace.rs).
        visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
        ty: wgpu::BindingType::Texture {
            sample_type: wgpu::TextureSampleType::Float { filterable: true },
            view_dimension: wgpu::TextureViewDimension::D2Array,
            multisampled: false,
        },
        count: None,
    }
}

fn comparison_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Comparison),
        count: None,
    }
}

fn sampler_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
        count: None,
    }
}

impl NativePost {
    /// Pipelines for `stages` (the pack's post passes, in order) with the option values
    /// `constants`, drawing onto a screen of `format`; under 0.2 the last onto `picture`, and
    /// `tonemap` (else Horizon's) onto the screen. Errors name the pass wgpu refused.
    pub fn new(
        device: &wgpu::Device,
        stages: Vec<CheckedStage>,
        manifest: &horizon_shader_api::Manifest,
        constants: &[(String, f64)],
        format: wgpu::TextureFormat,
        frame: &NativeFrame,
        tonemap: Option<&CheckedStage>,
    ) -> Result<Self, String> {
        let targets = manifest.post_targets();
        let outputs: Vec<Option<usize>> = manifest
            .post
            .iter()
            .map(|p| {
                let name = p.output.as_deref()?;
                targets.iter().position(|t| t.0 == name)
            })
            .collect();
        let plan = plan(&outputs);
        let float = wgpu::TextureSampleType::Float { filterable: true };
        let mut entries = vec![
            frame_entry(wgpu::ShaderStages::FRAGMENT),
            texture_entry(1, float),
            texture_entry(2, wgpu::TextureSampleType::Depth),
            texture_entry(3, float),
            sampler_entry(4),
            sampler_entry(5),
            texture_entry(6, wgpu::TextureSampleType::Depth),
            comparison_entry(7),
            texture_entry(8, float),
            texture_entry(9, wgpu::TextureSampleType::Depth),
            texture_entry(10, float),
            texture_entry(11, wgpu::TextureSampleType::Uint),
        ];
        entries.extend(pack_texture_entries(frame.textures.len()));
        entries
            .extend((0..targets.len() as u32).map(|j| {
                texture_entry(horizon_shader_api::stages::POST_TARGET_BINDING + j, float)
            }));
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Native shader pack post"),
            entries: &entries,
        });
        let mut groups: [Option<&wgpu::BindGroupLayout>; GROUP as usize + 1] = Default::default();
        groups[GROUP as usize] = Some(&layout);
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Native shader pack post"),
            bind_group_layouts: &groups,
            ..Default::default()
        });
        let count = stages.len();
        let mut passes = Vec::with_capacity(count);
        for (i, stage) in stages.into_iter().enumerate() {
            let present = present_constants(&stage.module, constants);
            let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
            let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some(&stage.label),
                source: wgpu::ShaderSource::Naga(std::borrow::Cow::Owned(stage.module)),
            });
            let options = wgpu::PipelineCompilationOptions {
                constants: &present,
                ..Default::default()
            };
            let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(&stage.label),
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &module,
                    entry_point: Some("horizon_vs_post"),
                    compilation_options: options.clone(),
                    buffers: &[],
                },
                primitive: Default::default(),
                depth_stencil: None,
                multisample: Default::default(),
                fragment: Some(wgpu::FragmentState {
                    module: &module,
                    entry_point: Some("horizon_fs_post"),
                    compilation_options: options,
                    targets: &[Some(wgpu::ColorTargetState {
                        format: if plan[i].writes == Writes::Screen && frame.decoder.is_none() {
                            format
                        } else {
                            INTERMEDIATE
                        },
                        blend: None,
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                multiview_mask: None,
                cache: None,
            });
            if let Some(e) = pollster::block_on(scope.pop()) {
                return Err(format!("{}: {e}", stage.file));
            }
            passes.push((stage.label, pipeline));
        }
        let display = match &frame.decoder {
            Some(decoder) => {
                let source = tonemap.map_or_else(
                    || StageKind::Tonemap.prelude(Api::V0_2) + DEFAULT_TONEMAP,
                    |s| s.source.clone(),
                ) + &tonemap_entries(tonemap.is_none());
                let file = tonemap.map_or("<tonemap>", |s| s.file.as_str());
                let module = naga::front::wgsl::parse_str(&source)
                    .map_err(|e| format!("{file}: {}", e.emit_to_string(&source)))?;
                let present = present_constants(&module, constants);
                let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
                let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
                    label: Some("Tonemap"),
                    source: wgpu::ShaderSource::Naga(std::borrow::Cow::Owned(module)),
                });
                let entries = (
                    Some(&pipeline_layout),
                    "horizon_vs_tonemap",
                    "horizon_fs_tonemap",
                );
                let pipeline =
                    fullscreen_pipeline(device, "Tonemap", &module, entries, format, &present);
                if let Some(e) = pollster::block_on(scope.pop()) {
                    return Err(format!("{file}: {e}"));
                }
                Some(Display {
                    decoder: decoder.clone(),
                    label: tonemap.map(|s| s.label.clone()),
                    tonemap: pipeline,
                    scene: None,
                    picture: None,
                })
            }
            None => None,
        };
        let sampler = |label, filter| {
            device.create_sampler(&wgpu::SamplerDescriptor {
                label: Some(label),
                mag_filter: filter,
                min_filter: filter,
                ..Default::default()
            })
        };
        Ok(Self {
            passes,
            layout,
            textures: frame.textures.clone(),
            frame: frame.buffer.clone(),
            shadow: (
                frame.shadow_map.view.clone(),
                frame.shadow_map.sampler.clone(),
                frame.shadow_map.translucent_view.clone(),
                frame.shadow_map.color_view.clone(),
            ),
            linear: sampler("Native shader pack linear", wgpu::FilterMode::Linear),
            nearest: sampler("Native shader pack nearest", wgpu::FilterMode::Nearest),
            targets: None,
            history: None,
            history_blank: history_target(device, "Native shader pack history blank", [1, 1]),
            groups: None,
            history_valid: false,
            scales: targets.iter().map(|t| t.1).collect(),
            named: None,
            plan,
            format,
            display,
        })
    }

    /// Whether the pack has a `[tonemap]` of its own, which replaces Horizon's tone curve.
    pub fn tonemaps(&self) -> bool {
        self.display.as_ref().is_some_and(|d| d.label.is_some())
    }

    /// The passes' labels, in order, then the pack's tonemap's.
    pub fn labels(&self) -> Vec<&str> {
        let tonemap = self.display.as_ref().and_then(|d| d.label.as_deref());
        self.passes
            .iter()
            .map(|(l, _)| l.as_str())
            .chain(tonemap)
            .collect()
    }

    /// Forgets the pack's own targets (`[[post]] output`): made again, blank, on the next frame,
    /// as for a pack just made. Its pipelines stay.
    pub(super) fn reset_history(&mut self) {
        self.named = None;
        self.groups = None;
    }

    fn prepare(
        &mut self,
        device: &wgpu::Device,
        world: (&wgpu::TextureView, &wgpu::TextureView, &wgpu::TextureView),
        size: [u32; 2],
        window: [u32; 2],
        history_reset: bool,
    ) {
        if let Some(d) = &mut self.display {
            if d.scene.as_ref().is_none_or(|s| s.0 != size) {
                d.scene = Some((
                    size,
                    intermediate(device, "Shader API 0.2 scene_color", size),
                ));
                self.groups = None;
            }
            let chain = !self.passes.is_empty();
            if chain && d.picture.as_ref().is_none_or(|p| p.0 != window) {
                d.picture = Some((
                    window,
                    intermediate(device, "Shader API 0.2 picture", window),
                ));
                self.groups = None;
            }
        }
        if !self.scales.is_empty() && self.named.as_ref().is_none_or(|n| n.0 != size) {
            let views = self
                .scales
                .iter()
                .map(|&scale| {
                    let size = scaled(size, scale);
                    device
                        .create_texture(&wgpu::TextureDescriptor {
                            label: Some("Native shader pack target"),
                            size: wgpu::Extent3d {
                                width: size[0],
                                height: size[1],
                                depth_or_array_layers: 1,
                            },
                            mip_level_count: 1,
                            sample_count: 1,
                            dimension: wgpu::TextureDimension::D2,
                            format: INTERMEDIATE,
                            usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                                | wgpu::TextureUsages::TEXTURE_BINDING,
                            view_formats: &[],
                        })
                        .create_view(&Default::default())
                })
                .collect();
            self.named = Some((size, views));
            self.groups = None;
        }
        let chain = self
            .plan
            .iter()
            .any(|p| matches!(p.writes, Writes::Chain(_)));
        if chain && self.targets.as_ref().is_none_or(|t| t.0 != size) {
            let target = |label| {
                device
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
                        format: INTERMEDIATE,
                        // COPY_SRC: the last pass's input is kept as next frame's `history`.
                        usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                            | wgpu::TextureUsages::TEXTURE_BINDING
                            | wgpu::TextureUsages::COPY_SRC,
                        view_formats: &[],
                    })
                    .create_view(&Default::default())
            };
            self.targets = Some((
                size,
                [
                    target("Native shader pack A"),
                    target("Native shader pack B"),
                ],
            ));
            self.history = Some(history_target(device, "Native shader pack history", size));
            self.history_valid = false;
            self.groups = None;
        }
        let blank_history = history_reset || !self.history_valid;
        if self
            .groups
            .as_ref()
            .is_some_and(|((color, depth, surface), blank, _)| {
                color == world.0
                    && depth == world.1
                    && surface == world.2
                    && *blank == blank_history
            })
        {
            return;
        }
        let history = if blank_history {
            &self.history_blank
        } else {
            self.history
                .as_ref()
                .expect("history exists after a post stage has more than one pass")
        }
        .create_view(&Default::default());
        let blank = self.history_blank.create_view(&Default::default());
        // Under 0.2 the passes read the world decoded.
        let scene = self
            .display
            .as_ref()
            .and_then(|d| d.scene.as_ref())
            .map_or(world.0, |s| &s.1);
        let group = |previous: &wgpu::TextureView, writes: Option<Writes>| {
            let mut entries = vec![
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: self.frame.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(scene),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(world.1),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::TextureView(previous),
                },
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: wgpu::BindingResource::Sampler(&self.linear),
                },
                wgpu::BindGroupEntry {
                    binding: 5,
                    resource: wgpu::BindingResource::Sampler(&self.nearest),
                },
                wgpu::BindGroupEntry {
                    binding: 6,
                    resource: wgpu::BindingResource::TextureView(&self.shadow.0),
                },
                wgpu::BindGroupEntry {
                    binding: 7,
                    resource: wgpu::BindingResource::Sampler(&self.shadow.1),
                },
                wgpu::BindGroupEntry {
                    binding: 8,
                    resource: wgpu::BindingResource::TextureView(&history),
                },
                wgpu::BindGroupEntry {
                    binding: 9,
                    resource: wgpu::BindingResource::TextureView(&self.shadow.2),
                },
                wgpu::BindGroupEntry {
                    binding: 11,
                    resource: wgpu::BindingResource::TextureView(world.2),
                },
                wgpu::BindGroupEntry {
                    binding: 10,
                    resource: wgpu::BindingResource::TextureView(&self.shadow.3),
                },
            ];
            entries.extend(pack_texture_group_entries(&self.textures));
            // The pack's targets, a blank one in place of the one this pass writes.
            if let Some((_, views)) = &self.named {
                entries.extend(views.iter().enumerate().map(|(j, view)| {
                    let view = if writes == Some(Writes::Target(j)) {
                        &blank
                    } else {
                        view
                    };
                    wgpu::BindGroupEntry {
                        binding: horizon_shader_api::stages::POST_TARGET_BINDING + j as u32,
                        resource: wgpu::BindingResource::TextureView(view),
                    }
                }));
            }
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("Native shader pack post"),
                layout: &self.layout,
                entries: &entries,
            })
        };
        let mut groups: Vec<_> = (0..self.passes.len())
            .map(|i| {
                // The chain's picture so far: the world's before its first pass.
                let previous = match (&self.targets, self.plan[i].reads) {
                    (Some((_, t)), Some(slot)) => &t[slot],
                    _ => scene,
                };
                group(previous, Some(self.plan[i].writes))
            })
            .collect();
        // The tonemap's, last: the chain's picture, else the decoded world.
        if let Some(d) = &self.display {
            groups.push(group(d.picture.as_ref().map_or(scene, |p| &p.1), None));
        }
        self.groups = Some((
            (world.0.clone(), world.1.clone(), world.2.clone()),
            blank_history,
            groups,
        ));
    }

    /// Runs the passes over the world target `world` (colour, depth) of `size`, the last onto
    /// `screen`, timed by `timing` when it times this frame.
    pub fn run(
        &mut self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        world: (&wgpu::TextureView, &wgpu::TextureView, &wgpu::TextureView),
        screen: &wgpu::TextureView,
        size: [u32; 2],
        history_reset: bool,
        mut timing: Option<&mut super::pack_timing::PassTiming>,
    ) {
        let window = [screen.texture().width(), screen.texture().height()];
        self.prepare(device, world, size, window, history_reset);
        if let Some(t) = timing.as_deref_mut() {
            t.mark(device, encoder, (world.0, self.format));
        }
        let Some((_, _, groups)) = &self.groups else {
            return;
        };
        if let Some(Display {
            decoder,
            scene: Some((_, scene)),
            ..
        }) = &self.display
        {
            decoder.run(device, encoder, world.0, scene);
        }
        let last = match &self.display {
            Some(Display {
                picture: Some((_, picture)),
                ..
            }) => picture,
            _ => screen,
        };
        for (i, ((label, pipeline), group)) in self.passes.iter().zip(groups).enumerate() {
            let view = match (self.plan[i].writes, &self.targets, &self.named) {
                (Writes::Chain(slot), Some((_, t)), _) => &t[slot],
                (Writes::Target(j), _, Some((_, n))) => &n[j],
                _ => last,
            };
            let attachments = [Some(wgpu::RenderPassColorAttachment {
                view,
                resolve_target: None,
                depth_slice: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                    store: wgpu::StoreOp::Store,
                },
            })];
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some(label),
                color_attachments: &attachments,
                timestamp_writes: timing.as_deref_mut().and_then(|t| t.writes(label)),
                ..Default::default()
            });
            pass.set_pipeline(pipeline);
            pass.set_bind_group(GROUP, group, &[]);
            pass.draw(0..3, 0..1);
        }
        if let (Some(d), Some(group)) = (&self.display, groups.last()) {
            let label = d.label.as_deref().unwrap_or("Tonemap");
            fullscreen_pass(encoder, label, screen, &d.tonemap, (GROUP, group), timing);
        }
        // The chain's last picture becomes next frame's `history`, once every pass has read
        // this frame's.
        let before_last = &self.plan[..self.plan.len().saturating_sub(1)];
        if let Some(Writes::Chain(slot)) = before_last
            .iter()
            .rfind(|p| matches!(p.writes, Writes::Chain(_)))
            .map(|p| p.writes)
            && let (Some(history), Some((_, targets))) = (&self.history, &self.targets)
        {
            encoder.copy_texture_to_texture(
                targets[slot].texture().as_image_copy(),
                history.as_image_copy(),
                history.size(),
            );
            self.history_valid = true;
        }
    }
}

/// A target of `size` the post stage's `history` is copied into.
fn history_target(device: &wgpu::Device, label: &str, size: [u32; 2]) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d {
            width: size[0].max(1),
            height: size[1].max(1),
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: INTERMEDIATE,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    })
}

/// Vanilla's gamma colour `c` in linear light (0.2's decode; negatives and NaN made 0).
fn decoded(c: [f32; 3]) -> [f32; 3] {
    c.map(|v| if v > 0.0 { v.powf(2.2) } else { 0.0 })
}

/// `wetness` after `dt` seconds of `rain`: halfway there in 30 seconds while it rains
/// harder, in 10 while it dries (the half-lives of the `shaders/` format's `wetness`).
fn wetness(wetness: f32, rain: f32, dt: f32) -> f32 {
    let half_life = if rain > wetness { 30.0 } else { 10.0 };
    rain + (wetness - rain) * 0.5f32.powf(dt / half_life)
}

/// The frame as packs read it (docs/api/shaders-reference.md), from the renderer's `Frame`.
pub fn frame_inputs(
    frame: &Frame,
    size: [u32; 2],
    seconds: f32,
    index: u32,
    shadow: Option<&horizon_shaders::Shadow>,
) -> FrameInputs {
    let aspect = size[0] as f32 / size[1].max(1) as f32;
    let view = crate::camera::view_matrix(frame.yaw, frame.pitch);
    let projection = super::world_projection(frame.fov_degrees, aspect, frame.depth_far)
        * frame.bob
        * frame.spin;
    // A celestial body's direction at `angle` degrees (SkyRenderer: Y -90°, then X by the
    // angle, of straight up): overhead at 0, rising in the east (+x).
    let body = |angle: f32| {
        let a = angle.to_radians();
        Vec3::new(-a.sin(), a.cos(), 0.0)
    };
    let (fog_color, fog_start, fog_end) = match &frame.fog {
        Some(f) => (
            [f.color[0], f.color[1], f.color[2]],
            f.environmental_start.min(f.render_distance_start),
            f.environmental_end.min(f.render_distance_end),
        ),
        None => (frame.sky.fog_color, frame.distance * 0.9, frame.distance),
    };
    let pack = &frame.pack;
    let level = |l: i32| l as f32 / 15.0;
    let m = |m: Mat4| m.to_cols_array_2d();
    FrameInputs {
        view: m(view),
        projection: m(projection),
        view_inverse: m(view.inverse()),
        projection_inverse: m(projection.inverse()),
        camera_position: frame.camera.as_vec3().to_array(),
        time: seconds,
        sun_direction: body(frame.sky.sun_angle).to_array(),
        day_time: pack.day_time.rem_euclid(24000) as f32 / 24000.0,
        moon_direction: body(frame.sky.moon_angle).to_array(),
        game_time: frame.shader_game_time,
        sky_color: frame.sky.sky_color,
        rain: pack.rain,
        fog_color,
        thunder: pack.thunder,
        resolution: [size[0] as f32, size[1] as f32],
        eye_light: [level(pack.eye_light[0]), level(pack.eye_light[1])],
        fog_start,
        fog_end,
        render_distance: frame.distance,
        far: frame.depth_far,
        night_vision: frame.night_vision,
        blindness: pack.blindness,
        darkness: frame.darkness.1.clamp(0.0, 1.0),
        brightness: pack.brightness,
        eye_in: pack.eye_in as u32,
        dimension: if frame.end_sky {
            1
        } else if frame.nether_lighting {
            -1
        } else {
            0
        },
        frame_index: index,
        hide_gui: u32::from(pack.hide_gui),
        precipitation: pack.biome_precipitation.clamp(0, 2) as u32,
        temperature: pack.temperature,
        downfall: pack.rainfall,
        wetness: pack.rain,
        // Filled by `NativeFrame::write`, which remembers the frames before.
        previous_view_projection: m(Mat4::IDENTITY),
        camera_motion: [0.0; 3],
        delta_time: 0.0,
        eye_light_smooth: [level(pack.eye_light[0]), level(pack.eye_light[1])],
        in_water: f32::from(u8::from(pack.eye_in as u32 == 1)),
        in_lava: f32::from(u8::from(pack.eye_in as u32 == 2)),
        water_plane: frame.water_plane.unwrap_or(0.0),
        has_reflection: u32::from(frame.water_plane.is_some()),
        reflection_resolution: reflection_size(size).map(|s| s as f32),
        mods: pack.mod_params,
        light_direction: match shadow {
            Some(s) => super::pack_uniforms::shadow_light(frame, s),
            None => Vec3::Y,
        }
        .to_array(),
        has_shadow: u32::from(shadow.is_some()),
        shadow_view_proj: m(shadow.map_or(Mat4::IDENTITY, |s| {
            super::pack_uniforms::shadow_culling(frame, s)
        })),
        shadow_distance: shadow.map_or(0.0, |s| s.distance),
        shadow_distortion: shadow.map_or(0.0, |s| {
            super::native_shadow::distortion(s.distance, s.resolution)
        }),
        shadow_resolution: shadow.map_or(0.0, |s| s.resolution as f32),
        shadow_center_texel: shadow.map_or(0.0, |s| {
            let k = super::native_shadow::distortion(s.distance, s.resolution);
            2.0 * s.distance / s.resolution.max(1) as f32 * (1.0 - k)
        }),
        // Shader API 0.2's display fields, which 0.1 packs don't declare: SDR's values, which
        // `NativeFrame::write` replaces on an HDR display.
        hdr: 0,
        headroom: 1.0,
        paper_white: 80.0,
        peak: 80.0,
        black_level: 0.0,
        padding: [0; 3],
    }
}

/// The pack's `[[textures]]` (`check::Image`s), uploaded with samplers as the manifest says.
pub fn pack_textures(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    report: &horizon_shader_api::Report,
) -> Vec<(wgpu::TextureView, wgpu::Sampler)> {
    report
        .textures
        .iter()
        .map(|(t, image)| {
            let texture = wgpu::util::DeviceExt::create_texture_with_data(
                device,
                queue,
                &wgpu::TextureDescriptor {
                    label: Some(&t.id),
                    size: wgpu::Extent3d {
                        width: image.width,
                        height: image.height,
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
                &image.rgba,
            );
            let filter = if t.linear {
                wgpu::FilterMode::Linear
            } else {
                wgpu::FilterMode::Nearest
            };
            let address = if t.repeat {
                wgpu::AddressMode::Repeat
            } else {
                wgpu::AddressMode::ClampToEdge
            };
            let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
                label: Some(&t.id),
                address_mode_u: address,
                address_mode_v: address,
                mag_filter: filter,
                min_filter: filter,
                ..Default::default()
            });
            (texture.create_view(&Default::default()), sampler)
        })
        .collect()
}

/// Layout entries of `count` pack textures and their samplers.
pub(super) fn pack_texture_entries(count: usize) -> Vec<wgpu::BindGroupLayoutEntry> {
    use horizon_shader_api::stages::{TEXTURE_BINDING, TEXTURE_SAMPLER_BINDING};
    (0..count as u32)
        .flat_map(|i| {
            [
                wgpu::BindGroupLayoutEntry {
                    binding: TEXTURE_BINDING + i,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: TEXTURE_SAMPLER_BINDING + i,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ]
        })
        .collect()
}

/// Group entries of the pack's textures, as `pack_texture_entries` lays them out.
pub(super) fn pack_texture_group_entries(
    textures: &[(wgpu::TextureView, wgpu::Sampler)],
) -> Vec<wgpu::BindGroupEntry<'_>> {
    use horizon_shader_api::stages::{TEXTURE_BINDING, TEXTURE_SAMPLER_BINDING};
    textures
        .iter()
        .enumerate()
        .flat_map(|(i, (view, sampler))| {
            [
                wgpu::BindGroupEntry {
                    binding: TEXTURE_BINDING + i as u32,
                    resource: wgpu::BindingResource::TextureView(view),
                },
                wgpu::BindGroupEntry {
                    binding: TEXTURE_SAMPLER_BINDING + i as u32,
                    resource: wgpu::BindingResource::Sampler(sampler),
                },
            ]
        })
        .collect()
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_pass_with_an_output_leaves_the_chain_as_it_was() {
        use super::{Writes, plan};
        // Bloom into a target, a pass of the chain, a second target, the picture.
        let p = plan(&[Some(0), None, Some(1), None, None]);
        let writes: Vec<_> = p.iter().map(|p| p.writes).collect();
        assert!(writes[0] == Writes::Target(0));
        assert!(writes[1] == Writes::Chain(0));
        assert!(writes[2] == Writes::Target(1));
        assert!(writes[3] == Writes::Chain(1));
        assert!(writes[4] == Writes::Screen);
        let reads: Vec<_> = p.iter().map(|p| p.reads).collect();
        assert_eq!(reads, [None, None, Some(0), Some(0), Some(1)]);
    }
    #[test]
    fn eased_values_start_at_the_target_and_halve_the_distance() {
        use super::eased;
        assert_eq!(eased(f32::NAN, 0.25, 2.0, 0.0), 0.25);
        assert_eq!(eased(1.0, 0.0, 2.0, 2.0), 0.5);
        assert_eq!(eased(1.0, 0.0, 2.0, 0.0), 1.0);
    }
    use super::*;
    use crate::gfx::output::DisplayValues;
    use horizon_shader_api::{MANIFEST, Pack, check};

    const SDR: DisplayValues = DisplayValues::SDR;

    #[test]
    fn wetness_rises_slower_than_it_dries() {
        assert_eq!(wetness(0.0, 1.0, 30.0), 0.5);
        assert_eq!(wetness(1.0, 0.0, 10.0), 0.5);
        assert_eq!(wetness(0.25, 0.25, 5.0), 0.25);
    }

    /// A scripted clock comes before STILL's ticks, eased values and wetness move by its
    /// delta, and a long wait between its frames is not a gap that starts them over.
    #[test]
    fn a_scripted_clock_drives_time_frame_index_and_eased_values() {
        use crate::gfx::pack_uniforms::FrameTiming;
        let Some((device, _)) = device() else {
            return;
        };
        let hud = crate::hud::HudBatch::new([4, 4]);
        let mut frame = crate::gfx::pack_uniforms::initial_history_tests::frame(&hud);
        let mut native = NativeFrame::new(
            &device,
            None,
            Vec::new(),
            Default::default(),
            None,
            Api::V0_1,
        );
        frame.pack.still_ticks = Some(47_811);
        frame.pack.eye_light = [0, 15];
        frame.pack.rain = 1.0;
        frame.pack.frame_timing = Some(FrameTiming::nth(1, 2000.0));
        let first = native.next_inputs(&frame, [4, 4], SDR);
        assert_eq!(
            (first.time, first.frame_index, first.delta_time),
            (0.0, 1, 0.0)
        );
        // Wetness starts dry, as on a fresh pack; frame 1's delta of 0 leaves it there.
        assert_eq!((first.eye_light_smooth[1], first.wetness), (1.0, 0.0));
        assert!(native.take_history_reset());
        native.last.as_mut().unwrap().2 -= std::time::Duration::from_secs(5);
        frame.pack.eye_light = [0, 0];
        frame.pack.frame_timing = Some(FrameTiming::nth(2, 2000.0));
        let second = native.next_inputs(&frame, [4, 4], SDR);
        assert_eq!(
            (second.time, second.frame_index, second.delta_time),
            (2.0, 2, 2.0)
        );
        // Two seconds: the eye light's 2 s half-life halves it, and wetness rises
        // by 1 - 0.5^(2/30) (a 30 s half-life while it rains harder).
        assert_eq!(second.eye_light_smooth[1], 0.5);
        assert!(
            (second.wetness - 0.045_158_4).abs() < 1e-6,
            "{}",
            second.wetness
        );
        assert!(!native.take_history_reset());
    }

    /// After a reset, the same frames give the same inputs, to the bit, as a pack just made,
    /// however the frames before moved the eased values, wetness and the frame count.
    #[test]
    fn a_reset_frame_gives_a_fresh_pack_s_inputs() {
        use crate::gfx::pack_uniforms::FrameTiming;
        let Some((device, _)) = device() else {
            return;
        };
        let hud = crate::hud::HudBatch::new([4, 4]);
        let mut frame = crate::gfx::pack_uniforms::initial_history_tests::frame(&hud);
        let new = || {
            NativeFrame::new(
                &device,
                None,
                Vec::new(),
                Default::default(),
                None,
                Api::V0_1,
            )
        };
        let (mut used, mut fresh) = (new(), new());
        frame.pack.rain = 1.0;
        frame.pack.eye_light = [12, 3];
        frame.pack.eye_in = 1;
        for n in 1..=5 {
            frame.pack.frame_timing = Some(FrameTiming::nth(n, 700.0));
            used.next_inputs(&frame, [4, 4], SDR);
        }
        used.reset_history();
        frame.pack.eye_light = [2, 15];
        frame.pack.eye_in = 0;
        for n in 1..=3 {
            frame.pack.frame_timing = Some(FrameTiming::nth(n, 50.0));
            let inputs = [&mut used, &mut fresh].map(|f| f.next_inputs(&frame, [4, 4], SDR));
            assert_eq!(
                bytemuck::bytes_of(&inputs[0]),
                bytemuck::bytes_of(&inputs[1]),
                "frame {n}"
            );
            assert_eq!(used.take_history_reset(), fresh.take_history_reset());
        }
    }

    /// After a reset a pass reads its pack's own targets blank, as in a pack just made: here
    /// `fb`, which a pass reads before the pass that writes it, so it holds last frame's.
    #[test]
    fn a_reset_post_reads_its_own_targets_blank() {
        let Some((device, queue)) = device() else {
            return;
        };
        let manifest = "schema = 1
id = \"t\"
name = \"T\"
version = \"1\"
api = \"0.1\"
            [[post]]
file = \"a.wgsl\"
[[post]]
file = \"b.wgsl\"
output = \"fb\"
            [[post]]
file = \"c.wgsl\"
";
        let a = "fn post(input: PostInput) -> vec4<f32> { return vec4<f32>(textureLoad(fb, vec2<i32>(input.pixel), 0).rgb + 0.25, 1.0); }
";
        let pass = "fn post(input: PostInput) -> vec4<f32> { return input.color; }
";
        let files = [
            (MANIFEST, manifest),
            ("a.wgsl", a),
            ("b.wgsl", pass),
            ("c.wgsl", pass),
        ]
        .map(|(n, t)| (n.to_owned(), t.as_bytes().to_vec()))
        .into();
        let report = check(&Pack::from_files(files).unwrap()).unwrap();
        let format = wgpu::TextureFormat::Rgba8Unorm;
        let frame = NativeFrame::new(
            &device,
            None,
            Vec::new(),
            Default::default(),
            None,
            Api::V0_1,
        );
        let mut post = NativePost::new(
            &device,
            report.stages,
            &report.manifest,
            &[],
            format,
            &frame,
            None,
        )
        .unwrap();
        let usage = wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING;
        let color = texture(&device, format, usage).create_view(&Default::default());
        let depth = texture(&device, wgpu::TextureFormat::Depth32Float, usage)
            .create_view(&Default::default());
        let screen = texture(
            &device,
            format,
            wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        );
        let view = screen.create_view(&Default::default());
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: u64::from(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT * 4),
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let red = |post: &mut NativePost| {
            let mut encoder = device.create_command_encoder(&Default::default());
            let world = (&color, &depth, &frame.surface_view);
            post.run(&device, &mut encoder, world, &view, [4, 4], false, None);
            encoder.copy_texture_to_buffer(
                screen.as_image_copy(),
                wgpu::TexelCopyBufferInfo {
                    buffer: &buffer,
                    layout: wgpu::TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT),
                        rows_per_image: Some(4),
                    },
                },
                screen.size(),
            );
            queue.submit([encoder.finish()]);
            buffer
                .slice(..)
                .map_async(wgpu::MapMode::Read, |r| r.unwrap());
            device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
            let red = buffer.slice(..).get_mapped_range().unwrap()[0];
            buffer.unmap();
            red
        };
        let near = |got: u8, want: u8| got.abs_diff(want) <= 1;
        for want in [64, 128, 191] {
            let got = red(&mut post);
            assert!(near(got, want), "{got} before the reset, {want} expected");
        }
        post.reset_history();
        let got = red(&mut post);
        assert!(near(got, 64), "{got} after the reset: fb kept last frame's");
    }

    fn device() -> Option<(wgpu::Device, wgpu::Queue)> {
        let instance = crate::gfx::test_gpu::instance();
        let adapter = pollster::block_on(instance.request_adapter(&Default::default())).ok()?;
        pollster::block_on(adapter.request_device(&Default::default())).ok()
    }

    fn texture(
        device: &wgpu::Device,
        format: wgpu::TextureFormat,
        usage: wgpu::TextureUsages,
    ) -> wgpu::Texture {
        device.create_texture(&wgpu::TextureDescriptor {
            label: None,
            size: wgpu::Extent3d {
                width: 4,
                height: 4,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage,
            view_formats: &[],
        })
    }

    /// Two passes: the first doubles the world's colour (over 1, so the half-float target
    /// must keep it) times an option, the second halves it and writes the depth as alpha.
    #[test]
    fn passes_chain_through_half_float_targets_with_options_and_depth() {
        let Some((device, queue)) = device() else {
            return;
        };
        let manifest = "schema = 1\nid = \"t\"\nname = \"T\"\nversion = \"1\"\napi = \"0.1\"\n\
            [[post]]\nfile = \"a.wgsl\"\n[[post]]\nfile = \"b.wgsl\"\n\
            [[options]]\nid = \"gain\"\nname = \"Gain\"\ntype = \"float\"\ndefault = 0.0\nmin = 0.0\nmax = 2.0\n";
        let a = "override gain: f32;\nfn post(input: PostInput) -> vec4<f32> { return vec4<f32>(input.color.rgb * 2.0 * gain, 1.0); }\n";
        let b = "fn post(input: PostInput) -> vec4<f32> { return vec4<f32>(input.color.rgb * 0.5, input.depth); }\n";
        let files = [(MANIFEST, manifest), ("a.wgsl", a), ("b.wgsl", b)]
            .map(|(n, t)| (n.to_owned(), t.as_bytes().to_vec()))
            .into();
        let report = check(&Pack::from_files(files).unwrap()).unwrap();
        let format = wgpu::TextureFormat::Rgba8Unorm;
        let frame = NativeFrame::new(
            &device,
            None,
            Vec::new(),
            Default::default(),
            None,
            Api::V0_1,
        );
        queue.write_buffer(
            &frame.buffer,
            0,
            bytemuck::bytes_of(&FrameInputs::default()),
        );
        let mut post = NativePost::new(
            &device,
            report.stages,
            &report.manifest,
            &[("gain".into(), 1.0)],
            format,
            &frame,
            None,
        )
        .unwrap();
        assert_eq!(post.labels(), ["a.wgsl", "b.wgsl"]);

        let usage = wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING;
        let color = texture(&device, format, usage).create_view(&Default::default());
        let depth = texture(&device, wgpu::TextureFormat::Depth32Float, usage)
            .create_view(&Default::default());
        let screen = texture(
            &device,
            format,
            wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        );
        let mut encoder = device.create_command_encoder(&Default::default());
        encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: None,
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &color,
                resolve_target: None,
                depth_slice: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color {
                        r: 0.8,
                        g: 0.4,
                        b: 0.2,
                        a: 1.0,
                    }),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: &depth,
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Clear(0.5),
                    store: wgpu::StoreOp::Store,
                }),
                stencil_ops: None,
            }),
            ..Default::default()
        });
        let view = screen.create_view(&Default::default());
        post.run(
            &device,
            &mut encoder,
            (&color, &depth, &frame.surface_view),
            &view,
            [4, 4],
            true,
            None,
        );
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: u64::from(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT * 4),
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        encoder.copy_texture_to_buffer(
            screen.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT),
                    rows_per_image: Some(4),
                },
            },
            screen.size(),
        );
        queue.submit([encoder.finish()]);
        buffer
            .slice(..)
            .map_async(wgpu::MapMode::Read, |r| r.unwrap());
        device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
        let pixel = buffer.slice(..).get_mapped_range().unwrap()[..4].to_vec();
        let near = |got: u8, want: u8| got.abs_diff(want) <= 1;
        assert!(
            near(pixel[0], 204) && near(pixel[1], 102) && near(pixel[2], 51) && near(pixel[3], 128),
            "{pixel:?}"
        );
    }

    /// `history` is black on the first frame, then what the pass before the last wrote the
    /// frame before.
    #[test]
    fn the_last_pass_reads_last_frames_input_as_history() {
        let Some((device, queue)) = device() else {
            return;
        };
        let manifest = "schema = 1
id = \"t\"
name = \"T\"
version = \"1\"
api = \"0.1\"
            [[post]]
file = \"a.wgsl\"
[[post]]
file = \"b.wgsl\"
";
        let a = "fn post(input: PostInput) -> vec4<f32> { return vec4<f32>(input.color.rgb * 0.5, 1.0); }
";
        let b = "fn post(input: PostInput) -> vec4<f32> { return textureLoad(history, vec2<i32>(input.pixel), 0); }
";
        let files = [(MANIFEST, manifest), ("a.wgsl", a), ("b.wgsl", b)]
            .map(|(n, t)| (n.to_owned(), t.as_bytes().to_vec()))
            .into();
        let report = check(&Pack::from_files(files).unwrap()).unwrap();
        let format = wgpu::TextureFormat::Rgba8Unorm;
        let frame = NativeFrame::new(
            &device,
            None,
            Vec::new(),
            Default::default(),
            None,
            Api::V0_1,
        );
        let mut post = NativePost::new(
            &device,
            report.stages,
            &report.manifest,
            &[],
            format,
            &frame,
            None,
        )
        .unwrap();
        let usage = wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING;
        let color = texture(&device, format, usage).create_view(&Default::default());
        let depth = texture(&device, wgpu::TextureFormat::Depth32Float, usage)
            .create_view(&Default::default());
        let screen = texture(
            &device,
            format,
            wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        );
        let view = screen.create_view(&Default::default());
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: u64::from(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT * 4),
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let mut frame_pixel = |history_reset| {
            let mut encoder = device.create_command_encoder(&Default::default());
            encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: None,
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &color,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::WHITE),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                ..Default::default()
            });
            post.run(
                &device,
                &mut encoder,
                (&color, &depth, &frame.surface_view),
                &view,
                [4, 4],
                history_reset,
                None,
            );
            encoder.copy_texture_to_buffer(
                screen.as_image_copy(),
                wgpu::TexelCopyBufferInfo {
                    buffer: &buffer,
                    layout: wgpu::TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT),
                        rows_per_image: Some(4),
                    },
                },
                screen.size(),
            );
            queue.submit([encoder.finish()]);
            buffer
                .slice(..)
                .map_async(wgpu::MapMode::Read, |r| r.unwrap());
            device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
            let pixel = buffer.slice(..).get_mapped_range().unwrap()[..4].to_vec();
            buffer.unmap();
            pixel
        };
        assert_eq!(
            frame_pixel(true)[..3],
            [0, 0, 0],
            "black on the first frame"
        );
        let second = frame_pixel(false);
        assert!(
            second[..3].iter().all(|&v| v.abs_diff(128) <= 1),
            "{second:?}"
        );
        assert_eq!(frame_pixel(true)[..3], [0, 0, 0], "black after a frame gap");
        let fourth = frame_pixel(false);
        assert!(
            fourth[..3].iter().all(|&v| v.abs_diff(128) <= 1),
            "{fourth:?}"
        );
    }

    /// OpenGL refuses `Rgba32Float` as a render attachment (GLES 3 semantics, absent
    /// `EXT_color_buffer_float`) but always renders to the integer formats, so the surface
    /// is an integer format, and `surface_at` gets the floats back from the stored bits.
    #[test]
    fn the_surface_is_an_integer_target_that_the_post_stage_reads_bit_exact() {
        assert_eq!(
            super::super::native_material::SURFACE_FORMAT.sample_type(None, None),
            Some(wgpu::TextureSampleType::Uint)
        );
        let Some((device, queue)) = device() else {
            return;
        };
        let manifest = "schema = 1\nid = \"t\"\nname = \"T\"\nversion = \"1\"\napi = \"0.1\"\n\
            [[post]]\nfile = \"a.wgsl\"\n";
        // The surface's fields on the colour: normal x, smoothness, data, and 1 when present.
        let a = "fn post(input: PostInput) -> vec4<f32> {\n    \
            let s = surface_at(vec2<i32>(input.pixel));\n    \
            return vec4<f32>(s.normal.x, s.smoothness, s.data, select(0.0, 1.0, s.present));\n}\n";
        let files = [(MANIFEST, manifest), ("a.wgsl", a)]
            .map(|(n, t)| (n.to_owned(), t.as_bytes().to_vec()))
            .into();
        let report = check(&Pack::from_files(files).unwrap()).unwrap();
        let format = wgpu::TextureFormat::Rgba8Unorm;
        let frame = NativeFrame::new(
            &device,
            None,
            Vec::new(),
            Default::default(),
            None,
            Api::V0_1,
        );
        let mut post = NativePost::new(
            &device,
            report.stages,
            &report.manifest,
            &[],
            format,
            &frame,
            None,
        )
        .unwrap();
        let usage = wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING;
        let color = texture(&device, format, usage).create_view(&Default::default());
        let depth = texture(&device, wgpu::TextureFormat::Depth32Float, usage)
            .create_view(&Default::default());
        let screen = texture(
            &device,
            format,
            wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        );
        let view = screen.create_view(&Default::default());
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: u64::from(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT * 4),
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        // What the material stage stores, as the bits of floats: the normal +x (octahedral
        // (1, 0), two snorm16), smoothness 0.75, data 0.25 and the fragment's depth 0.5.
        let writer = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: None,
            source: wgpu::ShaderSource::Wgsl(
                "@vertex fn vs(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
                    return vec4<f32>(f32((i << 1u) & 2u) * 2.0 - 1.0, f32(i & 2u) * 2.0 - 1.0, 0.0, 1.0);
                }
                @fragment fn fs() -> @location(0) vec4<u32> {
                    return bitcast<vec4<u32>>(vec4<f32>(
                        bitcast<f32>(pack2x16snorm(vec2<f32>(1.0, 0.0))), 0.75, 0.25, 0.5));
                }"
                .into(),
            ),
        });
        let writer = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: None,
            layout: None,
            vertex: wgpu::VertexState {
                module: &writer,
                entry_point: Some("vs"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState {
                module: &writer,
                entry_point: Some("fs"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: super::super::native_material::SURFACE_FORMAT,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });
        let mut read = |depth_of_world: f64| {
            let mut encoder = device.create_command_encoder(&Default::default());
            {
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: None,
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &frame.surface_view,
                        resolve_target: None,
                        depth_slice: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    ..Default::default()
                });
                pass.set_pipeline(&writer);
                pass.draw(0..3, 0..1);
            }
            encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: None,
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &color,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &depth,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(depth_of_world as f32),
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                ..Default::default()
            });
            post.run(
                &device,
                &mut encoder,
                (&color, &depth, &frame.surface_view),
                &view,
                [4, 4],
                true,
                None,
            );
            encoder.copy_texture_to_buffer(
                screen.as_image_copy(),
                wgpu::TexelCopyBufferInfo {
                    buffer: &buffer,
                    layout: wgpu::TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT),
                        rows_per_image: Some(4),
                    },
                },
                screen.size(),
            );
            queue.submit([encoder.finish()]);
            buffer
                .slice(..)
                .map_async(wgpu::MapMode::Read, |r| r.unwrap());
            device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
            let pixel = buffer.slice(..).get_mapped_range().unwrap()[..4].to_vec();
            buffer.unmap();
            pixel
        };
        // Present where the world's depth is the stored one: +x, 0.75, 0.25 as 8-bit. No value
        // here sits halfway between two 8-bit levels, where drivers round either way.
        assert_eq!(read(0.5), [255, 191, 64, 255]);
        // Hidden by something nearer drawn since: the stored values no longer show.
        assert_eq!(read(0.25), [0, 0, 0, 0]);
    }

    /// The surface holds float bit patterns, some of which are NaNs (a normal's two snorm16
    /// can form one). The material stage writes `bitcast<vec4<u32>>(vec4<f32>(..))`; this
    /// runs that same expression (not the whole material fragment, which needs a full terrain
    /// pipeline) on bits fed as integers and checks the texture holds them exactly: payloads,
    /// signs and denormals included.
    #[test]
    fn the_surface_keeps_every_bit_of_the_floats_written_through_the_material_stage_bitcast() {
        let Some((device, queue)) = device() else {
            return;
        };
        let cases: [[u32; 4]; 4] = [
            [0x7fc0_1234, 0xffbf_ffff, 0x7fff_8001, 0x7f80_0001],
            [0x8000_0000, 0x0000_0001, 0x007f_ffff, 0x8000_0001],
            [0xffff_ffff, 0x7f80_0000, 0xff80_0000, 0x3f80_0000],
            [0x7fff_8001, 0x0000_0000, 0x1234_5678, 0xdead_beef],
        ];
        let mut bits = [[0u32; 4]; 16];
        bits[..4].copy_from_slice(&cases);
        let inputs = device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: 256,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        queue.write_buffer(&inputs, 0, bytemuck::cast_slice(&bits));
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: None,
            source: wgpu::ShaderSource::Wgsl(
                "@group(0) @binding(0) var<uniform> bits: array<vec4<u32>, 16>;
                @vertex fn vs(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
                    return vec4<f32>(f32((i << 1u) & 2u) * 2.0 - 1.0, f32(i & 2u) * 2.0 - 1.0, 0.0, 1.0);
                }
                @fragment fn fs(@builtin(position) p: vec4<f32>) -> @location(0) vec4<u32> {
                    let v = bitcast<vec4<f32>>(bits[u32(p.y) * 4u + u32(p.x)]);
                    return bitcast<vec4<u32>>(v);
                }"
                .into(),
            ),
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: None,
            layout: None,
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: Some("vs"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some("fs"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: super::super::native_material::SURFACE_FORMAT,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &pipeline.get_bind_group_layout(0),
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: inputs.as_entire_binding(),
            }],
        });
        let surface = texture(
            &device,
            super::super::native_material::SURFACE_FORMAT,
            wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        );
        let row = u64::from(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT);
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: row * 4,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let mut encoder = device.create_command_encoder(&Default::default());
        {
            let view = surface.create_view(&Default::default());
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: None,
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                ..Default::default()
            });
            pass.set_pipeline(&pipeline);
            pass.set_bind_group(0, &group, &[]);
            pass.draw(0..3, 0..1);
        }
        encoder.copy_texture_to_buffer(
            surface.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT),
                    rows_per_image: Some(4),
                },
            },
            surface.size(),
        );
        queue.submit([encoder.finish()]);
        buffer
            .slice(..)
            .map_async(wgpu::MapMode::Read, |r| r.unwrap());
        device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
        let data = buffer.slice(..).get_mapped_range().unwrap().to_vec();
        buffer.unmap();
        for (i, expected) in bits.iter().enumerate() {
            let (x, y) = (i % 4, i / 4);
            let at = y * row as usize + x * 16;
            let got: [u32; 4] = std::array::from_fn(|c| {
                u32::from_ne_bytes(data[at + c * 4..at + c * 4 + 4].try_into().unwrap())
            });
            assert_eq!(
                got.map(|v| format!("{v:08x}")),
                expected.map(|v| format!("{v:08x}")),
                "texel {x},{y}"
            );
        }
    }

    /// Shader API 0.2: the world decoded, through a chain that passes it on or none, and
    /// Horizon's tonemap or the pack's identity one, comes back encoded as it went in (within
    /// an 8-bit step), at alpha 1.
    #[test]
    fn a_0_2_world_comes_back_through_decode_tonemap_and_encode() {
        assert!(tonemap_entries(false).contains("horizon_encode(min(horizon_sanitize(out)"));
        let Some((device, queue)) = device() else {
            return;
        };
        let post = "[[post]]\nfile = \"a.wgsl\"\n[[post]]\nfile = \"b.wgsl\"\n";
        let curve = "[tonemap]\nfile = \"t.wgsl\"\n";
        // A pack needs a stage: the first has only a sky, and no chain or tonemap.
        let sky = "[sky]\nfile = \"s.wgsl\"\n";
        for extra in [
            sky.into(),
            post.into(),
            curve.into(),
            format!("{post}{curve}"),
        ] {
            let manifest = format!(
                "schema = 1\nid = \"t\"\nname = \"T\"\nversion = \"1\"\napi = \"0.2\"\n{extra}"
            );
            let pass = "fn post(input: PostInput) -> vec4<f32> { return input.color; }\n";
            let identity = "fn tonemap(input: TonemapInput) -> vec3<f32> { return input.color; }\n";
            let mut files = vec![(MANIFEST, manifest.as_str())];
            if extra.contains("[[post]]") {
                files.extend([("a.wgsl", pass), ("b.wgsl", pass)]);
            }
            if extra.contains("[tonemap]") {
                files.push(("t.wgsl", identity));
            }
            if extra.contains("[sky]") {
                files.push((
                    "s.wgsl",
                    "fn sky(input: SkyInput) -> vec4<f32> { return input.color; }\n",
                ));
            }
            let files = files
                .into_iter()
                .map(|(n, t)| (n.to_owned(), t.as_bytes().to_vec()))
                .collect();
            let report = check(&Pack::from_files(files).unwrap()).unwrap();
            let (passes, others): (Vec<_>, Vec<_>) = report
                .stages
                .into_iter()
                .partition(|s| s.kind == StageKind::Post);
            let format = wgpu::TextureFormat::Rgba8Unorm;
            let frame = NativeFrame::new(
                &device,
                None,
                Vec::new(),
                Default::default(),
                None,
                Api::V0_2,
            );
            let inputs = FrameInputs {
                headroom: 1.0,
                ..Default::default()
            };
            queue.write_buffer(&frame.buffer, 0, bytemuck::bytes_of(&inputs));
            let tonemap = others.iter().find(|s| s.kind == StageKind::Tonemap);
            let mut post = NativePost::new(
                &device,
                passes,
                &report.manifest,
                &[],
                format,
                &frame,
                tonemap,
            )
            .unwrap();
            assert_eq!(post.labels().last() == Some(&"t.wgsl"), tonemap.is_some());
            let usage =
                wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING;
            let color = texture(&device, format, usage).create_view(&Default::default());
            let depth = texture(&device, wgpu::TextureFormat::Depth32Float, usage)
                .create_view(&Default::default());
            let screen = texture(
                &device,
                format,
                wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            );
            let mut encoder = device.create_command_encoder(&Default::default());
            let clear = wgpu::Color {
                r: 0.8,
                g: 0.4,
                b: 0.2,
                a: 0.5,
            };
            drop(encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &color,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(clear),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                ..Default::default()
            }));
            let view = screen.create_view(&Default::default());
            let world = (&color, &depth, &frame.surface_view);
            post.run(&device, &mut encoder, world, &view, [4, 4], true, None);
            let buffer = crate::gfx::copy_frame(&device, &mut encoder, &screen, [4, 4]);
            queue.submit([encoder.finish()]);
            buffer
                .slice(..)
                .map_async(wgpu::MapMode::Read, |r| r.unwrap());
            device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
            let pixel = buffer.slice(..).get_mapped_range().unwrap()[..4].to_vec();
            let want = [204, 102, 51, 255];
            assert!(
                pixel
                    .iter()
                    .zip(want)
                    .all(|(&got, want)| got.abs_diff(want) <= 1),
                "{extra}: {pixel:?}"
            );
        }
    }

    /// A 0.2 pack's `Frame` describes the HDR display its picture goes to, and SDR
    /// without one: `hdr` 0, `headroom` 1, `paper_white` and `peak` 80, `black_level` 0.
    #[test]
    fn a_0_2_frame_describes_the_display_it_goes_to() {
        let Some((device, _)) = device() else {
            return;
        };
        let hud = crate::hud::HudBatch::new([4, 4]);
        let frame = crate::gfx::pack_uniforms::initial_history_tests::frame(&hud);
        let mut native = NativeFrame::new(
            &device,
            None,
            Vec::new(),
            Default::default(),
            None,
            Api::V0_2,
        );
        let fields = |i: &FrameInputs| (i.hdr, i.headroom, i.paper_white, i.peak, i.black_level);
        assert_eq!(
            fields(&native.next_inputs(&frame, [4, 4], SDR)),
            (0, 1.0, 80.0, 80.0, 0.0)
        );
        let hdr = |headroom, paper_white, peak, black_level| DisplayValues {
            hdr: true,
            headroom,
            paper_white,
            peak,
            black_level,
        };
        assert_eq!(
            fields(&native.next_inputs(&frame, [4, 4], hdr(4.0, 250.0, 1000.0, 0.05))),
            (1, 4.0, 250.0, 1000.0, 0.05)
        );
        // Read every frame: a change of display shows on the next one.
        let next = native.next_inputs(&frame, [4, 4], hdr(2.0, 300.0, 600.0, 0.0));
        assert_eq!(fields(&next).1, 2.0);
        assert_eq!(fields(&native.next_inputs(&frame, [4, 4], SDR)).0, 0);
    }

    /// On the layered HDR frame a 0.2 pack's light is tone mapped once: by Horizon's curve,
    /// the one HDR without a pack uses, when the pack has no `[tonemap]`, else by the pack's
    /// alone, held to the headroom. In SDR Horizon's is a clip at white. An HDR output switched
    /// to scRGB and back to HDR10 (`Hdr::retargeted`) maps it once all the same.
    #[test]
    fn a_0_2_pack_is_tone_mapped_once_by_its_own_curve_or_horizons() {
        let Some((device, queue)) = device() else {
            return;
        };
        // Light 8 times paper white, as the world's float frame stores it, in gamma.
        let light = 8.0f32;
        let (paper, peak) = (203.0, 1000.0);
        let headroom = peak / paper;
        // The picture shown for that light, in linear light: (a pack's [tonemap], HDR, the
        // output switched).
        let shown = |pack_curve: bool, hdr: bool, switched: bool| {
            let (stage, file, source) = if pack_curve {
                (
                    "[tonemap]",
                    "t.wgsl",
                    "fn tonemap(input: TonemapInput) -> vec3<f32> { return input.color; }\n",
                )
            } else {
                (
                    "[sky]",
                    "s.wgsl",
                    "fn sky(input: SkyInput) -> vec4<f32> { return input.color; }\n",
                )
            };
            let manifest = format!(
                "schema = 1\nid = \"t\"\nname = \"T\"\nversion = \"1\"\napi = \"0.2\"\n\
                 {stage}\nfile = \"{file}\"\n"
            );
            let files = [(MANIFEST, manifest.as_str()), (file, source)]
                .into_iter()
                .map(|(n, t)| (n.to_owned(), t.as_bytes().to_vec()))
                .collect();
            let report = check(&Pack::from_files(files).unwrap()).unwrap();
            let tonemap = report.stages.iter().find(|s| s.kind == StageKind::Tonemap);
            let format = wgpu::TextureFormat::Rgba16Float;
            let frame = NativeFrame::new(
                &device,
                None,
                Vec::new(),
                Default::default(),
                None,
                Api::V0_2,
            );
            let inputs = FrameInputs {
                hdr: u32::from(hdr),
                headroom: if hdr { headroom } else { 1.0 },
                ..Default::default()
            };
            queue.write_buffer(&frame.buffer, 0, bytemuck::bytes_of(&inputs));
            let mut post = NativePost::new(
                &device,
                Vec::new(),
                &report.manifest,
                &[],
                format,
                &frame,
                tonemap,
            )
            .unwrap();
            assert_eq!(post.tonemaps(), pack_curve);
            let mut display = super::super::hdr::Hdr::new(
                &device,
                &queue,
                format,
                super::super::hdr::Output::Pq,
                true,
            );
            (display.paper_white, display.peak) = (paper, peak);
            if switched {
                use super::super::hdr::Output::{Pq, ScRgb};
                display = display.retargeted(&device, &queue, ScRgb);
                display = display.retargeted(&device, &queue, Pq);
            }
            let screen = display.frame(&device, [4, 4]);
            let usage =
                wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING;
            let color = texture(&device, format, usage);
            let depth = texture(&device, wgpu::TextureFormat::Depth32Float, usage);
            let [color, depth] = [&color, &depth].map(|t| t.create_view(&Default::default()));
            let mut encoder = device.create_command_encoder(&Default::default());
            let g = f64::from(light.powf(1.0 / 2.2));
            drop(encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &color,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: g,
                            g,
                            b: g,
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                ..Default::default()
            }));
            let view = screen.create_view(&Default::default());
            let world = (&color, &depth, &frame.surface_view);
            post.run(&device, &mut encoder, world, &view, [4, 4], true, None);
            // The renderer's rule, then its curve, as it runs them after the pack's chain.
            if crate::gfx::horizon_tone_curve(hdr, true, post.tonemaps()) {
                display.begin(&queue, true);
                display.tonemap(&mut encoder, None);
            }
            let buffer = device.create_buffer(&wgpu::BufferDescriptor {
                label: None,
                size: 256,
                usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                mapped_at_creation: false,
            });
            encoder.copy_texture_to_buffer(
                screen.as_image_copy(),
                wgpu::TexelCopyBufferInfo {
                    buffer: &buffer,
                    layout: wgpu::TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(256),
                        rows_per_image: None,
                    },
                },
                wgpu::Extent3d {
                    width: 1,
                    height: 1,
                    depth_or_array_layers: 1,
                },
            );
            queue.submit([encoder.finish()]);
            buffer.map_async(wgpu::MapMode::Read, .., |r| r.unwrap());
            device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
            let bytes = buffer.get_mapped_range(..).unwrap();
            half::f16::from_le_bytes([bytes[0], bytes[1]])
                .to_f32()
                .powf(2.2)
        };
        let near = |got: f32, want: f32| (got - want).abs() <= 0.01 * want;
        // Horizon's curve once: BT.2390's EETF at 203 and 1000 nits puts light 8 at 4.82
        // (worked out apart from the shader, as in the HDR tone curve's own test). Clipped at
        // the headroom first, or mapped twice, it would land lower.
        for switched in [false, true] {
            let horizon = shown(false, true, switched);
            assert!(near(horizon, 4.82043), "{switched}: {horizon}");
            // The pack's identity curve alone: the light held to the headroom, not eased again.
            let pack = shown(true, true, switched);
            assert!(near(pack, headroom), "{switched}: {pack}");
        }
        // SDR: a clip at white, the pack's or Horizon's.
        for pack_curve in [false, true] {
            let sdr = shown(pack_curve, false, false);
            assert!(near(sdr, 1.0), "{pack_curve}: {sdr}");
        }
    }
}
