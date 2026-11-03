//! Draws a scene `horizon-ctl render-capture DIR` wrote, offscreen, from its camera, with the
//! real `Renderer`, and prints GPU and CPU milliseconds per frame and Horizon's own passes'
//! GPU times as JSON (docs/graphics.md, "Render capture and replay").
//! `cargo run --release -p horizon-client-render --example scene_replay -- DIR [--frames 300]
//! [--warmup 60] [--yaw-offsets 0,90,180,270] [--texture-filtering 0|1|2] [--anisotropy-bit N]
//! [--mipmaps N] [--lod-distance CHUNKS] [--render-distance CHUNKS] [--size WxH]
//! [--section-draws flat|indirect] [--section-cull on|off] [--section-sides on|off]
//! [--gpu-culling on|off] [--root RESOURCES] [--screenshot PNG]
//! [--world SAVE --leaf-culling MODE]`
//! `--screenshot` saves the first view's last frame, after the timed frames.
//! `--world` meshes the captured sections again from that save's region files
//! (blocks, biomes and saved light; `ao`, `biomeBlendRadius` and `cutoutLeaves`
//! from `./options.txt`) with `--leaf-culling` (default `off`), so a mesher
//! change can be measured on the captured view. A section whose 3x3 columns are
//! not all saved keeps its captured mesh.
use horizon_client_render::{
    assets::{Assets, RenderAssets},
    biome::Biomes,
    blocks::BlockFacts,
    chunk::{BIOME_STRATEGY, Chunk, ColumnLayout, Light, block_strategy},
    fast_hash::{FxHashMap, FxHashSet},
    gfx::{
        PreparedTextures, Renderer, ScreenshotRequest,
        capture::{self, AtlasCheck, Scene},
        panorama::Faces,
    },
    graphics::LeafCullingMode,
    hud::HudBatch,
    lod::TileKey,
    mesher::{MeshScratch, MeshTables, Mesher, SectionMesh},
    models,
};
use horizon_core::{
    biome::{BiomeZoom, obfuscate_seed},
    paletted::PalettedContainer,
};
use horizon_world::{
    chunk::BlockState,
    nbt::{Limits, NamedTag, Tag},
    region::RegionFile,
    registry::BlockRegistry,
};
use serde_json::{Value, json};
use std::{
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
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
    world: Option<PathBuf>,
    leaf_culling: LeafCullingMode,
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
            "--world" => o.world = Some(value.into()),
            "--leaf-culling" => {
                o.leaf_culling = LeafCullingMode::ALL
                    .into_iter()
                    .find(|mode| mode.name() == value)
                    .ok_or(format!("no leaf culling mode {value}"))?;
            }
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
        "min": sorted[0],
        "p10": at(0.1),
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
    let near = |x: i32, z: i32| {
        o.render_distance
            .is_none_or(|r| (x - cx).pow(2) + (z - cz).pow(2) <= r * r)
    };
    let (mut remeshed, mut world_report) = (FxHashMap::default(), Value::Null);
    if let Some(world) = &o.world {
        let keys: Vec<[i32; 3]> = section_keys(&o.dir)?
            .into_iter()
            .filter(|&[x, _, z]| near(x, z))
            .collect();
        let started = Instant::now();
        remeshed = remesh(world, &root, &assets, &render, o.leaf_culling, &keys)?;
        world_report = json!({
            "save": world.display().to_string(),
            "leaf_culling": o.leaf_culling.name(),
            "remeshed": remeshed.len(),
            "kept": keys.len() - remeshed.len(),
            "remesh_ms": ms(started.elapsed()),
        });
    }
    let leaf_layers = leaf_layers(&render);
    let (mut sections, mut quads, mut leaf_quads) = (0, 0usize, 0usize);
    capture::read_meshes(&o.dir, "sections", |[x, y, z], captured| {
        if near(x, z) {
            let mesh = remeshed.remove(&[x, y, z]).unwrap_or(captured);
            quads += mesh.solid.len() / 4;
            leaf_quads += mesh
                .solid
                .iter()
                .filter(|v| leaf_layers.contains(&v.layer))
                .count()
                / 4;
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
            // Occlusion culling's last result: boxes hidden, boxes tested, share of solid
            // indices hidden.
            "culled": renderer.culled_sections(),
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
        "solid_quads": quads,
        "leaf_quads": leaf_quads,
        "world": world_report,
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

/// The captured sections' keys, from the index alone.
fn section_keys(dir: &Path) -> Result<Vec<[i32; 3]>, Box<dyn std::error::Error>> {
    #[derive(serde::Deserialize)]
    struct Key {
        key: [i32; 3],
    }
    let records: Vec<Key> = serde_json::from_reader(std::io::BufReader::new(std::fs::File::open(
        dir.join("sections.json"),
    )?))?;
    Ok(records.into_iter().map(|r| r.key).collect())
}

/// Sprite layers of every leaves block, as terrain vertices carry them (with or
/// without `OPAQUE_LAYER`).
fn leaf_layers(render: &RenderAssets) -> FxHashSet<u16> {
    render
        .sprite_layers
        .iter()
        .filter(|(name, _)| name.ends_with("_leaves"))
        .flat_map(|(_, &layer)| {
            [
                layer as u16,
                layer as u16 | horizon_client_render::mesher::OPAQUE_LAYER,
            ]
        })
        .collect()
}

/// `--world`: the sections `keys` meshed again from `save`'s overworld region
/// files, on every core, as the game's workers mesh them.
fn remesh(
    save: &Path,
    root: &Path,
    assets: &Assets,
    render: &RenderAssets,
    leaf_culling: LeafCullingMode,
    keys: &[[i32; 3]],
) -> Result<FxHashMap<[i32; 3], SectionMesh>, Box<dyn std::error::Error>> {
    let facts = BlockFacts::load(root, assets.registry)?;
    let visibility = models::load_visibility(root, assets.registry)?;
    let tables = MeshTables::new(assets, render, &facts, visibility)?;
    let biomes = Biomes::load(root)?;
    let seed = world_seed(save).unwrap_or_else(|| {
        eprintln!(
            "scene_replay: no seed in {}, biome blending uses 0",
            save.display()
        );
        0
    });
    let options = std::fs::read_to_string("options.txt").unwrap_or_default();
    let option = |key: &str| {
        options
            .lines()
            .find_map(|l| l.strip_prefix(key)?.strip_prefix(':').map(str::to_owned))
    };
    let smooth_lighting = option("ao").is_none_or(|v| v == "true");
    let cutout_leaves = option("cutoutLeaves").is_none_or(|v| v == "true");
    let blend_radius = option("biomeBlendRadius")
        .and_then(|v| v.parse().ok())
        .unwrap_or(2);

    let mut columns: Vec<(i32, i32)> = keys
        .iter()
        .flat_map(|&[x, _, z]| {
            (-1..=1).flat_map(move |dz| (-1..=1).map(move |dx| (x + dx, z + dz)))
        })
        .collect();
    columns.sort_unstable();
    columns.dedup();
    let threads = std::thread::available_parallelism().map_or(4, |n| n.get());
    let region_dir = save.join("dimensions/minecraft/overworld/region");
    let next = AtomicUsize::new(0);
    let chunks: FxHashMap<(i32, i32), Chunk> = std::thread::scope(|scope| {
        let workers: Vec<_> = (0..threads)
            .map(|_| {
                scope.spawn(|| {
                    let mut files: FxHashMap<(i32, i32), Option<RegionFile>> = FxHashMap::default();
                    let mut out = Vec::new();
                    loop {
                        let i = next.fetch_add(1, Ordering::Relaxed);
                        let Some(&(x, z)) = columns.get(i) else {
                            break;
                        };
                        let file = files.entry((x >> 5, z >> 5)).or_insert_with(|| {
                            RegionFile::open(region_dir.join(format!(
                                "r.{}.{}.mca",
                                x >> 5,
                                z >> 5
                            )))
                            .ok()
                        });
                        let root = file
                            .as_mut()
                            .and_then(|f| {
                                f.read(horizon_core::position::ChunkPos::new(x, z))
                                    .ok()
                                    .flatten()
                            })
                            .and_then(|record| record.nbt(Limits::default()).ok());
                        if let Some(chunk) =
                            root.and_then(|root| decode(&root, assets.registry, &biomes))
                        {
                            out.push(((x, z), chunk));
                        }
                    }
                    out
                })
            })
            .collect();
        workers
            .into_iter()
            .flat_map(|w| w.join().unwrap())
            .collect()
    });

    let meshable: Vec<[i32; 3]> = keys
        .iter()
        .copied()
        .filter(|&[x, _, z]| {
            (-1..=1).all(|dz| (-1..=1).all(|dx| chunks.contains_key(&(x + dx, z + dz))))
        })
        .collect();
    let next = AtomicUsize::new(0);
    let meshes = std::thread::scope(|scope| {
        let workers: Vec<_> = (0..threads)
            .map(|_| {
                scope.spawn(|| {
                    let mut mesher = Mesher {
                        nether_shading: false,
                        shader_pack_lighting: false,
                        separate_ao: false,
                        assets,
                        render,
                        tables: &tables,
                        biomes: &biomes,
                        visibility: tables.visibility.clone(),
                        zoom: BiomeZoom::new(obfuscate_seed(seed)),
                        blend_radius,
                        smooth_lighting,
                        cutout_leaves,
                        translucent_layers: &[],
                        leaf_culling,
                    };
                    let mut scratch = MeshScratch::default();
                    let mut out = Vec::new();
                    loop {
                        let i = next.fetch_add(1, Ordering::Relaxed);
                        let Some(&[x, y, z]) = meshable.get(i) else {
                            break;
                        };
                        let region: [[Option<&Chunk>; 3]; 3] = std::array::from_fn(|dz| {
                            std::array::from_fn(|dx| {
                                chunks.get(&(x + dx as i32 - 1, z + dz as i32 - 1))
                            })
                        });
                        let mut mesh =
                            mesher.mesh_reusing(&region, y, SectionMesh::default(), &mut scratch);
                        mesh.sort_solid();
                        out.push(([x, y, z], mesh));
                    }
                    out
                })
            })
            .collect();
        workers
            .into_iter()
            .flat_map(|w| w.join().unwrap())
            .collect::<FxHashMap<_, _>>()
    });
    Ok(meshes)
}

/// The world seed: `data/minecraft/world_gen_settings.dat`, or level.dat's
/// `Data.WorldGenSettings` in older saves.
fn world_seed(save: &Path) -> Option<i64> {
    let read = |path: PathBuf| {
        horizon_world::datafix::files::read_dat(&path)
            .ok()
            .flatten()
    };
    let seed = |tag: Option<&Tag>| match tag?.get("seed")? {
        Tag::Long(seed) => Some(*seed),
        _ => None,
    };
    seed(
        read(save.join("data/minecraft/world_gen_settings.dat"))
            .as_ref()
            .and_then(|t| t.get("data")),
    )
    .or_else(|| {
        seed(
            read(save.join("level.dat"))?
                .get("Data")?
                .get("WorldGenSettings"),
        )
    })
}

/// A saved full chunk's blocks, biomes and light, as the client's own save
/// loader reads them (`save::decode_lit`, in horizon-client).
fn decode(root: &NamedTag, registry: &BlockRegistry, biomes: &Biomes) -> Option<Chunk> {
    let tag = &root.tag;
    let text = |t: Option<&Tag>| match t {
        Some(Tag::String(s)) => Some(s.to_string_lossy().to_string()),
        _ => None,
    };
    if text(tag.get("Status")).is_some_and(|s| s != "minecraft:full" && s != "full") {
        return None;
    }
    if tag.get("isLightOn") != Some(&Tag::Byte(1)) {
        return None;
    }
    let (Some(Tag::Int(x)), Some(Tag::Int(z))) = (tag.get("xPos"), tag.get("zPos")) else {
        return None;
    };
    let mut chunk = Chunk::with_layout(*x, *z, ColumnLayout::VANILLA);
    let air = registry.default_state("minecraft:air")?.0;
    let Some(Tag::List(sections)) = tag.get("sections") else {
        return Some(chunk);
    };
    let longs = |t: Option<&Tag>| match t {
        Some(Tag::LongArray(data)) => Some(data.iter().map(|&w| w as u64).collect::<Vec<_>>()),
        _ => None,
    };
    let mut layers = vec![(None, None); chunk.sections.len()];
    for section in sections {
        let Some(Tag::Byte(y)) = section.get("Y") else {
            continue;
        };
        let Some(si) = chunk.section_index(i32::from(*y) * 16) else {
            continue;
        };
        let target = Arc::make_mut(&mut chunk.sections[si]);
        if let Some(states) = section.get("block_states")
            && let Some(Tag::List(palette)) = states.get("palette")
        {
            let palette: Vec<u32> = palette
                .iter()
                .map(|p| {
                    BlockState::from_nbt(p)
                        .ok()
                        .and_then(|s| {
                            registry.id(&s).or_else(|| {
                                let default = registry.default_state(&s.name)?;
                                let mut full = registry.state(default)?.clone();
                                for (k, v) in &s.properties {
                                    if full.properties.contains_key(k) {
                                        full.properties.insert(k.clone(), v.clone());
                                    }
                                }
                                registry.id(&full).or(Some(default))
                            })
                        })
                        .map_or(air, |id| id.0)
                })
                .collect();
            let data = longs(states.get("data"));
            if let Ok(blocks) =
                PalettedContainer::from_packed(block_strategy(), palette, data.as_deref())
            {
                target.blocks = blocks;
            }
        }
        if let Some(b) = section.get("biomes")
            && let Some(Tag::List(palette)) = b.get("palette")
        {
            let palette: Vec<u32> = palette
                .iter()
                .map(|p| match p {
                    Tag::String(s) => u32::from(biomes.id(&s.to_string_lossy())),
                    _ => 0,
                })
                .collect();
            let data = longs(b.get("data"));
            if let Ok(container) =
                PalettedContainer::from_packed(BIOME_STRATEGY, palette, data.as_deref())
            {
                target.biomes = container;
            }
        }
        let layer = |name: &str| match section.get(name) {
            Some(Tag::ByteArray(a)) if a.len() == 2048 => Some(a.clone()),
            _ => None,
        };
        layers[si] = (layer("SkyLight"), layer("BlockLight"));
    }
    // `save::load_light`: a section without saved sky light takes the light
    // of the blocks above it, from the top of the column down.
    let mut above = [15u8; 256];
    for (si, (sky, block)) in layers.into_iter().enumerate().rev() {
        let mut data = Box::new([0u8; 4096]);
        for (i, v) in data.iter_mut().enumerate() {
            let nibble =
                |layer: &Option<Vec<u8>>| layer.as_ref().map(|l| l[i >> 1] >> ((i & 1) << 2) & 15);
            let s = nibble(&sky).unwrap_or(above[i & 255]);
            let b = nibble(&block).unwrap_or(0);
            *v = s << 4 | b;
        }
        if sky.is_some() {
            for (i, a) in above.iter_mut().enumerate() {
                *a = data[i] >> 4;
            }
        }
        let section = Arc::make_mut(&mut chunk.sections[si]);
        section.light = Light::Full(data);
        section.compact();
    }
    chunk.lit = true;
    chunk.light_loaded = true;
    Some(chunk)
}
