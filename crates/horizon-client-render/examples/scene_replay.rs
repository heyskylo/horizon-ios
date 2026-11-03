//! Draws a scene `horizon-ctl render-capture DIR` wrote, offscreen, from its camera, with the
//! real `Renderer`, and prints GPU and CPU milliseconds per frame and Horizon's own passes'
//! GPU times as JSON (docs/graphics.md, "Render capture and replay").
//! `cargo run --release -p horizon-client-render --example scene_replay -- DIR [--frames 300]
//! [--warmup 60] [--yaw-offsets 0,90,180,270] [--texture-filtering 0|1|2] [--anisotropy-bit N]
//! [--mipmaps N] [--lod-distance CHUNKS] [--render-distance CHUNKS] [--size WxH]
//! [--section-draws flat|indirect] [--section-cull on|off] [--section-sides on|off]
//! [--gpu-culling on|off] [--root RESOURCES] [--screenshot PNG]`
//! `--screenshot` saves the first view's last frame, after the timed frames.
use horizon_client_render::{
    assets::Assets,
    chunk::ColumnLayout,
    fast_hash::FxHashSet,
    gfx::{
        PreparedTextures, Renderer, ScreenshotRequest,
        capture::{self, AtlasCheck, Scene},
        panorama::Faces,
    },
    hud::HudBatch,
    lod::TileKey,
    mesher::SectionMesh,
};
use serde_json::{Value, json};
use std::{
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant},
};
use winit::{
    application::ApplicationHandler,
    event::WindowEvent,
    event_loop::{ActiveEventLoop, EventLoop},
    window::{Window, WindowId},
};

#[derive(Default)]
struct Options {
    dir: PathBuf,
    frames: usize,
    warmup: usize,
    yaw_offsets: Vec<f32>,
    root: Option<PathBuf>,
    texture_filtering: Option<i32>,
    anisotropy_bit: Option<i32>,
    mipmaps: Option<u32>,
    lod_distance: Option<u32>,
    render_distance: Option<i32>,
    size: Option<[u32; 2]>,
    flat: Option<bool>,
    cull: Option<bool>,
    sides: Option<bool>,
    gpu_culling: Option<bool>,
    screenshot: Option<PathBuf>,
}

fn on_off(value: &str) -> Result<bool, String> {
    match value {
        "on" | "flat" => Ok(true),
        "off" | "indirect" => Ok(false),
        other => Err(format!("expected on or off, got {other}")),
    }
}

fn options() -> Result<Options, String> {
    let mut o = Options {
        frames: 300,
        warmup: 60,
        yaw_offsets: vec![0.0],
        ..Default::default()
    };
    let mut args = std::env::args().skip(1);
    let mut dir = None;
    while let Some(arg) = args.next() {
        if !arg.starts_with("--") {
            dir = Some(PathBuf::from(arg));
            continue;
        }
        let value = args.next().ok_or(format!("{arg} needs a value"))?;
        let number = |v: &str| v.parse::<i64>().map_err(|e| format!("{arg}: {e}"));
        match arg.as_str() {
            "--frames" => o.frames = number(&value)?.max(1) as usize,
            "--warmup" => o.warmup = number(&value)?.max(0) as usize,
            "--yaw-offsets" => {
                o.yaw_offsets = value
                    .split(',')
                    .map(|v| v.trim().parse::<f32>().map_err(|e| format!("{arg}: {e}")))
                    .collect::<Result<_, _>>()?
            }
            "--root" => o.root = Some(value.into()),
            "--texture-filtering" => o.texture_filtering = Some(number(&value)? as i32),
            "--anisotropy-bit" => o.anisotropy_bit = Some(number(&value)? as i32),
            "--mipmaps" => o.mipmaps = Some(number(&value)?.max(0) as u32),
            "--lod-distance" => o.lod_distance = Some(number(&value)?.max(0) as u32),
            "--render-distance" => o.render_distance = Some(number(&value)?.max(0) as i32),
            "--size" => {
                let (w, h) = value.split_once('x').ok_or("--size takes WxH")?;
                o.size = Some([number(w)? as u32, number(h)? as u32]);
            }
            "--section-draws" => o.flat = Some(on_off(&value)?),
            "--section-cull" => o.cull = Some(on_off(&value)?),
            "--section-sides" => o.sides = Some(on_off(&value)?),
            "--gpu-culling" => o.gpu_culling = Some(on_off(&value)?),
            "--screenshot" => o.screenshot = Some(value.into()),
            other => return Err(format!("unknown option {other}")),
        }
    }
    o.dir = dir.ok_or("the capture directory is required")?;
    Ok(o)
}

/// Milliseconds: average, median, 95th percentile and slowest, over `samples`.
fn summary(samples: &[f32]) -> Value {
    if samples.is_empty() {
        return Value::Null;
    }
    let mut sorted = samples.to_vec();
    sorted.sort_by(f32::total_cmp);
    let at = |q: f32| sorted[((sorted.len() - 1) as f32 * q).round() as usize];
    json!({
        "avg": samples.iter().sum::<f32>() / samples.len() as f32,
        "p50": at(0.5),
        "p95": at(0.95),
        "max": sorted[sorted.len() - 1],
        "samples": samples.len(),
    })
}

fn ms(d: Duration) -> f32 {
    d.as_secs_f32() * 1000.0
}

fn replay(window: Arc<Window>) -> Result<(), Box<dyn std::error::Error>> {
    let o = options()?;
    let scene = Scene::read(&o.dir)?;
    let settings = &scene.settings;
    let root = o.root.clone().unwrap_or_else(|| settings.resources.clone());
    let loading = Instant::now();
    let assets = Assets::load(&root)?;
    let render = assets.reload(&root)?;
    if AtlasCheck::of(&render) != settings.atlas {
        return Err(format!(
            "the block sprites at {} are not the ones the capture was made with (resource packs \
             or mods?): pass --root with the game's resource root",
            root.display()
        )
        .into());
    }
    let one = [255u8; 4];
    let zero = [0u8; 4];
    let prepared = PreparedTextures::new(
        &render.pixels,
        render.sprite_size,
        render.texture_layers,
        &render.sprite_mips,
        (&zero[..], 1),
    )
    .with_pbr(
        &render.pbr_normals,
        &render.pbr_specular,
        render.pbr_size,
        render.texture_layers,
    );
    let mut renderer = pollster::block_on(Renderer::new(
        window,
        true,
        &render.pixels,
        render.sprite_size,
        render.sprite_minimum_size,
        render.texture_layers,
        &render.sprite_mips,
        &render.animations,
        (&one[..], 1, 1),
        (&zero[..], 1, 1),
        (&zero[..], 1, 1),
        (&zero[..], 1),
        Arc::new(Faces::colour([0, 0, 0, 255])),
        Some(prepared),
        None,
        settings.graphics_api(),
        settings.translucent_batching,
        false,
    ))?;
    let state = &scene.renderer;
    let [width, height] = o.size.unwrap_or(state.size);
    renderer.resize(width, height);
    renderer.gamma = state.gamma;
    renderer.fullbright = state.fullbright;
    renderer.improved_transparency = state.improved_transparency;
    renderer.animated_textures = state.animated_textures;
    let filtering = o.texture_filtering.unwrap_or(settings.texture_filtering);
    let anisotropy = o.anisotropy_bit.unwrap_or(settings.anisotropy_bit);
    renderer.set_texture_filtering(filtering, anisotropy);
    renderer.set_mipmap_levels(o.mipmaps.unwrap_or(settings.mipmap_levels));
    renderer.set_world_post(
        settings.resolution_scale,
        settings.fxaa,
        settings.taa,
        settings.upscale,
    );
    let mut notes = Vec::new();
    let mut note = |what: &str, result: horizon_client_render::Result<()>| {
        if let Err(e) = result {
            notes.push(format!("{what}: {e}"));
        }
    };
    note(
        "section-draws",
        renderer.set_flat_draws(o.flat.unwrap_or(state.flat_draws)),
    );
    note(
        "section-cull",
        renderer.set_section_cull(o.cull.unwrap_or(state.section_cull)),
    );
    note(
        "gpu-culling",
        renderer.set_gpu_culling(o.gpu_culling.unwrap_or(state.gpu_culling)),
    );
    renderer.set_section_sides(o.sides.unwrap_or(state.section_sides));
    renderer.set_pass_timing(true);
    let layout = ColumnLayout::of_level(state.min_y, state.sections as i32 * 16);
    if layout != ColumnLayout::VANILLA {
        renderer.clear_sections(layout);
    }

    // Near sections first: with no far tile resident yet, none fades in.
    let camera = scene.frame.camera;
    let column = |x: f64| (x.floor() as i32) >> 4;
    let (cx, cz) = (column(camera[0]), column(camera[2]));
    let mut sections = 0;
    capture::read_meshes(&o.dir, "sections", |[x, y, z], mesh| {
        let near = o
            .render_distance
            .is_none_or(|r| (x - cx).pow(2) + (z - cz).pow(2) <= r * r);
        if near {
            renderer.upload((x, y, z), &mesh);
            sections += 1;
        }
    })?;
    let mut tiles: Vec<(TileKey, SectionMesh)> = Vec::new();
    capture::read_meshes(&o.dir, "tiles", |[x, z, level], mesh| {
        let key = TileKey {
            x,
            z,
            level: level as u8,
        };
        let [ox, _, oz] = key.origin();
        let half = f64::from(key.size()) / 2.0;
        let (dx, dz) = (
            f64::from(ox) + half - camera[0],
            f64::from(oz) + half - camera[2],
        );
        let near = o
            .lod_distance
            .is_none_or(|d| (dx * dx + dz * dz).sqrt() <= f64::from(d) * 16.0);
        if near {
            tiles.push((key, mesh));
        }
    })?;
    let tile_count = tiles.len();
    if !tiles.is_empty() {
        let cutout: FxHashSet<(i32, i32)> = scene.lod_cutout.iter().copied().collect();
        renderer.queue_lods(tiles, Vec::new(), Some(cutout));
        renderer.upload_lods(Duration::from_secs(3600));
    }
    let loaded_ms = ms(loading.elapsed());

    let hud = HudBatch::new([width, height]);
    let mut views = Vec::new();
    for &offset in &o.yaw_offsets {
        let mut frame = scene.frame(&hud);
        frame.yaw += offset;
        frame.eye_rotation[0] += offset;
        if let Some(r) = o.render_distance {
            frame.distance = (r * 16) as f32;
            frame.detail_distance = frame.distance;
        }
        let draw = |renderer: &mut Renderer| -> horizon_client_render::Result<Duration> {
            if frame.cave_culling {
                renderer.start_occlusion(frame.camera, frame.distance);
            }
            let started = Instant::now();
            renderer.render(&frame)?;
            Ok(started.elapsed())
        };
        for _ in 0..o.warmup {
            draw(&mut renderer)?;
        }
        renderer.gpu_times.clear();
        let (mut call, mut encode, mut gpu) = (Vec::new(), Vec::new(), Vec::new());
        for _ in 0..o.frames {
            call.push(ms(draw(&mut renderer)?));
            encode.push(ms(renderer.last_encode));
            gpu.append(&mut renderer.gpu_times);
        }
        // Timestamps arrive a few frames late: a few more of the same frame bring them in.
        for _ in 0..16 {
            if gpu.len() >= o.frames {
                break;
            }
            draw(&mut renderer)?;
            gpu.append(&mut renderer.gpu_times);
        }
        gpu.truncate(o.frames);
        if let Some(path) = o.screenshot.as_ref().filter(|_| views.is_empty()) {
            renderer.screenshot_result = None;
            renderer.screenshot = Some(ScreenshotRequest {
                path: path.clone(),
                size: None,
                scaled: false,
            });
            for _ in 0..32 {
                draw(&mut renderer)?;
                if renderer.screenshot_result.is_some() {
                    break;
                }
            }
            match renderer.screenshot_result.take() {
                Some(Ok(_)) => {}
                Some(Err(e)) => notes.push(format!("screenshot: {e}")),
                None => notes.push("screenshot: not saved".into()),
            }
        }
        views.push(json!({
            "yaw_offset": offset,
            "gpu_ms": summary(&gpu),
            "render_call_ms": summary(&call),
            "cpu_encode_ms": summary(&encode),
            "gpu_passes_median_ms": renderer
                .own_pass_times()
                .into_iter()
                .map(|(name, ms)| (name, json!(ms)))
                .collect::<serde_json::Map<_, _>>(),
            "drawn_sections": renderer.drawn_sections,
            "section_draws": renderer.section_draws,
            "section_draw_calls": renderer.section_draw_calls,
        }));
    }
    let section_draws = if renderer.flat_draws() {
        "flat"
    } else {
        "indirect"
    };
    let report = json!({
        "capture": o.dir.display().to_string(),
        "adapter": renderer.info.name,
        "captured_on": state.adapter,
        "size": [width, height],
        "sections": sections,
        "tiles": tile_count,
        "load_ms": loaded_ms,
        "frames": o.frames,
        "warmup": o.warmup,
        "settings": {
            "texture_filtering": filtering,
            "anisotropy_bit": anisotropy,
            "mipmaps": o.mipmaps.unwrap_or(settings.mipmap_levels),
            "lod_distance": o.lod_distance,
            "render_distance": o.render_distance,
            "section_draws": section_draws,
            "section_cull": renderer.section_cull(),
            "section_sides": renderer.section_sides(),
            "gpu_culling": renderer.gpu_culling(),
            "shader_pack_left_out": state.shader_pack,
        },
        "notes": notes,
        "views": views,
    });
    println!("{}", serde_json::to_string_pretty(&report)?);
    Ok(())
}

struct App;
impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        let window = Arc::new(
            event_loop
                .create_window(Window::default_attributes().with_visible(false))
                .unwrap(),
        );
        if let Err(error) = replay(window) {
            eprintln!("scene_replay: {error}");
            std::process::exit(1);
        }
        event_loop.exit();
    }
    fn window_event(&mut self, _: &ActiveEventLoop, _: WindowId, _: WindowEvent) {}
}

fn main() {
    EventLoop::new().unwrap().run_app(&mut App).unwrap();
}
