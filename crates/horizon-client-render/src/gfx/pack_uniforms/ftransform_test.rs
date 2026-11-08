//! Original GPU probe of a pack's `ftransform()`, in gbuffers and shadow programs.

use wgpu::util::DeviceExt;

/// Iris 1.11.7's `ftransform()` read back with a probe pack of ours (#8464) has the bits of
/// `gl_ProjectionMatrix * (gl_ModelViewMatrix * gl_Vertex)` at every vertex our probes read: the
/// model-view matrix first, then the projection, not their product matrix. These matrices tell
/// the two apart (one rounding against two): the model-view matrix takes 16777 from x, so the
/// vertex at 16777.25 comes out at exactly 0.25, and a third of that is one rounding; the product
/// matrix holds a third of 16777 rounded to a float, which leaves an error near 1e-4 after the
/// subtraction.
#[test]
fn ftransform_applies_the_model_view_matrix_before_the_projection() {
    let instance = crate::gfx::test_gpu::instance();
    let Ok(adapter) = pollster::block_on(instance.request_adapter(&Default::default())) else {
        return;
    };
    let (device, queue) = pollster::block_on(adapter.request_device(&Default::default())).unwrap();
    let third = 1.0f32 / 3.0;
    let model_view = glam::Mat4::from_translation(glam::Vec3::new(-16777.0, 2.0, 0.0));
    let projection = glam::Mat4::from_scale(glam::Vec3::new(third, 0.5, 1.0));
    let vertices = [[16777.25f32, 1.0, -3.0], [16778.0, -2.0, 5.0]];
    // `0.25 * third` and `1.0 * third` are exact products: one rounding of each.
    let expected = [[0.25 * third, 1.5, -3.0, 1.0], [third, 0.0, 5.0, 1.0]];
    let mut failures = Vec::new();
    for shadow in [false, true] {
        let got = ftransform(&device, &queue, model_view, projection, &vertices, shadow);
        for ((v, want), got) in vertices.iter().zip(&expected).zip(&got) {
            if got.map(f32::to_bits) != want.map(f32::to_bits) {
                failures.push(format!("{v:?}, shadow={shadow}: {got:?} != {want:?}"));
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// `ftransform()` of each of `vertices` in a gbuffers program (or a shadow program, with the
/// matrices as the shadow camera's), read back from a point per vertex as float bits.
fn ftransform(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    model_view: glam::Mat4,
    projection: glam::Mat4,
    vertices: &[[f32; 3]],
    shadow: bool,
) -> Vec<[f32; 4]> {
    let pp = |code: &str| horizon_shaders::Preprocessed {
        code: code.to_owned(),
        ..Default::default()
    };
    let vertex = pp(&format!(
        "varying vec4 probe;\nvoid main() {{\n    probe = ftransform();\n    \
         gl_Position = vec4((float(gl_VertexID) + 0.5) * {} - 1.0, 0.0, 0.0, 1.0);\n}}",
        2.0 / vertices.len() as f32,
    ));
    // An integer target keeps the float bits on any adapter.
    let fragment = pp(
        "varying vec4 probe;\nlayout(location = 0) out uvec4 bits;\n\
         void main() { bits = floatBitsToUint(probe); }",
    );
    let inputs = horizon_shaders::GbuffersInputs {
        declarations: "layout(location = 0) in vec3 hz_v_pos;".into(),
        body: "    hz_Vertex = vec4(hz_v_pos, 1.0);\n    hz_Color = vec4(1.0);\n".into(),
        ..Default::default()
    };
    let program = if shadow {
        horizon_shaders::translate_shadow(&vertex, &fragment, &inputs)
    } else {
        horizon_shaders::translate_gbuffers(
            &vertex,
            &fragment,
            horizon_shaders::Target::Buffers,
            &inputs,
        )
    }
    .unwrap();
    let prefix = if shadow { "shadow" } else { "gbuffer" };
    let mut block = program.interface.initial_values.clone();
    block.resize(program.interface.uniform_size.max(16) as usize, 0);
    for (name, matrix) in [("ModelView", model_view), ("Projection", projection)] {
        let name = format!("{prefix}{name}");
        let at = (program.interface.uniforms.iter())
            .find(|u| u.name == name)
            .unwrap_or_else(|| panic!("no {name}"))
            .offset as usize;
        block[at..at + 64].copy_from_slice(bytemuck::cast_slice(&matrix.to_cols_array()));
    }
    let uniform = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: None,
        contents: &block,
        usage: wgpu::BufferUsages::UNIFORM,
    });
    let data = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: None,
        contents: bytemuck::cast_slice(vertices),
        usage: wgpu::BufferUsages::VERTEX,
    });
    let group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: None,
        entries: &[wgpu::BindGroupLayoutEntry {
            binding: 0,
            visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        }],
    });
    let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: None,
        layout: &group_layout,
        entries: &[wgpu::BindGroupEntry {
            binding: 0,
            resource: uniform.as_entire_binding(),
        }],
    });
    let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: None,
        bind_group_layouts: &[Some(&group_layout)],
        ..Default::default()
    });
    let module = |module| {
        device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: None,
            source: wgpu::ShaderSource::Naga(std::borrow::Cow::Owned(module)),
        })
    };
    let vs = module(program.vertex);
    let fs = module(program.fragment);
    let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: None,
        layout: Some(&layout),
        vertex: wgpu::VertexState {
            module: &vs,
            entry_point: Some("main"),
            compilation_options: Default::default(),
            buffers: &[Some(wgpu::VertexBufferLayout {
                array_stride: 12,
                step_mode: wgpu::VertexStepMode::Vertex,
                attributes: &wgpu::vertex_attr_array![0 => Float32x3],
            })],
        },
        primitive: wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::PointList,
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
            width: vertices.len() as u32,
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
        size: vertices.len() as u64 * 16,
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&Default::default());
    {
        let view = output.create_view(&Default::default());
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
        pass.set_bind_group(0, &group, &[]);
        pass.set_vertex_buffer(0, data.slice(..));
        pass.draw(0..vertices.len() as u32, 0..1);
    }
    encoder.copy_texture_to_buffer(
        output.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &read,
            layout: wgpu::TexelCopyBufferLayout::default(),
        },
        output.size(),
    );
    queue.submit([encoder.finish()]);
    read.slice(..).map_async(wgpu::MapMode::Read, |_| {});
    device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
    let mapped = read.slice(..).get_mapped_range().unwrap();
    bytemuck::cast_slice::<u8, [f32; 4]>(&mapped).to_vec()
}
