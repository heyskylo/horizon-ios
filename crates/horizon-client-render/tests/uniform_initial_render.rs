//! Original synthetic rendering controls for ordinary uniform initial values.
use horizon_client_render::gfx::pack_uniforms::Value;
use horizon_shaders::{Origin, Preprocessed, Target, translate_composite};
use wgpu::util::DeviceExt;

fn source(code: &str) -> Preprocessed {
    Preprocessed {
        code: code.into(),
        files: vec!["original-render.glsl".into()],
        lines: code
            .lines()
            .enumerate()
            .map(|(i, _)| Origin {
                file: 0,
                line: i as u32 + 1,
            })
            .collect(),
        version: Some((430, None)),
        ..Default::default()
    }
}

#[test]
#[ignore = "requires a host GPU; run with --ignored --nocapture"]
fn authored_defaults_render_and_runtime_values_override_including_zero() {
    let program = translate_composite(
        &source("void main() { gl_Position = ftransform(); }"),
        &source("uniform float gain = 0.375; uniform mat3 basis = mat3(2.0); uniform mat4 transform = mat4(1.0); uniform vec3 tint = vec3(0.125,0.25,0.5); uniform float points[2] = float[](0.0625,0.125); uniform int mode = 3; uniform bool enabled = true; out vec4 color; void main() { color = vec4(gain * basis[0][0] * transform[2][2], tint.g + points[1], float(mode), enabled ? 1.0 : 0.0); }"),
        Target::Buffers,
    ).unwrap();
    let instance =
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle().with_env());
    let adapter = pollster::block_on(instance.request_adapter(&Default::default()))
        .expect("GPU adapter required; missing adapter is not a pass");
    eprintln!("Uniform defaults GPU: {:?}", adapter.get_info());
    assert_ne!(adapter.get_info().device_type, wgpu::DeviceType::Cpu);
    let (device, queue) = pollster::block_on(adapter.request_device(&Default::default())).unwrap();
    let vertex = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: None,
        source: wgpu::ShaderSource::Naga(std::borrow::Cow::Owned(program.vertex)),
    });
    let fragment = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: None,
        source: wgpu::ShaderSource::Naga(std::borrow::Cow::Owned(program.fragment)),
    });
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
            module: &fragment,
            entry_point: Some("main"),
            compilation_options: Default::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format: wgpu::TextureFormat::Rgba32Float,
                blend: None,
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        multiview_mask: None,
        cache: None,
    });
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
        format: wgpu::TextureFormat::Rgba32Float,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: 256,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    for (name, zero_buffer, overrides, expected) in [
        ("authored defaults", false, vec![], [0.75, 0.375, 3.0, 1.0]),
        (
            "runtime precedence",
            false,
            vec![
                ("gain", Value::F([0.125; 16])),
                ("transform", Value::F([0.5; 16])),
                ("mode", Value::I([7, 0, 0])),
                ("enabled", Value::I([0; 3])),
            ],
            [0.125, 0.375, 7.0, 0.0],
        ),
        (
            "runtime explicit zero",
            false,
            vec![("gain", Value::F([0.; 16])), ("mode", Value::I([0; 3]))],
            [0.0, 0.375, 0.0, 1.0],
        ),
        ("zero upload negative control", true, vec![], [0.; 4]),
    ] {
        let mut bytes = if zero_buffer {
            vec![0; program.interface.uniform_size as usize]
        } else {
            program.interface.initial_values.clone()
        };
        for (name, value) in overrides {
            let uniform = program
                .interface
                .uniforms
                .iter()
                .find(|u| u.name == name)
                .unwrap();
            value.write(&uniform.ty, &mut bytes, uniform.offset as usize);
        }
        let buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some(name),
            contents: &bytes,
            usage: wgpu::BufferUsages::UNIFORM,
        });
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &pipeline.get_bind_group_layout(0),
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: buffer.as_entire_binding(),
            }],
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
            pass.set_bind_group(0, &group, &[]);
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
            wgpu::Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: 1,
            },
        );
        queue.submit([encoder.finish()]);
        readback
            .slice(..)
            .map_async(wgpu::MapMode::Read, |r| r.unwrap());
        device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
        {
            let mapped = readback.slice(..).get_mapped_range().unwrap();
            let actual: &[f32] = bytemuck::cast_slice(&mapped[..16]);
            eprintln!("{name}: {actual:?}");
            assert_eq!(actual, expected, "{name}");
        }
        readback.unmap();
    }
}
