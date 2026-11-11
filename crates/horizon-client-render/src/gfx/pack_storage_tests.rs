//! Executes a synthetic, clean-room storage-image shader pack through the real Metal path.
use super::*;

struct ScratchPack(std::path::PathBuf);
impl ScratchPack {
    fn new() -> Self {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root =
            std::env::temp_dir().join(format!("horizon-storage-{}-{unique}", std::process::id()));
        std::fs::create_dir_all(root.join("shaders")).unwrap();
        let files = [
            (
                "shaders.properties",
                "image.volume=volumeSampler rgba rgba8 unsigned_byte false false 2 2 2\n",
            ),
            (
                "shadowcomp.csh",
                "#version 430\n\
                 layout(local_size_x=1, local_size_y=1, local_size_z=1) in;\n\
                 const ivec3 workGroups = ivec3(2, 2, 2);\n\
                 layout(rgba8) uniform writeonly image3D volume;\n\
                 uniform sampler3D volumeSampler;\n\
                 void main() {\n\
                   imageStore(volume, ivec3(gl_GlobalInvocationID), texelFetch(volumeSampler, ivec3(gl_GlobalInvocationID), 0) + vec4(0.25, 0.5, 0.75, 1.0));\n\
                 }\n",
            ),
            (
                "final.vsh",
                "#version 330 compatibility\nvoid main() { gl_Position = vec4(gl_Vertex.xy * 2.0 - 1.0, 0.0, 1.0); }\n",
            ),
            (
                "final.fsh",
                "#version 330 compatibility\n\
                 uniform sampler3D volumeSampler;\n\
                 void main() { gl_FragColor = texture(volumeSampler, vec3(0.75)); }\n",
            ),
        ];
        for (name, source) in files {
            std::fs::write(root.join("shaders").join(name), source).unwrap();
        }
        Self(root)
    }
}
impl Drop for ScratchPack {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Covers actual image3D writes, sampler3D aliasing, command ordering, retained history,
/// and explicit per-frame clears. Pipeline construction alone cannot prove any of these.
#[test]
#[ignore = "requires a Metal adapter; run explicitly on macOS"]
fn storage_volume_compute_is_sampled_and_retains_history_on_metal() {
    use horizon_shaders::{GameInputs, Macros, Pack, Properties, load_post};
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
        backends: wgpu::Backends::METAL,
        ..wgpu::InstanceDescriptor::new_without_display_handle()
    });
    let adapter = pollster::block_on(instance.request_adapter(&Default::default()))
        .expect("this execution regression requires a Metal adapter");
    assert!(adapter.features().contains(wgpu::Features::CLEAR_TEXTURE));
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        required_features: wgpu::Features::CLEAR_TEXTURE,
        required_limits: adapter.limits(),
        ..Default::default()
    }))
    .unwrap();
    let fixture = ScratchPack::new();
    let pack = Pack::open(&fixture.0).unwrap();
    let macros = Macros::standard();
    let properties = Properties::from_pack(&pack, "", &macros);
    let parsed = load_post(&pack, "", &properties, &macros, &GameInputs::default());
    assert!(parsed.complete, "parse notes: {:?}", parsed.notes);
    let world = World {
        gui_size: [1, 1],
        sprite_rects: &[],
        material_maps: None,
        atlas: None,
        lightmap: texture(&device, &queue, "storage regression lightmap", 1, &[255; 4]),
        terrain: None,
        block_ids: &[],
        celestial: None,
        clouds: None,
        weather: false,
        hand: false,
    };
    let mut post = PackPost::test_new(
        &device,
        &queue,
        &adapter,
        parsed,
        wgpu::TextureFormat::Rgba8Unorm,
        world,
    )
    .expect("the pack runs");
    assert!(post.complete, "pipeline notes: {:?}", post.notes);
    assert_eq!(
        post.shadow_comp.len(),
        1,
        "the actual compute pipeline must exist"
    );
    assert_eq!(post.images.len(), 1);
    assert!(matches!(
        post.shadow_comp[0].pipeline,
        Pipeline::Compute { .. }
    ));
    post.resize(&device, &queue, [2, 2]).unwrap();
    assert_eq!(post.shadow_comp[0].image_copies, 1);
    assert!(post.images[0].copy.is_some());
    assert_eq!(post.images[0].dimension, wgpu::TextureViewDimension::D3);
    post.resize(&device, &queue, [2, 2]).unwrap();
    assert!(post.complete, "resize notes: {:?}", post.notes);
    let target = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("storage regression output"),
        size: wgpu::Extent3d {
            width: 2,
            height: 2,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let output = target.create_view(&Default::default());
    let read = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("storage regression readback"),
        size: 512,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let final_index = post
        .passes
        .iter()
        .position(|p| p.group == Group::Final)
        .unwrap();
    let mut final_pass = post.passes.remove(final_index);
    for frame in 0..3 {
        let mut encoder = device.create_command_encoder(&Default::default());
        if frame == 2 {
            // The same allocation must clear only when its definition requests it.
            post.images[0].def.clear = true;
        }
        pack_images::clear(
            &device,
            &mut encoder,
            &post.images,
            post.gpu.clear,
            &mut post.zeros,
        );
        if frame == 0 {
            let mut pass = post.shadow_comp.remove(0);
            post.dispatch(&device, &queue, &mut encoder, &mut pass, [2, 2]);
            post.shadow_comp.push(pass);
            // The readback below proves the dispatch wrote the sampled volume.
        }
        // Frame two deliberately performs no write: sampling must retain frame one's result.
        let group = post.bind(&device, &queue, &mut encoder, &mut final_pass, [2, 2]);
        {
            let mut render = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("storage regression final"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &output,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                ..Default::default()
            });
            let Pipeline::Render(pipeline) = &final_pass.pipeline else {
                panic!("final must be a render pass");
            };
            render.set_pipeline(pipeline.get().expect("compiled pipeline"));
            render.set_bind_group(0, &group, &[]);
            render.draw(0..6, 0..1);
        }
        encoder.copy_texture_to_buffer(
            target.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &read,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(256),
                    rows_per_image: None,
                },
            },
            target.size(),
        );
        queue.submit([encoder.finish()]);
        let (send, done) = std::sync::mpsc::channel();
        read.slice(..)
            .map_async(wgpu::MapMode::Read, move |result| {
                send.send(result).unwrap();
            });
        device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
        done.recv().unwrap().unwrap();
        {
            let data = read.slice(..).get_mapped_range().unwrap();
            let expected: &[u8] = if frame < 2 {
                &[64, 128, 191, 255]
            } else {
                &[0; 4]
            };
            for y in 0..2 {
                for x in 0..2 {
                    assert_eq!(
                        &data[y * 256 + x * 4..y * 256 + x * 4 + 4],
                        expected,
                        "frame {frame}, pixel ({x},{y})"
                    );
                }
            }
        }
        read.unmap();
    }
}

#[test]
#[ignore = "requires a Metal adapter; run explicitly on macOS"]
fn mismatched_storage_image_dimension_is_rejected_before_first_frame() {
    use horizon_shaders::{GameInputs, Macros, Pack, Properties, load_post};
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
        backends: wgpu::Backends::METAL,
        ..wgpu::InstanceDescriptor::new_without_display_handle()
    });
    let adapter = pollster::block_on(instance.request_adapter(&Default::default())).unwrap();
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        required_limits: adapter.limits(),
        ..Default::default()
    }))
    .unwrap();
    let fixture = ScratchPack::new();
    std::fs::write(
        fixture.0.join("shaders/shaders.properties"),
        "image.volume=volumeSampler rgba rgba8 unsigned_byte false false 2 2\n",
    )
    .unwrap();
    std::fs::write(fixture.0.join("shaders/shadowcomp.csh"),
        "#version 430\nlayout(local_size_x=1) in;\nlayout(rgba8) uniform writeonly image3D volume;\nvoid main(){imageStore(volume,ivec3(0),vec4(1.0));}\n").unwrap();
    std::fs::write(fixture.0.join("shaders/final.fsh"),
        "#version 330 compatibility\nuniform sampler2D volumeSampler;\nvoid main(){gl_FragColor=texture(volumeSampler,vec2(0.5));}\n").unwrap();
    let pack = Pack::open(&fixture.0).unwrap();
    let macros = Macros::standard();
    let properties = Properties::from_pack(&pack, "", &macros);
    let parsed = load_post(&pack, "", &properties, &macros, &GameInputs::default());
    assert!(parsed.complete, "{:?}", parsed.notes);
    let world = World {
        gui_size: [1, 1],
        sprite_rects: &[],
        material_maps: None,
        atlas: None,
        lightmap: texture(&device, &queue, "mismatch test lightmap", 1, &[255; 4]),
        terrain: None,
        block_ids: &[],
        celestial: None,
        clouds: None,
        weather: false,
        hand: false,
    };
    let post = PackPost::test_new(
        &device,
        &queue,
        &adapter,
        parsed,
        wgpu::TextureFormat::Rgba8Unorm,
        world,
    )
    .expect("the pack runs");
    assert!(
        !post.complete,
        "a 2D image must not reach a 3D storage binding at render time"
    );
    assert!(
        post.notes
            .iter()
            .any(|note| note.contains("image volume") && note.contains("D3")),
        "{:?}",
        post.notes
    );
}
