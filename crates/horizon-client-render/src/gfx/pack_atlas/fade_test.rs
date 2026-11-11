//! Original pack-to-pixel controls of terrain's visibility input, with no external fixture.
use super::PackAtlas;
use crate::gfx::{section_instance, section_visibility};
use crate::mesher::TerrainVertex;
use std::{
    path::PathBuf,
    time::{Duration, Instant},
};
use wgpu::util::DeviceExt;

struct ProbePack(PathBuf);
impl Drop for ProbePack {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).unwrap();
    }
}

#[test]
fn terrain_fade_reaches_pack_pixels_through_real_instances() {
    let instance = crate::gfx::test_gpu::instance();
    let adapter = match pollster::block_on(instance.request_adapter(&Default::default())) {
        Ok(adapter) => adapter,
        Err(error) => {
            eprintln!("SKIP terrain fade GPU control: no adapter: {error}");
            return;
        }
    };
    eprintln!("terrain fade adapter: {:?}", adapter.get_info());
    let (device, queue) = pollster::block_on(adapter.request_device(&Default::default())).unwrap();
    let array = device.create_texture(&wgpu::TextureDescriptor {
        label: None,
        size: wgpu::Extent3d {
            width: 2,
            height: 2,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::COPY_SRC | wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    });
    let mut encoder = device.create_command_encoder(&Default::default());
    let mut atlas = PackAtlas::new(&device, &mut encoder, &[array], 1, 1, 2).unwrap();
    atlas.separate_ao = true;
    queue.submit([encoder.finish()]);
    let folder = ProbePack(
        std::env::temp_dir().join(format!("horizon-terrain-fade-{}", std::process::id())),
    );
    std::fs::create_dir_all(folder.0.join("shaders")).unwrap();
    // The initializer, varying and integer output are independent probe code. The program reads
    // `mc_chunkFade` unguarded: Horizon defines no `IRIS_FEATURE_*` (spec OQ-2).
    let vertex = "#version 330\nout vec4 observation;\n\
        float first = mc_chunkFade;\n\
        void main() {\n\
        observation = vec4(mc_chunkFade, first, gl_Color.a, gl_MultiTexCoord0.x);\n\
        gl_Position = vec4(0.0, 0.0, 0.0, 1.0);\n}";
    let fragment = "#version 330\nin vec4 observation;\nlayout(location = 0) out uvec4 pixel;\n\
        void main() { pixel = floatBitsToUint(observation); }";
    let geometry_vertex = vertex
        .replace("float first = mc_chunkFade;", "")
        .replace("mc_chunkFade, first,", "mc_chunkFade, mc_chunkFade,")
        .replace("observation", "vertex_observation");
    let geometry_shader = "#version 330\nlayout(triangles) in;\n\
        layout(triangle_strip, max_vertices=3) out;\nin vec4 vertex_observation[];\n\
        out vec4 observation;\nconst float mc_chunkFade = 0.8125;\nvoid main() {\n\
        for (int k=0; k<3; ++k) {\nobservation = vertex_observation[k];\n\
        observation.y = mc_chunkFade;\n\
        gl_Position = vec4(k == 1 ? 3.0 : -1.0, k == 2 ? 3.0 : -1.0, 0.0, 1.0);\n\
        EmitVertex();\n}\nEndPrimitive();\n}";
    for name in ["gbuffers_terrain", "gbuffers_water", "shadow"] {
        for (extension, code) in [("vsh", vertex), ("fsh", fragment)] {
            std::fs::write(folder.0.join(format!("shaders/{name}.{extension}")), code).unwrap();
        }
    }
    let uploaded = Instant::now();
    let cases = [
        ("first frame", Some(&uploaded), 0, 1000, 0, 0.0),
        ("137ms", Some(&uploaded), 137, 1000, 0, 0.137),
        ("midpoint", Some(&uploaded), 500, 1000, 0, 0.5),
        ("finished", Some(&uploaded), 1000, 1000, 0, 1.0),
        ("disabled", Some(&uploaded), 0, 0, 0, 1.0),
        ("no upload transition", None, 0, 1000, 0, 1.0),
        ("distant handover", Some(&uploaded), 500, 1000, 17, 1.0),
    ];
    let mut count = 0;
    for requested in [false, true] {
        std::fs::write(
            folder.0.join("shaders/shaders.properties"),
            if requested {
                "shadow.enabled=true\niris.features.optional=FADE_VARIABLE\niris.features.required=FADE_VARIABLE\n"
            } else {
                "shadow.enabled=true\n"
            },
        )
        .unwrap();
        let pack = horizon_shaders::Pack::open(&folder.0).unwrap();
        let mut macros = horizon_shaders::Macros::standard();
        let (properties, notes) = horizon_shaders::read_properties(&pack, &mut macros);
        assert!(notes.is_empty(), "{notes:?}");
        for flat in [false, true] {
            for geometry in [false, true].into_iter().filter(|g| !*g || flat) {
                let geometry_path = folder.0.join("shaders/shadow.gsh");
                if geometry {
                    std::fs::write(&geometry_path, geometry_shader).unwrap();
                } else if geometry_path.exists() {
                    std::fs::remove_file(&geometry_path).unwrap();
                }
                std::fs::write(
                    folder.0.join("shaders/shadow.vsh"),
                    if geometry {
                        geometry_vertex.as_str()
                    } else {
                        vertex
                    },
                )
                .unwrap();
                let post = horizon_shaders::load_post(
                    &pack,
                    "",
                    &properties,
                    &macros,
                    &horizon_shaders::GameInputs {
                        terrain: Some(atlas.terrain_inputs(flat)),
                        terrain_geometry: geometry.then(|| horizon_shaders::GeometryInputs {
                            inputs: atlas.terrain_inputs(true),
                            pull: String::new(),
                            index: "@".to_owned(),
                        }),
                        ..Default::default()
                    },
                );
                for (name, program) in [
                    ("terrain", &post.terrain),
                    ("water", &post.water),
                    ("shadow", &post.shadow),
                ] {
                    if geometry && name != "shadow" {
                        continue;
                    }
                    let program = program
                        .as_ref()
                        .unwrap_or_else(|| panic!("{name}: {:?}", post.notes));
                    for &(case, timestamp, elapsed, fade_ms, handover, expected) in &cases {
                        let visibility = section_visibility(
                            timestamp,
                            fade_ms,
                            uploaded + Duration::from_millis(elapsed),
                        );
                        let origin = section_instance([48.0, 0.0, -16.0], handover, visibility);
                        let pixel = sample(
                            &device,
                            &queue,
                            &atlas,
                            program.program.as_ref().unwrap(),
                            flat,
                            geometry,
                            origin,
                            0x80ffffff,
                        );
                        // The fade reaches the program whether the pack requested it or not.
                        let fade = expected;
                        let expected = [
                            fade,
                            if geometry { 0.8125 } else { fade },
                            128.0 / 255.0,
                            0.25,
                        ];
                        assert!(
                            pixel
                                .iter()
                                .zip(expected)
                                .all(|(a, b)| a.is_finite() && (a - b).abs() < 2e-7),
                            "{name} {case} flat={flat} geometry={geometry} requested={requested}: {pixel:?} != {expected:?}"
                        );
                        count += 1;
                    }
                }
            }
        }
    }
    eprintln!("terrain fade pack-to-pixel cases executed: {count}");
    assert_eq!(count, 98);
}

/// `horizonTier`: the HDR tier the mesher marks a light source's faces with (`mesher::emitting`),
/// read by a pack's terrain program, flat or instanced.
#[test]
fn terrain_tier_reaches_pack_pixels() {
    let instance = crate::gfx::test_gpu::instance();
    let adapter = match pollster::block_on(instance.request_adapter(&Default::default())) {
        Ok(adapter) => adapter,
        Err(error) => {
            eprintln!("SKIP terrain tier GPU control: no adapter: {error}");
            return;
        }
    };
    let (device, queue) = pollster::block_on(adapter.request_device(&Default::default())).unwrap();
    let array = device.create_texture(&wgpu::TextureDescriptor {
        label: None,
        size: wgpu::Extent3d {
            width: 2,
            height: 2,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::COPY_SRC | wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    });
    let mut encoder = device.create_command_encoder(&Default::default());
    let atlas = PackAtlas::new(&device, &mut encoder, &[array], 1, 1, 2).unwrap();
    queue.submit([encoder.finish()]);
    let folder = ProbePack(
        std::env::temp_dir().join(format!("horizon-terrain-tier-{}", std::process::id())),
    );
    std::fs::create_dir_all(folder.0.join("shaders")).unwrap();
    std::fs::write(
        folder.0.join("shaders/gbuffers_terrain.vsh"),
        "#version 330\nin float horizonTier;\nout vec4 observation;\n\
         void main() {\nobservation = vec4(horizonTier, 0.0, 0.0, 0.0);\n\
         gl_Position = vec4(0.0, 0.0, 0.0, 1.0);\n}",
    )
    .unwrap();
    std::fs::write(
        folder.0.join("shaders/gbuffers_terrain.fsh"),
        "#version 330\nin vec4 observation;\nlayout(location = 0) out uvec4 pixel;\n\
         void main() { pixel = floatBitsToUint(observation); }",
    )
    .unwrap();
    let pack = horizon_shaders::Pack::open(&folder.0).unwrap();
    let mut macros = horizon_shaders::Macros::standard();
    let (properties, _) = horizon_shaders::read_properties(&pack, &mut macros);
    use crate::mesher::{BRIGHT, LOW, VERY_BRIGHT, WHOLE, emitting};
    // No light, low, very bright, and bright all over (4 more).
    let cases = [
        (0, 0.0),
        (LOW, 1.0),
        (VERY_BRIGHT, 4.0),
        (BRIGHT | WHOLE, 7.0),
    ];
    for flat in [false, true] {
        let post = horizon_shaders::load_post(
            &pack,
            "",
            &properties,
            &macros,
            &horizon_shaders::GameInputs {
                terrain: Some(atlas.terrain_inputs(flat)),
                ..Default::default()
            },
        );
        let program = post
            .terrain
            .as_ref()
            .unwrap_or_else(|| panic!("{:?}", post.notes));
        for (tier, expected) in cases {
            let origin = section_instance([0.0; 3], 0, 1.0);
            let pixel = sample(
                &device,
                &queue,
                &atlas,
                program.program.as_ref().unwrap(),
                flat,
                false,
                origin,
                emitting(0xffff_ffff, tier),
            );
            assert_eq!(pixel[0], expected, "tier {tier} flat={flat}");
        }
    }
}

fn sample(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    atlas: &PackAtlas,
    program: &horizon_shaders::Program,
    flat: bool,
    geometry: bool,
    origin: [f32; 4],
    color: u32,
) -> [f32; 4] {
    let buffer = |bytes: &[u8], usage| {
        device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: None,
            contents: bytes,
            usage,
        })
    };
    let uniform = buffer(
        &vec![0; program.interface.uniform_size as usize],
        wgpu::BufferUsages::UNIFORM,
    );
    let origins = buffer(
        bytemuck::bytes_of(&origin),
        wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::VERTEX,
    );
    let slots = buffer(bytemuck::bytes_of(&0u32), wgpu::BufferUsages::STORAGE);
    let vertices: Vec<_> = [[0., 0., 0.], [1., 0., 0.], [1., 1., 0.], [0., 1., 0.]]
        .into_iter()
        .map(|pos| TerrainVertex::new(pos, [0.25, 0.75], 0, color, 0))
        .collect();
    let data = buffer(
        bytemuck::cast_slice(&vertices),
        wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::VERTEX,
    );
    let ids = buffer(bytemuck::bytes_of(&[0u32; 2]), wgpu::BufferUsages::STORAGE);
    let rects = buffer(
        bytemuck::cast_slice(&super::super::pack_post::terrain_tables(
            &[],
            &atlas.rects_table(),
        )),
        wgpu::BufferUsages::STORAGE,
    );
    let binding = |n, storage| wgpu::BindGroupLayoutEntry {
        binding: n,
        visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
        ty: wgpu::BindingType::Buffer {
            ty: if storage {
                wgpu::BufferBindingType::Storage { read_only: true }
            } else {
                wgpu::BufferBindingType::Uniform
            },
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        count: None,
    };
    let samplers: Vec<wgpu::BindGroupLayoutEntry> = (program.interface.samplers.iter())
        .flat_map(|s| {
            [
                wgpu::BindGroupLayoutEntry {
                    binding: s.binding,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: false },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: s.binding + 1,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::NonFiltering),
                    count: None,
                },
            ]
        })
        .collect();
    let entries = [
        [vec![binding(0, false)], samplers].concat(),
        if flat {
            vec![binding(0, true), binding(1, true)]
        } else {
            vec![]
        },
        vec![],
        if flat {
            vec![binding(0, true), binding(1, true), binding(2, true)]
        } else {
            vec![]
        },
    ];
    let layouts: Vec<_> = entries
        .iter()
        .map(|entries| {
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: None,
                entries,
            })
        })
        .collect();
    let bindings = [
        vec![(0, &uniform)],
        if flat {
            vec![(0, &slots), (1, &origins)]
        } else {
            vec![]
        },
        vec![],
        if flat {
            vec![(0, &ids), (1, &data), (2, &rects)]
        } else {
            vec![]
        },
    ];
    // The program's samplers (water's boats' patches, `horizonWaterMask`): a 1x1 stand-in each,
    // as the pack binds when there is none, its sampler beside it.
    let stand_in = device
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
            format: wgpu::TextureFormat::R32Float,
            usage: wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        })
        .create_view(&Default::default());
    let nearest = device.create_sampler(&Default::default());
    let samplers = &program.interface.samplers;
    let groups: Vec<_> = layouts
        .iter()
        .zip(bindings)
        .enumerate()
        .map(|(group, (layout, buffers))| {
            let mut entries: Vec<_> = buffers
                .into_iter()
                .map(|(binding, buffer)| wgpu::BindGroupEntry {
                    binding,
                    resource: buffer.as_entire_binding(),
                })
                .collect();
            if group == 0 {
                for s in samplers {
                    entries.push(wgpu::BindGroupEntry {
                        binding: s.binding,
                        resource: wgpu::BindingResource::TextureView(&stand_in),
                    });
                    entries.push(wgpu::BindGroupEntry {
                        binding: s.binding + 1,
                        resource: wgpu::BindingResource::Sampler(&nearest),
                    });
                }
            }
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: None,
                layout,
                entries: &entries,
            })
        })
        .collect();
    let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: None,
        bind_group_layouts: &layouts.iter().map(Some).collect::<Vec<_>>(),
        ..Default::default()
    });
    let module = |module| {
        device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: None,
            source: wgpu::ShaderSource::Naga(std::borrow::Cow::Owned(module)),
        })
    };
    let vs = module(program.vertex.clone());
    let fs = module(program.fragment.clone());
    let vertices = [
        Some(wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<TerrainVertex>() as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &crate::gfx::TERRAIN_ATTRIBUTES,
        }),
        Some(wgpu::VertexBufferLayout {
            array_stride: 16,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &wgpu::vertex_attr_array![5 => Float32x4],
        }),
    ];
    let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: None,
        layout: Some(&layout),
        vertex: wgpu::VertexState {
            module: &vs,
            entry_point: Some("main"),
            compilation_options: Default::default(),
            buffers: &vertices[..if flat { 1 } else { 2 }],
        },
        primitive: wgpu::PrimitiveState {
            topology: if geometry {
                wgpu::PrimitiveTopology::TriangleList
            } else {
                wgpu::PrimitiveTopology::PointList
            },
            ..Default::default()
        },
        depth_stencil: None,
        multisample: Default::default(),
        fragment: Some(wgpu::FragmentState {
            module: &fs,
            entry_point: Some("main"),
            compilation_options: Default::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format: wgpu::TextureFormat::Rgba32Uint,
                blend: None,
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        multiview_mask: None,
        cache: None,
    });
    let output = device.create_texture(&wgpu::TextureDescriptor {
        label: None,
        size: wgpu::Extent3d {
            width: 1,
            height: 1,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba32Uint,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let read = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: 16,
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let view = output.create_view(&Default::default());
    let mut encoder = device.create_command_encoder(&Default::default());
    {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: None,
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                    store: wgpu::StoreOp::Store,
                },
            })],
            ..Default::default()
        });
        pass.set_pipeline(&pipeline);
        for (n, group) in groups.iter().enumerate() {
            pass.set_bind_group(n as u32, group, &[]);
        }
        pass.set_vertex_buffer(0, data.slice(..));
        if !flat {
            pass.set_vertex_buffer(1, origins.slice(..));
        }
        pass.draw(0..if geometry { 3 } else { 1 }, 0..1);
    }
    encoder.copy_texture_to_buffer(
        output.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &read,
            layout: Default::default(),
        },
        output.size(),
    );
    queue.submit([encoder.finish()]);
    read.map_async(wgpu::MapMode::Read, .., |_| {});
    device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
    *bytemuck::from_bytes(&read.get_mapped_range(..).unwrap())
}
