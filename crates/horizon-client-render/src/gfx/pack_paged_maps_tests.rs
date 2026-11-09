//! Paged maps (`paged_maps.rs`) through the translator's `hz_pagedTexture` and the pass's
//! binding: a pack reads the map's texels at the colour's coordinates.
use super::*;
use wgpu::util::DeviceExt;

/// The atlas the fixture's coordinates are in, and the one mapped texture's place.
const ATLAS: (u32, u32) = (64, 32);
const SPRITE: [u32; 4] = [20, 4, 20, 20];

/// The fixture's normal map texel at texel (x, y) of its texture.
fn texel(x: u32, y: u32) -> [u8; 4] {
    [x as u8 * 10, y as u8 * 10, 7, 255]
}

#[test]
#[ignore = "requires a host GPU; missing adapter is a failure"]
fn a_pack_reads_paged_maps_at_the_colours_coordinates() {
    let instance = crate::gfx::test_gpu::instance();
    let adapter = pollster::block_on(instance.request_adapter(&Default::default())).unwrap();
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        required_limits: adapter.limits(),
        ..Default::default()
    }))
    .unwrap();
    let root = std::env::temp_dir().join(format!("horizon-paged-pack-{}", std::process::id()));
    std::fs::create_dir_all(root.join("shaders")).unwrap();
    std::fs::write(
        root.join("shaders/gbuffers_terrain.vsh"),
        "#version 120\nvoid main(){gl_Position=gl_Vertex;}",
    )
    .unwrap();
    // Each pixel reads one atlas texel's centre: in the texture (on four pages), beside it,
    // on a page without maps, and the specular map, which no texture has.
    std::fs::write(
        root.join("shaders/gbuffers_terrain.fsh"),
        r#"#version 120
uniform sampler2D normals;
uniform sampler2D specular;
vec2 at(float x, float y) { return (vec2(x, y) + 0.5) / vec2(64.0, 32.0); }
void main() {
    int x = int(floor(gl_FragCoord.x));
    if (x == 0) gl_FragData[0] = texture2D(normals, at(20.0, 4.0));
    else if (x == 1) gl_FragData[0] = texture2D(normals, at(39.0, 23.0));
    else if (x == 2) gl_FragData[0] = texture2D(normals, at(31.0, 15.0), 2.0);
    else if (x == 3) gl_FragData[0] = texture2DLod(normals, at(32.0, 16.0), 3.0);
    else if (x == 4) gl_FragData[0] = texture2D(normals, at(19.0, 4.0));
    else if (x == 5) gl_FragData[0] = texture2D(normals, at(50.0, 30.0));
    else if (x == 6) gl_FragData[0] = texture2D(specular, at(25.0, 10.0));
    else gl_FragData[0] = texture2D(normals, at(33.0, 5.0));
}
"#,
    )
    .unwrap();
    let [x0, y0, w, h] = SPRITE;
    let normal: Vec<u8> = (0..w * h).flat_map(|i| texel(i % w, i / w)).collect();
    let [normals, specular] = crate::paged_maps::build(
        ATLAS,
        &[crate::paged_maps::SpriteMaps {
            rect: SPRITE,
            maps: [Some(normal), None],
        }],
        16384,
    );
    assert!(specular.is_none());
    let normals = normals.unwrap();
    let map = device
        .create_texture_with_data(
            &queue,
            &wgpu::TextureDescriptor {
                label: Some("Fixture paged normal map"),
                size: wgpu::Extent3d {
                    width: normals.width,
                    height: normals.height,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8Unorm,
                usage: wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            },
            wgpu::util::TextureDataOrder::LayerMajor,
            &normals.pixels,
        )
        .create_view(&Default::default());
    let pack = horizon_shaders::Pack::open(&root).unwrap();
    let macros = horizon_shaders::Macros::standard();
    let properties = horizon_shaders::Properties::parse("alphaTest.gbuffers_terrain=off", &macros);
    let inputs = horizon_shaders::GameInputs {
        terrain: Some(horizon_shaders::GbuffersInputs {
            body: "vec2 p = vec2(float((gl_VertexIndex << 1) & 2), float(gl_VertexIndex & 2)); hz_Vertex=vec4(p*2.0-1.0,0.0,1.0);".into(),
            paged_maps: true,
            water_mask: false,
            water_patches: false,
            ..Default::default()
        }),
        ..Default::default()
    };
    let post = horizon_shaders::load_post(&pack, "", &properties, &macros, &inputs);
    assert!(post.terrain.is_some(), "{:?}", post.notes);
    let world = World {
        sprite_rects: &[],
        atlas: None,
        material_maps: None,
        lightmap: map.clone(),
        terrain: Some(Geometry {
            buffers: &[],
            table: None,
            depth_write: true,
            atlas: None,
            material_maps: false,
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
        &Default::default(),
    )
    .expect("the pack runs");
    let mut pass = post
        .terrain
        .take()
        .unwrap_or_else(|| panic!("terrain pipeline must compile: {:?}", post.notes));
    // As the entity passes are (`paged`), with the maps the renderer hands over.
    pass.paged = true;
    post.set_paged_maps([Some(map), None]);
    post.copied |= 1;
    post.resize(&device, [8, 1]);
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
    let group = post.bind(&device, &queue, &mut encoder, &mut pass, [8, 1]);
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
            width: 8,
            height: 1,
            depth_or_array_layers: 1,
        },
    );
    queue.submit([encoder.finish()]);
    read.slice(..).map_async(wgpu::MapMode::Read, |_| {});
    device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
    let data = read.slice(..).get_mapped_range().unwrap();
    let local = |x: u32, y: u32| texel(x - x0, y - y0);
    let expected = [
        local(20, 4),
        local(39, 23),
        local(31, 15),
        local(32, 16),
        crate::sprites::FLAT_NORMAL,
        crate::sprites::FLAT_NORMAL,
        crate::sprites::NO_SPECULAR,
        local(33, 5),
    ];
    for (i, e) in expected.iter().enumerate() {
        assert_eq!(&data[i * 4..i * 4 + 4], e, "pixel {i}");
    }
    std::fs::remove_dir_all(root).unwrap();
}
