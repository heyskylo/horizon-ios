//! Original numeric controls for the production terrain attribute expressions.
//! A compute invocation stands for a terrain vertex; it runs the actual input body.
use super::*;
use wgpu::util::DeviceExt;

fn dispatch(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    pipeline: &wgpu::ComputePipeline,
    groups: &[wgpu::BindGroup],
    result: &wgpu::Buffer,
) -> Vec<[f32; 4]> {
    let read = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: result.size(),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&Default::default());
    {
        let mut pass = encoder.begin_compute_pass(&Default::default());
        pass.set_pipeline(pipeline);
        for (i, g) in groups.iter().enumerate() {
            pass.set_bind_group(i as u32, g, &[]);
        }
        pass.dispatch_workgroups((result.size() / 32) as u32, 1, 1);
    }
    encoder.copy_buffer_to_buffer(result, 0, &read, 0, result.size());
    queue.submit([encoder.finish()]);
    read.slice(..)
        .map_async(wgpu::MapMode::Read, |r| r.unwrap());
    device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
    bytemuck::cast_slice(&read.slice(..).get_mapped_range().unwrap()).to_vec()
}

#[test]
#[ignore = "offscreen GPU control; run explicitly with an available adapter"]
fn terrain_source_ownership_gpu() {
    let instance = crate::gfx::test_gpu::instance();
    let adapter = pollster::block_on(instance.request_adapter(&Default::default()))
        .expect("executed GPU control requires an adapter");
    eprintln!("TERRAIN_SOURCE_ADAPTER {:?}", adapter.get_info());
    let (device, queue) = pollster::block_on(adapter.request_device(&Default::default())).unwrap();
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: None,
        size: wgpu::Extent3d {
            width: 1,
            height: 1,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let mut encoder = device.create_command_encoder(&Default::default());
    let atlas = PackAtlas::new(&device, &mut encoder, &[texture], 1, 1, 1).unwrap();
    let inputs = atlas.terrain_inputs(true);
    // Inset faces of two source states sharing sprite zero, an extended multipart face,
    // and the second state's fluid face. Ownership is authored
    // fixture data, not inferred from normal, sprite, position or diagonal.
    let mut vertices = Vec::new();
    for (x, y) in [(2.0, 3.25), (6.0, 3.25), (7.25, 3.75), (6.0, 3.875)] {
        for [dx, dz] in [[0.0, 0.0], [0.0, 0.5], [0.5, 0.5], [0.5, 0.0]] {
            vertices.push(crate::mesher::TerrainVertex::new(
                [x + dx, y, 4.0 + dz],
                [0.0; 2],
                0,
                u32::MAX,
                0,
            ));
        }
    }
    let declarations = inputs
        .declarations
        .lines()
        .filter(|l| !l.starts_with("layout(location"))
        .collect::<Vec<_>>()
        .join("\n");
    let attribute = |name: &str| {
        inputs
            .attributes
            .iter()
            .find(|(n, _)| n == name)
            .unwrap()
            .1
            .as_str()
    };
    let source = format!(
        "#version 450\n{declarations}\n\
         layout(local_size_x=1) in;\n\
         layout(set=0,binding=0) buffer Results {{ vec4 result[]; }};\n\
         void main() {{\n\
         uint hz_index=gl_GlobalInvocationID.x;\n\
         vec3 hz_v_pos=uintBitsToFloat(uvec3(hz_vertices[6u*hz_index],hz_vertices[6u*hz_index+1u],hz_vertices[6u*hz_index+2u]));\n\
         uvec2 hz_v_uv=uvec2(0); uint hz_v_layer=0u; vec4 hz_v_color=vec4(1); uvec2 hz_v_light=uvec2(0);\n\
         vec4 hz_Vertex, hz_MultiTexCoord0, hz_MultiTexCoord1, hz_MultiTexCoord2, hz_Color; vec3 hz_Normal;\n\
         {}\nresult[2u*hz_index]={}; result[2u*hz_index+1u]={};\n}}",
        inputs.body.replace("gl_VertexIndex", "hz_index"),
        attribute("mc_Entity"),
        attribute("at_midBlock")
    );
    let mut frontend = naga::front::glsl::Frontend::default();
    let module = frontend
        .parse(
            &naga::front::glsl::Options::from(naga::ShaderStage::Compute),
            &source,
        )
        .unwrap();
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("Original terrain numeric control"),
        source: wgpu::ShaderSource::Naga(std::borrow::Cow::Owned(module)),
    });
    let layouts: Vec<_> = [1, 2, 0, 3]
        .into_iter()
        .map(|count| {
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: None,
                entries: &(0..count)
                    .map(|binding| wgpu::BindGroupLayoutEntry {
                        binding,
                        visibility: if count == 1 {
                            wgpu::ShaderStages::COMPUTE
                        } else {
                            wgpu::ShaderStages::COMPUTE | wgpu::ShaderStages::VERTEX
                        },
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Storage {
                                read_only: binding != 0 || count != 1,
                            },
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                        count: None,
                    })
                    .collect::<Vec<_>>(),
            })
        })
        .collect();
    let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: None,
        bind_group_layouts: &layouts.iter().map(Some).collect::<Vec<_>>(),
        ..Default::default()
    });
    let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: None,
        layout: Some(&layout),
        module: &shader,
        entry_point: Some("main"),
        compilation_options: Default::default(),
        cache: None,
    });
    let storage = |bytes: &[u8]| {
        device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: None,
            contents: bytes,
            usage: wgpu::BufferUsages::STORAGE,
        })
    };
    let result = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: vertices.len() as u64 * 32,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
        mapped_at_creation: false,
    });
    let slots = storage(bytemuck::cast_slice(&[0u32]));
    let origins = storage(bytemuck::cast_slice(&[[128.0f32, -32.0, -256.0, -1.0]]));
    let ids = storage(bytemuck::cast_slice(&[
        crate::mesher::TerrainSource::new(0, [2, 3, 4], 0, false),
        crate::mesher::TerrainSource::new(1, [6, 3, 4], 13, false),
        crate::mesher::TerrainSource::new(1, [6, 3, 4], 13, false),
        crate::mesher::TerrainSource::new(1, [6, 3, 4], 13, true),
    ]));
    let vertex_storage = storage(bytemuck::cast_slice(&vertices));
    let rects = storage(bytemuck::cast_slice(
        &super::super::pack_post::terrain_tables(&[31, -1, 47, 59], &[0, 1]),
    ));
    let group = |set: u32, buffers: &[&wgpu::Buffer]| {
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &pipeline.get_bind_group_layout(set),
            entries: &buffers
                .iter()
                .enumerate()
                .map(|(i, b)| wgpu::BindGroupEntry {
                    binding: i as u32,
                    resource: b.as_entire_binding(),
                })
                .collect::<Vec<_>>(),
        })
    };
    let groups = [
        group(0, &[&result]),
        group(1, &[&slots, &origins]),
        group(2, &[]),
        group(3, &[&ids, &vertex_storage, &rects]),
    ];
    queue.submit([encoder.finish()]);
    let actual = dispatch(&device, &queue, &pipeline, &groups, &result);
    let mut mismatches = Vec::new();
    for (i, vertex) in vertices.iter().enumerate() {
        let q = i / 4;
        let expected_id = [31.0, 47.0, 47.0, 59.0][q];
        let expected_light = [0.0, 13.0, 13.0, 13.0][q];
        let owner = [
            [2.5, 3.5, 4.5],
            [6.5, 3.5, 4.5],
            [6.5, 3.5, 4.5],
            [6.5, 3.5, 4.5],
        ][q];
        let expected_mid = [
            (owner[0] - vertex.pos[0]) * 64.0,
            (owner[1] - vertex.pos[1]) * 64.0,
            (owner[2] - vertex.pos[2]) * 64.0,
            expected_light,
        ];
        if actual[i * 2] != [expected_id, if q == 3 { 1.0 } else { 0.0 }, 0.0, 0.0]
            || actual[i * 2 + 1] != expected_mid
        {
            mismatches.push((
                i,
                actual[i * 2],
                actual[i * 2 + 1],
                expected_id,
                expected_mid,
            ));
        }
    }
    assert!(
        mismatches.is_empty(),
        "terrain source mismatches: {mismatches:?}"
    );

    // Rasterize the same native vertices and production input body, with a diagnostic
    // projection placing one face in each strip. The fragment reports id, emission and
    // reconstructed owner Y/X after a nonzero section translation; no texture color
    // can hide a wrong ownership value.
    let vertex_source = format!(
        "#version 450\n{}\nlayout(location=0) flat out vec4 control;\n\
        void main(){{vec4 hz_Vertex,hz_MultiTexCoord0,hz_MultiTexCoord1,hz_MultiTexCoord2,hz_Color;vec3 hz_Normal;\n\
        {}\nvec4 entity={};vec4 mid={};\n\
        control=vec4(entity.x,mid.w,hz_Vertex.y+mid.y/64.0,hz_Vertex.x+mid.x/64.0);\n\
        uint corner=uint(gl_VertexIndex)%4u;float q=float(uint(gl_VertexIndex)/4u);\n\
        gl_Position=vec4(-1.0+(q+float(corner>=2u))*0.5,(corner==1u||corner==2u)?1.0:-1.0,0.5,1.0);}}",
        inputs.declarations,
        inputs.body,
        attribute("mc_Entity"),
        attribute("at_midBlock")
    );
    let vertex_module = frontend
        .parse(
            &naga::front::glsl::Options::from(naga::ShaderStage::Vertex),
            &vertex_source,
        )
        .unwrap();
    let vertex_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: None,
        source: wgpu::ShaderSource::Naga(std::borrow::Cow::Owned(vertex_module)),
    });
    let fragment_shader=device.create_shader_module(wgpu::ShaderModuleDescriptor { label:None,source:wgpu::ShaderSource::Wgsl(
        "@fragment fn fs(@location(0) @interpolate(flat) control:vec4<f32>)->@location(0) vec4<f32>{return control;}".into()) });
    let raster = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: None,
        layout: Some(&layout),
        vertex: wgpu::VertexState {
            module: &vertex_shader,
            entry_point: Some("main"),
            compilation_options: Default::default(),
            buffers: &[Some(wgpu::VertexBufferLayout {
                array_stride: 24,
                step_mode: wgpu::VertexStepMode::Vertex,
                attributes: &super::super::TERRAIN_ATTRIBUTES,
            })],
        },
        fragment: Some(wgpu::FragmentState {
            module: &fragment_shader,
            entry_point: Some("fs"),
            compilation_options: Default::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format: wgpu::TextureFormat::Rgba32Float,
                blend: None,
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        primitive: Default::default(),
        depth_stencil: None,
        multisample: Default::default(),
        multiview_mask: None,
        cache: None,
    });
    let vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: None,
        contents: bytemuck::cast_slice(&vertices),
        usage: wgpu::BufferUsages::VERTEX,
    });
    let indices: Vec<u32> = (0..4)
        .flat_map(|q| [4 * q, 4 * q + 1, 4 * q + 2, 4 * q, 4 * q + 2, 4 * q + 3])
        .collect();
    let index_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: None,
        contents: bytemuck::cast_slice(&indices),
        usage: wgpu::BufferUsages::INDEX,
    });
    let target = device.create_texture(&wgpu::TextureDescriptor {
        label: None,
        size: wgpu::Extent3d {
            width: 16,
            height: 1,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba32Float,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = target.create_view(&Default::default());
    let pixels = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: 256,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
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
        pass.set_pipeline(&raster);
        for (i, g) in groups.iter().enumerate() {
            pass.set_bind_group(i as u32, g, &[]);
        }
        pass.set_vertex_buffer(0, vertex_buffer.slice(..));
        pass.set_index_buffer(index_buffer.slice(..), wgpu::IndexFormat::Uint32);
        pass.draw_indexed(0..24, 0, 0..1);
    }
    encoder.copy_texture_to_buffer(
        target.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &pixels,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(256),
                rows_per_image: Some(1),
            },
        },
        target.size(),
    );
    queue.submit([encoder.finish()]);
    pixels
        .slice(..)
        .map_async(wgpu::MapMode::Read, |r| r.unwrap());
    device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
    let pixels = pixels.slice(..).get_mapped_range().unwrap();
    let pixels: &[[f32; 4]] = bytemuck::cast_slice(&pixels);
    for (i, pixel) in pixels.iter().enumerate() {
        assert_eq!(
            *pixel,
            [
                [31.0, 0.0, -28.5, 130.5],
                [47.0, 13.0, -28.5, 134.5],
                [47.0, 13.0, -28.5, 134.5],
                [59.0, 13.0, -28.5, 134.5]
            ][i / 4],
            "diagnostic raster pixel {i}"
        );
    }

    // Reload changes only the immutable mapping; the same raw state/owner records
    // resolve to new material ids without baking a previous pack's ids into a mesh.
    let changed = storage(bytemuck::cast_slice(
        &super::super::pack_post::terrain_tables(&[211, -1, 223, 59], &[0, 1]),
    ));
    let changed_groups = [
        groups[0].clone(),
        groups[1].clone(),
        groups[2].clone(),
        group(3, &[&ids, &vertex_storage, &changed]),
    ];
    let reloaded = dispatch(&device, &queue, &pipeline, &changed_groups, &result);
    for i in 0..vertices.len() {
        assert_eq!(
            reloaded[2 * i],
            [
                if i < 4 {
                    211.0
                } else if i < 12 {
                    223.0
                } else {
                    59.0
                },
                if i >= 12 { 1.0 } else { 0.0 },
                0.0,
                0.0
            ]
        );
        assert_eq!(reloaded[2 * i + 1], actual[2 * i + 1]);
    }
    let missing = storage(&[0; 8]);
    let missing_groups = [
        groups[0].clone(),
        groups[1].clone(),
        groups[2].clone(),
        group(3, &[&missing, &vertex_storage, &rects]),
    ];
    let neutral = dispatch(&device, &queue, &pipeline, &missing_groups, &result);
    for i in 0..vertices.len() {
        assert_eq!(neutral[2 * i], [-1.0, 0.0, 0.0, 0.0]);
        assert_eq!(neutral[2 * i + 1], [0.0; 4]);
    }
}
