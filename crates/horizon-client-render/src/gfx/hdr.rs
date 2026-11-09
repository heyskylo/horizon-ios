//! HDR output (Horizon setting, off by default): the frame renders exactly as
//! without it, in vanilla's gamma space, into a 16-bit target when HDR output
//! was on when the renderer was made, else an 8-bit one
//! (`Renderer::frame_format`), and one pass (`hdr.wgsl`) writes it to an HDR10
//! or scRGB swapchain at the player's paper white, expanding the world's
//! highlights towards their peak brightness. The world is copied aside before
//! the hand and HUD so only it is expanded.
//!
//! On the float frame Horizon's world light is the same on every display (`TIERS`,
//! `SUNLIT_GAIN`, up to `SCENE_PEAK`), and the tone curve alone fits it to the display's
//! peak (docs/design/hdr-linear-light.md, step 6a).

/// The swapchain HDR output presents to.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Output {
    /// HDR10: 10-bit BT.2020 with the PQ curve, the display's own signal.
    Pq,
    /// scRGB: linear 16-bit float, BT.709 primaries, 1.0 = 80 nits.
    ScRgb,
}

impl Output {
    pub fn name(self) -> &'static str {
        match self {
            Self::Pq => "HDR10 (PQ)",
            Self::ScRgb => "scRGB",
        }
    }
    pub fn format(self) -> wgpu::TextureFormat {
        match self {
            Self::Pq => wgpu::TextureFormat::Rgb10a2Unorm,
            Self::ScRgb => wgpu::TextureFormat::Rgba16Float,
        }
    }
    pub fn color_space(self) -> wgpu::SurfaceColorSpace {
        match self {
            Self::Pq => wgpu::SurfaceColorSpace::Bt2100Pq,
            Self::ScRgb => wgpu::SurfaceColorSpace::ExtendedSrgbLinear,
        }
    }
    fn space(self) -> wgpu::SurfaceColorSpaces {
        match self {
            Self::Pq => wgpu::SurfaceColorSpaces::BT2100_PQ,
            Self::ScRgb => wgpu::SurfaceColorSpaces::EXTENDED_SRGB_LINEAR,
        }
    }
}

/// `HORIZON_HDR_OUTPUT`'s names, in order of preference.
const NAMES: [(Output, &str); 2] = [(Output::Pq, "pq"), (Output::ScRgb, "scrgb")];

/// The HDR output `caps` can present: HDR10 where offered, else scRGB.
/// `HORIZON_HDR_OUTPUT=pq` or `scrgb` allows only that one.
pub fn output(caps: &wgpu::SurfaceCapabilities) -> Option<Output> {
    pick(caps, std::env::var("HORIZON_HDR_OUTPUT").ok().as_deref())
}
fn pick(caps: &wgpu::SurfaceCapabilities, only: Option<&str>) -> Option<Output> {
    NAMES
        .into_iter()
        .filter(|(_, name)| only.is_none_or(|o| o == *name))
        .map(|(output, _)| output)
        .find(|o| caps.color_spaces(o.format()).contains(o.space()))
}
/// The HDR output an offscreen renderer (`--offscreen`) offers: it has no display to ask, so
/// only the one `HORIZON_HDR_OUTPUT` names, for HDR screenshots without an HDR display.
pub fn offscreen_output() -> Option<Output> {
    named(std::env::var("HORIZON_HDR_OUTPUT").ok().as_deref())
}
fn named(name: Option<&str>) -> Option<Output> {
    NAMES
        .into_iter()
        .find(|(_, n)| name == Some(*n))
        .map(|(output, _)| output)
}

struct Targets {
    size: [u32; 2],
    frame: wgpu::Texture,
    /// The world as `keep_world` kept it; also the tone curve's input scratch on a layered frame.
    world: wgpu::Texture,
    coverage: Option<wgpu::Texture>,
    /// The layered frame's HUD target: what the HUD draws on.
    hud: Option<wgpu::Texture>,
    group: wgpu::BindGroup,
    /// The layered frame's bindings for `tonemap`, `keep_world` and `composite`.
    passes: Option<[wgpu::BindGroup; 3]>,
}

/// The float frame with HUD coverage (`hud_coverage::supported`, docs/design/hdr-linear-light.md
/// "The HUD and menus"): the default world draws write light directly; the tone curve eases
/// it into the peak after the post effects (`tonemap`); the
/// camera overlays draw on that light; the HUD draws on a target of its own,
/// the world clipped to white (`keep_world`); and the last pass composites the HUD over the
/// world's light by its coverage (`fs_hdr_layered`), as the menu blur does before it blurs
/// (`composite`).
struct Layered {
    tonemap: (wgpu::BindGroupLayout, wgpu::RenderPipeline),
    keep: (wgpu::BindGroupLayout, wgpu::RenderPipeline),
    composite: (wgpu::BindGroupLayout, wgpu::RenderPipeline),
    /// Compiled only when a screenshot needs it, for its readback format and HUD mode.
    screenshot: Option<(wgpu::TextureFormat, bool, bool, wgpu::RenderPipeline)>,
    /// What `tonemap` and `composite` read, written before they run (`begin`).
    params: wgpu::Buffer,
}

/// How one tier of light sources (`mesher::hdr_tier`) shines: its brightest pixels are
/// multiplied by `gain`, in multiples of paper white, lifted from brightness `start` (none) to
/// `full` (all of it), the texel's own brightness before lighting.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tier {
    pub gain: f32,
    pub start: f32,
    pub full: f32,
}
/// Low, incandescent, bright and very bright: faint glowers such as magma, flames and lava,
/// glowstone and campfires, the sun.
pub const TIERS: [Tier; 4] = [
    Tier {
        gain: 1.6,
        start: 0.08,
        full: 0.09,
    },
    Tier {
        gain: 2.4,
        start: 0.14,
        full: 0.38,
    },
    Tier {
        gain: 2.8,
        start: 0.14,
        full: 0.38,
    },
    Tier {
        gain: 4.0,
        start: 0.14,
        full: 0.38,
    },
];
/// The scene's peak, in multiples of paper white: just above the brightest light the world
/// writes (the sun's disc, white at the very bright tier's 4), and what the tone curve maps
/// onto the display's peak; light past it (where the sky adds onto the sun) is held at it. The
/// world is mastered for a 1000-nit display at ITU-R BT.2408's 203-nit reference white, as
/// HDR10 content usually is: there it shows as written, a dimmer display eases its highlights
/// into its peak (BT.2390's EETF), and a brighter one shows it as at 1000.
pub const SCENE_PEAK: f32 = 1000.0 / 203.0;
/// HDR lighting: what the strongest light multiplies a surface by (`hdr_light.wgsl`).
pub const SUNLIT_GAIN: f32 = 3.0;
/// The compatibility frame's tiers (the 16-bit or 8-bit frame, until step 5 removes it): each
/// one's share of the display's headroom, as before one peak, so the look HDR has by default
/// changes once, when the float frame replaces it.
const COMPAT_SHARES: [f32; 4] = [0.2, 0.5, 0.6, 1.0];

/// Which pixels of a light source's sprite may shine (`hdr_stats.wgsl`), as a fraction of
/// the way from its darkest to its brightest opaque pixel: `parts` for one whose bright parts
/// shine (a torch's flame, not its handle), `whole` for one that shines all over
/// (`mesher::WHOLE`). The tier windows still decide how much.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Masks {
    pub parts: f32,
    pub whole: f32,
}
pub const MASKS: Masks = Masks {
    parts: 0.32,
    whole: 0.0,
};
/// A step of HDR calibration (`crate::hdr_calibration`): its test picture is drawn in nits
/// above the controls instead of the frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Calibration {
    /// 1 black level, 2 peak brightness, 3 paper white.
    pub step: u32,
    /// The slider's value, in nits.
    pub nits: f32,
    /// Where the controls start, as a share of the height.
    pub band: f32,
}

pub struct Hdr {
    layout: wgpu::BindGroupLayout,
    /// Horizon's logo as a 1-bit mask, for calibration's test pictures.
    logo: wgpu::TextureView,
    pipeline: wgpu::RenderPipeline,
    /// Zeroes the frame's alpha once the world is kept (`keep_world`): what draws after,
    /// the hand, HUD and menus, raises it, so HDR output knows where they cover the world.
    clear_alpha: wgpu::RenderPipeline,
    /// Scales the frame's alpha down to 1/255 where the hand drew (`mark_hand`), so HDR
    /// output keeps the hand at the world's brightness.
    hand_mark: wgpu::RenderPipeline,
    params: wgpu::Buffer,
    /// The same pass drawing HDR10 for a screenshot (`capture`), with its own parameters,
    /// so it can differ from the swapchain's in one frame.
    capture_pipeline: wgpu::RenderPipeline,
    capture_params: wgpu::Buffer,
    pub output: Output,
    /// The frame's format: what every other pipeline draws in.
    format: wgpu::TextureFormat,
    targets: Option<Targets>,
    /// `HORIZON_HDR_DEBUG=1`: grey what isn't expanded, tint what is magenta.
    debug: bool,
    layered: Option<Layered>,
    /// Paper white and peak brightness, in nits.
    pub paper_white: f32,
    pub peak: f32,
    /// The display's black level, in nits: shadows are lifted onto it (`hdr.wgsl`).
    pub black: f32,
    pub tiers: [Tier; 4],
    /// The HUD and menus' brightness, as a share of paper white (0.25..=1).
    pub ui: f32,
    /// Which pixels of a light source shine (`Masks`).
    pub masks: Masks,
    /// HDR lighting, whether Horizon lifts this frame's world: off under a Horizon shader pack
    /// that draws, while a `shaders/` pack's post or material stage runs (the pack's own),
    /// with HDR Lighting (`look`) off, and with `HORIZON_HDR_LIGHTING=0`, to compare.
    pub lighting: bool,
    /// The HDR Lighting setting: off, nothing in the world is lifted, light sources and the
    /// sun included, so the picture is SDR's on the HDR canvas.
    pub look: bool,
    /// Calibration's test picture while it runs.
    pub calibration: Option<Calibration>,
}

pub(super) fn shader() -> String {
    include_str!("../hdr_light.wgsl").to_owned() + include_str!("../hdr.wgsl")
}

impl Hdr {
    pub fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        format: wgpu::TextureFormat,
        output: Output,
        coverage: bool,
    ) -> Self {
        let debug = std::env::var("HORIZON_HDR_DEBUG").ok();
        Self::with_debug(device, queue, format, output, coverage, debug.as_deref())
    }

    fn with_debug(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        format: wgpu::TextureFormat,
        output: Output,
        coverage: bool,
        debug: Option<&str>,
    ) -> Self {
        let coverage_debug = coverage && debug == Some("coverage");
        // The layered frame's last pass, else today's.
        let entry_point = if coverage { "fs_hdr_layered" } else { "fs_hdr" };
        let mut source = shader();
        if coverage_debug {
            // The debug view wraps the last pass as it is, so the picture under the tint is
            // exactly the one without it.
            let entry = format!(
                "@fragment fn {entry_point}(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32>"
            );
            assert!(
                source.contains(&entry),
                "hdr.wgsl's {entry_point} changed its signature"
            );
            source = source.replace(&entry, "fn picture_hdr(pos: vec4<f32>) -> vec4<f32>");
            source.push_str(&entry);
            source.push_str(
                r#" {
    let picture = picture_hdr(pos);
    let c = textureLoad(coverage, vec2<i32>(pos.xy), 0).r;
    // Magenta at paper white, in the output's own encoding.
    let paper = vec3(params.paper, 0.0, params.paper);
    let magenta = select(paper, pq(BT709_TO_BT2020 * paper * 80.0), params.pq > 0.5);
    return vec4(mix(picture.rgb, magenta, c), picture.a);
}
"#,
            );
        }
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Horizon HDR"),
            source: wgpu::ShaderSource::Wgsl(source.into()),
        });
        // hdr.wgsl's bindings: the textures at `textures`, and its `Params` at 2 if `params`.
        let layout = |label, textures: &[u32], params: bool| {
            let texture = |binding| wgpu::BindGroupLayoutEntry {
                binding,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: false },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            };
            let mut entries: Vec<_> = textures.iter().map(|&b| texture(b)).collect();
            if params {
                entries.push(wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                });
            }
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some(label),
                entries: &entries,
            })
        };
        // One full-screen pass of hdr.wgsl's `entry` into `format`, reading `layout`.
        let pass = |label, layout: &wgpu::BindGroupLayout, entry, format| {
            let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some(label),
                bind_group_layouts: &[Some(layout)],
                ..Default::default()
            });
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some("vs_hdr"),
                    compilation_options: Default::default(),
                    buffers: &[],
                },
                primitive: Default::default(),
                depth_stencil: None,
                multisample: Default::default(),
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some(entry),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format,
                        blend: None,
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        let layout_bindings: &[u32] = if coverage { &[0, 3, 4, 5] } else { &[0, 1, 3] };
        let display = layout("Horizon HDR bindings", layout_bindings, true);
        let pipeline_for = |format| pass("Horizon HDR", &display, entry_point, format);
        let params = || {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("Horizon HDR parameters"),
                size: 80,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            })
        };
        let layered = coverage.then(|| {
            let with = |label, textures: &[u32], params, entry, format| {
                let layout = layout(label, textures, params);
                let pipeline = pass(label, &layout, entry, format);
                (layout, pipeline)
            };
            let hud = super::hud_coverage::HUD_FORMAT;
            Layered {
                tonemap: with("Horizon HDR tone curve", &[1], true, "fs_tonemap", format),
                keep: with("Horizon HDR HUD backdrop", &[0], false, "fs_keep", hud),
                composite: with(
                    "Horizon HDR composite",
                    &[0, 4, 5],
                    true,
                    "fs_composite",
                    format,
                ),
                screenshot: None,
                params: params(),
            }
        });
        let alpha_pass = |label, blend| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(&device.create_pipeline_layout(&Default::default())),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some("vs_hdr"),
                    compilation_options: Default::default(),
                    buffers: &[],
                },
                primitive: Default::default(),
                depth_stencil: None,
                multisample: Default::default(),
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some("fs_clear_alpha"),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format,
                        blend,
                        write_mask: wgpu::ColorWrites::ALPHA,
                    })],
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        let clear_alpha = alpha_pass("Horizon HDR UI mask", None);
        // Alpha times the blend constant (1/255): the hand's 1 becomes 1/255, the world's 0
        // stays 0.
        let scale_alpha = wgpu::BlendComponent {
            src_factor: wgpu::BlendFactor::Zero,
            dst_factor: wgpu::BlendFactor::Constant,
            operation: wgpu::BlendOperation::Add,
        };
        let hand_mark = alpha_pass(
            "Horizon HDR hand mark",
            Some(wgpu::BlendState {
                color: scale_alpha,
                alpha: scale_alpha,
            }),
        );
        Self {
            logo: logo_mask(device, queue),
            pipeline: pipeline_for(output.format()),
            capture_pipeline: pipeline_for(Output::Pq.format()),
            layout: display,
            clear_alpha,
            hand_mark,
            params: params(),
            capture_params: params(),
            output,
            format,
            targets: None,
            debug: debug == Some("1"),
            layered,
            paper_white: 200.0,
            peak: 1000.0,
            black: 0.0,
            tiers: TIERS,
            ui: 1.0,
            masks: MASKS,
            lighting: true,
            look: true,
            calibration: None,
        }
    }

    /// The same HDR output presenting `output` instead (`output_apply`): the same frame and
    /// HUD layer, and the player's tuning as it is (HDR Lighting, the world's lighting, the UI
    /// brightness, the masks and tiers, the debug view, calibration and the luminance). Its
    /// targets are made again on the next frame.
    pub fn retargeted(&self, device: &wgpu::Device, queue: &wgpu::Queue, output: Output) -> Self {
        Self {
            debug: self.debug,
            paper_white: self.paper_white,
            peak: self.peak,
            black: self.black,
            tiers: self.tiers,
            ui: self.ui,
            masks: self.masks,
            lighting: self.lighting,
            look: self.look,
            calibration: self.calibration,
            ..Self::new(device, queue, self.format, output, self.layered.is_some())
        }
    }

    /// The frame every pass draws into this frame, sized like the window.
    pub fn frame(&mut self, device: &wgpu::Device, size: [u32; 2]) -> wgpu::Texture {
        if self.targets.as_ref().is_none_or(|t| t.size != size) {
            let texture = |label, format, usage| {
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
                    usage: usage | wgpu::TextureUsages::TEXTURE_BINDING,
                    view_formats: &[],
                })
            };
            let frame = texture(
                "Horizon HDR frame",
                self.format,
                wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            );
            let world = texture(
                "Horizon HDR world",
                self.format,
                wgpu::TextureUsages::COPY_DST | wgpu::TextureUsages::RENDER_ATTACHMENT,
            );
            let layered = self.layered.is_some();
            let coverage = layered.then(|| {
                texture(
                    "HUD coverage",
                    super::hud_coverage::FORMAT,
                    wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
                )
            });
            // 8-bit, so each HUD draw clips and rounds as on the frame with HDR off.
            let hud = layered.then(|| {
                texture(
                    "HUD",
                    super::hud_coverage::HUD_FORMAT,
                    wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
                )
            });
            let group = self.group(
                device,
                &frame,
                &world,
                coverage.as_ref(),
                hud.as_ref(),
                &self.params,
            );
            let passes = self
                .layered
                .as_ref()
                .zip(coverage.as_ref().zip(hud.as_ref()))
                .map(|(l, (coverage, hud))| {
                    let view = |t: &wgpu::Texture| t.create_view(&Default::default());
                    let (frame, world, coverage, hud) =
                        (view(&frame), view(&world), view(coverage), view(hud));
                    let entry = |binding, view| wgpu::BindGroupEntry {
                        binding,
                        resource: wgpu::BindingResource::TextureView(view),
                    };
                    let params = wgpu::BindGroupEntry {
                        binding: 2,
                        resource: l.params.as_entire_binding(),
                    };
                    let group = |layout, entries: &[wgpu::BindGroupEntry]| {
                        device.create_bind_group(&wgpu::BindGroupDescriptor {
                            label: Some("Horizon HDR layered bindings"),
                            layout,
                            entries,
                        })
                    };
                    [
                        group(&l.tonemap.0, &[entry(1, &world), params.clone()]),
                        group(&l.keep.0, &[entry(0, &frame)]),
                        group(
                            &l.composite.0,
                            &[
                                entry(0, &frame),
                                params,
                                entry(4, &coverage),
                                entry(5, &hud),
                            ],
                        ),
                    ]
                });
            self.targets = Some(Targets {
                size,
                frame,
                world,
                coverage,
                hud,
                group,
                passes,
            });
        }
        self.targets.as_ref().unwrap().frame.clone()
    }

    /// Lets the targets go, a scaled screenshot's: the next `frame` makes them again.
    pub fn release_targets(&mut self) {
        self.targets = None;
    }

    /// The float frame with HUD coverage (`Layered`).
    pub fn layered(&self) -> bool {
        self.layered.is_some()
    }

    /// The layered frame's HUD target, which the HUD and menus draw on instead of the frame
    /// once `keep_world` made it.
    pub fn hud(&self) -> Option<&wgpu::Texture> {
        self.targets.as_ref()?.hud.as_ref()
    }

    /// The layered frame's SDR screenshot, directly in the readback's texture; the icon
    /// maps only the world. Where Horizon lifted nothing (`lighting` off: HDR Lighting off,
    /// or under a shader pack) nothing is mapped. Neither the display's targets nor its
    /// parameters are changed.
    pub fn sdr_screenshot(
        &mut self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        format: wgpu::TextureFormat,
        world_only: bool,
    ) -> Option<wgpu::Texture> {
        let shoulder = self.lighting;
        let (l, t) = (self.layered.as_mut()?, self.targets.as_ref()?);
        if l.screenshot
            .as_ref()
            .is_none_or(|(f, w, s, _)| (*f, *w, *s) != (format, world_only, shoulder))
        {
            let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("HDR SDR screenshot"),
                source: wgpu::ShaderSource::Wgsl(shader().into()),
            });
            let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("HDR SDR screenshot"),
                layout: None,
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some("vs_hdr"),
                    compilation_options: Default::default(),
                    buffers: &[],
                },
                primitive: Default::default(),
                depth_stencil: None,
                multisample: Default::default(),
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some("fs_sdr_screenshot"),
                    compilation_options: wgpu::PipelineCompilationOptions {
                        constants: &[
                            ("screenshot_world_only", f64::from(u8::from(world_only))),
                            ("screenshot_shoulder", f64::from(u8::from(shoulder))),
                        ],
                        ..Default::default()
                    },
                    targets: &[Some(format.into())],
                }),
                multiview_mask: None,
                cache: None,
            });
            l.screenshot = Some((format, world_only, shoulder, pipeline));
        }
        let pipeline = &l.screenshot.as_ref()?.3;
        let views = [
            (0, &t.frame),
            (4, t.coverage.as_ref()?),
            (5, t.hud.as_ref()?),
        ]
        .map(|(binding, texture)| (binding, texture.create_view(&Default::default())));
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("HDR SDR screenshot"),
            layout: &pipeline.get_bind_group_layout(0),
            entries: &views
                .each_ref()
                .map(|(binding, view)| wgpu::BindGroupEntry {
                    binding: *binding,
                    resource: wgpu::BindingResource::TextureView(view),
                }),
        });
        let copy = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Horizon screenshot copy"),
            size: t.frame.size(),
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        Self::draw(
            encoder,
            pipeline,
            &group,
            &copy.create_view(&Default::default()),
            "HDR SDR screenshot",
            None,
        );
        Some(copy)
    }

    /// On the layered frame, before `tonemap` and `composite`: what they read, for a frame that
    /// drew a world (`world`) or not.
    pub fn begin(&self, queue: &wgpu::Queue, world: bool) {
        if let Some(l) = &self.layered {
            queue.write_buffer(
                &l.params,
                0,
                bytemuck::cast_slice(&self.params(world, false)),
            );
        }
    }

    /// On the layered frame, after the post effects and before the camera overlays: the tone
    /// curve eases the light into the peak (`fs_tonemap`).
    pub fn tonemap(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        timing: Option<&mut super::pack_timing::PassTiming>,
    ) {
        let (Some(l), Some(t)) = (&self.layered, &self.targets) else {
            return;
        };
        let Some([group, ..]) = &t.passes else {
            return;
        };
        encoder.copy_texture_to_texture(
            t.frame.as_image_copy(),
            t.world.as_image_copy(),
            t.frame.size(),
        );
        let view = t.frame.create_view(&Default::default());
        Self::draw(
            encoder,
            &l.tonemap.1,
            group,
            &view,
            "HDR tone curve",
            timing,
        );
    }

    /// On the layered frame: the picture so far (`fs_composite`), as the frame stores light,
    /// into `target`, for the menu blur to blur it and make it the world (`keep_world` after).
    pub fn composite(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
        timing: Option<&mut super::pack_timing::PassTiming>,
    ) {
        if let (
            Some(l),
            Some(Targets {
                passes: Some([_, _, group]),
                ..
            }),
        ) = (&self.layered, &self.targets)
        {
            Self::draw(
                encoder,
                &l.composite.1,
                group,
                target,
                "HDR composite",
                timing,
            );
        }
    }

    pub fn coverage_view(&self) -> Option<wgpu::TextureView> {
        Some(
            self.targets
                .as_ref()?
                .coverage
                .as_ref()?
                .create_view(&Default::default()),
        )
    }

    /// The peak the HDR metadata gives: while calibration's peak step runs, PQ's own
    /// 10 000 nits, so a compositor that clips at the declared peak (KWin) lets the test
    /// patch through as bright as the display goes and the logo under it shows.
    pub fn declared_peak(&self) -> f32 {
        match self.calibration {
            Some(c) if c.step == 2 => 10_000.0,
            _ => self.peak,
        }
    }

    /// Keeps the world as drawn so far, before the hand and HUD cover it, then zeroes the
    /// frame's alpha: the world's marks live on in the copy, and whatever draws from here
    /// on raises the alpha where it covers the world.
    ///
    /// On the layered frame the frame keeps the world, the hand and the camera overlays, as
    /// light, and this makes the HUD's backdrop from it: clipped to white, alpha 0 (`fs_keep`).
    pub fn keep_world(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        timing: Option<&mut super::pack_timing::PassTiming>,
    ) {
        let Some(t) = &self.targets else {
            return;
        };
        if let (Some(l), Some(hud), Some([_, group, _])) = (&self.layered, &t.hud, &t.passes) {
            let view = hud.create_view(&Default::default());
            Self::draw(encoder, &l.keep.1, group, &view, "HDR HUD backdrop", timing);
            return;
        }
        encoder.copy_texture_to_texture(
            t.frame.as_image_copy(),
            t.world.as_image_copy(),
            t.frame.size(),
        );
        let view = t.frame.create_view(&Default::default());
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
            label: Some("Horizon HDR UI mask"),
            timestamp_writes: timing.and_then(|t| t.writes("HDR UI mask")),
            color_attachments: &attachments,
            ..Default::default()
        });
        pass.set_pipeline(&self.clear_alpha);
        pass.draw(0..3, 0..1);
    }

    /// After the hand: its pixels' alpha goes down to 1/255, below anything the HUD and
    /// menus leave, so HDR output tells the hand (the world's brightness, never lifted)
    /// from the UI (UI Brightness). The layered frame keeps the hand before `keep_world`,
    /// so it needs no mark there.
    pub fn mark_hand(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        timing: Option<&mut super::pack_timing::PassTiming>,
    ) {
        let Some(t) = self.targets.as_ref().filter(|_| self.layered.is_none()) else {
            return;
        };
        let view = t.frame.create_view(&Default::default());
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
            label: Some("Horizon HDR hand mark"),
            timestamp_writes: timing.and_then(|t| t.writes("HDR hand mark")),
            color_attachments: &attachments,
            ..Default::default()
        });
        pass.set_pipeline(&self.hand_mark);
        let a = 1.0 / 255.0;
        pass.set_blend_constant(wgpu::Color {
            r: a,
            g: a,
            b: a,
            a,
        });
        pass.draw(0..3, 0..1);
    }

    /// The world as `keep_world` kept it: where the hand's marks go (its alpha, which HDR output
    /// reads for it too), and what tints the world rather than covering it. None on the
    /// layered frame, where both draw on the frame before `keep_world`.
    pub fn hand_light_view(&self) -> Option<wgpu::TextureView> {
        let t = self.targets.as_ref().filter(|_| self.layered.is_none())?;
        Some(t.world.create_view(&Default::default()))
    }

    /// Writes the finished frame to the swapchain image `target`.
    /// The tiers' windows for the shaders that mark the frame (`Globals::hdr_windows`):
    /// where each starts, then where each is full.
    pub fn windows(&self) -> [[f32; 4]; 2] {
        [self.tiers.map(|t| t.start), self.tiers.map(|t| t.full)]
    }
    pub fn present(
        &self,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::Texture,
        world: bool,
        timing: Option<&mut super::pack_timing::PassTiming>,
    ) {
        let Some(t) = &self.targets else {
            return;
        };
        queue.write_buffer(
            &self.params,
            0,
            bytemuck::cast_slice(&self.params(world, self.output == Output::Pq)),
        );
        let view = target.create_view(&Default::default());
        Self::draw(
            encoder,
            &self.pipeline,
            &t.group,
            &view,
            "HDR output",
            timing,
        );
    }

    /// The frame `present` writes, as HDR10 (BT.2020, PQ, 10 bits a channel) whatever
    /// the swapchain is, in a texture of its own for a screenshot (`Rgb10a2Unorm`).
    pub fn capture(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        world: bool,
    ) -> Option<wgpu::Texture> {
        let t = self.targets.as_ref()?;
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Horizon HDR screenshot"),
            size: t.frame.size(),
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: Output::Pq.format(),
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let group = self.group(
            device,
            &t.frame,
            &t.world,
            t.coverage.as_ref(),
            t.hud.as_ref(),
            &self.capture_params,
        );
        queue.write_buffer(
            &self.capture_params,
            0,
            bytemuck::cast_slice(&self.params(world, true)),
        );
        let view = texture.create_view(&Default::default());
        Self::draw(
            encoder,
            &self.capture_pipeline,
            &group,
            &view,
            "HDR output",
            None,
        );
        Some(texture)
    }

    fn group(
        &self,
        device: &wgpu::Device,
        frame: &wgpu::Texture,
        world: &wgpu::Texture,
        coverage: Option<&wgpu::Texture>,
        hud: Option<&wgpu::Texture>,
        params: &wgpu::Buffer,
    ) -> wgpu::BindGroup {
        let view = |t: &wgpu::Texture| t.create_view(&Default::default());
        let (frame, world) = (view(frame), view(world));
        let layered = coverage.zip(hud).map(|(c, h)| (view(c), view(h)));
        let texture = |binding, view| wgpu::BindGroupEntry {
            binding,
            resource: wgpu::BindingResource::TextureView(view),
        };
        let mut entries = vec![
            texture(0, &frame),
            wgpu::BindGroupEntry {
                binding: 2,
                resource: params.as_entire_binding(),
            },
            texture(3, &self.logo),
        ];
        match &layered {
            Some((coverage, hud)) => entries.extend([texture(4, coverage), texture(5, hud)]),
            None => entries.push(texture(1, &world)),
        }
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Horizon HDR bindings"),
            layout: &self.layout,
            entries: &entries,
        })
    }

    /// What the world's writers read (`hdr_written`): the scene's peak, HDR lighting's gain,
    /// world/look and debug, then the tiers' gains. No part of the display reaches them.
    pub(super) fn draw_params(&self, world: bool) -> ([f32; 4], [f32; 4]) {
        let p = self.params(world, false);
        let sunlit = if self.lighting { SUNLIT_GAIN } else { 1.0 };
        ([SCENE_PEAK, sunlit, p[2], p[3]], self.tiers.map(|t| t.gain))
    }

    /// hdr.wgsl's `Params`.
    fn params(&self, world: bool, pq: bool) -> [f32; 20] {
        let paper = self.paper_white.max(1.0);
        let c = self.calibration;
        let mut params = [0.0; 20];
        params[..5].copy_from_slice(&[
            paper / 80.0,
            (self.peak / paper).max(1.0),
            f32::from(u8::from(world && self.look)),
            f32::from(u8::from(self.debug)),
            f32::from(u8::from(pq)),
        ]);
        params[5] = self.black.clamp(0.0, paper);
        params[6] = self.ui.clamp(0.25, 1.0);
        params[7] = if self.lighting {
            scene_gain(self.peak, self.paper_white)
        } else {
            1.0
        };
        params[8..12].copy_from_slice(&COMPAT_SHARES);
        params[12] = c.map_or(0.0, |c| c.step as f32);
        params[13] = c.map_or(0.0, |c| c.nits);
        params[14] = c.map_or(0.0, |c| c.band);
        // Only Horizon's lifted world is known to end at the scene's peak; elsewhere (HDR
        // Lighting off, a pack's own light, the title) the curve's source follows the display.
        params[16] = if world && self.lighting {
            SCENE_PEAK
        } else {
            0.0
        };
        params
    }

    /// One full-screen pass of `pipeline` into `view`.
    fn draw(
        encoder: &mut wgpu::CommandEncoder,
        pipeline: &wgpu::RenderPipeline,
        group: &wgpu::BindGroup,
        view: &wgpu::TextureView,
        label: &'static str,
        timing: Option<&mut super::pack_timing::PassTiming>,
    ) {
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
            timestamp_writes: timing.and_then(|t| t.writes(label)),
            color_attachments: &attachments,
            ..Default::default()
        });
        pass.set_pipeline(pipeline);
        pass.set_bind_group(0, group, &[]);
        pass.draw(0..3, 0..1);
    }
}

/// Horizon's icon (horizon-theme) as a 1-bit mask: its white strokes, one colour with hard
/// edges, are the logo; everything else is not.
fn logo_mask(device: &wgpu::Device, queue: &wgpu::Queue) -> wgpu::TextureView {
    use wgpu::util::DeviceExt;
    let (w, h, rgba) =
        crate::hud::decode_bytes(horizon_theme::ICON_PNG).unwrap_or((1, 1, vec![0; 4]));
    let mask: Vec<u8> = rgba
        .as_chunks::<4>()
        .0
        .iter()
        .map(|p| {
            if p[3] > 128 && p[..3].iter().all(|&c| c > 200) {
                255
            } else {
                0
            }
        })
        .collect();
    device
        .create_texture_with_data(
            queue,
            &wgpu::TextureDescriptor {
                label: Some("Horizon HDR calibration logo"),
                size: wgpu::Extent3d {
                    width: w,
                    height: h,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::R8Unorm,
                usage: wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            },
            wgpu::util::TextureDataOrder::LayerMajor,
            &mask,
        )
        .create_view(&Default::default())
}

/// The compatibility frame's HDR lighting (`fs_hdr`): what the strongest light multiplies a
/// surface by, 4, which looked right across biomes; less only where there is little headroom
/// to ease into. The float frame's is `SUNLIT_GAIN`.
fn scene_gain(peak: f32, paper_white: f32) -> f32 {
    (peak / paper_white.max(1.0) * 1.75).clamp(1.0, 4.0)
}

#[cfg(test)]
mod tests {
    use super::{Hdr, Output, named, pick};
    use wgpu::{SurfaceColorSpaces as S, TextureFormat as F};

    #[test]
    fn shared_light_matches_the_original_for_every_mark() {
        use wgpu::util::DeviceExt;
        let Some((device, queue)) = gpu() else {
            eprintln!("HDR equivalence GPU unavailable");
            return;
        };
        // All 8-bit marks, both easing policies, non-grey and over-white colours, look,
        // debug, headroom and tier shares; half-float neighbours straddle mark boundaries. The
        // writers take the gain that a share gives.
        let source = include_str!("../hdr_light.wgsl").to_owned()
            + r#"
struct ReferenceParams { peak: f32, scene: f32, world: f32, debug: f32, share: vec4<f32> }
var<private> params: ReferenceParams;
@group(0) @binding(0) var<storage, read_write> errors: array<u32>;
"# + include_str!("hdr_light_reference.txt")
            + r#"
@compute @workgroup_size(64) fn check(@builtin(global_invocation_id) id: vec3<u32>) {
    let i = id.x;
    let mark = f32(i % 256u) / 255.0;
    let mode = i / 256u;
    let eased = (mode & 1u) != 0u;
    let shows = (mode & 2u) != 0u;
    params = ReferenceParams(select(1.0, 1000.0 / 203.0, (mode & 4u) != 0u),
        select(1.0, 4.0, (mode & 8u) != 0u), select(0.0, 1.0, (mode & 16u) != 0u),
        select(0.0, 1.0, (mode & 32u) != 0u), vec4(0.2, 0.5, 0.6, 1.0));
    let color = select(vec3(0.9, 0.4, 0.02), vec3(1.4, 0.8, 0.2), (mode & 64u) != 0u);
    let expected = reference_lit(color, mark, shows, eased);
    let gains = 1.0 + (params.peak - 1.0) * params.share;
    let actual = hdr_mark_light(color, mark, shows, eased,
        vec4(params.peak, params.scene, params.world, params.debug), (params.peak - 1.0) * params.share);
    errors[i] = select(0u, 1u, any(abs(expected.light - actual.light) > vec3(1e-6)) || expected.lifted != actual.lifted);
    // Gain stays above the tone knee until the separate tone curve runs.
    let gain = 1.0 + 3.0 * (1.0 - 194.0 / 255.0) / 0.24;
    let white = hdr_written(vec3(1.0), 0.76, vec4(5.0, 4.0, 1.0, 0.0), gains);
    errors[i] |= select(0u, 4u, any(abs(pow(white, vec3(2.2)) - vec3(gain)) > vec3(1e-5)));
    let boundary = (f32(i % 255u) + 0.5) / 255.0;
    for (var step = -1; step <= 1; step++) {
        let stored = unpack2x16float(pack2x16float(vec2(boundary + f32(step) / 65536.0))).x;
        let quantized = round(saturate(stored) * 255.0) / 255.0;
        let want = pow(reference_lit(color, quantized, params.world > 0.5, false).light, vec3(1.0 / 2.2));
        let got = hdr_written(color, stored, vec4(params.peak, params.scene, params.world, params.debug), gains);
        errors[i] |= select(0u, 2u, any(abs(want - got) > vec3(1e-6)));
    }
}
"#;
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("shared HDR equivalence"),
            source: wgpu::ShaderSource::Wgsl(source.into()),
        });
        let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: None,
            layout: None,
            module: &shader,
            entry_point: Some("check"),
            compilation_options: Default::default(),
            cache: None,
        });
        let count = 256 * 128;
        let output = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: None,
            contents: &vec![0; count * 4],
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
        });
        let readback = device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: (count * 4) as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &pipeline.get_bind_group_layout(0),
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: output.as_entire_binding(),
            }],
        });
        let mut encoder = device.create_command_encoder(&Default::default());
        {
            let mut pass = encoder.begin_compute_pass(&Default::default());
            pass.set_pipeline(&pipeline);
            pass.set_bind_group(0, &group, &[]);
            pass.dispatch_workgroups(count as u32 / 64, 1, 1);
        }
        encoder.copy_buffer_to_buffer(&output, 0, &readback, 0, (count * 4) as u64);
        queue.submit([encoder.finish()]);
        readback.map_async(wgpu::MapMode::Read, .., |r| r.unwrap());
        device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
        let data = readback.get_mapped_range(..).unwrap();
        for (i, bytes) in data.as_chunks::<4>().0.iter().enumerate() {
            assert_eq!(u32::from_le_bytes(*bytes), 0, "case {i}");
        }
        eprintln!(
            "HDR equivalence GPU ran {count} cases and {} half-float boundary cases",
            count * 3
        );
    }

    #[test]
    fn coverage_target_resizes_resets_and_shows_in_the_debug_view() {
        let instance = crate::gfx::test_gpu::instance();
        let Ok(adapter) = pollster::block_on(instance.request_adapter(&Default::default())) else {
            return;
        };
        let Ok((device, queue)) = pollster::block_on(adapter.request_device(&Default::default()))
        else {
            return;
        };
        for enabled in [false, true] {
            for debug in [None, Some("coverage")] {
                let mut hdr =
                    Hdr::with_debug(&device, &queue, F::Rgba16Float, Output::Pq, enabled, debug);
                hdr.frame(&device, [2, 1]);
                assert_eq!(hdr.coverage_view().is_some(), enabled);
                hdr.frame(&device, [4, 2]);
                let t = hdr.targets.as_ref().unwrap();
                if let Some(coverage) = &t.coverage {
                    assert_eq!(coverage.size(), t.frame.size());
                }
                let mut encoder = device.create_command_encoder(&Default::default());
                let frame_view = t.frame.create_view(&Default::default());
                let mut color = super::super::hud_coverage::attachment(&frame_view);
                color.ops.load = wgpu::LoadOp::Clear(wgpu::Color::BLACK);
                encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    color_attachments: &[Some(color)],
                    ..Default::default()
                });
                hdr.keep_world(&mut encoder, None);
                if let Some(view) = hdr.coverage_view() {
                    super::super::hud_coverage::clear(&mut encoder, &view);
                    let mut color = super::super::hud_coverage::attachment(&view);
                    color.ops.load = wgpu::LoadOp::Clear(wgpu::Color::WHITE);
                    encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                        color_attachments: &[Some(color)],
                        ..Default::default()
                    });
                }
                let capture = hdr.capture(&device, &queue, &mut encoder, false).unwrap();
                let buffer = super::super::copy_frame(&device, &mut encoder, &capture, [4, 2]);
                queue.submit([encoder.finish()]);
                buffer.map_async(wgpu::MapMode::Read, .., |r| r.unwrap());
                device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
                let bytes = buffer.get_mapped_range(..).unwrap();
                let pixel = u32::from_le_bytes(bytes[..4].try_into().unwrap());
                let [r, g, b] = [0, 10, 20].map(|shift| (pixel >> shift) & 0x3ff);
                if enabled && debug.is_some() {
                    // Fully covered: BT.709 magenta at paper white (200 nits here) in BT.2020
                    // PQ, not PQ's 10 000 nits (1023, 0, 1023).
                    let near = |got: u32, want: u32| got.abs_diff(want) <= 4;
                    assert!(near(r, 550) && near(g, 346) && near(b, 583), "{r} {g} {b}");
                } else {
                    assert_eq!(pixel, 0xc0000000);
                }
                if let Some(view) = hdr.coverage_view() {
                    let mut encoder = device.create_command_encoder(&Default::default());
                    super::super::hud_coverage::clear(&mut encoder, &view);
                    let coverage = hdr.targets.as_ref().unwrap().coverage.as_ref().unwrap();
                    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
                        label: None,
                        size: 512,
                        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                        mapped_at_creation: false,
                    });
                    encoder.copy_texture_to_buffer(
                        coverage.as_image_copy(),
                        wgpu::TexelCopyBufferInfo {
                            buffer: &buffer,
                            layout: wgpu::TexelCopyBufferLayout {
                                offset: 0,
                                bytes_per_row: Some(256),
                                rows_per_image: None,
                            },
                        },
                        coverage.size(),
                    );
                    queue.submit([encoder.finish()]);
                    buffer.map_async(wgpu::MapMode::Read, .., |r| r.unwrap());
                    device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
                    assert_eq!(&buffer.get_mapped_range(..).unwrap()[..4], &[0; 4]);
                }
            }
        }
    }

    pub(super) fn gpu() -> Option<(wgpu::Device, wgpu::Queue)> {
        let instance = crate::gfx::test_gpu::instance();
        let adapter = pollster::block_on(instance.request_adapter(&Default::default())).ok()?;
        eprintln!("HDR GPU {:?}", adapter.get_info());
        pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            required_features: adapter.features()
                & (wgpu::Features::TEXTURE_FORMAT_16BIT_NORM
                    | wgpu::Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES),
            ..Default::default()
        }))
        .ok()
    }

    /// Clears all of `texture` to `color`.
    pub(super) fn fill(
        encoder: &mut wgpu::CommandEncoder,
        texture: &wgpu::Texture,
        color: [f64; 4],
    ) {
        let view = texture.create_view(&Default::default());
        let mut attachment = super::super::hud_coverage::attachment(&view);
        let [r, g, b, a] = color;
        attachment.ops.load = wgpu::LoadOp::Clear(wgpu::Color { r, g, b, a });
        encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            color_attachments: &[Some(attachment)],
            ..Default::default()
        });
    }

    /// The first texel of `texture` once `encoder` ran: four half floats, 10-bit PQ codes
    /// for HDR10, or 8-bit values over 255 for the HUD's target.
    pub(super) fn texel(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        mut encoder: wgpu::CommandEncoder,
        texture: &wgpu::Texture,
    ) -> [f32; 4] {
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: 256,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        encoder.copy_texture_to_buffer(
            texture.as_image_copy(),
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
        if texture.format() == Output::Pq.format() {
            let pixel = u32::from_le_bytes(bytes[..4].try_into().unwrap());
            return [0, 10, 20, 30].map(|shift| ((pixel >> shift) & 0x3ff) as f32);
        }
        if texture.format() == super::super::hud_coverage::HUD_FORMAT {
            return std::array::from_fn(|i| f32::from(bytes[i]) / 255.0);
        }
        std::array::from_fn(|i| half::f16::from_le_bytes([bytes[2 * i], bytes[2 * i + 1]]).to_f32())
    }

    // Independent pre-refactor light arithmetic applied to a compatibility attachment.
    // Float targets in these tests only receive the resulting colour and coverage.
    // `lift`: the reference's peak, HDR lighting gain, world and debug, then the tiers' shares
    // of the headroom up to that peak (`writers_lift`, `display_lift`).
    pub(super) fn legacy_light(
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        lift: [[f32; 4]; 2],
        source: &wgpu::Texture,
        target: &wgpu::Texture,
    ) {
        use wgpu::util::DeviceExt;
        assert_eq!(source.format(), F::Rgba16Unorm);
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("legacy non-float light oracle"),
            source: wgpu::ShaderSource::Wgsl(
                (r#"
struct ReferenceParams { peak: f32, scene: f32, world: f32, debug: f32, share: vec4<f32> }
@group(0) @binding(0) var<uniform> params: ReferenceParams;
@group(0) @binding(1) var source: texture_2d<f32>;
"#
                .to_owned()
                    + include_str!("hdr_light_reference.txt")
                    + r#"
@vertex fn vs(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
    return vec4(vec2(f32((i << 1u) & 2u), f32(i & 2u)) * 2.0 - 1.0, 0.0, 1.0);
}
@fragment fn fs() -> @location(0) vec4<f32> {
    let c = textureLoad(source, vec2(0), 0);
    let mark = round(saturate(c.a) * 255.0) / 255.0;
    let light = reference_lit(c.rgb, mark, params.world > 0.5, false).light;
    return vec4(pow(light, vec3(1.0 / 2.2)), 1.0);
}
"#)
                .into(),
            ),
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: None,
            layout: None,
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs"),
                compilation_options: Default::default(),
                targets: &[Some(F::Rgba16Float.into())],
            }),
            multiview_mask: None,
            cache: None,
        });
        let params = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: None,
            contents: bytemuck::cast_slice(&lift),
            usage: wgpu::BufferUsages::UNIFORM,
        });
        let view = source.create_view(&Default::default());
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &pipeline.get_bind_group_layout(0),
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: params.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
            ],
        });
        Hdr::draw(
            encoder,
            &pipeline,
            &group,
            &target.create_view(&Default::default()),
            "legacy oracle",
            None,
        );
    }

    /// The float frame's writers' light as `legacy_light` takes it: each tier's gain as a share
    /// of the headroom up to the scene's peak.
    pub(super) fn writers_lift(hdr: &Hdr) -> [[f32; 4]; 2] {
        let (settings, gains) = hdr.draw_params(true);
        [settings, gains.map(|g| (g - 1.0) / (settings[0] - 1.0))]
    }

    /// The compatibility frame's display pass's light (`fs_hdr`) as `legacy_light` takes it.
    fn display_lift(hdr: &Hdr) -> [[f32; 4]; 2] {
        let p = hdr.params(true, false);
        [[p[1], p[7], p[2], p[3]], p[8..12].try_into().unwrap()]
    }

    #[test]
    fn direct_light_matches_the_legacy_non_float_display_below_the_tone_knee() {
        let Some((device, queue)) = gpu() else {
            return;
        };
        if !device.features().contains(
            wgpu::Features::TEXTURE_FORMAT_16BIT_NORM
                | wgpu::Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES,
        ) {
            eprintln!("legacy display GPU skipped: 16-bit UNORM unavailable");
            return;
        }
        // Compare the compatibility display lift with direct colour under the layered HUD:
        // the frozen oracle turns the marks into the light that display gives them, and the
        // layered display, whose curve is the identity at 1000 nits, must show it the same.
        let shown = |layered: bool, world: [f64; 4]| {
            let mut hdr = Hdr::with_debug(
                &device,
                &queue,
                if layered {
                    F::Rgba16Float
                } else {
                    F::Rgba16Unorm
                },
                Output::Pq,
                layered,
                None,
            );
            let frame = hdr.frame(&device, [1, 1]);
            let mut encoder = device.create_command_encoder(&Default::default());
            if layered {
                let mut legacy =
                    Hdr::with_debug(&device, &queue, F::Rgba16Unorm, Output::Pq, false, None);
                let marked = legacy.frame(&device, [1, 1]);
                fill(&mut encoder, &marked, world);
                if world[3] == 1.0 {
                    fill(&mut encoder, &frame, world);
                } else {
                    // The light the compatibility frame shows: its own lift, not the writers'.
                    legacy_light(
                        &device,
                        &mut encoder,
                        display_lift(&legacy),
                        &marked,
                        &frame,
                    );
                }
                hdr.begin(&queue, true);
                hdr.tonemap(&mut encoder, None);
                hdr.keep_world(&mut encoder, None);
                super::super::hud_coverage::clear(&mut encoder, &hdr.coverage_view().unwrap());
            } else {
                fill(&mut encoder, &frame, world);
                hdr.keep_world(&mut encoder, None);
            }
            let capture = hdr.capture(&device, &queue, &mut encoder, true).unwrap();
            texel(&device, &queue, encoder, &capture)
        };
        // Unmarked, HDR lighting and light sources of each tier: all up to
        // the tone curve's knee, which the curve leaves as they are.
        for world in [
            [0.5, 0.4, 0.3, 1.0],
            [0.8, 0.7, 0.6, 0.88],
            [0.9, 0.8, 0.4, 0.05],
            [0.9, 0.8, 0.4, 0.2],
            [0.9, 0.8, 0.4, 0.4],
            [0.9, 0.8, 0.4, 0.6],
        ] {
            let (today, layered) = (shown(false, world), shown(true, world));
            // Half floats round the light apart by a step at most.
            assert!(
                today.iter().zip(layered).all(|(a, b)| (a - b).abs() <= 1.0),
                "{world:?}: {today:?} {layered:?}"
            );
            if world[3] < 1.0 {
                assert!(layered[0] > shown(true, [world[0], world[1], world[2], 1.0])[0]);
            }
        }
        // Above-white unmarked colour belongs to float storage, not the UNORM oracle.
        let above = shown(true, [1.5, 1.2, 0.6, 1.0]);
        let white = shown(true, [1.0, 1.0, 0.6, 1.0]);
        assert!(above[0] > white[0] && above[1] > white[1]);
    }

    /// The world's writers read nothing of the display: the same gains and the same scene peak
    /// at any paper white and peak (docs/design/hdr-linear-light.md, step 6a).
    #[test]
    fn the_writers_light_is_the_same_on_every_display() {
        let Some((device, queue)) = gpu() else {
            return;
        };
        let mut hdr = Hdr::with_debug(&device, &queue, F::Rgba16Float, Output::Pq, true, None);
        let written = hdr.draw_params(true);
        assert_eq!(
            written,
            (
                [super::SCENE_PEAK, super::SUNLIT_GAIN, 1.0, 0.0],
                [1.6, 2.4, 2.8, 4.0]
            )
        );
        for (paper, peak) in [
            (203.0, 300.0),
            (203.0, 600.0),
            (203.0, 1500.0),
            (400.0, 600.0),
            (480.0, 200.0),
            (80.0, 10000.0),
        ] {
            (hdr.paper_white, hdr.peak) = (paper, peak);
            assert_eq!(hdr.draw_params(true), written, "{paper}/{peak}");
            assert_eq!(
                hdr.params(true, false)[16],
                super::SCENE_PEAK,
                "{paper}/{peak}"
            );
        }
        // The title, and frames Horizon doesn't light (HDR Lighting off, a pack drawing):
        // nothing says where their light ends, so the curve's source follows the display.
        assert_eq!(hdr.params(false, false)[16], 0.0);
        hdr.lighting = false;
        assert_eq!(hdr.draw_params(true).0[1], 1.0);
        assert_eq!(hdr.params(true, false)[16], 0.0);
    }

    /// The tone curve maps the scene's peak S onto the display's peak T with ITU-R BT.2390's
    /// EETF, keeping hue, after holding light above S at S. Checkpoints are worked out apart
    /// from the shader, in f64 from SMPTE ST 2084 and BT.2390.
    #[test]
    fn the_tone_curve_maps_the_scene_peak_onto_the_display() {
        let Some((device, queue)) = gpu() else {
            return;
        };
        let mut hdr = Hdr::with_debug(&device, &queue, F::Rgba16Float, Output::Pq, true, None);
        let scene_peak = super::SCENE_PEAK;
        // The frame holding `color` (in gamma, as it stores light), after the curve if `curve`.
        let through = |hdr: &mut Hdr, color: [f64; 4], curve: bool| {
            let frame = hdr.frame(&device, [1, 1]);
            let mut encoder = device.create_command_encoder(&Default::default());
            fill(&mut encoder, &frame, color);
            hdr.begin(&queue, true);
            if curve {
                hdr.tonemap(&mut encoder, None);
            }
            texel(&device, &queue, encoder, &frame)
        };
        let unchanged = |hdr: &mut Hdr, color: [f64; 4]| {
            through(hdr, color, true) == through(hdr, color, false)
        };
        // Linear light in and out, in units of paper white.
        let mapped = |hdr: &mut Hdr, light: [f32; 3]| {
            let [r, g, b] = light.map(|c| f64::from(c.powf(1.0 / 2.2)));
            let [r, g, b, a] = through(hdr, [r, g, b, 1.0], true);
            assert_eq!(a, 1.0);
            [r, g, b].map(|c| c.powf(2.2))
        };
        let near = |got: f32, want: f32| (got - want).abs() <= 0.005 * want.max(1.0);
        // A display at or above the mastering one (T >= S): every stored value up to S comes
        // out exactly as it went in, grey or coloured, and light above S is held at S with its
        // hue, so 1500 and 10 000 nits show what 1000 does; so do the settings' extremes (Paper
        // White 10 to 480 nits, Peak up to 10 000) wherever T reaches S.
        for (paper, peak) in [
            (203.0, 1000.0),
            (203.0, 1500.0),
            (203.0, 10000.0),
            (480.0, 10000.0),
            (10.0, 200.0),
        ] {
            (hdr.paper_white, hdr.peak) = (paper, peak);
            for i in 0..=64 {
                let g = f64::from((scene_peak * i as f32 / 64.0).powf(1.0 / 2.2));
                assert!(unchanged(&mut hdr, [g, g, g, 1.0]), "{peak}: {i}");
                assert!(
                    unchanged(&mut hdr, [g, g * 0.6, g * 0.1, 1.0]),
                    "{peak}: {i}"
                );
            }
            for light in [8.0f32, 50.0] {
                let [r, g, b] = mapped(&mut hdr, [light, light / 4.0, 0.0]);
                assert!(
                    near(r, scene_peak) && near(g * 4.0, r) && b == 0.0,
                    "{peak}: {light}: {r} {g} {b}"
                );
            }
        }
        // Dimmer displays: unchanged up to the knee, then the EETF into T, T at S and past it.
        // At 300 nits the knee falls just below white (161 nits): white shows at 198 nits
        // where the gates' curve, whose source followed the display, kept it at 203; the
        // highlights above it keep more of their steps.
        for (paper, peak, knee, checkpoints) in [
            (
                203.0,
                600.0,
                2.0,
                &[
                    (2.5, 2.47516),
                    (3.0, 2.75699),
                    (4.0, 2.9404),
                    (scene_peak, 600.0 / 203.0),
                    (8.0, 600.0 / 203.0),
                ][..],
            ),
            (
                203.0,
                300.0,
                0.75,
                &[
                    (0.9, 0.89289),
                    (1.0, 0.97365),
                    (1.2, 1.10406),
                    (1.4, 1.20134),
                    (2.0, 1.36833),
                    (4.0, 1.47641),
                    (8.0, 300.0 / 203.0),
                ],
            ),
            (
                400.0,
                600.0,
                0.8,
                &[
                    (1.0, 0.97973),
                    (1.2, 1.11501),
                    (1.5, 1.25607),
                    (2.0, 1.38818),
                    (scene_peak, 1.5),
                ],
            ),
            // No headroom (a peak under paper white): the scene is compressed into white.
            (
                203.0,
                150.0,
                0.4,
                &[
                    (1.0, 0.78346),
                    (1.5, 0.90244),
                    (2.0, 0.95589),
                    (4.0, 0.99944),
                    (scene_peak, 1.0),
                ],
            ),
        ] {
            (hdr.paper_white, hdr.peak) = (paper, peak);
            for light in [0.0f32, 0.3, knee] {
                let g = f64::from(light.powf(1.0 / 2.2));
                assert!(
                    unchanged(&mut hdr, [g, g * 0.5, 0.0, 1.0]),
                    "{paper}/{peak}: {light}"
                );
            }
            for &(light, want) in checkpoints {
                let [r, ..] = mapped(&mut hdr, [light; 3]);
                assert!(near(r, want), "{paper}/{peak}: {light}: {r}, not {want}");
            }
            // It rises, stays under the peak, and the brightest channel scales the others; a
            // half float's step in gamma is 0.1% of the light near white.
            let headroom = (peak / paper).max(1.0);
            let mut last = 0.0;
            for i in 0..40 {
                let light = 0.5 * 1.08f32.powi(i);
                let [r, g, b] = mapped(&mut hdr, [light, light / 4.0, 0.0]);
                assert!(
                    r >= last * 0.998 && r <= headroom * 1.002,
                    "{peak}: {light}: {r}"
                );
                assert!(near(g * 4.0, r) && b == 0.0, "{peak}: {light}: {r} {g} {b}");
                last = r;
            }
        }
        // Light Horizon doesn't lift (HDR Lighting off, a 0.2 pack's own) has no known peak:
        // the curve's source follows the display, the knee at 60% of its peak and at least
        // white, as before one peak. At 300 nits the source's peak is 645 nits.
        hdr.lighting = false;
        (hdr.paper_white, hdr.peak) = (203.0, 300.0);
        let headroom = 300.0 / 203.0;
        for light in [0.0f32, 0.5, 0.9, 1.0] {
            let g = f64::from(light.powf(1.0 / 2.2));
            assert!(unchanged(&mut hdr, [g, g * 0.5, 0.0, 1.0]), "{light}");
        }
        for (light, want) in [
            (1.2, 1.16839),
            (1.4, 1.28356),
            (headroom, 1.31719),
            (2.0, 1.44024),
            (4.0, headroom),
        ] {
            let [r, ..] = mapped(&mut hdr, [light; 3]);
            assert!(near(r, want), "{light}: {r}, not {want}");
        }
        // With no headroom there, a clip at white, channel by channel.
        hdr.peak = 150.0;
        let [r, g, b] = mapped(&mut hdr, [1.5, 0.5, 0.2]);
        assert!(r == 1.0 && near(g, 0.5) && near(b, 0.2), "{r} {g} {b}");
    }

    /// HDR output switched from HDR10 to scRGB and back (`output_apply`): the player's tuning
    /// stays, each swapchain gets the same light in its own encoding, and the HDR10 capture
    /// and the HDR10 picture after the switch back are the ones before it.
    #[test]
    fn a_switched_encoding_keeps_the_tuning_and_shows_the_same_light() {
        let Some((device, queue)) = gpu() else {
            eprintln!("HDR encoding switch GPU skipped: no adapter");
            return;
        };
        let mut hdr = Hdr::with_debug(&device, &queue, F::Rgba16Float, Output::Pq, true, None);
        (hdr.paper_white, hdr.peak, hdr.black) = (250.0, 800.0, 0.0);
        (hdr.ui, hdr.look, hdr.lighting) = (0.5, false, false);
        hdr.masks.parts = 0.4;
        hdr.tiers[1].share = 0.7;
        hdr.debug = true;
        hdr.calibration = Some(super::Calibration {
            step: 2,
            nits: 700.0,
            band: 0.6,
        });
        let tuning = |h: &Hdr| {
            (
                [h.paper_white, h.peak, h.black, h.ui],
                (h.look, h.lighting, h.debug, h.layered.is_some(), h.format),
                (h.masks, h.tiers, h.calibration),
            )
        };
        let switched = hdr.retargeted(&device, &queue, Output::ScRgb);
        assert_eq!(switched.output, Output::ScRgb);
        assert_eq!(tuning(&switched), tuning(&hdr));
        hdr.calibration = None;
        hdr.debug = false;
        // The swapchain's first texel (`capture`: the HDR10 screenshot's) for light 2 times
        // paper white, grey, as the layered frame stores it.
        let shot = |hdr: &mut Hdr, capture: bool| {
            let frame = hdr.frame(&device, [1, 1]);
            let mut encoder = device.create_command_encoder(&Default::default());
            let g = 2f64.powf(1.0 / 2.2);
            fill(&mut encoder, &frame, [g, g, g, 1.0]);
            hdr.begin(&queue, true);
            hdr.keep_world(&mut encoder, None);
            super::super::hud_coverage::clear(&mut encoder, &hdr.coverage_view().unwrap());
            let target = if capture {
                hdr.capture(&device, &queue, &mut encoder, true).unwrap()
            } else {
                device.create_texture(&wgpu::TextureDescriptor {
                    label: None,
                    size: frame.size(),
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: hdr.output.format(),
                    usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
                    view_formats: &[],
                })
            };
            if !capture {
                hdr.present(&queue, &mut encoder, &target, true, None);
            }
            texel(&device, &queue, encoder, &target)
        };
        // BT.2100's PQ EOTF: a 10-bit code's nits.
        let pq_nits = |code: f32| {
            let (m1, m2) = (2610.0 / 16384.0, 2523.0 / 4096.0 * 128.0);
            let (c1, c2, c3) = (3424.0 / 4096.0, 2413.0 / 128.0, 2392.0 / 128.0);
            let p = (code / 1023.0f32).powf(1.0 / m2);
            ((p - c1).max(0.0) / (c2 - c3 * p)).powf(1.0 / m1) * 10000.0
        };
        let pq = shot(&mut hdr, false);
        let pq_capture = shot(&mut hdr, true);
        let mut scrgb = hdr.retargeted(&device, &queue, Output::ScRgb);
        let linear = shot(&mut scrgb, false);
        assert_eq!(shot(&mut scrgb, true), pq_capture);
        // Two times paper white in both: 500 nits, as HDR10's code and as scRGB's 80-nit units.
        let near = |got: f32| (got - 500.0).abs() <= 0.015 * 500.0;
        assert!(
            near(pq_nits(pq[0])) && pq[..3].iter().all(|&c| c == pq[0]),
            "{pq:?}"
        );
        assert!(near(linear[0] * 80.0), "{linear:?}");
        let mut back = scrgb.retargeted(&device, &queue, Output::Pq);
        assert_eq!(
            (shot(&mut back, false), shot(&mut back, true)),
            (pq, pq_capture)
        );
    }

    /// ITU-R BT.2390's black level lift on grey light, in HDR10 and in scRGB, against values
    /// worked out apart from the shader, in f64 from SMPTE ST 2084: E3 = E2 + b (1 - E2)^4 on
    /// the PQ signal, for a source from 0 to the peak and b the black level's share of it.
    #[test]
    fn the_black_level_lift_follows_bt2390_in_pq_and_scrgb() {
        let Some((device, queue)) = gpu() else {
            return;
        };
        let (m1, m2) = (2610.0 / 16384.0, 2523.0 / 4096.0 * 128.0);
        let (c1, c2, c3) = (3424.0 / 4096.0, 2413.0 / 128.0, 2392.0 / 128.0);
        let pq = |nits: f64| {
            let y = (nits / 10_000.0).powf(m1);
            ((c1 + c2 * y) / (1.0 + c3 * y)).powf(m2)
        };
        let nits = |e: f64| {
            let p = e.powf(1.0 / m2);
            10_000.0 * ((p - c1).max(0.0) / (c2 - c3 * p)).powf(1.0 / m1)
        };
        let (paper, peak) = (203.0, 1000.0);
        let lifted = |light: f64, black: f64| {
            let top = pq(peak);
            let e2 = (pq(light) / top).min(1.0);
            nits((e2 + pq(black) / top * (1.0 - e2).powi(4)) * top)
        };
        for output in [Output::Pq, Output::ScRgb] {
            let mut hdr = Hdr::with_debug(&device, &queue, F::Rgba16Float, output, true, None);
            (hdr.paper_white, hdr.peak) = (paper as f32, peak as f32);
            let target = device.create_texture(&wgpu::TextureDescriptor {
                label: None,
                size: wgpu::Extent3d {
                    width: 1,
                    height: 1,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: output.format(),
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
                view_formats: &[],
            });
            for black in [0.0, 0.005, 0.05, 0.5] {
                hdr.black = black as f32;
                // Black, the shadows, paper white and the peak, in units of paper white.
                for light in [0.0, 0.001, 0.01, 0.1, 1.0, peak / paper] {
                    let g = half::f16::from_f64(f64::powf(light, 1.0 / 2.2)).to_f64();
                    let shown = (g.powf(2.2) * paper).min(peak);
                    let want = if black > 0.0 {
                        lifted(shown, black)
                    } else {
                        shown
                    };
                    let frame = hdr.frame(&device, [1, 1]);
                    let mut encoder = device.create_command_encoder(&Default::default());
                    fill(&mut encoder, &frame, [g, g, g, 1.0]);
                    hdr.keep_world(&mut encoder, None);
                    super::super::hud_coverage::clear(&mut encoder, &hdr.coverage_view().unwrap());
                    hdr.begin(&queue, true);
                    hdr.present(&queue, &mut encoder, &target, true, None);
                    let got = texel(&device, &queue, encoder, &target);
                    for c in &got[..3] {
                        let c = f64::from(*c);
                        let ok = match output {
                            // 10-bit codes, a step either way for rounding.
                            Output::Pq => (c - 1023.0 * pq(want)).abs() <= 1.0,
                            // 80 nits a unit, in half floats.
                            Output::ScRgb => (c * 80.0 - want).abs() <= 2e-3 * want + 1e-4,
                        };
                        assert!(
                            ok,
                            "{output:?} black {black}, light {light}: {got:?}, not {want} nits"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn a_scaled_screenshot_shoots_at_its_size_and_leaves_the_window_its_own_targets() {
        let Some((device, queue)) = gpu() else {
            eprintln!("scaled screenshot GPU unavailable");
            return;
        };
        let mut hdr = Hdr::with_debug(&device, &queue, F::Rgba16Float, Output::Pq, true, None);
        let extent = |t: &wgpu::Texture| [t.width(), t.height()];
        hdr.frame(&device, [4, 2]);
        // `Renderer::draw_scaled_frame`: the frame at the screenshot's size, its SDR picture
        // and HDR10 copy at that size too, then the targets let go.
        assert_eq!(extent(&hdr.frame(&device, [8, 4])), [8, 4]);
        let mut encoder = device.create_command_encoder(&Default::default());
        let sdr = hdr
            .sdr_screenshot(&device, &mut encoder, F::Bgra8Unorm, false)
            .unwrap();
        let pq = hdr.capture(&device, &queue, &mut encoder, true).unwrap();
        queue.submit([encoder.finish()]);
        assert_eq!((extent(&sdr), extent(&pq)), ([8, 4], [8, 4]));
        hdr.release_targets();
        assert!(hdr.hud().is_none() && hdr.coverage_view().is_none());
        // The window's next frame makes its own again.
        assert_eq!(extent(&hdr.frame(&device, [4, 2])), [4, 2]);
        assert_eq!(extent(hdr.hud().unwrap()), [4, 2]);
    }

    #[test]
    fn sdr_screenshots_roll_off_the_world_and_keep_the_hud_as_drawn() {
        let Some((device, queue)) = gpu() else {
            eprintln!("SDR screenshot GPU unavailable");
            return;
        };
        let mut hdr = Hdr::with_debug(&device, &queue, F::Rgba16Float, Output::Pq, true, None);
        // These display settings must not scale either the SDR world or the HUD.
        (hdr.paper_white, hdr.peak, hdr.black, hdr.ui) = (300.0, 1500.0, 0.2, 0.25);
        let shoot = |hdr: &mut Hdr, world: [f64; 3], hud: [f64; 3], c: f64, icon| {
            let frame = hdr.frame(&device, [1, 1]);
            let t = hdr.targets.as_ref().unwrap();
            let mut encoder = device.create_command_encoder(&Default::default());
            fill(&mut encoder, &frame, [world[0], world[1], world[2], 1.0]);
            fill(
                &mut encoder,
                t.hud.as_ref().unwrap(),
                [hud[0], hud[1], hud[2], 0.0],
            );
            fill(&mut encoder, t.coverage.as_ref().unwrap(), [c; 4]);
            // Read half-floats so 8-bit quantization cannot conceal the curve or RGB ratios.
            let copy = hdr
                .sdr_screenshot(&device, &mut encoder, F::Rgba16Float, icon)
                .unwrap();
            texel(&device, &queue, encoder, &copy)
        };
        // Below the linear knee, through the last half-float below it, the stored
        // channels survive exactly. All inputs here are exactly representable.
        for gamma in [0.0f32, 0.25, 0.5, 0.75, 0.79248047] {
            let w = [gamma, gamma / 2.0, 0.0];
            let got = shoot(&mut hdr, w.map(f64::from), w.map(f64::from), 0.0, false);
            assert_eq!(got[..3], w.map(|v| half::f16::from_f32(v).to_f32()));
        }
        // Independent numeric checkpoints in linear light, not a CPU copy of the shader.
        for (light, expected) in [
            (1.0f32, 0.8),
            (2.0, 0.9111111),
            (4.0, 0.9578947),
            (8.0, 0.9794872),
        ] {
            let g = f64::from(light.powf(1.0 / 2.2));
            let got = shoot(&mut hdr, [g; 3], [1.0; 3], 0.0, false);
            assert!(
                (got[0].powf(2.2) - expected).abs() < 0.002,
                "{light}: {got:?}"
            );
            assert!(got[0] < 1.0);
        }
        let mut last = 0.0;
        for i in 0..65 {
            let g = f64::from(i) / 16.0;
            let got = shoot(
                &mut hdr,
                [g, g / 2.0, 0.0],
                [g.min(1.0), (g / 2.0).min(1.0), 0.0],
                0.0,
                false,
            );
            assert!(got[0] >= last && got[0] <= 1.0, "{g}: {got:?}");
            assert!((got[0] - 2.0 * got[1]).abs() < 0.002 && got[2] == 0.0);
            last = got[0];
        }
        // Fully covered is exactly the HUD as its 8-bit target holds it, regardless of the
        // world or UI Brightness.
        let hud = [64.0, 128.0, 192.0].map(|v: f64| v / 255.0);
        assert_eq!(
            shoot(&mut hdr, [2.0, 1.0, 0.5], hud, 1.0, false),
            [hud[0], hud[1], hud[2], 1.0].map(|v| half::f16::from_f64(v).to_f32())
        );
        // At zero coverage the mapped world; the icon also ignores a fully opaque HUD.
        for (hud, c, icon) in [([1.0, 1.0, 0.5], 0.0, false), ([0.25; 3], 1.0, true)] {
            let got = shoot(&mut hdr, [2.0, 1.0, 0.5], hud, c, icon);
            for (g, expected) in got.into_iter().zip([0.983284, 0.491642, 0.245821, 1.0]) {
                assert!((g - expected).abs() < 0.002, "{got:?}");
            }
        }
        // R8's 128/255 coverage: recover the HUD against the clipped backdrop, then
        // composite in gamma. The original HUD target and world's light stay intact.
        let got = shoot(
            &mut hdr,
            [2.0, 1.0, 0.5],
            [0.75, 0.875, 0.4375],
            128.0 / 255.0,
            false,
        );
        for (g, expected) in got.into_iter().zip([0.741675, 0.62182, 0.31091, 1.0]) {
            assert!((g - expected).abs() < 0.002, "{got:?}");
        }
        let t = hdr.targets.as_ref().unwrap();
        let encoder = device.create_command_encoder(&Default::default());
        assert_eq!(
            texel(&device, &queue, encoder, &t.frame),
            [2.0, 1.0, 0.5, 1.0]
        );
        // The HUD's 8-bit target rounds what is drawn on it.
        let encoder = device.create_command_encoder(&Default::default());
        let stored = [191.0, 223.0, 112.0, 0.0].map(|v| v / 255.0);
        let hud = t.hud.as_ref().unwrap();
        assert_eq!(texel(&device, &queue, encoder, hud), stored);
        // With HDR Lighting off or under a pack, Horizon lifts nothing: the screenshot is the
        // SDR picture, clipped at white.
        hdr.lighting = false;
        for gamma in [0.5f32, 0.9, 1.0] {
            let w = [gamma, gamma / 2.0, 0.0];
            // Uncovered, the HUD's target holds the world as drawn; the icon ignores the HUD.
            let got = shoot(&mut hdr, w.map(f64::from), w.map(f64::from), 0.0, false);
            assert_eq!(got[..3], w.map(|v| half::f16::from_f32(v).to_f32()));
            let got = shoot(&mut hdr, w.map(f64::from), [0.25; 3], 1.0, true);
            assert_eq!(got[..3], w.map(|v| half::f16::from_f32(v).to_f32()));
        }
        // Partly covered, the 8-bit screenshot is the HUD's target as drawn.
        let got = shoot(
            &mut hdr,
            [1.0, 0.5, 0.0],
            [0.75, 0.875, 0.4375],
            128.0 / 255.0,
            false,
        );
        assert_eq!(
            got.map(|v| (v * 255.0).round()),
            [191.0, 223.0, 112.0, 255.0]
        );
    }

    #[test]
    fn the_hud_lays_over_the_worlds_light_by_its_coverage() {
        let Some((device, queue)) = gpu() else {
            return;
        };
        let mut hdr = Hdr::with_debug(&device, &queue, F::Rgba16Float, Output::Pq, true, None);
        let target = device.create_texture(&wgpu::TextureDescriptor {
            label: None,
            size: wgpu::Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: F::Rgba16Float,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        // The world's light, the HUD's target over it (on the world clipped to white), its
        // coverage, UI Brightness, and the composite, all in gamma as the frame stores them.
        for (world, hud, coverage, ui, want) in [
            // Nothing over it: the world, light above white included.
            (3.0, 1.0, 0.0, 1.0, 3.0),
            // Half-transparent black: half of the world shows, as the HUD blends.
            (2.0, 0.5, 0.5, 1.0, 1.0),
            // Covered: the HUD as drawn, at UI Brightness.
            (2.0, 0.3, 1.0, 1.0, 0.3),
            (2.0, 0.3, 1.0, 0.5, 0.3 * 0.5f32.powf(1.0 / 2.2)),
            // Added over the world (the text highlight), which it doesn't hide.
            (0.6, 0.8, 0.0, 1.0, 0.8),
            // At or below white, at 100%: the HUD's picture as drawn.
            (0.7, 0.45, 0.5, 1.0, 0.45),
        ] {
            hdr.ui = ui;
            let frame = hdr.frame(&device, [1, 1]);
            let t = hdr.targets.as_ref().unwrap();
            let mut encoder = device.create_command_encoder(&Default::default());
            fill(&mut encoder, &frame, [world, world, world, 1.0]);
            fill(&mut encoder, t.hud.as_ref().unwrap(), [hud, hud, hud, 0.0]);
            fill(&mut encoder, t.coverage.as_ref().unwrap(), [coverage; 4]);
            hdr.begin(&queue, true);
            hdr.composite(&mut encoder, &target.create_view(&Default::default()), None);
            let got = texel(&device, &queue, encoder, &target);
            assert!(
                (got[0] - want).abs() < 0.01 && got[3] == 1.0,
                "{world} {hud} {coverage} {ui}: {got:?}"
            );
        }
    }

    #[test]
    fn the_menu_blur_keeps_the_light_it_blurs() {
        let Some((device, queue)) = gpu() else {
            return;
        };
        let mut hdr = Hdr::with_debug(&device, &queue, F::Rgba16Float, Output::Pq, true, None);
        let size = [8, 8];
        let frame = hdr.frame(&device, size);
        let mut encoder = device.create_command_encoder(&Default::default());
        fill(&mut encoder, &frame, [4.0, 4.0, 4.0, 1.0]);
        hdr.keep_world(&mut encoder, None);
        super::super::hud_coverage::clear(&mut encoder, &hdr.coverage_view().unwrap());
        let mut blur = crate::menu_blur::MenuBlur::new(&device, F::Rgba16Float, false);
        let (_, source) = blur.source(&device, size);
        hdr.begin(&queue, true);
        hdr.composite(&mut encoder, &source, None);
        let view = frame.create_view(&Default::default());
        blur.blur(&device, &queue, &mut encoder, &view, size, 3.0, None);
        hdr.keep_world(&mut encoder, None);
        let hud = hdr.hud().unwrap().clone();
        // The blurred world keeps its light, and the HUD's backdrop starts again from it.
        let [r, ..] = texel(&device, &queue, encoder, &frame);
        assert!((r - 4.0).abs() < 0.01, "{r}");
        let encoder = device.create_command_encoder(&Default::default());
        assert_eq!(texel(&device, &queue, encoder, &hud), [1.0, 1.0, 1.0, 0.0]);
    }

    #[test]
    fn hdr_shaders_validate() {
        let entity = super::super::entity_shader();
        for source in [
            &super::shader(),
            &super::super::terrain_shader(),
            include_str!("../hdr_stats.wgsl"),
            &(include_str!("../hdr_light.wgsl").to_owned() + include_str!("stars.wgsl")),
            &entity,
        ] {
            let module = naga::front::wgsl::parse_str(source).unwrap();
            naga::valid::Validator::new(
                naga::valid::ValidationFlags::all(),
                naga::valid::Capabilities::all(),
            )
            .validate(&module)
            .unwrap();
        }
    }

    fn caps(offered: &[(F, S)]) -> wgpu::SurfaceCapabilities {
        wgpu::SurfaceCapabilities {
            format_capabilities: offered
                .iter()
                .map(|&(format, color_spaces)| wgpu::SurfaceFormatCapabilities {
                    format,
                    color_spaces,
                })
                .collect(),
            ..Default::default()
        }
    }

    #[test]
    fn hdr10_first_then_scrgb() {
        let both = caps(&[
            (F::Bgra8UnormSrgb, S::SRGB),
            (F::Rgb10a2Unorm, S::BT2100_PQ | S::BT2100_HLG),
            (F::Rgba16Float, S::EXTENDED_SRGB_LINEAR),
        ]);
        assert_eq!(pick(&both, None), Some(Output::Pq));
        assert_eq!(pick(&both, Some("scrgb")), Some(Output::ScRgb));
        let scrgb = caps(&[(F::Rgba16Float, S::EXTENDED_SRGB_LINEAR)]);
        assert_eq!(pick(&scrgb, None), Some(Output::ScRgb));
        assert_eq!(pick(&scrgb, Some("pq")), None);
        assert_eq!(pick(&caps(&[(F::Bgra8UnormSrgb, S::SRGB)]), None), None);
    }

    #[test]
    fn offscreen_offers_only_the_named_output() {
        assert_eq!(named(Some("pq")), Some(Output::Pq));
        assert_eq!(named(Some("scrgb")), Some(Output::ScRgb));
        assert_eq!(named(Some("hdr10")), None);
        assert_eq!(named(None), None);
    }
}

#[cfg(test)]
mod lighting_tests {
    use super::scene_gain;

    #[test]
    fn the_lighting_scales_with_the_headroom() {
        // Enough headroom: the full strength, whatever the paper white.
        assert_eq!(scene_gain(1000.0, 430.0), 4.0);
        assert_eq!(scene_gain(1060.0, 203.0), 4.0);
        // Little headroom, and none.
        assert_eq!(scene_gain(600.0, 400.0), 2.625);
        assert_eq!(scene_gain(200.0, 480.0), 1.0);
    }
}

#[cfg(test)]
#[path = "hdr_written_tests.rs"]
mod written_tests;
