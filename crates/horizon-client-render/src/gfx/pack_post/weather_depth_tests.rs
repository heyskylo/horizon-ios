//! Original synthetic weather oracle: no pack assets or external shader algorithms.
use super::*;
use std::io::{Cursor, Write};
use wgpu::util::DeviceExt;

#[test]
#[ignore = "requires a host GPU; missing adapter is a failure"]
fn rain_depth_controls_writes_for_rain_snow_and_fallbacks() {
    let instance = crate::gfx::test_gpu::instance();
    let adapter = pollster::block_on(instance.request_adapter(&Default::default())).unwrap();
    eprintln!("Weather depth GPU: {:?}", adapter.get_info());
    assert_ne!(adapter.get_info().device_type, wgpu::DeviceType::Cpu);
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        required_limits: adapter.limits(),
        ..Default::default()
    }))
    .unwrap();
    let mut depth_failures = Vec::new();
    for program in ["gbuffers_weather", "gbuffers_textured_lit"] {
        for (property, writes) in [
            ("rain.depth=true", true),
            ("rain.depth=false", false),
            // An evidence-limited compatibility choice: preserve accepted main here.
            // The permitted packs do not establish the external omitted-value default.
            ("", true),
        ] {
            let mut post = pack(&device, &queue, &adapter, program, property);
            // Retain the pipelines for another frame, then resize their targets.
            for size in [[6, 1], [6, 1], [12, 2]] {
                post.resize(&device, size);
                let mut encoder = device.create_command_encoder(&Default::default());
                let targets = post.targets.as_ref().unwrap();
                let color = &targets.color[0].as_ref().unwrap()[0];
                encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    color_attachments: &[attach(
                        color,
                        wgpu::Operations {
                            load: wgpu::LoadOp::Clear(wgpu::Color::BLUE),
                            store: wgpu::StoreOp::Store,
                        },
                    )],
                    depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                        view: &targets.depth,
                        depth_ops: Some(wgpu::Operations {
                            load: wgpu::LoadOp::Clear(0.5),
                            store: wgpu::StoreOp::Store,
                        }),
                        stencil_ops: None,
                    }),
                    ..Default::default()
                });
                post.set_weather(Some(geometry(&device)));
                post.draw_weather(&device, &queue, &mut encoder, size);
                assert!(post.weather_draw.is_none(), "draw consumes this frame only");
                assert!(post.has_weather(), "pipelines survive the draw");
                // A dry frame must neither redraw old quads nor discard the pipelines.
                post.set_weather(None);
                post.draw_weather(&device, &queue, &mut encoder, size);
                assert!(post.has_weather());
                let targets = post.targets.as_ref().unwrap();
                let color = targets.color[0].as_ref().unwrap()[0].texture();
                let colors = readback(&device, &mut encoder, color);
                let depths = readback(&device, &mut encoder, &targets.depth_texture);
                queue.submit([encoder.finish()]);
                for buffer in [&colors, &depths] {
                    buffer.slice(..).map_async(wgpu::MapMode::Read, |_| {});
                }
                device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
                let colors = colors.slice(..).get_mapped_range().unwrap();
                let depths = depths.slice(..).get_mapped_range().unwrap();
                for y in 0..size[1] as usize {
                    for x in 0..size[0] as usize {
                        let quad = x / (size[0] as usize / 6);
                        let visible = quad.is_multiple_of(3);
                        // First quad is in front; second is behind scene depth; third discards.
                        let expected_color = if visible && quad < 3 {
                            [128, 0, 127, 255]
                        } else if visible {
                            [0, 128, 127, 255]
                        } else {
                            [0, 0, 255, 255]
                        };
                        let offset = y * 256 + x * 4;
                        let actual_color = &colors[offset..offset + 4];
                        for (actual, expected) in actual_color.iter().zip(expected_color) {
                            assert!(
                                i16::from(*actual).abs_diff(expected) <= 1,
                                "{program} {property:?} size={size:?} ({x},{y}): color {actual_color:?}"
                            );
                        }
                        let depth =
                            f32::from_le_bytes(depths[offset..offset + 4].try_into().unwrap());
                        let expected_depth = if visible && writes { 0.25 } else { 0.5 };
                        if depth != expected_depth {
                            depth_failures.push(format!(
                                "{program} {property:?} size={size:?} ({x},{y}): depth={depth}, expected={expected_depth}"
                            ));
                        }
                    }
                }
                eprintln!(
                    "Weather oracle {program} {property:?} {size:?}: color, occlusion and discard pass; cumulative depth mismatches={}",
                    depth_failures.len()
                );
            }
        }
    }
    assert!(depth_failures.is_empty(), "{}", depth_failures.join("\n"));
}

fn pack(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    adapter: &wgpu::Adapter,
    program: &str,
    property: &str,
) -> PackPost {
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (extension, source) in [
        (
            "vsh",
            "#version 120\nvarying vec4 color; varying float marker; void main(){gl_Position=gl_Vertex;color=gl_Color;marker=gl_MultiTexCoord0.x;}",
        ),
        (
            "fsh",
            "#version 120\nvarying vec4 color; varying float marker; /* DRAWBUFFERS:0 */ void main(){if(marker>1.5) discard;gl_FragData[0]=color;}",
        ),
    ] {
        zip.start_file(
            format!("shaders/{program}.{extension}"),
            zip::write::SimpleFileOptions::default(),
        )
        .unwrap();
        zip.write_all(source.as_bytes()).unwrap();
    }
    let pack = horizon_shaders::Pack::from_zip_bytes(
        "original-weather-depth",
        zip.finish().unwrap().into_inner(),
    )
    .unwrap();
    let macros = horizon_shaders::Macros::standard();
    let properties = horizon_shaders::Properties::parse(property, &macros);
    let post = horizon_shaders::load_post(
        &pack,
        "",
        &properties,
        &macros,
        &horizon_shaders::GameInputs {
            weather: Some(super::super::pack_weather::inputs()),
            ..Default::default()
        },
    );
    assert!(post.notes.is_empty(), "{:?}", post.notes);
    assert_eq!(post.weather.as_ref().unwrap().name, program);
    let lightmap = device
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
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        })
        .create_view(&Default::default());
    let mut post = PackPost::new(
        device,
        queue,
        adapter,
        post,
        wgpu::TextureFormat::Rgba8Unorm,
        World {
            atlas: None,
            material_maps: None,
            lightmap,
            terrain: None,
            block_ids: &[],
            sprite_rects: &[],
            celestial: None,
            gui_size: [1, 1],
            clouds: None,
            weather: true,
            hand: false,
        },
        &Default::default(),
    )
    .expect("weather pack runs");
    assert!(
        post.has_weather(),
        "production weather pipelines must compile: {:?}",
        post.notes
    );
    post.copied |= 1; // Keep colortex0 readable for the independent color oracle.
    post
}

fn geometry(device: &wgpu::Device) -> Weather {
    let mut vertices = Vec::new();
    let mut indices = Vec::new();
    for quad in 0..6u32 {
        let left = quad as f32 / 3.0 - 1.0;
        let right = (quad + 1) as f32 / 3.0 - 1.0;
        let z = if quad % 3 == 1 { 0.5 } else { -0.5 };
        for pos in [
            [left, -1.0, z],
            [right, -1.0, z],
            [right, 1.0, z],
            [left, 1.0, z],
        ] {
            vertices.push(super::super::Vertex {
                pos,
                uv: [(quad % 3) as f32, 0.0],
                layer: 0,
                color: u32::from_le_bytes(if quad < 3 {
                    [255, 0, 0, 128]
                } else {
                    [0, 255, 0, 128]
                }),
                light: 0,
                normal: [0; 4],
                glint_uv: [0.0; 2],
            });
        }
        indices.extend([0, 1, 2, 0, 2, 3].map(|i| quad * 4 + i));
    }
    let buffer = |contents: &[u8], usage| {
        device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: None,
            contents,
            usage,
        })
    };
    Weather {
        vertices: buffer(bytemuck::cast_slice(&vertices), wgpu::BufferUsages::VERTEX),
        indices: buffer(bytemuck::cast_slice(&indices), wgpu::BufferUsages::INDEX),
        quads: [0..3, 3..6],
        sprites: [None, None],
        generation: 1,
    }
}

fn readback(
    device: &wgpu::Device,
    encoder: &mut wgpu::CommandEncoder,
    texture: &wgpu::Texture,
) -> wgpu::Buffer {
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: u64::from(256 * texture.height()),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    encoder.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            aspect: if texture.format().is_depth_stencil_format() {
                wgpu::TextureAspect::DepthOnly
            } else {
                wgpu::TextureAspect::All
            },
            ..texture.as_image_copy()
        },
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(256),
                rows_per_image: None,
            },
        },
        texture.size(),
    );
    buffer
}
