use super::{
    Hdr, Output,
    tests::{gpu, texel},
};
use wgpu::TextureFormat as F;
use wgpu::util::DeviceExt;

const TERRAIN_VERTEX: &str = r#"
override TEST_TIER: u32 = 0u;
override TEST_OVERLAY: u32 = 0u;
@vertex fn test_vertex(@builtin(vertex_index) i: u32) -> TerrainWire {
    let p = vec2(f32((i << 1u) & 2u), f32(i & 2u));
    var o: TerrainOut;
    o.clip = vec4(p * 2.0 - 1.0, 0.0, 1.0);
    o.uv = vec2(0.5);
    o.local = vec3(p.x, 0.0, p.y);
    o.color = vec4(0.8, 0.9, 0.7, select(1.0, 0.5, globals.misc.z > 0.5));
    o.scene = vec2(15.0);
    o.spherical = globals.misc.x;
    o.cylindrical = globals.misc.x;
    o.visibility = globals.misc.y;
    o.emission = TEST_TIER;
    o.emissive_layer = select(0xffffffffu, 1u, TEST_OVERLAY != 0u);
    return to_wire(o);
}
"#;
const ENTITY_VERTEX: &str = r#"
override TEST_TIER: u32 = 0u;
override TEST_OVERLAY: u32 = 0u;
@vertex fn test_vertex(@builtin(vertex_index) i: u32) -> Out {
    let p = vec2(f32((i << 1u) & 2u), f32(i & 2u));
    var o: Out;
    o.clip = vec4(p * 2.0 - 1.0, 0.0, 1.0);
    o.uv = vec2(0.5);
    o.color = vec4(0.8, 0.9, 0.7, 1.0);
    o.back_color = o.color;
    o.light_color = vec4(0.7, 0.8, 0.9, 1.0);
    o.overlay_color = vec4(1.0);
    o.scene = 0.8;
    o.tier = TEST_TIER;
    o.overlay = TEST_OVERLAY;
    o.wrap = vec2(0.0, 1.0);
    o.proj = vec4(0.4, 0.6, 0.0, 1.0);
    o.fog_distance = vec2(globals.misc.x);
    return o;
}
"#;

struct Draw<'a> {
    device: &'a wgpu::Device,
    queue: &'a wgpu::Queue,
    terrain: bool,
    shader: wgpu::ShaderModule,
    layout: wgpu::PipelineLayout,
    bindings: wgpu::BindGroupLayout,
    group: wgpu::BindGroup,
}
impl<'a> Draw<'a> {
    fn render_oit(
        &self,
        hdr: &mut Hdr,
        kind: &str,
        tier: u32,
        overlay: u32,
        written: bool,
    ) -> [f32; 4] {
        use super::super::oit::{Stage, stage_targets};
        let source = if self.terrain {
            super::super::terrain_shader()
                + include_str!("../oit.wgsl")
                + include_str!("../oit_world.wgsl")
                + TERRAIN_VERTEX
        } else {
            super::super::entity_shader()
                + include_str!("../oit.wgsl")
                + include_str!("../oit_features.wgsl")
                + include_str!("../oit_clouds.wgsl")
                + ENTITY_VERTEX
        }
        .replace(
            "o.clip = vec4(p * 2.0 - 1.0, 0.0, 1.0)",
            "o.clip = vec4(p * 2.0 - 1.0, 0.5, 1.0)",
        );
        let shader = self
            .device
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("actual OIT fragments"),
                source: wgpu::ShaderSource::Wgsl(source.into()),
            });
        let screen_shader = self
            .device
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: None,
                source: wgpu::ShaderSource::Wgsl(
                    (super::super::terrain_shader()
                        + include_str!("../oit.wgsl")
                        + include_str!("../oit_world.wgsl")
                        + TERRAIN_VERTEX)
                        .into(),
                ),
            });
        let make = |format| {
            self.device
                .create_texture(&wgpu::TextureDescriptor {
                    label: None,
                    size: wgpu::Extent3d {
                        width: 1,
                        height: 1,
                        depth_or_array_layers: 1,
                    },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format,
                    usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                        | wgpu::TextureUsages::TEXTURE_BINDING,
                    view_formats: &[],
                })
                .create_view(&Default::default())
        };
        let bounds = make(F::Rgba32Float);
        let depth = make(F::Depth32Float);
        let coeff = [make(F::Rgba16Float), make(F::Rgba16Float)];
        let accumulated = make(F::Rgba16Float);
        let uniform = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: None,
                contents: bytemuck::cast_slice(&[0.0_f32, 1.0, 0.0, 0.0]),
                usage: wgpu::BufferUsages::UNIFORM,
            });
        let frame = hdr.frame(self.device, [1, 1]);
        let view = frame.create_view(&Default::default());
        let mut encoder = self.device.create_command_encoder(&Default::default());
        super::tests::fill(&mut encoder, &frame, [2.5, 1.8, 0.9, 1.0]);
        let constants = [
            ("HDR_WRITE_LIGHT", f64::from(u8::from(written))),
            ("TEST_TIER", f64::from(tier)),
            ("TEST_OVERLAY", f64::from(overlay)),
        ];
        for stage in 0..4 {
            let textures: Vec<_> = match stage {
                0 => vec![],
                1 => vec![&bounds],
                2 => vec![&bounds, &coeff[0], &coeff[1]],
                _ => vec![&bounds, &coeff[0], &coeff[1], &accumulated],
            };
            let mut entries = vec![wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }];
            entries.extend(
                textures
                    .iter()
                    .enumerate()
                    .map(|(i, _)| wgpu::BindGroupLayoutEntry {
                        binding: i as u32 + 1,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Float { filterable: false },
                            view_dimension: wgpu::TextureViewDimension::D2,
                            multisampled: false,
                        },
                        count: None,
                    }),
            );
            let bindings = self
                .device
                .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                    label: None,
                    entries: &entries,
                });
            let mut resources = vec![wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform.as_entire_binding(),
            }];
            resources.extend(
                textures
                    .iter()
                    .enumerate()
                    .map(|(i, t)| wgpu::BindGroupEntry {
                        binding: i as u32 + 1,
                        resource: wgpu::BindingResource::TextureView(t),
                    }),
            );
            let group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: None,
                layout: &bindings,
                entries: &resources,
            });
            let layout = self
                .device
                .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                    label: None,
                    bind_group_layouts: &[Some(&self.bindings), None, Some(&bindings)],
                    ..Default::default()
                });
            let suffix = ["bounds", "transmittance", "accumulate", "composite"][stage];
            let entry = if stage == 3 {
                "fs_oit_composite".to_owned()
            } else if self.terrain {
                format!("fs_oit_{suffix}")
            } else {
                format!("fs_oit_{kind}_{suffix}")
            };
            let targets = if stage == 3 {
                vec![Some(wgpu::ColorTargetState {
                    format: F::Rgba16Float,
                    blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })]
            } else {
                stage_targets([Stage::DepthBounds, Stage::Transmittance, Stage::Accumulate][stage])
            };
            let shader = if stage == 3 { &screen_shader } else { &shader };
            let pipeline = self
                .device
                .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                    label: Some(&entry),
                    layout: Some(&layout),
                    vertex: wgpu::VertexState {
                        module: shader,
                        entry_point: Some("test_vertex"),
                        compilation_options: wgpu::PipelineCompilationOptions {
                            constants: &constants,
                            ..Default::default()
                        },
                        buffers: &[],
                    },
                    primitive: Default::default(),
                    depth_stencil: (stage == 3).then_some(wgpu::DepthStencilState {
                        format: F::Depth32Float,
                        depth_write_enabled: Some(true),
                        depth_compare: Some(wgpu::CompareFunction::Always),
                        stencil: Default::default(),
                        bias: Default::default(),
                    }),
                    multisample: Default::default(),
                    fragment: Some(wgpu::FragmentState {
                        module: shader,
                        entry_point: Some(&entry),
                        compilation_options: wgpu::PipelineCompilationOptions {
                            constants: &constants,
                            ..Default::default()
                        },
                        targets: &targets,
                    }),
                    multiview_mask: None,
                    cache: None,
                });
            let outputs = match stage {
                0 => vec![&bounds],
                1 => vec![&coeff[0], &coeff[1]],
                2 => vec![&accumulated],
                _ => vec![&view],
            };
            let attachments: Vec<_> = outputs
                .iter()
                .map(|v| {
                    Some(wgpu::RenderPassColorAttachment {
                        view: v,
                        resolve_target: None,
                        depth_slice: None,
                        ops: wgpu::Operations {
                            load: if stage == 3 {
                                wgpu::LoadOp::Load
                            } else {
                                wgpu::LoadOp::Clear(if stage == 0 {
                                    wgpu::Color {
                                        r: -(f32::MAX as f64),
                                        g: 0.0,
                                        b: 0.0,
                                        a: 0.0,
                                    }
                                } else {
                                    wgpu::Color::TRANSPARENT
                                })
                            },
                            store: wgpu::StoreOp::Store,
                        },
                    })
                })
                .collect();
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                color_attachments: &attachments,
                depth_stencil_attachment: (stage == 3).then_some(
                    wgpu::RenderPassDepthStencilAttachment {
                        view: &depth,
                        depth_ops: Some(wgpu::Operations {
                            load: wgpu::LoadOp::Clear(0.0),
                            store: wgpu::StoreOp::Store,
                        }),
                        stencil_ops: None,
                    },
                ),
                ..Default::default()
            });
            pass.set_pipeline(&pipeline);
            pass.set_bind_group(0, &self.group, &[]);
            pass.set_bind_group(2, &group, &[]);
            pass.draw(0..3, 0..1);
        }
        texel(self.device, self.queue, encoder, &frame)
    }
    fn new(
        device: &'a wgpu::Device,
        queue: &'a wgpu::Queue,
        terrain: bool,
        globals: &super::super::Globals,
        alpha: u8,
    ) -> Self {
        let source = if terrain {
            super::super::terrain_shader() + TERRAIN_VERTEX
        } else {
            super::super::entity_shader() + ENTITY_VERTEX
        };
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("actual HDR draw shader"),
            source: wgpu::ShaderSource::Wgsl(source.into()),
        });
        let mut entries = vec![];
        for binding in 0..if terrain { 13 } else { 8 } {
            let sampler = if terrain {
                binding == 2 || binding == 4
            } else {
                binding == 2 || binding == 4 || binding == 6
            };
            let ty = if binding == 0 {
                wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                }
            } else if sampler {
                wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering)
            } else {
                wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    multisampled: false,
                    view_dimension: if terrain && binding != 3 && binding != 12 {
                        wgpu::TextureViewDimension::D2Array
                    } else {
                        wgpu::TextureViewDimension::D2
                    },
                }
            };
            entries.push(wgpu::BindGroupLayoutEntry {
                binding,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty,
                count: None,
            });
        }
        let bindings = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: None,
            entries: &entries,
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: None,
            bind_group_layouts: &[Some(&bindings)],
            ..Default::default()
        });
        let uniform = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: None,
            contents: bytemuck::bytes_of(globals),
            usage: wgpu::BufferUsages::UNIFORM,
        });
        let texture = |layers, data: &[u8]| {
            device.create_texture_with_data(
                queue,
                &wgpu::TextureDescriptor {
                    label: None,
                    size: wgpu::Extent3d {
                        width: 1,
                        height: 1,
                        depth_or_array_layers: layers,
                    },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: F::Rgba8Unorm,
                    usage: wgpu::TextureUsages::TEXTURE_BINDING,
                    view_formats: &[],
                },
                wgpu::util::TextureDataOrder::LayerMajor,
                data,
            )
        };
        let array = texture(2, &[200, 100, 50, alpha, 240, 210, 80, 128]);
        let array = array.create_view(&wgpu::TextureViewDescriptor {
            dimension: Some(wgpu::TextureViewDimension::D2Array),
            ..Default::default()
        });
        let image = texture(1, &[200, 100, 50, alpha]).create_view(&Default::default());
        let threshold = texture(1, &[0, 0, 0, 0]).create_view(&Default::default());
        let sampler = device.create_sampler(&Default::default());
        let resources: Vec<_> = entries
            .iter()
            .map(|entry| wgpu::BindGroupEntry {
                binding: entry.binding,
                resource: match entry.ty {
                    wgpu::BindingType::Buffer { .. } => uniform.as_entire_binding(),
                    wgpu::BindingType::Sampler(_) => wgpu::BindingResource::Sampler(&sampler),
                    wgpu::BindingType::Texture {
                        view_dimension: wgpu::TextureViewDimension::D2Array,
                        ..
                    } => wgpu::BindingResource::TextureView(&array),
                    _ => wgpu::BindingResource::TextureView(if entry.binding == 12 {
                        &threshold
                    } else {
                        &image
                    }),
                },
            })
            .collect();
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &bindings,
            entries: &resources,
        });
        Self {
            device,
            queue,
            terrain,
            shader,
            layout,
            bindings,
            group,
        }
    }
    fn render(
        &self,
        hdr: &mut Hdr,
        written: bool,
        entry: &str,
        tier: u32,
        overlay: u32,
        background: [f64; 4],
    ) -> [f32; 4] {
        let constants = [
            ("HDR_WRITE_LIGHT", f64::from(u8::from(written))),
            ("TEST_TIER", f64::from(tier)),
            ("TEST_OVERLAY", f64::from(overlay)),
        ];
        let pipeline = |entry, mark| {
            self.device
                .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                    label: None,
                    layout: Some(&self.layout),
                    vertex: wgpu::VertexState {
                        module: &self.shader,
                        entry_point: Some("test_vertex"),
                        compilation_options: wgpu::PipelineCompilationOptions {
                            constants: &constants,
                            ..Default::default()
                        },
                        buffers: &[],
                    },
                    primitive: Default::default(),
                    depth_stencil: None,
                    multisample: Default::default(),
                    fragment: Some(wgpu::FragmentState {
                        module: &self.shader,
                        entry_point: Some(entry),
                        compilation_options: wgpu::PipelineCompilationOptions {
                            constants: &constants,
                            ..Default::default()
                        },
                        targets: &[Some(wgpu::ColorTargetState {
                            format: F::Rgba16Float,
                            blend: Some(if mark || (self.terrain && entry != "fs_translucent") {
                                wgpu::BlendState::REPLACE
                            } else {
                                wgpu::BlendState::ALPHA_BLENDING
                            }),
                            write_mask: if mark {
                                wgpu::ColorWrites::ALPHA
                            } else {
                                wgpu::ColorWrites::ALL
                            },
                        })],
                    }),
                    multiview_mask: None,
                    cache: None,
                })
        };
        let color = pipeline(entry, false);
        let mark = (!self.terrain && !written && entry == "fs_entity").then(|| {
            pipeline(
                if overlay == 4 {
                    "fs_hdr_cloud_mark"
                } else {
                    "fs_hdr_mark"
                },
                true,
            )
        });
        let frame = hdr.frame(self.device, [1, 1]);
        let view = frame.create_view(&Default::default());
        let mut encoder = self.device.create_command_encoder(&Default::default());
        {
            let [r, g, b, a] = background;
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color { r, g, b, a }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                ..Default::default()
            });
            pass.set_bind_group(0, &self.group, &[]);
            pass.set_pipeline(&color);
            pass.draw(0..3, 0..1);
            if let Some(mark) = &mark {
                pass.set_pipeline(mark);
                pass.draw(0..3, 0..1);
            }
        }
        if !written {
            hdr.begin(self.queue, true);
            hdr.resolve(&mut encoder, None);
        }
        texel(self.device, self.queue, encoder, &frame)
    }
}

#[test]
fn actual_opaque_writers_match_marks_then_resolve() {
    let Some((device, queue)) = gpu() else {
        eprintln!("HDR draw GPU unavailable");
        return;
    };
    let mut hdr = Hdr::with_debug(&device, &queue, F::Rgba16Float, Output::Pq, true, None);
    (hdr.paper_white, hdr.peak) = (203.0, 1000.0);
    let (settings, shares) = hdr.draw_params(true);
    let mut count = 0;
    for fog in [0.0, 0.5, 1.0] {
        for visibility in [1.0, 0.4] {
            let globals = super::super::Globals {
                fog_color: [0.2, 0.3, 0.4, 1.0],
                fog: [0.0, 1.0, 0.0, 1.0],
                fog_ends: [100.0, 100.0, 0.0, 0.0],
                misc: [fog, visibility, 0.0, 0.0],
                filtering: [0.0, 1.0, 0.0, 0.0],
                hdr_windows: hdr.windows(),
                hdr_light: [0.8, 0.8, 1.0, 1.0],
                hdr_settings: settings,
                hdr_shares: shares,
                ..bytemuck::Zeroable::zeroed()
            };
            for terrain in [true, false] {
                let draw = Draw::new(&device, &queue, terrain, &globals, 255);
                for tier in 0..=4 {
                    for overlay in if terrain {
                        vec![0, 1]
                    } else {
                        vec![0, 6, 9, 10, 38]
                    } {
                        let entries = if terrain {
                            vec!["fs_opaque", "fs_solid"]
                        } else {
                            vec!["fs_entity"]
                        };
                        for entry in entries {
                            let marked =
                                draw.render(&mut hdr, false, entry, tier, overlay, [0.0; 4]);
                            let written =
                                draw.render(&mut hdr, true, entry, tier, overlay, [0.0; 4]);
                            // The marked path stores half floats before conversion; the written
                            // one stores after it. Allow two half-float ULPs in gamma light.
                            for i in 0..3 {
                                assert!(
                                    (marked[i] - written[i]).abs() <= 0.002 * marked[i].max(1.0),
                                    "{terrain} {entry} tier {tier} overlay {overlay} fog {fog} visibility {visibility}: {marked:?} {written:?}"
                                );
                            }
                            assert_eq!(written[3], 1.0);
                            count += 1;
                        }
                    }
                }
            }
        }
    }
    eprintln!("HDR actual draw GPU ran {count} opaque comparisons");
}

#[test]
fn actual_translucent_writers_blend_their_own_light() {
    let Some((device, queue)) = gpu() else {
        eprintln!("HDR blend GPU unavailable");
        return;
    };
    let mut hdr = Hdr::with_debug(&device, &queue, F::Rgba16Float, Output::Pq, true, None);
    let (settings, shares) = hdr.draw_params(true);
    let globals = super::super::Globals {
        fog: [0.0, 100.0, 0.0, 100.0],
        fog_ends: [100.0, 100.0, 0.0, 0.0],
        misc: [0.0, 1.0, 0.0, 0.0],
        filtering: [0.0, 1.0, 0.0, 0.0],
        hdr_windows: hdr.windows(),
        hdr_light: [0.8, 0.8, 1.0, 1.0],
        hdr_settings: settings,
        hdr_shares: shares,
        ..bytemuck::Zeroable::zeroed()
    };
    // Texel alpha 100 is clear glass's kind of texel, below the 0.6 the marked frame's
    // `fs_translucent_mark` skips: written, it still gets its own light.
    for (terrain, entry, overlay, tier, tint_alpha, texel_alpha) in [
        (true, "fs_translucent", 0, 0, 1.0, 180),
        (true, "fs_translucent", 0, 0, 0.5, 180),
        (true, "fs_translucent", 0, 0, 1.0, 100),
        (false, "fs_entity", 0, 3, 1.0, 180),
        (false, "fs_entity", 4, 0, 1.0, 180),
    ] {
        let mut globals = globals;
        globals.misc[2] = if tint_alpha < 1.0 { 1.0 } else { 0.0 };
        let draw = Draw::new(&device, &queue, terrain, &globals, texel_alpha);
        let src = draw.render(&mut hdr, true, entry, tier, overlay, [0.0; 4]);
        let dst = [2.5, 1.8, 0.9, 1.0];
        let got = draw.render(&mut hdr, true, entry, tier, overlay, dst);
        let alpha = f32::from(texel_alpha) / 255.0 * tint_alpha;
        if terrain {
            // Full block light leaves mark 194/255 whatever the texel's alpha, even when
            // vertex coverage is halved.
            let gain = (1.0 + (settings[1] - 1.0) * (1.0 - 194.0 / 255.0) / 0.24).powf(1.0 / 2.2);
            let expected = 200.0 / 255.0 * 0.8 * gain * alpha;
            assert!(
                (src[0] - expected).abs() < 0.002,
                "coverage {tint_alpha}: {src:?}, expected {expected}"
            );
        }
        for i in 0..3 {
            let want = src[i] + dst[i] as f32 * (1.0 - alpha);
            assert!(
                (got[i] - want).abs() < 0.003,
                "{entry} {overlay}: {got:?} expected {want}"
            );
        }
        assert!(got[0] > 1.0);
    }
    eprintln!("HDR actual blend GPU ran terrain, particle and cloud equations");
}

#[test]
fn unmarked_entity_users_keep_their_source_gain() {
    let Some((device, queue)) = gpu() else {
        return;
    };
    let mut hdr = Hdr::with_debug(&device, &queue, F::Rgba16Float, Output::Pq, true, None);
    let (settings, shares) = hdr.draw_params(true);
    let globals = super::super::Globals {
        fog: [0.0, 100.0, 0.0, 100.0],
        fog_ends: [100.0, 100.0, 0.0, 0.0],
        hdr_windows: hdr.windows(),
        hdr_light: [1.0, 1.0, 1.0, 1.0],
        hdr_settings: settings,
        hdr_shares: shares,
        ..bytemuck::Zeroable::zeroed()
    };
    let draw = Draw::new(&device, &queue, false, &globals, 255);
    for (entry, overlay) in [
        ("fs_entity_unmarked", 0),
        ("fs_entity_shadow", 0),
        ("fs_energy_swirl", 0),
        ("fs_entity", 11),
        ("fs_entity", 12),
    ] {
        let raw = draw.render(&mut hdr, true, entry, 0, overlay, [0.0; 4]);
        let mut hdr_off = Hdr::with_debug(&device, &queue, F::Rgba16Float, Output::Pq, true, None);
        hdr_off.look = false;
        let legacy = draw.render(&mut hdr_off, false, entry, 0, overlay, [0.0; 4]);
        for i in 0..3 {
            assert!(
                (raw[i] - legacy[i]).abs() < 0.001,
                "{entry} {overlay}: {raw:?} {legacy:?}"
            );
        }
    }
}

#[test]
fn additive_texels_below_the_mark_cutoff_keep_their_gain() {
    let Some((device, queue)) = gpu() else {
        return;
    };
    let mut hdr = Hdr::with_debug(&device, &queue, F::Rgba16Float, Output::Pq, true, None);
    let (settings, shares) = hdr.draw_params(true);
    let globals = super::super::Globals {
        fog: [0.0, 100.0, 0.0, 100.0],
        hdr_windows: hdr.windows(),
        hdr_light: [1.0, 1.0, 1.0, 1.0],
        hdr_settings: settings,
        hdr_shares: shares,
        ..bytemuck::Zeroable::zeroed()
    };
    let draw = Draw::new(&device, &queue, false, &globals, 16);
    let background = [0.0, 0.0, 0.0, 1.0];
    let marked = draw.render(&mut hdr, false, "fs_celestial", 4, 0, background);
    let written = draw.render(&mut hdr, true, "fs_celestial", 4, 0, background);
    // The old mark draw discards this texel; source alpha still contributes its colour.
    let red = (200.0 / 255.0) * 0.8 * 0.7 * (16.0 / 255.0);
    assert!(
        (written[0] - red).abs() < 0.0001,
        "{written:?}: expected {red}"
    );
    for i in 0..3 {
        assert!(
            (written[i] - marked[i]).abs() < 0.0001,
            "{written:?} {marked:?}"
        );
    }
}

fn check_oit_writer(terrain: bool, kind: &str, overlay: u32, tier: u32, alphas: &[u8]) {
    let instance = crate::gfx::test_gpu::instance();
    let Ok(adapter) = pollster::block_on(instance.request_adapter(&Default::default())) else {
        eprintln!("OIT GPU unavailable");
        return;
    };
    if !adapter
        .features()
        .contains(wgpu::Features::FLOAT32_BLENDABLE)
    {
        eprintln!("OIT unsupported on {:?}", adapter.get_info());
        return;
    }
    eprintln!(
        "OIT GPU {:?}: {kind}, alphas {alphas:?}",
        adapter.get_info()
    );
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        required_features: wgpu::Features::FLOAT32_BLENDABLE,
        ..Default::default()
    }))
    .unwrap();
    let mut hdr = Hdr::with_debug(&device, &queue, F::Rgba16Float, Output::Pq, true, None);
    let (settings, shares) = hdr.draw_params(true);
    let globals = super::super::Globals {
        fog: [0.0, 100.0, 0.0, 100.0],
        fog_ends: [100.0, 100.0, 0.0, 0.0],
        misc: [0.0, 1.0, 0.0, 0.0],
        filtering: [0.0, 1.0, 0.0, 0.0],
        hdr_windows: hdr.windows(),
        hdr_light: [0.8, 0.8, 1.0, 1.0],
        hdr_settings: settings,
        hdr_shares: shares,
        ..bytemuck::Zeroable::zeroed()
    };
    for &alpha in alphas {
        let draw = Draw::new(&device, &queue, terrain, &globals, alpha);
        let got = draw.render_oit(&mut hdr, kind, tier, overlay, true);
        let unconverted = draw.render_oit(&mut hdr, kind, tier, overlay, false);
        let entry = if terrain {
            "fs_translucent"
        } else if kind == "lightning" {
            "fs_celestial"
        } else {
            "fs_entity"
        };
        let mut expected = draw.render(
            &mut hdr,
            true,
            entry,
            tier,
            overlay,
            if kind == "lightning" {
                [0.0; 4]
            } else {
                [2.5, 1.8, 0.9, 1.0]
            },
        );
        if kind == "lightning" {
            for (c, background) in expected[..3].iter_mut().zip([2.5, 1.8, 0.9]) {
                *c += background;
            }
        }
        for i in 0..3 {
            assert!(
                (got[i] - expected[i]).abs() < 0.006,
                "{kind} alpha {alpha}: {got:?} expected {expected:?}"
            );
        }
        assert_eq!(got[3], 1.0);
        assert!(got[0] > 1.0, "OIT clipped light: {got:?}");
        assert!(
            (got[0] - unconverted[0]).abs() > 0.01,
            "test must distinguish missing conversion: {got:?} {unconverted:?}"
        );
    }
}

#[test]
fn actual_oit_terrain_writes_its_own_light_including_clear_glass() {
    check_oit_writer(true, "terrain", 0, 0, &[180, 100]);
}
#[test]
fn actual_oit_particle_writes_its_own_light() {
    check_oit_writer(false, "entity", 0, 3, &[180]);
}
#[test]
fn actual_oit_cloud_writes_its_own_light() {
    check_oit_writer(false, "cloud", 4, 0, &[180]);
}
#[test]
fn actual_oit_additive_adds_converted_light_without_coverage() {
    check_oit_writer(false, "lightning", 0, 4, &[180]);
}
