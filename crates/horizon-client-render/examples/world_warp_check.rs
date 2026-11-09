//! A world warp (`native_world_warp`, docs/native-mods.md "World warps") through the real
//! `Renderer`, offscreen: a wall of terrain and an entity quad drawn without a warp, with one,
//! with one that is off, with one that cuts the wall, with a pixel stage of its own and with
//! one that doesn't compile. Run with throwaway HORIZON_HOME, HORIZON_LAUNCHER_HOME,
//! XDG_DATA_HOME and XDG_CONFIG_HOME (AGENTS.md):
//! `cargo run -p horizon-client-render --example world_warp_check -- <output-dir>`
//! `--release ... -- <output-dir> --bench`: instead, the time of a frame of rolling land 49 by 49
//! chunks around the camera without a warp, and with one (nothing culled), at 1920 by 1080.
use client::{
    gfx::{Frame, Renderer, ScreenshotRequest, native_render},
    graphics::GraphicsApi,
    hud::HudBatch,
    mesher::{SectionMesh, TerrainVertex, Vertex},
};
use horizon_client_render as client;
use std::{
    path::Path,
    sync::{Arc, Mutex},
};
use winit::{
    application::ApplicationHandler,
    event::WindowEvent,
    event_loop::{ActiveEventLoop, EventLoop},
    window::{Window, WindowId},
};

/// What the mod below gives the renderer each frame.
static WARP: Mutex<Option<native_render::WorldWarp>> = Mutex::new(None);
struct Mod;
impl native_render::RenderHooks for Mod {
    fn world_warp(&self, event: &mut native_render::WorldWarpEvent<'_>) {
        if let Some(warp) = WARP.lock().unwrap().as_ref() {
            *event.warp = warp.clone();
        }
    }
}

/// Lifts what is left of x = 8 by `params[0].x` and tints it red.
const LIFT: &str = "fn world_warp(pos: vec3<f32>, section: vec3<f32>) -> Warped {
    let here = pos.x + warp.camera.x + warp.camera_rest.x;
    if here < 8.0 { return Warped(pos + vec3(0.0, warp.params[0].x, 0.0), vec3(1.0, 0.0, 0.0)); }
    return Warped(pos, vec3(1.0));
}
";
/// Cuts every face.
const CUT: &str = "fn world_warp(pos: vec3<f32>, section: vec3<f32>) -> Warped {
    return Warped(pos, vec3(-1.0));
}
";
/// Paints the terrain green by its pixels and moves nothing.
const PAINT: &str = "fn world_warp(pos: vec3<f32>, section: vec3<f32>) -> Warped {
    return Warped(pos, vec3(1.0));
}
// [fragments]
fn world_fragment(f: Fragment) -> Painted {
    if !f.front { return Painted(vec4(0.0, 0.0, 0.0, -1.0), 1.0); }
    return Painted(vec4(0.0, 1.0, 0.0, 1.0), 0.0);
}
";
/// Moves entities ten blocks right: off the screen's middle.
const SHIFT: &str = "fn world_warp(pos: vec3<f32>, section: vec3<f32>) -> Warped {
    return Warped(pos + vec3(-10.0, 0.0, 0.0), vec3(1.0));
}
";

fn read(path: &Path) -> Vec<[u8; 4]> {
    let decoder = png::Decoder::new(std::io::BufReader::new(std::fs::File::open(path).unwrap()));
    let mut reader = decoder.read_info().unwrap();
    let mut bytes = vec![0; reader.output_buffer_size().unwrap()];
    let info = reader.next_frame(&mut bytes).unwrap();
    assert_eq!(info.color_type, png::ColorType::Rgba);
    bytes.truncate(info.buffer_size());
    bytes.as_chunks::<4>().0.to_vec()
}

/// Pixels like `want` (within 40 a channel), of an image.
fn count(pixels: &[[u8; 4]], want: [u8; 3]) -> usize {
    pixels
        .iter()
        .filter(|p| (0..3).all(|i| p[i].abs_diff(want[i]) < 40))
        .count()
}

/// A wall across x 0..16, y 0..16 at z = 12 of the section at y 96, both windings.
fn wall() -> SectionMesh {
    let mut mesh = SectionMesh::default();
    let corners = [
        [0., 0., 12.],
        [0., 16., 12.],
        [16., 16., 12.],
        [16., 0., 12.],
    ];
    let uvs = [[0., 1.], [0., 0.], [1., 0.], [1., 1.]];
    for order in [[0, 1, 2, 3], [3, 2, 1, 0]] {
        for i in order {
            mesh.solid
                .push(TerrainVertex::new(corners[i], uvs[i], 0, u32::MAX, 0xf0f0));
        }
    }
    mesh.sort_solid();
    mesh
}

fn fixture(window: Arc<Window>) -> Result<(), Box<dyn std::error::Error>> {
    let out = std::env::args().nth(1).ok_or("output directory required")?;
    let out = Path::new(&out);
    std::fs::create_dir_all(out)?;
    native_render::install(Box::new([&Mod])).map_err(|_| "mod already installed")?;
    let white = [255; 16 * 16 * 4];
    let zero = [0; 4];
    let mut renderer = pollster::block_on(Renderer::new(
        window,
        true,
        &white,
        16,
        16,
        1,
        &[],
        &[],
        (&white, 16, 16),
        (&zero, 1, 1),
        (&zero, 1, 1),
        (&zero, 1),
        Arc::new(client::gfx::panorama::Faces::colour([0, 0, 0, 255])),
        None,
        None,
        GraphicsApi::Vulkan,
        false,
        false,
        true,
        None,
        false,
    ))?;
    renderer.resize(320, 180);
    renderer.fullbright = true;
    // (keys count sections from the bottom of the world, y -64: 10 is y 96 to 112)
    renderer.upload((0, 10, 0), &wall());
    // an entity quad in front of the wall, middle of the screen
    let entity: Vec<Vertex> = [
        [7.5, 103.5, 6.0],
        [7.5, 104.5, 6.0],
        [8.5, 104.5, 6.0],
        [8.5, 103.5, 6.0],
    ]
    .into_iter()
    .zip([[0., 1.], [0., 0.], [1., 0.], [1., 1.]])
    .map(|(p, uv)| Vertex {
        pos: [p[0] - 8.0, p[1] - 104.0, p[2] - 0.5],
        uv,
        layer: 0,
        color: 0xff40_40ff,
        light: 0xf0,
        normal: [0, 0, 0, 0],
        glint_uv: [0.0; 2],
    })
    .collect();
    if std::env::args().any(|a| a == "--bench") {
        return bench(&mut renderer, out);
    }
    let hud = HudBatch::new([320, 180]);
    let mut frame = fixture_frame(&hud);
    frame.entities = &entity;
    let mut shot = |name: &str, warp: Option<native_render::WorldWarp>| {
        *WARP.lock().unwrap() = warp;
        let path = out.join(format!("{name}.png"));
        renderer.screenshot = Some(ScreenshotRequest {
            path: path.clone(),
            size: None,
            scaled: false,
        });
        // twice: the first frame of a new warp builds it
        renderer.render(&frame).unwrap();
        renderer.render(&frame).unwrap();
        read(&path)
    };
    let warp = |source: &str, on: bool, lift: f32| {
        let mut w = native_render::WorldWarp::default();
        w.source = Some(source.into());
        w.on = on;
        w.params[0] = lift;
        Some(w)
    };
    let plain = shot("plain", None);
    let wall_px = count(&plain, [255, 255, 255]);
    let entity_px = count(&plain, [255, 64, 64]);
    assert!(wall_px > 1000, "the wall shows without a warp: {wall_px}");
    assert!(
        entity_px > 50,
        "the entity shows without a warp: {entity_px}"
    );

    let off = shot("off", warp(LIFT, false, 4.0));
    assert!(off == plain, "a warp that is off draws as no warp");

    let lifted = shot("lift", warp(LIFT, true, 4.0));
    let red = count(&lifted, [255, 0, 0]);
    assert!(red > 500, "the warp tints the wall's left half red: {red}");
    assert!(lifted != plain);

    let cut = shot("cut", warp(CUT, true, 0.0));
    let left = count(&cut, [255, 255, 255]);
    assert!(
        left < wall_px / 20,
        "a cut leaves the wall out: {left} of {wall_px}"
    );

    let painted = shot("paint", warp(PAINT, true, 0.0));
    let green = count(&painted, [0, 255, 0]);
    assert!(
        green > wall_px / 2,
        "the warp's own pixels paint the wall: {green}"
    );

    let shifted = shot("shift", warp(SHIFT, true, 0.0));
    let entity_left = count(&shifted[..], [255, 64, 64]);
    let mid = |p: &[[u8; 4]]| -> Vec<[u8; 4]> {
        (60..120)
            .flat_map(|y| p[y * 320 + 130..y * 320 + 190].to_vec())
            .collect()
    };
    assert!(
        count(&mid(&shifted), [255, 64, 64]) < count(&mid(&plain), [255, 64, 64]) / 4,
        "the warp moves the entity out of the middle ({entity_left} red-ish pixels left)"
    );

    let broken = shot(
        "broken",
        warp("fn world_warp() -> f32 { return 1; }", true, 0.0),
    );
    assert!(broken == plain, "a warp that doesn't compile is left out");

    let back = shot("after", None);
    assert!(back == plain, "with the warp gone the frame is as before");
    std::fs::write(
        out.join("result.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "wall_pixels": wall_px, "entity_pixels": entity_px, "lifted_red_pixels": red,
            "cut_wall_pixels_left": left, "painted_green_pixels": green,
            "off_equals_plain": true, "broken_equals_plain": true, "after_equals_plain": true,
            "graphics_adapter": client::crash::system_details("fixture").into_iter()
                .find(|(name, _)| name == "Graphics Adapter").map(|(_, detail)| detail),
        }))?,
    )?;
    println!(
        "world warp check passed: {}",
        out.join("result.json").display()
    );
    Ok(())
}

/// Rolling land: a column of a section's top faces and the sides they show, at y 64 to 80.
fn hills(cx: i32, cz: i32) -> SectionMesh {
    let height = |x: i32, z: i32| {
        let (x, z) = (f64::from(cx * 16 + x), f64::from(cz * 16 + z));
        (8.0 + 4.0 * (x * 0.09).sin() + 3.0 * (z * 0.07 + x * 0.02).cos()).floor() as i32
    };
    let mut mesh = SectionMesh::default();
    let mut quad = |c: [[f32; 3]; 4]| {
        for (p, uv) in c.into_iter().zip([[0., 1.], [0., 0.], [1., 0.], [1., 1.]]) {
            mesh.solid
                .push(TerrainVertex::new(p, uv, 0, 0xff60_a060, 0xf0));
        }
    };
    for x in 0..16 {
        for z in 0..16 {
            let h = height(x, z);
            let (fx, fz, fh) = (x as f32, z as f32, h as f32);
            quad([
                [fx, fh, fz],
                [fx, fh, fz + 1.],
                [fx + 1., fh, fz + 1.],
                [fx + 1., fh, fz],
            ]);
            // the step down to a lower neighbour, toward +x and +z
            for (dx, dz) in [(1, 0), (0, 1)] {
                let n = height(x + dx, z + dz);
                if n < h {
                    let (lo, hi) = (n as f32, fh);
                    if dx == 1 {
                        quad([
                            [fx + 1., lo, fz],
                            [fx + 1., hi, fz],
                            [fx + 1., hi, fz + 1.],
                            [fx + 1., lo, fz + 1.],
                        ]);
                    } else {
                        quad([
                            [fx, lo, fz + 1.],
                            [fx + 1., lo, fz + 1.],
                            [fx + 1., hi, fz + 1.],
                            [fx, hi, fz + 1.],
                        ]);
                    }
                }
            }
        }
    }
    mesh.sort_solid();
    mesh
}

/// `--bench`: milliseconds a frame without a warp and with one.
fn bench(renderer: &mut Renderer, out: &Path) -> Result<(), Box<dyn std::error::Error>> {
    const R: i32 = 24;
    renderer.resize(1920, 1080);
    let mut sections = 0;
    for cx in -R..=R {
        for cz in -R..=R {
            // (keys count sections from y -64: 8 is y 64 to 80)
            renderer.upload((cx, 8, cz), &hills(cx, cz));
            sections += 1;
        }
    }
    let hud = HudBatch::new([1920, 1080]);
    let mut frame = fixture_frame(&hud);
    frame.camera = glam::DVec3::new(8.0, 90.0, 8.0);
    frame.eye = frame.camera;
    frame.pitch = 15.0;
    frame.eye_rotation = [0.0, 15.0];
    frame.distance = 16.0 * R as f32;
    frame.detail_distance = frame.distance;
    frame.depth_far = 2000.0;
    frame.cave_culling = true;
    let mut time = |warp: Option<native_render::WorldWarp>| {
        *WARP.lock().unwrap() = warp;
        for _ in 0..30 {
            renderer.render(&frame).unwrap();
        }
        let started = std::time::Instant::now();
        const FRAMES: u32 = 240;
        for _ in 0..FRAMES {
            renderer.render(&frame).unwrap();
        }
        // the last frame's picture, so the GPU has caught up
        renderer.screenshot = Some(ScreenshotRequest {
            path: out.join("bench.png"),
            size: None,
            scaled: false,
        });
        renderer.render(&frame).unwrap();
        renderer.render(&frame).unwrap();
        started.elapsed().as_secs_f64() * 1000.0 / f64::from(FRAMES + 2)
    };
    let mut wave = native_render::WorldWarp::default();
    wave.source = Some(
        "fn world_warp(pos: vec3<f32>, section: vec3<f32>) -> Warped {
            let x = pos.x + warp.camera.x + warp.camera_rest.x;
            return Warped(pos + vec3(0.0, sin(x * 0.05) * 3.0, 0.0), vec3(1.0));
        }\n"
        .into(),
    );
    wave.on = true;
    let plain = time(None);
    let warped = time(Some(wave));
    let again = time(None);
    let result = serde_json::json!({
        "sections": sections, "frame_ms_without_warp": plain, "frame_ms_with_warp": warped,
        "frame_ms_without_warp_after": again,
        "graphics_adapter": client::crash::system_details("bench").into_iter()
            .find(|(name, _)| name == "Graphics Adapter").map(|(_, detail)| detail),
    });
    println!("{result}");
    std::fs::write(out.join("bench.json"), serde_json::to_vec_pretty(&result)?)?;
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
        if let Err(error) = fixture(window) {
            eprintln!("world warp check failed: {error}");
            std::process::exit(1);
        }
        event_loop.exit();
    }
    fn window_event(&mut self, _: &ActiveEventLoop, _: WindowId, _: WindowEvent) {}
}
fn main() {
    for key in [
        "HORIZON_HOME",
        "HORIZON_LAUNCHER_HOME",
        "XDG_DATA_HOME",
        "XDG_CONFIG_HOME",
    ] {
        assert!(
            std::env::var_os(key).is_some(),
            "set throwaway {key} (AGENTS.md)"
        );
    }
    EventLoop::new().unwrap().run_app(&mut App).unwrap();
}

fn fixture_frame(hud: &HudBatch) -> Frame<'_> {
    Frame {
        weather: &[],
        weather_time: client::weather::Time::new(0, 0.0),
        camera: glam::DVec3::new(8.0, 104.0, 0.5),
        yaw: 0.0,
        pitch: 0.0,
        fov_degrees: 85.0,
        eye: glam::DVec3::new(8.0, 104.0, 0.5),
        eye_rotation: [0.0, 0.0],
        first_person: true,
        bob: glam::Mat4::IDENTITY,
        spin: glam::Mat4::IDENTITY,
        sky: client::environment::Sky::default(),
        shadow_scene_revision: (0, 0),
        shadow_cache: false,
        distance: 128.0,
        detail_distance: 128.0,
        depth_far: 1000.0,
        underwater: false,
        water_plane: None,
        cave_culling: false,
        title: false,
        sky_blocked: false,
        skybox: false,
        end_sky: false,
        end_sky_sprite: None,
        boss_world_fog: false,
        environment_fog: None,
        water_fog: [0.0; 3],
        fog: None,
        dark_disc: false,
        outline: Vec::new(),
        outline_after_terrain: false,
        outline_high_contrast: false,
        pvp: Default::default(),
        debug_lines: &[],
        debug_lines_on_top: &[],
        debug_crosshair: &[],
        hud,
        glint_alpha: 0.75,
        glint_ticks: 0.0,
        nether_lighting: false,
        entities: &[],
        static_block_entities: &[],
        visible_static_block_entity_sections: &[],
        static_block_entity_stats: Default::default(),
        entity_motion: &[],
        entity_motion_posed: &[],
        entity_spans: &[],
        skin: None,
        glowing: &[],
        dynamic_lights: &[],
        colored_lights: &[],
        emissive_layers: &[],
        clouds: None,
        lightning: Default::default(),
        additive: &[],
        see_through: &[],
        world_primitives: Default::default(),
        shadow_only: &[],
        celestial: &[],
        stars: None,
        hand: &[],
        hand_items: &[],
        hand_fov_degrees: 70.0,
        screen: &[],
        pack: Default::default(),
        border: None,
        game_time: 0.0,
        shader_game_time: 0.0,
        end_portal_sprites: [[0.0; 4]; 2],
        crumbling_sprite: [0.0; 4],
        ticks: 0,
        darkness: (0.0, 0.0),
        night_vision: 0.0,
        block_flicker: 0.0,
        boss_darken: 0.0,
        chunk_fade: 0.0,
        post_effects: client::post_chain::Request::none(),
        menu_blur: None,
    }
}
