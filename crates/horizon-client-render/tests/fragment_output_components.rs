//! Synthetic rendering checks (two output shapes mirror RenderPearl v2.7.2 @86de79c, MIT; see the
//! PR): a fragment output's `component` layout qualifier (GLSL 4.60, 4.4.2) picks the channels of
//! its location that it writes.
use horizon_shaders::{Origin, Preprocessed, Target, translate_composite};

fn source(code: &str) -> Preprocessed {
    Preprocessed {
        code: code.into(),
        files: vec!["components.glsl".into()],
        lines: code
            .lines()
            .enumerate()
            .map(|(i, _)| Origin {
                file: 0,
                line: i as u32 + 1,
            })
            .collect(),
        version: Some((440, None)),
        ..Default::default()
    }
}

/// Draws `fragment` over a 1×1 target of `format` and reads back its 16 bytes.
fn draw(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    fragment: &str,
    format: wgpu::TextureFormat,
) -> [u8; 16] {
    let program = translate_composite(
        &source("void main() { gl_Position = ftransform(); }"),
        &source(fragment),
        Target::Buffers,
    )
    .unwrap_or_else(|e| panic!("{fragment}: {e:?}"));
    let module = |module| {
        device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: None,
            source: wgpu::ShaderSource::Naga(std::borrow::Cow::Owned(module)),
        })
    };
    let (vertex, fragment_module) = (module(program.vertex), module(program.fragment));
    let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: None,
        layout: None,
        vertex: wgpu::VertexState {
            module: &vertex,
            entry_point: Some("main"),
            compilation_options: Default::default(),
            buffers: &[],
        },
        primitive: wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::TriangleStrip,
            ..Default::default()
        },
        depth_stencil: None,
        multisample: Default::default(),
        fragment: Some(wgpu::FragmentState {
            module: &fragment_module,
            entry_point: Some("main"),
            compilation_options: Default::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format,
                blend: None,
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        multiview_mask: None,
        cache: None,
    });
    let extent = wgpu::Extent3d {
        width: 1,
        height: 1,
        depth_or_array_layers: 1,
    };
    let target = device.create_texture(&wgpu::TextureDescriptor {
        label: None,
        size: extent,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: 256,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let view = target.create_view(&Default::default());
    let mut encoder = device.create_command_encoder(&Default::default());
    {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: None,
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &view,
                resolve_target: None,
                depth_slice: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                    store: wgpu::StoreOp::Store,
                },
            })],
            ..Default::default()
        });
        pass.set_pipeline(&pipeline);
        // The composite program's uniform block, if it kept one, reads zero.
        if program.interface.uniform_size > 0 {
            let buffer = device.create_buffer(&wgpu::BufferDescriptor {
                label: None,
                size: u64::from(program.interface.uniform_size).max(16),
                usage: wgpu::BufferUsages::UNIFORM,
                mapped_at_creation: false,
            });
            let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: None,
                layout: &pipeline.get_bind_group_layout(0),
                entries: &[wgpu::BindGroupEntry {
                    binding: 0,
                    resource: buffer.as_entire_binding(),
                }],
            });
            pass.set_bind_group(0, &group, &[]);
        }
        pass.draw(0..4, 0..1);
    }
    encoder.copy_texture_to_buffer(
        target.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &readback,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(256),
                rows_per_image: Some(1),
            },
        },
        extent,
    );
    queue.submit([encoder.finish()]);
    readback
        .slice(..)
        .map_async(wgpu::MapMode::Read, |r| r.unwrap());
    device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
    let bytes = readback.slice(..).get_mapped_range().unwrap()[..16]
        .try_into()
        .unwrap();
    readback.unmap();
    bytes
}

#[test]
#[ignore = "requires a host GPU; run with --ignored --nocapture"]
fn component_qualified_outputs_write_their_own_channels() {
    let instance =
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle().with_env());
    let adapter = pollster::block_on(instance.request_adapter(&Default::default()))
        .expect("GPU adapter required; missing adapter is not a pass");
    eprintln!("Output components GPU: {:?}", adapter.get_info());
    assert_ne!(adapter.get_info().device_type, wgpu::DeviceType::Cpu);
    let (device, queue) = pollster::block_on(adapter.request_device(&Default::default())).unwrap();
    // The channels no output writes keep the fill of a narrow output: 0, and 1 in alpha.
    for (name, fragment, expected) in [
        (
            "uvec3 at component 1",
            "layout(location = 0, component = 1) out uvec3 words;\n\
             void main() { words = uvec3(7u, 8u, 9u); }",
            [0u32, 7, 8, 9],
        ),
        (
            "uint at component 2",
            "layout(location = 0, component = 2) out uint word;\n\
             void main() { word = 5u; }",
            [0, 0, 5, 1],
        ),
        (
            "two outputs share location 0",
            "layout(location = 0, component = 0) out uint low;\n\
             layout(location = 0, component = 1) out uvec3 high;\n\
             void main() { low = 3u; high = uvec3(4u, 5u, 6u); }",
            [3, 4, 5, 6],
        ),
        (
            "the component 1 output declared first",
            "layout(location = 0, component = 1) out uvec2 middle;\n\
             layout(location = 0) out uint low;\n\
             void main() { low = 11u; middle = uvec2(12u, 13u); }",
            [11, 12, 13, 1],
        ),
    ] {
        let bytes = draw(&device, &queue, fragment, wgpu::TextureFormat::Rgba32Uint);
        let actual: [u32; 4] = bytemuck::cast(bytes);
        eprintln!("{name}: {actual:?}");
        assert_eq!(actual, expected, "{name}");
    }
    let bytes = draw(
        &device,
        &queue,
        "layout(location = 0, component = 0) out float red;\n\
         layout(location = 0, component = 1) out vec3 rest;\n\
         void main() { red = 0.0; rest = vec3(1.0, 0.0, 1.0); }",
        wgpu::TextureFormat::Rgba32Float,
    );
    let actual: [f32; 4] = bytemuck::cast(bytes);
    eprintln!("float outputs share location 0: {actual:?}");
    assert_eq!(actual, [0.0, 1.0, 0.0, 1.0]);
}
