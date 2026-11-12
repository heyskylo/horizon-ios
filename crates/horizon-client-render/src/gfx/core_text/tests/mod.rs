use super::*;

fn device() -> Option<(wgpu::Device, wgpu::Queue)> {
    let instance = crate::gfx::test_gpu::instance();
    let adapter = pollster::block_on(instance.request_adapter(&Default::default())).ok()?;
    pollster::block_on(adapter.request_device(&Default::default())).ok()
}

#[test]
fn a_pack_text_shader_reads_the_light_as_vanilla_does() {
    let Some((device, queue)) = device() else {
        eprintln!("no GPU: skipping");
        return;
    };
    let vsh = "#version 330\n#extension GL_ARB_separate_shader_objects : require\n\
        layout(location = 0) in vec3 Position;\nlayout(location = 1) in vec4 Color;\n\
        layout(location = 2) in vec2 UV0;\nlayout(location = 3) in ivec2 UV2;\n\
        layout(location = 0) flat out ivec2 light;\n\
        void main() { gl_Position = vec4(Position, 1.0); light = UV2; }\n";
    let fsh = "#version 330\n#extension GL_ARB_separate_shader_objects : require\n\
        layout(location = 0) flat in ivec2 light;\nlayout(location = 0) out vec4 fragColor;\n\
        void main() { fragColor = vec4(vec2(light) / 255.0, 0.0, 1.0); }\n";
    let program =
        crate::core_shader::translate_core(vsh, fsh, &[], &|_| None, &WORLD_GLUE).unwrap();
    const SIZE: u32 = 64;
    let texture = |format, usage| {
        device.create_texture(&wgpu::TextureDescriptor {
            label: None,
            size: wgpu::Extent3d {
                width: SIZE,
                height: SIZE,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage,
            view_formats: &[],
        })
    };
    let binding = wgpu::TextureUsages::TEXTURE_BINDING;
    let atlas = texture(wgpu::TextureFormat::Rgba8Unorm, binding).create_view(&Default::default());
    let sampler = device.create_sampler(&Default::default());
    let text = CoreText::new(
        &device,
        wgpu::TextureFormat::Rgba8Unorm,
        Some(&program),
        None,
        &atlas,
        (&atlas, &sampler),
    )
    .expect("the pack's pipeline");
    let target = wgpu::TextureUsages::RENDER_ATTACHMENT;
    let color = texture(
        wgpu::TextureFormat::Rgba8Unorm,
        target | wgpu::TextureUsages::COPY_SRC,
    );
    let depth = texture(wgpu::TextureFormat::Depth32Float, target);
    let light = 0x30 | (0xC0 << 8);
    let corner = |x: f32, y: f32| crate::mesher::Vertex {
        pos: [x, y, 0.5],
        uv: [0.0; 2],
        layer: 0,
        color: u32::MAX,
        light,
        normal: [0; 4],
        glint_uv: [0.0; 2],
    };
    let quad = [
        corner(-1.0, -1.0),
        corner(1.0, -1.0),
        corner(1.0, 1.0),
        corner(-1.0, -1.0),
        corner(1.0, 1.0),
        corner(-1.0, 1.0),
    ];
    let vertices = wgpu::util::DeviceExt::create_buffer_init(
        &device,
        &wgpu::util::BufferInitDescriptor {
            label: None,
            contents: bytemuck::cast_slice(&quad),
            usage: wgpu::BufferUsages::VERTEX,
        },
    );
    let read = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: u64::from(SIZE * SIZE * 4),
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&Default::default());
    {
        let color_view = color.create_view(&Default::default());
        let depth_view = depth.create_view(&Default::default());
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: None,
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &color_view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: &depth_view,
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Clear(0.0),
                    store: wgpu::StoreOp::Store,
                }),
                stencil_ops: None,
            }),
            ..Default::default()
        });
        pass.set_pipeline(text.polygon_offset.as_ref().unwrap());
        pass.set_bind_group(0, &text.group, &[]);
        pass.set_vertex_buffer(0, vertices.slice(..));
        pass.draw(0..6, 0..1);
    }
    encoder.copy_texture_to_buffer(
        color.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &read,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(SIZE * 4),
                rows_per_image: Some(SIZE),
            },
        },
        color.size(),
    );
    queue.submit([encoder.finish()]);
    read.slice(..)
        .map_async(wgpu::MapMode::Read, |r| r.unwrap());
    device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
    let pixels = read.slice(..).get_mapped_range().unwrap().to_vec();
    let middle = ((SIZE / 2 * SIZE + SIZE / 2) * 4) as usize;
    assert_eq!(
        &pixels[middle..middle + 4],
        &[0xC0, 0x30, 0, 0xFF],
        "UV2 is (block, sky)"
    );
}

#[test]
#[ignore = "requires prepared resources in HORIZON_TEST_RESOURCES or .local/horizon-client/26.3, and a GPU"]
fn vanillas_text_shader_makes_both_world_text_pipelines() {
    let root = std::env::var_os("HORIZON_TEST_RESOURCES")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../../.local/horizon-client")
                .join(horizon_core::VANILLA_VERSION)
        });
    let read = |path: &str| std::fs::read_to_string(root.join(path)).ok();
    let vertex = read("assets/minecraft/shaders/core/text.vsh").expect("vanilla's text.vsh");
    let fragment = read("assets/minecraft/shaders/core/text.fsh").expect("vanilla's text.fsh");
    let include = |id: &str| {
        let (namespace, path) = id.split_once(':')?;
        read(&format!("assets/{namespace}/{path}"))
    };
    let translate = |defines: &[&str], glue: &Glue| {
        crate::core_shader::translate_core(&vertex, &fragment, defines, &include, glue)
            .expect("vanilla's text shader translates")
    };
    let world = translate(&[], &WORLD_GLUE);
    let see_through = translate(&["IS_SEE_THROUGH"], &SEE_THROUGH_GLUE);
    let (device, _) = device().expect("a GPU");
    let texture = || {
        device
            .create_texture(&wgpu::TextureDescriptor {
                label: None,
                size: wgpu::Extent3d {
                    width: 4,
                    height: 4,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8Unorm,
                usage: wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            })
            .create_view(&Default::default())
    };
    let (atlas, lightmap) = (texture(), texture());
    let sampler = device.create_sampler(&Default::default());
    let text = CoreText::new(
        &device,
        wgpu::TextureFormat::Rgba8Unorm,
        Some(&world),
        Some(&see_through),
        &atlas,
        (&lightmap, &sampler),
    )
    .expect("the pack's world text pipelines");
    assert!(text.polygon_offset.is_some() && text.see_through.is_some());
}
