use super::*;
use crate::gfx::hud_coverage;

#[test]
fn standalone_picture_draw_order_and_screen_exit_release() {
    let Some(mut gpu) = Gpu::new() else { return };
    let image = Arc::new(pictures::Picture {
        id: u64::MAX,
        width: 1,
        height: 1,
        pixels: vec![255, 0, 0, 255],
        flags: TextureFlags::default(),
    });
    let weak = Arc::downgrade(&image);
    let mut batch = HudBatch::new([4, 2]);
    batch.pictures.push(image.clone());
    let rect = Rect {
        u0: 0.0,
        v0: 0.0,
        u1: 1.0,
        v1: 1.0,
        w: 1,
        h: 1,
    };
    batch.rect_uv(0.0, 0.0, 4.0, 2.0, rect, u32::MAX);
    batch.blend_since(0, Blend::Picture(image.id));
    batch.rect_uv(
        2.0,
        0.0,
        2.0,
        2.0,
        Rect {
            u0: 0.0,
            u1: 0.5,
            ..rect
        },
        rgba(0, 255, 0, 255),
    );
    let mut uploads = crate::gfx::upload::Uploads::new(&gpu.device);
    let mut encoder = gpu.device.create_command_encoder(&Default::default());
    gpu.hud
        .prepare_pictures(&gpu.device, &mut uploads, &mut encoder, &batch);
    uploads.finish(&encoder);
    gpu.queue.submit([encoder.finish()]);
    let (pixels, mask) = gpu.draw(&batch, 0.0, 0.0, true);
    for row in pixels.as_chunks::<16>().0 {
        assert_eq!(&row[..8], &[255, 0, 0, 255, 255, 0, 0, 255]);
        assert_eq!(&row[8..], &[0, 255, 0, 255, 0, 255, 0, 255]);
    }
    assert!(mask.iter().all(|v| *v == 255));
    drop(image);
    drop(batch);
    assert!(weak.upgrade().is_some());
    let mut encoder = gpu.device.create_command_encoder(&Default::default());
    gpu.hud.prepare_pictures(
        &gpu.device,
        &mut uploads,
        &mut encoder,
        &HudBatch::new([4, 2]),
    );
    assert!(gpu.hud.pictures.is_empty());
    assert!(
        weak.upgrade().is_none(),
        "closing the screen releases pixels and the GPU bind group"
    );
    eprintln!("standalone picture GPU rendering and release exercised");
}

struct Gpu {
    device: wgpu::Device,
    queue: wgpu::Queue,
    hud: HudRenderer,
}

impl Gpu {
    fn new() -> Option<Self> {
        let instance = crate::gfx::test_gpu::instance();
        let adapter = pollster::block_on(instance.request_adapter(&Default::default())).ok()?;
        if !hud_coverage::supported(
            adapter.limits().max_color_attachments,
            adapter.get_downlevel_capabilities().flags,
            adapter.get_texture_format_features(hud_coverage::FORMAT),
        ) {
            eprintln!("skipping HUD coverage: adapter lacks independent blending or R8 rendering");
            return None;
        }
        let (device, queue) =
            pollster::block_on(adapter.request_device(&Default::default())).ok()?;
        let atlas = device.create_texture_with_data(
            &queue,
            &wgpu::TextureDescriptor {
                label: None,
                size: wgpu::Extent3d {
                    width: 2,
                    height: 1,
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
            &[255, 255, 255, 255, 255, 255, 255, 0],
        );
        let view = atlas.create_view(&Default::default());
        let hud = HudRenderer::new(
            &device,
            hud_coverage::HUD_FORMAT,
            &view,
            &view,
            &view,
            &device.create_sampler(&Default::default()),
            true,
        )
        .unwrap();
        Some(Self { device, queue, hud })
    }

    fn texture(&self, format: wgpu::TextureFormat) -> wgpu::Texture {
        self.device.create_texture(&wgpu::TextureDescriptor {
            label: None,
            size: wgpu::Extent3d {
                width: 4,
                height: 2,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        })
    }

    /// `batch` drawn on the HUD's target cleared to `backdrop`, with coverage cleared to
    /// `initial`: the target's and the coverage's 8-bit texels.
    fn draw(
        &self,
        batch: &HudBatch,
        backdrop: f64,
        initial: f64,
        coverage: bool,
    ) -> (Vec<u8>, Vec<u8>) {
        let frame = self.texture(hud_coverage::HUD_FORMAT);
        let mask = self.texture(hud_coverage::FORMAT);
        let depth = self.texture(wgpu::TextureFormat::Depth32Float);
        let [frame_view, mask_view, depth_view] =
            [&frame, &mask, &depth].map(|t| t.create_view(&Default::default()));
        self.queue
            .write_buffer(&self.hud.buffer, 0, bytemuck::cast_slice(&batch.vertices));
        self.queue.write_buffer(
            &self.hud.uniform,
            0,
            bytemuck::bytes_of(&HudUniform::new([4.0, 2.0, 1.0, 0.75], batch.end_portal, 0)),
        );
        let mut encoder = self.device.create_command_encoder(&Default::default());
        for (view, value) in [(&frame_view, backdrop), (&mask_view, initial)] {
            let mut attachment = hud_coverage::attachment(view);
            attachment.ops.load = wgpu::LoadOp::Clear(wgpu::Color {
                r: value,
                g: value,
                b: value,
                a: value,
            });
            encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                color_attachments: &[Some(attachment)],
                ..Default::default()
            });
        }
        self.hud.draw_range(
            &mut encoder,
            &frame_view,
            coverage.then_some(&mask_view),
            &depth_view,
            batch,
            [4, 2],
            0..batch.vertices.len() as u32,
            None,
        );
        let buffers = [&frame, &mask].map(|t| {
            let buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
                label: None,
                size: 512,
                usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                mapped_at_creation: false,
            });
            encoder.copy_texture_to_buffer(
                t.as_image_copy(),
                wgpu::TexelCopyBufferInfo {
                    buffer: &buffer,
                    layout: wgpu::TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(256),
                        rows_per_image: None,
                    },
                },
                t.size(),
            );
            buffer
        });
        self.queue.submit([encoder.finish()]);
        let [color, mask] = buffers.map(|buffer| {
            buffer.map_async(wgpu::MapMode::Read, .., |r| r.unwrap());
            self.device
                .poll(wgpu::PollType::wait_indefinitely())
                .unwrap();
            buffer.get_mapped_range(..).unwrap().to_vec()
        });
        (
            color
                .chunks(256)
                .flat_map(|r| r[..16].iter().copied())
                .collect(),
            mask.chunks(256)
                .flat_map(|r| r[..4].iter().copied())
                .collect(),
        )
    }

    fn check(&self, batch: &HudBatch, initial: f64, expected: [u8; 4]) {
        let (plain, _) = self.draw(batch, 0.3, initial, false);
        let (color, mask) = self.draw(batch, 0.3, initial, true);
        assert_eq!(color, plain, "coverage changed the primary colour or alpha");
        for (got, want) in mask.iter().zip(expected.repeat(2)) {
            assert!(
                got.abs_diff(want) <= 1,
                "coverage {mask:?}, expected {expected:?}"
            );
        }
    }
}

fn quad(batch: &mut HudBatch, blend: Blend, color: u32, x: f32, uv: f32) {
    let start = batch.vertices.len();
    batch.quad(
        [[x, 0.0], [x + 2.0, 0.0], [x + 2.0, 2.0], [x, 2.0]],
        [[uv, 0.5]; 4],
        color,
    );
    batch.blend_since(start, blend);
}

#[test]
fn coverage_follows_hud_blends_discard_scissor_and_depth_without_changing_colour() {
    let Some(gpu) = Gpu::new() else {
        return;
    };
    for (blend, color, initial, count, expected) in [
        (Blend::Alpha, rgba(255, 255, 255, 128), 0.0, 2, 192),
        (Blend::Additive, rgba(255, 255, 255, 128), 0.3, 1, 77),
        (Blend::Invert, rgba(255, 255, 255, 32), 0.0, 1, 255),
        (Blend::EndPortal, rgba(255, 255, 255, 255), 0.0, 1, 255),
        (Blend::Vignette, rgba(102, 102, 102, 255), 0.0, 1, 102),
    ] {
        let mut batch = HudBatch::new([4, 2]);
        for _ in 0..count {
            quad(&mut batch, blend, color, 0.0, 0.25);
        }
        // Crosshair fragments with zero texture alpha discard; EndPortal has no discard.
        if blend == Blend::Invert {
            quad(&mut batch, blend, color, 2.0, 0.75);
        }
        gpu.check(
            &batch,
            initial,
            [
                expected,
                expected,
                (initial * 255.0).round() as u8,
                (initial * 255.0).round() as u8,
            ],
        );
    }
    let mut batch = HudBatch::new([4, 2]);
    for depth in [0.25, 0.75] {
        quad(
            &mut batch,
            Blend::Alpha,
            rgba(255, 255, 255, 128),
            0.0,
            0.25,
        );
        let start = batch.vertices.len() - 6;
        for vertex in &mut batch.vertices[start..] {
            vertex.depth = depth;
        }
    }
    batch.blends.push((0..12, Blend::Depth([1, 0, 2, 2])));
    gpu.check(&batch, 0.0, [0, 128, 0, 0]);
}

/// The HUD's target is 8-bit as vanilla's frame is, so each draw clips at white before the
/// next blends over it: a text highlight added over a light backdrop, then a 50% black panel.
#[test]
fn each_hud_draw_clips_at_white_as_on_an_8_bit_frame() {
    let Some(gpu) = Gpu::new() else {
        return;
    };
    let mut batch = HudBatch::new([4, 2]);
    quad(
        &mut batch,
        Blend::Additive,
        rgba(128, 128, 128, 255),
        0.0,
        0.25,
    );
    quad(&mut batch, Blend::Alpha, rgba(0, 0, 0, 128), 0.0, 0.25);
    // Vanilla's frame in integers: 204 + 128 clips at 255, then the panel keeps 127/255 of it.
    let (backdrop, highlight, panel) = (204u32, 128u32, 128u32);
    let clipped = (backdrop + highlight).min(255);
    let want = ((clipped * (255 - panel)) as f64 / 255.0).round() as u8;
    assert_eq!(want, 127);
    // The HUD layer's target, and the 8-bit frame a 0.2 pack finishes on in SDR, where the
    // HUD draws without coverage.
    for coverage in [true, false] {
        let (color, mask) = gpu.draw(&batch, f64::from(backdrop) / 255.0, 0.0, coverage);
        // Unclipped, the panel would have kept 127/255 of 332: 165.
        for pixel in [0, 1, 4, 5] {
            assert_eq!(
                color[pixel * 4..pixel * 4 + 3],
                [want; 3],
                "{coverage}: {color:?}"
            );
        }
        // The highlight hides nothing; the panel hides half of what is under it.
        if coverage {
            assert_eq!(mask[0], 128, "{mask:?}");
        }
    }
}

#[test]
fn coverage_preserves_each_cit_blend_and_its_destination_alpha() {
    let Some(mut gpu) = Gpu::new() else {
        return;
    };
    use horizon_optifine::cit::parse_blend;
    for (name, expected) in [
        ("alpha", 128),
        ("add", 0),
        ("dodge", 0),
        ("glint", 0),
        ("burn", 102),
        ("multiply", 26),
        ("overlay", 51),
        ("screen", 255),
        ("subtract", 255),
        ("replace", 255),
        ("0 768", 153),
        ("774 0", 153),
        ("770 771 772 773", 128),
    ] {
        let textures = cit_glint::GlintTextures {
            width: 1,
            height: 1,
            pixels: vec![102, 102, 102, 128],
            slots: vec![cit_glint::Slot {
                rgba: [1.0; 4],
                blur: false,
                blend: 0,
            }],
            blends: vec![parse_blend(name).unwrap()],
            ..Default::default()
        };
        gpu.hud
            .set_cit_glints(&gpu.device, &gpu.queue, Some(&textures));
        let mut batch = HudBatch::new([4, 2]);
        quad(
            &mut batch,
            Blend::CitGlint(0),
            rgba(0, 0, 0, 255),
            0.0,
            0.25,
        );
        quad(
            &mut batch,
            Blend::CitGlint(0),
            rgba(0, 0, 0, 255),
            2.0,
            0.75,
        );
        gpu.check(&batch, 0.0, [expected, expected, 0, 0]);
    }
}

#[test]
fn coverage_uses_pack_texts_final_alpha_and_discard() {
    let Some(mut gpu) = Gpu::new() else {
        return;
    };
    // A translated-style struct result and early return; no dependency on game resources.
    let program = crate::core_shader::CoreProgram {
        vertex: naga::front::wgsl::parse_str("@vertex fn main(@location(0) p: vec2<f32>) -> @builtin(position) vec4<f32> { return vec4(p.x / 2.0 - 1.0, 1.0 - p.y, 0.0, 1.0); }").unwrap(),
        fragment: naga::front::wgsl::parse_str("struct Result { @location(0) color: vec4<f32> } @fragment fn main(@builtin(position) p: vec4<f32>) -> Result { if p.x > 2.0 { discard; } return Result(vec4(0.4, 0.2, 0.1, 0.5)); }").unwrap(),
        other: Vec::new(),
    };
    gpu.hud.set_text_shader(&gpu.device, Some(&program));
    assert!(gpu.hud.pack_text.is_some());
    let mut batch = HudBatch::new([4, 2]);
    for x in [0.0, 2.0, 0.0, 2.0] {
        quad(&mut batch, Blend::Alpha, rgba(255, 255, 255, 255), x, 0.25);
    }
    for v in &mut batch.vertices {
        v.text = 1;
    }
    gpu.check(&batch, 0.0, [191, 191, 0, 0]);
}

#[test]
fn a_pack_text_shader_without_a_colour_output_is_dropped_not_wrapped() {
    let Some(mut gpu) = Gpu::new() else {
        return;
    };
    let program = crate::core_shader::CoreProgram {
        vertex: naga::front::wgsl::parse_str("@vertex fn main(@location(0) p: vec2<f32>) -> @builtin(position) vec4<f32> { return vec4(p, 0.0, 1.0); }").unwrap(),
        fragment: naga::front::wgsl::parse_str("@fragment fn main() -> @location(1) vec4<f32> { return vec4(1.0); }").unwrap(),
        other: Vec::new(),
    };
    gpu.hud.set_text_shader(&gpu.device, Some(&program));
    assert!(gpu.hud.pack_text.is_none());
}
