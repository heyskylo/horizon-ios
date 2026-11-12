use super::super::{
    LINEAR_FRAME,
    native_post::{Decoder, INTERMEDIATE, line_helpers},
};
use super::{frame_constants, stored_clear};
use horizon_shader_api::Api;
use wgpu::util::DeviceExt;

/// One full-screen draw into the frame: `vec4(rgb, alpha)` from Horizon's own shaders, which
/// give vanilla's gamma (`frame_stored`), or a 0.2 pack's, which gives linear light
/// (`horizon_line_out`).
#[derive(Clone, Copy)]
enum Writer {
    Horizon,
    Pack,
}

const SOURCE: &str = "
// The colour comes in a buffer: naga 30 rejects this fragment with the colour made of
// overrides (`ExpressionAlreadyInScope` once they are processed).
@group(0) @binding(0) var<uniform> test_color: vec4<f32>;
override TEST_PACK: bool = false;
@vertex fn test_vs(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
    return vec4<f32>(vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u)) * 2.0 - 1.0, 0.0, 1.0);
}
@fragment fn test_fs() -> @location(0) vec4<f32> {
    let c = test_color;
    if TEST_PACK {
        return vec4<f32>(horizon_line_out(c.rgb), c.a);
    }
    return frame_stored(c);
}
";

fn gpu() -> Option<(wgpu::Device, wgpu::Queue)> {
    let instance = crate::gfx::test_gpu::instance();
    let adapter = pollster::block_on(instance.request_adapter(&Default::default())).ok()?;
    pollster::block_on(adapter.request_device(&Default::default())).ok()
}

/// What a 0.2 stage reads of a 1x1 frame (`scene_color`): the world cleared to `clear`, in
/// vanilla's gamma, then `draws` alpha-blended over it, then the production decoding copy.
fn stage_input(
    (device, queue): &(wgpu::Device, wgpu::Queue),
    linear: bool,
    clear: [f64; 3],
    draws: &[(Writer, [f32; 4])],
) -> [f32; 4] {
    let texture = |format, usage| {
        device.create_texture(&wgpu::TextureDescriptor {
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
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | usage,
            view_formats: &[],
        })
    };
    let frame = texture(LINEAR_FRAME, wgpu::TextureUsages::TEXTURE_BINDING);
    let input = texture(INTERMEDIATE, wgpu::TextureUsages::COPY_SRC);
    let frame_view = frame.create_view(&Default::default());
    let source =
        include_str!("../hdr_light.wgsl").to_owned() + line_helpers(Api::V0_2, linear) + SOURCE;
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: None,
        source: wgpu::ShaderSource::Wgsl(source.into()),
    });
    let mut encoder = device.create_command_encoder(&Default::default());
    let [r, g, b] = clear;
    let clear = stored_clear(wgpu::Color { r, g, b, a: 1.0 }, linear);
    let mut load = wgpu::LoadOp::Clear(clear);
    for &(writer, color) in draws {
        let mut constants = frame_constants(LINEAR_FRAME, linear).to_vec();
        constants.push((
            "TEST_PACK",
            f64::from(u8::from(matches!(writer, Writer::Pack))),
        ));
        let options = wgpu::PipelineCompilationOptions {
            constants: &constants,
            ..Default::default()
        };
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: None,
            layout: None,
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: Some("test_vs"),
                compilation_options: options.clone(),
                buffers: &[],
            },
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some("test_fs"),
                compilation_options: options,
                targets: &[Some(wgpu::ColorTargetState {
                    format: LINEAR_FRAME,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            multiview_mask: None,
            cache: None,
        });
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &frame_view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load,
                    store: wgpu::StoreOp::Store,
                },
            })],
            ..Default::default()
        });
        let color = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: None,
            contents: bytemuck::cast_slice(&color),
            usage: wgpu::BufferUsages::UNIFORM,
        });
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &pipeline.get_bind_group_layout(0),
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: color.as_entire_binding(),
            }],
        });
        pass.set_pipeline(&pipeline);
        pass.set_bind_group(0, &group, &[]);
        pass.draw(0..3, 0..1);
        load = wgpu::LoadOp::Load;
    }
    if let wgpu::LoadOp::Clear(_) = load {
        encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &frame_view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load,
                    store: wgpu::StoreOp::Store,
                },
            })],
            ..Default::default()
        });
    }
    Decoder::new(device, linear).run(
        device,
        &mut encoder,
        &frame_view,
        &input.create_view(&Default::default()),
    );
    texel((device, queue), encoder, &input)
}

/// The first texel of the half-float `texture` once `encoder` ran.
fn texel(
    (device, queue): (&wgpu::Device, &wgpu::Queue),
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
    std::array::from_fn(|i| half::f16::from_le_bytes([bytes[2 * i], bytes[2 * i + 1]]).to_f32())
}

fn assert_near(got: [f32; 4], want: [f32; 3], what: &str) {
    for i in 0..3 {
        // Half floats, and a gamma frame's half float decoded, hold about three digits.
        assert!(
            (got[i] - want[i]).abs() < 0.003 * want[i].max(1.0),
            "{what}: {got:?}, expected {want:?}"
        );
    }
    assert_eq!(got[3], 1.0, "{what}: {got:?}");
}

// The background is gamma (0.8, 0.2, 0.5), linear (0.6121, 0.0290, 0.2176). Horizon's glass
// gives gamma (0.1, 0.9, 0.4), the pack's linear (0.0, 0.5, 1.0); both cover a quarter. Under
// "gamma" the frame mixes gamma, a stage reads `decode(0.25 * src + 0.75 * bg)`: Horizon's
// (0.3556, 0.1156, 0.1944), the pack's, encoded first, (0.3250, 0.0887, 0.3556). Under
// "linear" it mixes light, `0.25 * decode(src) + 0.75 * decode(bg)`: Horizon's
// (0.4606, 0.2200, 0.1965), the pack's (0.4590, 0.1467, 0.4132).
#[test]
fn translucent_draws_blend_in_the_light_the_frame_holds() {
    let Some(gpu) = gpu() else {
        eprintln!("linear blend GPU unavailable");
        return;
    };
    let background = [0.8, 0.2, 0.5];
    for (linear, horizon, pack) in [
        (false, [0.3556, 0.1156, 0.1944], [0.3250, 0.0887, 0.3556]),
        (true, [0.4606, 0.2200, 0.1965], [0.4590, 0.1467, 0.4132]),
    ] {
        let glass = [(Writer::Horizon, [0.1, 0.9, 0.4, 0.25])];
        let got = stage_input(&gpu, linear, background, &glass);
        assert_near(got, horizon, &format!("Horizon's glass, linear {linear}"));
        let glass = [(Writer::Pack, [0.0, 0.5, 1.0, 0.25])];
        let got = stage_input(&gpu, linear, background, &glass);
        assert_near(got, pack, &format!("the pack's glass, linear {linear}"));
    }
}

// What a stage reads of solid surfaces is the same light under either blend: Horizon's gamma
// (0.3, 0.7, 1.5) decodes to (0.0707, 0.4563, 2.4401), the pack's light (0.25, 0.5, 2.0) and
// the clear colour, gamma (0.8, 0.2, 0.5), to (0.6121, 0.0290, 0.2176).
#[test]
fn stages_read_the_same_light_under_either_blend() {
    let Some(gpu) = gpu() else {
        eprintln!("linear blend GPU unavailable");
        return;
    };
    let clear = [0.8, 0.2, 0.5];
    let cases = [
        (
            vec![(Writer::Horizon, [0.3, 0.7, 1.5, 1.0])],
            [0.0707, 0.4563, 2.4401],
        ),
        (
            vec![(Writer::Pack, [0.25, 0.5, 2.0, 1.0])],
            [0.25, 0.5, 2.0],
        ),
        (vec![], [0.6121, 0.0290, 0.2176]),
    ];
    for (draws, want) in cases {
        let [gamma, linear] = [false, true].map(|linear| stage_input(&gpu, linear, clear, &draws));
        assert_near(gamma, want, "gamma");
        assert_near(linear, want, "linear");
        for i in 0..3 {
            assert!((gamma[i] - linear[i]).abs() < 0.003 * want[i].max(1.0));
        }
    }
}

// A section fading in mixes the pack's material result toward the fog colour in vanilla's
// gamma, as the fog does. Fog gamma 0.8, the pack's light 0.2 (gamma 0.2^(1/2.2) = 0.4812), half
// visible: under "linear" the frame stores (0.5 * 0.8 + 0.5 * 0.4812)^2.2 = 0.3757, not the
// 0.5 a mix in light gives; under "gamma" the result is already gamma, 0.5 * 0.8 + 0.5 * 0.2.
#[test]
fn a_fading_section_mixes_toward_the_fog_in_gamma() {
    let Some((device, queue)) = gpu() else {
        eprintln!("linear blend GPU unavailable");
        return;
    };
    let globals = super::super::Globals {
        fog_color: [0.8; 4],
        // The pack's material result, as `horizon_material_color` gives it.
        sky_color: [0.2, 0.2, 0.2, 1.0],
        ..bytemuck::Zeroable::zeroed()
    };
    let globals = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: None,
        contents: bytemuck::bytes_of(&globals),
        usage: wgpu::BufferUsages::UNIFORM,
    });
    for (linear, want) in [(false, 0.5), (true, 0.3757)] {
        let source = super::super::native_material::coverage_test_shader((Api::V0_2, linear))
            + "
@fragment fn test_faded() -> @location(0) vec4<f32> {
    var o: HorizonMaterialOut;
    // Half visible (`horizon_material_vertex`), past 32 blocks.
    o.hdr_code = 32768u << 16u;
    o.spherical = 40.0;
    return horizon_material_faded(o, horizon_terrain_globals.sky_color);
}
";
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: None,
            source: wgpu::ShaderSource::Wgsl(source.into()),
        });
        let constants = frame_constants(LINEAR_FRAME, linear)
            .map(|(name, value)| (format!("horizon_terrain_{name}"), value));
        let constants: Vec<(&str, f64)> = constants.iter().map(|(n, v)| (n.as_str(), *v)).collect();
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: None,
            layout: None,
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: Some("test_vertex"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some("test_faded"),
                compilation_options: wgpu::PipelineCompilationOptions {
                    constants: &constants,
                    ..Default::default()
                },
                targets: &[Some(LINEAR_FRAME.into())],
            }),
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            multiview_mask: None,
            cache: None,
        });
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &pipeline.get_bind_group_layout(0),
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: globals.as_entire_binding(),
            }],
        });
        let frame = device.create_texture(&wgpu::TextureDescriptor {
            label: None,
            size: wgpu::Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: LINEAR_FRAME,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let mut encoder = device.create_command_encoder(&Default::default());
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &frame.create_view(&Default::default()),
                    depth_slice: None,
                    resolve_target: None,
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
        let got = texel((&device, &queue), encoder, &frame);
        assert!(
            (got[0] - want).abs() < 0.002,
            "linear {linear}: {got:?}, expected {want}"
        );
        assert_eq!(got[3], 1.0, "alpha kept");
    }
}

// The frame O1 resolves (`output::resolve_frame`) holds linear light only for a 0.2 pack that
// asks for it, on the float frame. On a device without the float frame the pack draws anyway,
// in gamma. A blend changed is another frame, so the game makes the renderer again
// (`Renderer::frame_follows`).
#[test]
fn only_a_float_frame_holds_linear_light() {
    use super::super::output::{FrameRequest, resolve_frame, tests::window};
    let settings = crate::graphics::Graphics::default();
    for float_frame in [false, true] {
        for (linear_pack, linear_blend) in [(false, false), (true, false), (true, true)] {
            let (mut caps, reading) = window(None, None);
            caps.float_frame = float_frame;
            let request = FrameRequest {
                linear_pack,
                linear_blend,
                hdr_float: false,
            };
            let frame = resolve_frame(
                &settings,
                request,
                caps,
                reading,
                wgpu::TextureFormat::Bgra8UnormSrgb,
            );
            assert_eq!(frame.format == LINEAR_FRAME, float_frame && linear_pack);
            assert_eq!(
                frame.linear_light,
                float_frame && linear_blend,
                "float {float_frame}, linear {linear_blend}"
            );
        }
    }
}
