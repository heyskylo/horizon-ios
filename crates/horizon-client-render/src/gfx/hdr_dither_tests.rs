//! HDR10's dither (`hdr.wgsl`, `dithered`): the last pass into the 10-bit swapchain, and what
//! must stay without it.
use super::{
    Hdr, Output,
    tests::{fill, gpu},
};
use wgpu::TextureFormat as F;

const SIZE: [u32; 2] = [64, 1024];
/// The ramp: grey rising by one half-float step every `ROWS` rows from `FIRST / 2048`. That is
/// three PQ codes down the picture around 91 nits: as slow as a clear sky, and as fine as the
/// float frame holds.
const FIRST: u32 = 1434;
const ROWS: u32 = 52;

/// SMPTE ST 2084: nits as a 10-bit PQ code, before rounding.
fn code(nits: f64) -> f64 {
    let y = (nits / 10_000.0).powf(0.1593017578125);
    1023.0 * ((0.8359375 + 18.8515625 * y) / (1.0 + 18.6875 * y)).powf(78.84375)
}

/// The ramp's row `y` as a PQ code, at the 200-nit paper white `Hdr` starts with.
fn ramp_code(y: u32) -> f64 {
    code(200.0 * (f64::from(FIRST + y / ROWS) / 2048.0).powf(2.2))
}

fn ramp(device: &wgpu::Device, encoder: &mut wgpu::CommandEncoder, frame: &wgpu::Texture) {
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: None,
        source: wgpu::ShaderSource::Wgsl(
            format!(
                r#"
@vertex fn vs(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {{
    return vec4(vec2(f32((i << 1u) & 2u), f32(i & 2u)) * 2.0 - 1.0, 0.0, 1.0);
}}
@fragment fn fs(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {{
    return vec4(vec3(f32({FIRST}u + u32(pos.y) / {ROWS}u) / 2048.0), 1.0);
}}
"#
            )
            .into(),
        ),
    });
    let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: None,
        layout: None,
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: Some("vs"),
            compilation_options: Default::default(),
            buffers: &[],
        },
        primitive: Default::default(),
        depth_stencil: None,
        multisample: Default::default(),
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: Some("fs"),
            compilation_options: Default::default(),
            targets: &[Some(frame.format().into())],
        }),
        multiview_mask: None,
        cache: None,
    });
    let view = frame.create_view(&Default::default());
    let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        color_attachments: &[Some(super::super::hud_coverage::attachment(&view))],
        ..Default::default()
    });
    pass.set_pipeline(&pipeline);
    pass.draw(0..3, 0..1);
}

/// What `hdr` writes for the float frame `paint` fills, with no HUD over it: its last pass into
/// a swapchain image of its output (`Hdr::present`), or a screenshot's HDR10 copy
/// (`Hdr::capture`). Every pixel's bytes, row by row.
fn shown(
    (device, queue): &(wgpu::Device, wgpu::Queue),
    hdr: &mut Hdr,
    screenshot: bool,
    paint: impl Fn(&mut wgpu::CommandEncoder, &wgpu::Texture),
) -> Vec<u8> {
    let frame = hdr.frame(device, SIZE);
    let mut encoder = device.create_command_encoder(&Default::default());
    paint(&mut encoder, &frame);
    hdr.keep_world(&mut encoder, None);
    super::super::hud_coverage::clear(&mut encoder, &hdr.coverage_view().unwrap());
    let image = if screenshot {
        hdr.capture(device, queue, &mut encoder, true).unwrap()
    } else {
        let image = device.create_texture(&wgpu::TextureDescriptor {
            label: None,
            size: frame.size(),
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: hdr.output.format(),
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        hdr.present(queue, &mut encoder, &image, true, None);
        image
    };
    let row = SIZE[0] * image.format().block_copy_size(None).unwrap();
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: u64::from(row * SIZE[1]),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    encoder.copy_texture_to_buffer(
        image.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(row),
                rows_per_image: None,
            },
        },
        image.size(),
    );
    queue.submit([encoder.finish()]);
    buffer.map_async(wgpu::MapMode::Read, .., |r| r.unwrap());
    device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
    buffer.get_mapped_range(..).unwrap().to_vec()
}

fn float_hdr((device, queue): &(wgpu::Device, wgpu::Queue), output: Output) -> Hdr {
    Hdr::with_debug(device, queue, F::Rgba16Float, output, true, None)
}

/// An HDR10 picture's 10-bit codes, red, green and blue.
fn codes(bytes: &[u8]) -> Vec<[u32; 3]> {
    let pixels = bytes.as_chunks::<4>().0.iter();
    pixels
        .map(|p| [0, 10, 20].map(|shift| (u32::from_le_bytes(*p) >> shift) & 0x3ff))
        .collect()
}

/// Down every column of a channel: the longest run of rows with one code, and the runs' mean
/// length.
fn runs(codes: &[[u32; 3]], channel: usize) -> (u32, f64) {
    let (mut longest, mut count) = (0, 0);
    for x in 0..SIZE[0] {
        let mut run = 0;
        for y in 0..SIZE[1] {
            let at = |y: u32| codes[(y * SIZE[0] + x) as usize][channel];
            if y > 0 && at(y) != at(y - 1) {
                (longest, count, run) = (longest.max(run), count + 1, 0);
            }
            run += 1;
        }
        (longest, count) = (longest.max(run), count + 1);
    }
    (longest, f64::from(SIZE[0] * SIZE[1]) / f64::from(count))
}

/// Whether every row of a picture is one value from side to side: no noise in it.
fn rows_are_flat(bytes: &[u8]) -> bool {
    let pixel = bytes.len() / (SIZE[0] * SIZE[1]) as usize;
    bytes
        .chunks(pixel * SIZE[0] as usize)
        .all(|row| row.chunks(pixel).all(|p| p == &row[..pixel]))
}

#[test]
fn a_slow_gradient_gets_grain_instead_of_bands_on_hdr10() {
    let Some(gpu) = gpu() else {
        return;
    };
    let ramp = |encoder: &mut wgpu::CommandEncoder, frame: &wgpu::Texture| {
        ramp(&gpu.0, encoder, frame);
    };
    let mut hdr = float_hdr(&gpu, Output::Pq);
    // The ramp is what the test says: three codes, top to bottom.
    let span = ramp_code(SIZE[1] - 1) - ramp_code(0);
    assert!((2.9..3.2).contains(&span), "{span}");
    // Without dither, as a screenshot's copy stays: each row its nearest code, in bands hundreds
    // of rows tall. A GPU need not round at exactly half a code: an NVIDIA one rounds up from
    // 0.55 here, which shows as 0.05 of a code less, so the bounds below leave a quarter code.
    let plain = codes(&shown(&gpu, &mut hdr, true, ramp));
    let dithered = codes(&shown(&gpu, &mut hdr, false, ramp));
    let want = |i: usize| ramp_code(i as u32 / SIZE[0]);
    for channel in 0..3 {
        for (i, p) in plain.iter().enumerate() {
            let off = (f64::from(p[channel]) - want(i)).abs();
            assert!(
                off <= 0.75,
                "plain {channel} at {i}: {p:?}, not {}",
                want(i)
            );
        }
        let (longest, _) = runs(&plain, channel);
        assert!(longest >= 250, "plain {channel}: bands of {longest} rows");
        // With it: each pixel within the noise's one code of its light (and half a code of
        // rounding), no column holds a code for long, and a run is a few rows on average.
        for (i, d) in dithered.iter().enumerate() {
            let off = (f64::from(d[channel]) - want(i)).abs();
            assert!(
                off <= 1.75,
                "dithered {channel} at {i}: {d:?}, not {}",
                want(i)
            );
        }
        let (longest, mean) = runs(&dithered, channel);
        eprintln!(
            "HDR10 dither GPU, channel {channel}: longest run {longest} rows, mean {mean:.2}"
        );
        assert!(longest <= 64, "dithered {channel}: a run of {longest} rows");
        assert!(mean < 4.0, "dithered {channel}: runs of {mean} rows");
        // The mean of each 64-row block is the light meant.
        let mut worst = 0.0_f64;
        for block in 0..SIZE[1] / 64 {
            let rows = (block * 64 * SIZE[0]) as usize..((block + 1) * 64 * SIZE[0]) as usize;
            let n = rows.len() as f64;
            let got = dithered[rows.clone()]
                .iter()
                .map(|d| f64::from(d[channel]))
                .sum::<f64>()
                / n;
            let meant = rows.map(want).sum::<f64>() / n;
            worst = worst.max((got - meant).abs());
            assert!(
                (got - meant).abs() < 0.25,
                "block {block} of {channel}: {got}, not {meant}"
            );
        }
        eprintln!("HDR10 dither GPU, channel {channel}: block means within {worst:.3} of a code");
    }
}

#[test]
fn hdr10s_dither_keeps_both_ends_exact_and_leans_to_them_in_their_last_code() {
    let Some(gpu) = gpu() else {
        return;
    };
    let mut hdr = float_hdr(&gpu, Output::Pq);
    let grey = |g: f64| {
        move |encoder: &mut wgpu::CommandEncoder, frame: &wgpu::Texture| {
            fill(encoder, frame, [g, g, g, 1.0]);
        }
    };
    // How far the picture's mean code is from `end`.
    let mean_from = |picture: &[[u32; 3]], end: f64| {
        let sum: f64 = picture.iter().flatten().map(|c| f64::from(*c)).sum();
        (sum / (picture.len() * 3) as f64 - end).abs()
    };
    // Within a code of an end the noise shrinks to the distance to it. A third of a code from
    // the end, the signal plus that noise passes half a code only where the noise is over half
    // its range, an eighth of a triangle: so the picture's mean is an eighth of a code from the
    // end, not a third. That lean to the end, in the last code alone, is the price of keeping
    // the end itself exact. Bound: a GPU that rounds a tenth of a code off the half (as above)
    // shows between 0.02 and 0.32 of a code, always short of the third.
    let leans = |mean: f64| (0.015..0.325).contains(&mean);

    // Black gets no noise: every pixel stays off.
    let black = codes(&shown(&gpu, &mut hdr, false, grey(0.0)));
    assert!(black.iter().all(|p| *p == [0; 3]));
    // A third of a code above black (this grey is a half float): 0 or 1, never lifted past it.
    let dim = 0.00038433074951171875;
    let meant = code(200.0 * f64::powf(dim, 2.2));
    assert!((meant - 1.0 / 3.0).abs() < 0.001, "{meant}");
    let near_black = codes(&shown(&gpu, &mut hdr, false, grey(dim)));
    assert!(near_black.iter().flatten().all(|c| *c <= 1));
    let mean = mean_from(&near_black, 0.0);
    eprintln!("HDR10 dither GPU: a third of a code above black shows as {mean:.3}");
    assert!(leans(mean), "{mean} above black");

    // The top of the range, PQ's 10 000 nits, stays the top: nothing wraps or dims. Light past
    // the peak is held at it, so the Peak setting is the signal here.
    hdr.peak = 10_000.0;
    let top = codes(&shown(&gpu, &mut hdr, false, grey(20.0)));
    assert!(top.iter().all(|p| *p == [1023; 3]));
    // A third of a code under the top, mirrored: 1022 or 1023.
    let (mut under, mut over) = (9000.0, 10_000.0);
    for _ in 0..60 {
        let middle = (under + over) / 2.0;
        if code(middle) < 1023.0 - 1.0 / 3.0 {
            under = middle;
        } else {
            over = middle;
        }
    }
    hdr.peak = under as f32;
    let meant = code(f64::from(hdr.peak));
    assert!((1023.0 - meant - 1.0 / 3.0).abs() < 0.001, "{meant}");
    let near_top = codes(&shown(&gpu, &mut hdr, false, grey(20.0)));
    assert!(near_top.iter().flatten().all(|c| *c >= 1022));
    let mean = mean_from(&near_top, 1023.0);
    eprintln!("HDR10 dither GPU: a third of a code under the top shows as {mean:.3} under");
    assert!(leans(mean), "{mean} under the top");
}

#[test]
fn scrgb_and_screenshots_get_no_dither() {
    let Some(gpu) = gpu() else {
        return;
    };
    let ramp = |encoder: &mut wgpu::CommandEncoder, frame: &wgpu::Texture| {
        ramp(&gpu.0, encoder, frame);
    };
    let (mut hdr10, mut scrgb) = (float_hdr(&gpu, Output::Pq), float_hdr(&gpu, Output::ScRgb));
    // scRGB's swapchain is float: every row is one value from side to side.
    assert!(rows_are_flat(&shown(&gpu, &mut scrgb, false, ramp)));
    // A screenshot's HDR10 copy is the same bytes whichever swapchain is presented, the
    // dithered one included, and has no noise in it.
    let screenshot = shown(&gpu, &mut hdr10, true, ramp);
    assert!(rows_are_flat(&screenshot));
    assert_eq!(screenshot, shown(&gpu, &mut scrgb, true, ramp));
    // The swapchain's own picture differs from it: the dither is on there.
    assert_ne!(screenshot, shown(&gpu, &mut hdr10, false, ramp));
}
