//! Original synthetic material transport oracle; no external shader algorithms or assets.
use super::*;
use wgpu::util::DeviceExt;

#[test]
#[ignore = "requires a host GPU; missing adapter is a failure"]
fn terrain_material_samplers_preserve_surface_and_encoded_bytes() {
    let instance = crate::gfx::test_gpu::instance();
    let adapter = pollster::block_on(instance.request_adapter(&Default::default())).unwrap();
    eprintln!("Material transport GPU: {:?}", adapter.get_info());
    assert_ne!(adapter.get_info().device_type, wgpu::DeviceType::Cpu);
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        required_limits: adapter.limits(),
        ..Default::default()
    }))
    .unwrap();
    let root = std::env::temp_dir().join(format!("horizon-material-pack-{}", std::process::id()));
    std::fs::create_dir_all(root.join("shaders")).unwrap();
    std::fs::write(
        root.join("shaders/gbuffers_terrain.vsh"),
        "#version 120\nvoid main(){gl_Position=gl_Vertex;}",
    )
    .unwrap();
    std::fs::write(
        root.join("shaders/gbuffers_terrain.fsh"),
        r#"#version 120
uniform sampler2D gtexture;
uniform sampler2D normals;
uniform sampler2D specular;
void main() {
    int x = int(floor(gl_FragCoord.x));
    int layer = x - (x / 5) * 5;
    // Source coordinate (1.25,1.25): inside a texel, away from both its centre and boundary.
    vec2 uv = (vec2(float(layer - (layer / 3) * 3), float(layer / 3)) + 0.3125) / vec2(3.0, 2.0);
    if (x < 5) gl_FragData[0] = texture2DLod(specular, uv, 12.0);
    else if (x < 10) gl_FragData[0] = texture2DLod(normals, uv, 12.0);
    else gl_FragData[0] = texture2DLod(gtexture, uv, 0.0);
}
"#,
    )
    .unwrap();
    let (split, layers) = (2u32, 5u32);
    let mut encoder = device.create_command_encoder(&Default::default());
    let atlases: Vec<_> = (0..3)
        .map(|kind| {
            let side = if kind == 0 { 4 } else { 2 };
            let levels = if kind == 0 { 3 } else { 1 };
            let arrays: Vec<_> = (0..layers.div_ceil(split))
                .map(|array| {
                    let count = (layers - array * split).min(split);
                    let pixels: Vec<u8> = (0..count)
                        .flat_map(|l| {
                            let layer = (array * split + l) as u8;
                            (0..levels).flat_map(move |level| {
                                let edge = side >> level;
                                (0..edge * edge).flat_map(move |pixel| {
                                    if level != 0 {
                                        // Sampling unused source mip levels must fail the oracle.
                                        [211, 212, 213, 214]
                                    } else {
                                        match kind {
                                            0 if layer == 3 => crate::sprites::NO_SPECULAR,
                                            0 => [
                                                17 + layer,
                                                if pixel % 2 == 0 { 0 } else { 240 },
                                                65,
                                                if pixel % 2 == 0 { 0 } else { 255 },
                                            ],
                                            1 if layer == 2 => crate::sprites::FLAT_NORMAL,
                                            1 => [128 + layer, 126, 255, 19 + layer],
                                            _ => [40 + layer, 50, 60, 255],
                                        }
                                    }
                                })
                            })
                        })
                        .collect();
                    device.create_texture_with_data(
                        &queue,
                        &wgpu::TextureDescriptor {
                            label: Some("Original fixture companion array"),
                            size: wgpu::Extent3d {
                                width: side,
                                height: side,
                                depth_or_array_layers: count,
                            },
                            mip_level_count: levels,
                            sample_count: 1,
                            dimension: wgpu::TextureDimension::D2,
                            format: wgpu::TextureFormat::Rgba8Unorm,
                            usage: wgpu::TextureUsages::TEXTURE_BINDING
                                | wgpu::TextureUsages::COPY_SRC,
                            view_formats: &[],
                        },
                        wgpu::util::TextureDataOrder::LayerMajor,
                        &pixels,
                    )
                })
                .collect();
            super::super::pack_atlas::PackAtlas::material(
                &device,
                &mut encoder,
                &arrays,
                split,
                layers,
            )
            .unwrap()
        })
        .collect();
    queue.submit([encoder.finish()]);
    let pack = horizon_shaders::Pack::open(&root).unwrap();
    let macros = horizon_shaders::Macros::standard();
    // Encoded zero alpha is data here and must remain observable in readback.
    let properties = horizon_shaders::Properties::parse("alphaTest.gbuffers_terrain=off", &macros);
    let inputs = horizon_shaders::GameInputs {
        terrain: Some(horizon_shaders::GbuffersInputs {
            body: "vec2 p = vec2(float((gl_VertexIndex << 1) & 2), float(gl_VertexIndex & 2)); hz_Vertex=vec4(p*2.0-1.0,0.0,1.0);".into(),
            ..Default::default()
        }),
        ..Default::default()
    };
    let post = horizon_shaders::load_post(&pack, "", &properties, &macros, &inputs);
    assert!(post.terrain.is_some(), "{:?}", post.notes);
    let world = World {
        atlas: Some((atlases[2].view.clone(), atlases[2].size())),
        material_maps: Some([atlases[1].view.clone(), atlases[0].view.clone()]),
        lightmap: atlases[2].view.clone(),
        terrain: Some(Geometry {
            buffers: &[],
            table: None,
            depth_write: true,
            atlas: None,
            material_maps: true,
            ids: None,
            cull: false,
        }),
        block_ids: &[],
        celestial: None,
        gui_size: [1, 1],
        clouds: None,
        weather: false,
        hand: false,
    };
    let mut post = PackPost::new(
        &device,
        &queue,
        &adapter,
        post,
        wgpu::TextureFormat::Rgba8Unorm,
        world,
    )
    .expect("the pack runs");
    let mut pass = post
        .terrain
        .take()
        .unwrap_or_else(|| panic!("terrain pipeline must compile: {:?}", post.notes));
    // Request the existing copy usage for the fixture's colour readback.
    post.copied |= 1;
    // First draw preserves production Geometry -> Pass scope, so a routing regression fails.
    for case in 0..3 {
        if case == 1 {
            pass.material_maps = false;
        }
        if case == 2 {
            pass.material_maps = true;
            post.material_maps = None;
        }
        let maps_expected = case == 0;
        pass.groups.clear();
        post.resize(&device, [15, 1]);
        let read = device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: 256,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = device.create_command_encoder(&Default::default());
        {
            let _clear = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Initialize fixture depth"),
                color_attachments: &[],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &post.targets.as_ref().unwrap().depth,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                ..Default::default()
            });
        }
        let group = post.bind(&device, &queue, &mut encoder, &mut pass, [15, 1]);
        {
            let mut render = post.begin(&mut encoder, &pass);
            render.set_pipeline(pass.render());
            render.set_bind_group(0, &group, &[]);
            if pass.draw {
                post.draws.bind(&mut render, "TERRAIN_SOLID");
            }
            render.draw(0..3, 0..1);
        }
        let output = post.targets.as_ref().unwrap().color[0].as_ref().unwrap()[0].texture();
        encoder.copy_texture_to_buffer(
            output.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &read,
                layout: wgpu::TexelCopyBufferLayout {
                    bytes_per_row: Some(256),
                    ..Default::default()
                },
            },
            wgpu::Extent3d {
                width: 15,
                height: 1,
                depth_or_array_layers: 1,
            },
        );
        queue.submit([encoder.finish()]);
        read.slice(..).map_async(wgpu::MapMode::Read, |_| {});
        device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
        let data = read.slice(..).get_mapped_range().unwrap();
        for i in 0..15usize {
            let layer = (i % 5) as u8;
            let expected = if i >= 10 {
                [40 + layer, 50, 60, 255]
            } else if i >= 5 && (!maps_expected || layer == 2) {
                crate::sprites::FLAT_NORMAL
            } else if i >= 5 {
                [128 + layer, 126, 255, 19 + layer]
            } else if !maps_expected || layer == 3 {
                crate::sprites::NO_SPECULAR
            } else {
                [17 + layer, 240, 65, 255]
            };
            assert_eq!(&data[i * 4..i * 4 + 4], &expected, "case={case}, pixel={i}");
        }
        eprintln!("Material oracle case={case}: 15 exact RGBA pixels pass");
    }
    // Fault injection: the same production binding must refuse a filtering sampler.
    // If a regression changed both the binding and sampler, the fractional positive
    // probes above would instead mix 0.75*240=180 and 0.75*255=191.25, not 240/255.
    pass.material_maps = true;
    post.material_maps = Some([atlases[1].view.clone(), atlases[0].view.clone()]);
    post.nearest_repeat = sampler(&device, wgpu::FilterMode::Linear, wgpu::AddressMode::Repeat);
    pass.groups.clear();
    let validation = device.push_error_scope(wgpu::ErrorFilter::Validation);
    let mut encoder = device.create_command_encoder(&Default::default());
    let _invalid_group = post.bind(&device, &queue, &mut encoder, &mut pass, [15, 1]);
    let error = pollster::block_on(validation.pop())
        .expect("production material binding must reject a filtering sampler");
    assert!(
        error.to_string().to_ascii_lowercase().contains("filtering"),
        "{error}"
    );
    eprintln!("Material oracle negative filtering control: rejected by production binding");
    std::fs::remove_dir_all(root).unwrap();
}
