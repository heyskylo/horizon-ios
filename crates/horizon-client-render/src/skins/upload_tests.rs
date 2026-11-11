//! A real downloaded wing PNG, atlas eviction/cache reload, and the production
//! HUD upload/draw pipeline. This is opt-in so a machine without a GPU cannot
//! silently count an unexecuted render as a pass.
use crate::{
    gfx::upload::Uploads,
    hud::{HudAtlas, HudBatch, HudRenderer, Rect},
    skins,
};
use std::time::{Duration, Instant};

#[test]
fn a_failed_registration_still_submits_the_other_texture_jobs() {
    let textures = skins::Textures {
        skin: Some("https://textures.minecraft.net/texture/body".into()),
        cape: Some("https://textures.minecraft.net/texture/cape".into()),
        elytra: Some("https://textures.minecraft.net/texture/elytra".into()),
        signed: true,
        slim: false,
    };
    for failed in [skins::Kind::Skin, skins::Kind::Cape, skins::Kind::Elytra] {
        let mut submitted = Vec::new();
        assert!(
            skins::lookup_locations(&textures, true, |kind, _| {
                submitted.push(kind);
                if kind == failed {
                    skins::Download::Failed
                } else {
                    skins::Download::Ready
                }
            })
            .is_none()
        );
        assert_eq!(
            submitted,
            [skins::Kind::Skin, skins::Kind::Cape, skins::Kind::Elytra]
        );
    }
}

#[test]
#[ignore = "requires a working graphics adapter; executes actual upload/draw/readback"]
fn downloaded_wings_survive_cache_and_atlas_eviction_and_render_source_pixels() {
    let root = std::env::temp_dir().join(format!("horizon-wing-upload-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let pixel = |x: usize, y: usize| [(x * 7) as u8, (y * 11) as u8, ((x ^ y) * 5) as u8, 255];
    let mut raw = Vec::new();
    for y in 0..19 {
        for x in 0..37 {
            raw.extend_from_slice(&pixel(x, y));
        }
    }
    let mut png = Vec::new();
    let mut encoder = png::Encoder::new(&mut png, 37, 19);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().unwrap();
    writer.write_image_data(&raw).unwrap();
    writer.finish().unwrap();
    let hash = "wing-upload-fixture";
    let digest = skins::skin_location(hash)["skins/".len()..].to_owned();
    let folder = root.join(&digest[..2]);
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::write(folder.join(&digest), png).unwrap();
    let url = format!("https://example.invalid/texture/{hash}");
    let mut downloads = skins::Downloader::new(root.clone());
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        match downloads.state(skins::Kind::Elytra, &url) {
            skins::Download::Ready => break,
            skins::Download::Failed => panic!("valid cached wing PNG failed"),
            skins::Download::Pending => assert!(Instant::now() < deadline),
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    let textures = skins::Textures {
        elytra: Some(url),
        signed: true,
        ..Default::default()
    };
    let locations =
        skins::lookup_locations(&textures, true, |kind, url| downloads.state(kind, url)).unwrap();
    assert!(
        locations.skin.is_none(),
        "ELYTRA-only profiles keep their UUID default body"
    );
    let location = locations.elytra.unwrap();
    let uploads = downloads.take_ready();
    assert_eq!(uploads.len(), 1);
    assert_eq!(uploads[0].0, location);
    let mut atlas = HudAtlas::boot();
    atlas.width = 128;
    atlas.height = 64;
    atlas.pixels = vec![0; 128 * 64 * 4];
    atlas.sprites.clear();
    atlas.sprites.insert(
        "horizon/skin0".into(),
        Rect {
            u0: 0.0,
            v0: 0.0,
            u1: 0.5,
            v1: 1.0,
            w: 64,
            h: 64,
        },
    );
    let mut slots = skins::SkinSlots::new(1);
    for (name, rgba) in uploads {
        skins::atlas_upload(&mut slots, &mut atlas, name, rgba).unwrap();
    }
    assert_eq!(atlas.sprites[&location].texels((128, 64)), (64, 32));
    slots.next_frame([location.clone()].into());
    assert!(
        skins::atlas_upload(
            &mut slots,
            &mut atlas,
            "skins/other".into(),
            vec![0; 64 * 64 * 4]
        )
        .is_none()
    );
    assert!(atlas.sprites.contains_key(&location));
    slots.next_frame(Default::default());
    skins::atlas_upload(
        &mut slots,
        &mut atlas,
        "skins/other".into(),
        vec![0; 64 * 64 * 4],
    )
    .unwrap();
    assert!(!atlas.sprites.contains_key(&location));
    // Exceed the real image LRU. The compressed disk cache remains authoritative.
    for n in 0..1025 {
        downloads.remember_image(&format!("skins/lru-{n}"), &[0; 64 * 64 * 4]);
    }
    assert!(downloads.image(&location).is_none());
    assert!(downloads.again(&location));
    let reloaded = downloads.take_ready();
    assert_eq!(reloaded.len(), 1);
    slots.next_frame(Default::default());
    let mut batch = HudBatch::new([64, 32]);
    for (name, rgba) in reloaded {
        batch
            .patches
            .push(skins::atlas_upload(&mut slots, &mut atlas, name, rgba).unwrap());
    }
    let chosen = skins::wings_texture(Some(&location), None, false, |s| {
        atlas.sprites.contains_key(s)
    })
    .unwrap();
    batch.rect_uv(0.0, 0.0, 64.0, 32.0, atlas.sprites[chosen], u32::MAX);
    let instance =
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle().with_env());
    let adapter = pollster::block_on(instance.request_adapter(&Default::default()))
        .expect("a real graphics adapter is required");
    eprintln!("WING_UPLOAD_ADAPTER {:?}", adapter.get_info());
    let (device, queue) =
        pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default())).unwrap();
    let texture = |width, height, format, usage| {
        device.create_texture(&wgpu::TextureDescriptor {
            label: Some("wing upload regression"),
            size: wgpu::Extent3d {
                width,
                height,
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
    let gpu_atlas = texture(
        128,
        64,
        wgpu::TextureFormat::Rgba8Unorm,
        wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
    );
    queue.write_texture(
        gpu_atlas.as_image_copy(),
        &atlas.pixels,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(512),
            rows_per_image: Some(64),
        },
        gpu_atlas.size(),
    );
    let view = gpu_atlas.create_view(&Default::default());
    let sampler = device.create_sampler(&Default::default());
    let mut hud = HudRenderer::new(
        &device,
        wgpu::TextureFormat::Rgba8Unorm,
        &view,
        &view,
        &view,
        &sampler,
        false,
    )
    .unwrap();
    hud.atlas_texture = Some(gpu_atlas);
    let target = texture(
        64,
        32,
        wgpu::TextureFormat::Rgba8Unorm,
        wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
    );
    let depth = texture(
        64,
        32,
        wgpu::TextureFormat::Depth32Float,
        wgpu::TextureUsages::RENDER_ATTACHMENT,
    );
    let mut uploads = Uploads::new(&device);
    let mut encoder = device.create_command_encoder(&Default::default());
    hud.prepare(&device, &mut uploads, &mut encoder, &batch, [64, 32]);
    hud.draw(
        &mut encoder,
        &target.create_view(&Default::default()),
        &depth.create_view(&Default::default()),
        &batch,
        [64, 32],
    );
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("wing pixels"),
        size: 64 * 32 * 4,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    encoder.copy_texture_to_buffer(
        target.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &readback,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(256),
                rows_per_image: Some(32),
            },
        },
        target.size(),
    );
    uploads.finish(&encoder);
    queue.submit([encoder.finish()]);
    let (send, done) = std::sync::mpsc::channel();
    readback
        .slice(..)
        .map_async(wgpu::MapMode::Read, move |result| {
            send.send(result).unwrap()
        });
    device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
    done.recv().unwrap().unwrap();
    let pixels = readback.slice(..).get_mapped_range().unwrap();
    for y in 0..32 {
        for x in 0..64 {
            assert_eq!(
                &pixels[(y * 64 + x) * 4..][..4],
                &pixel((2 * x + 1) * 37 / 128, (2 * y + 1) * 19 / 64),
                "rendered ({x},{y})"
            );
        }
    }
    drop(pixels);
    readback.unmap();
    while !downloads.shutdown() {
        std::thread::yield_now();
    }
    std::fs::remove_dir_all(root).unwrap();
}
