//! `textureMatrix` in a gbuffers program (#9080), through the translated program on a host GPU.
//! Expected values: our probe packs K-texmat3 and K-texmat2 in Iris 1.11.7 read the identity in
//! the terrain vertex stage and zero in the terrain fragment stage.

use horizon_shaders::{GbuffersInputs, Macros, Pack, Target, preprocess, translate_gbuffers};

/// Draws one pixel with `vertex` and `fragment` as `gbuffers_terrain`; returns its RGBA8.
fn pixel(device: &wgpu::Device, queue: &wgpu::Queue, vertex: &str, fragment: &str) -> [u8; 4] {
    use std::io::Write;
    let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    for (path, source) in [
        ("shaders/gbuffers_terrain.vsh", vertex),
        ("shaders/gbuffers_terrain.fsh", fragment),
    ] {
        zip.start_file(path, zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(source.as_bytes()).unwrap();
    }
    let pack =
        Pack::from_zip_bytes("texture-matrix.zip", zip.finish().unwrap().into_inner()).unwrap();
    let macros = Macros::standard();
    let program = translate_gbuffers(
        &preprocess(&pack, "gbuffers_terrain.vsh", &macros),
        &preprocess(&pack, "gbuffers_terrain.fsh", &macros),
        Target::Buffers,
        &GbuffersInputs::default(),
    )
    .unwrap_or_else(|e| panic!("{e:?}"));
    assert!(!program.interface.draw, "no per-draw block in this probe");
    use wgpu::util::DeviceExt;
    // The frame's uniforms are all zero: `textureMatrix` must not come from them.
    let uniform = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: None,
        contents: &vec![0; program.interface.uniform_size as usize],
        usage: wgpu::BufferUsages::UNIFORM,
    });
    let module = |module| {
        device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: None,
            source: wgpu::ShaderSource::Naga(std::borrow::Cow::Owned(module)),
        })
    };
    let (vs, fs) = (module(program.vertex), module(program.fragment));
    let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: None,
        layout: None,
        vertex: wgpu::VertexState {
            module: &vs,
            entry_point: Some("main"),
            compilation_options: Default::default(),
            buffers: &[],
        },
        primitive: Default::default(),
        depth_stencil: None,
        multisample: Default::default(),
        fragment: Some(wgpu::FragmentState {
            module: &fs,
            entry_point: Some("main"),
            compilation_options: Default::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format: wgpu::TextureFormat::Rgba8Unorm,
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
            resource: uniform.as_entire_binding(),
        }],
    });
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
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = texture.create_view(&Default::default());
    let read = device.create_buffer(&wgpu::BufferDescriptor {
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
        texture.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &read,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(256),
                rows_per_image: None,
            },
        },
        texture.size(),
    );
    queue.submit([encoder.finish()]);
    read.slice(..)
        .map_async(wgpu::MapMode::Read, |result| result.unwrap());
    device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
    let mapped = read.slice(..).get_mapped_range().unwrap();
    [mapped[0], mapped[1], mapped[2], mapped[3]]
}

/// A full-screen triangle; `extra` declares and sets more.
fn vertex(extra: &str, set: &str) -> String {
    format!(
        "#version 460 core\n{extra}\nvoid main() {{\n\
         vec2 p = vec2((gl_VertexID << 1) & 2, gl_VertexID & 2);\n\
         gl_Position = vec4(p * 2.0 - 1.0, 0.0, 1.0);\n{set}\n}}\n"
    )
}

/// Four values `v` as v / 4 + 0.5 (K-texmat3's encoding): 0 is 128, 1 is 191.
const ENCODE: &str = "color = vec4(e) / 4.0 + 0.5;";

fn assert_close(actual: [u8; 4], expected: [u8; 4], what: &str) {
    assert!(
        actual.iter().zip(expected).all(|(a, e)| a.abs_diff(e) <= 1),
        "{what}: {actual:?}, expected {expected:?}"
    );
}

#[test]
#[ignore = "requires a host GPU; run with --ignored --nocapture"]
fn texture_matrix_is_the_identity_in_the_vertex_stage_and_zero_in_the_fragment_stage() {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let adapter = pollster::block_on(instance.request_adapter(&Default::default())).unwrap();
    eprintln!("Texture matrix GPU: {:?}", adapter.get_info().name);
    let (device, queue) = pollster::block_on(adapter.request_device(&Default::default())).unwrap();
    let fragment_in = "#version 460 core\nflat in vec4 e;\nlayout(location = 0) out vec4 color;\n\
                       void main() { ";
    // The vertex stage hands elements [0][0], [1][1], [3][0], [3][3] on (K-texmat3).
    let elements = pixel(
        &device,
        &queue,
        &vertex(
            "uniform mat4 textureMatrix;\nflat out vec4 e;",
            "e = vec4(textureMatrix[0][0], textureMatrix[1][1], textureMatrix[3][0], textureMatrix[3][3]);",
        ),
        &format!("{fragment_in}{ENCODE} }}\n"),
    );
    assert_close(elements, [191, 191, 128, 191], "vertex-stage elements");
    // A UV through it is the UV (K-texmat's `mat2(m) * uv + m[3].xy`).
    let uv = pixel(
        &device,
        &queue,
        &vertex(
            "uniform mat4 textureMatrix;\nflat out vec4 e;",
            "vec2 uv = vec2(0.25, 0.75);\n\
             e = vec4(mat2(textureMatrix) * uv + textureMatrix[3].xy, (textureMatrix * vec4(uv, 0.0, 1.0)).xy);",
        ),
        "#version 460 core\nflat in vec4 e;\nlayout(location = 0) out vec4 color;\n\
         void main() { color = e; }\n",
    );
    assert_close(uv, [64, 191, 64, 191], "transformed UV");
    // The fragment stage's own declaration reads zero, with or without the vertex stage's (K-texmat2).
    for vertex_declares in [false, true] {
        let v = if vertex_declares {
            vertex(
                "uniform mat4 textureMatrix;\nflat out vec4 e;",
                "e = textureMatrix[0];",
            )
        } else {
            vertex("flat out vec4 e;", "e = vec4(0.0);")
        };
        let f = "#version 460 core\nuniform mat4 textureMatrix;\nflat in vec4 e;\n\
                 layout(location = 0) out vec4 color;\nvoid main() {\n\
                 vec4 m = vec4(textureMatrix[0][0], textureMatrix[1][1], textureMatrix[3][0], textureMatrix[3][3]);\n\
                 color = m / 4.0 + 0.5 + e * 0.0;\n}\n";
        assert_close(
            pixel(&device, &queue, &v, f),
            [128, 128, 128, 128],
            &format!("fragment stage (vertex declares: {vertex_declares})"),
        );
    }
}
