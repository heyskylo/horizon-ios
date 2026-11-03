//! `horizon-ctl render-capture`: what the renderer needs to draw the current frame again
//! without the game, for `examples/scene_replay.rs` (docs/graphics.md, "Render capture and
//! replay").
//!
//! The renderer keeps no CPU copy of the meshes it uploads, so while capturing is on
//! (`Renderer::keep_capture_copies`) it keeps one of every section and far tile mesh as it is
//! handed over, at the mesher-to-renderer seam. A capture writes those of the sections and
//! tiles it holds, the frame's camera and inputs, and the settings that change GPU work. Only
//! the build that wrote a capture reads it.
//!
//! Files: `scene.json` (`Scene`), `sections.json` and `tiles.json` (one `MeshRecord` per mesh,
//! in order), `sections.bin` and `tiles.bin` (each mesh's solid then translucent
//! `TerrainVertex`es), `celestial.bin` (the sun and moon `Vertex`es).
use super::{Frame, Renderer, SectionKey};
use crate::{
    Result,
    environment::Sky,
    fast_hash::{FxHashMap, FxHashSet},
    fog::FogData,
    graphics::GraphicsApi,
    hud::HudBatch,
    lod::TileKey,
    mesher::{SectionMesh, Sides, TerrainVertex, Vertex},
};
use glam::{DVec3, Mat4, Vec3};
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File},
    io::{BufReader, BufWriter, Read, Write},
    path::{Path, PathBuf},
};

/// CPU copies of the meshes uploaded since capturing was turned on.
#[derive(Default)]
pub(super) struct Copies {
    sections: FxHashMap<SectionKey, SectionMesh>,
    tiles: FxHashMap<TileKey, SectionMesh>,
}

impl Copies {
    pub(super) fn section(&mut self, key: SectionKey, mesh: &SectionMesh) {
        self.sections.insert(key, copy(mesh));
    }
    /// Columns unloaded (`Renderer::remove_columns`).
    pub(super) fn remove_columns(&mut self, columns: &[(i32, i32)]) {
        if columns.is_empty() {
            return;
        }
        let gone: FxHashSet<(i32, i32)> = columns.iter().copied().collect();
        self.sections
            .retain(|&(x, _, z), _| !gone.contains(&(x, z)));
    }
    pub(super) fn tile(&mut self, key: TileKey, mesh: &SectionMesh) {
        self.tiles.insert(key, copy(mesh));
    }
    pub(super) fn forget_tile(&mut self, key: TileKey) {
        self.tiles.remove(&key);
    }
    pub(super) fn clear_tiles(&mut self) {
        self.tiles.clear();
    }
}

fn copy(mesh: &SectionMesh) -> SectionMesh {
    SectionMesh {
        solid: mesh.solid.clone(),
        sides: mesh.sides,
        translucent: mesh.translucent.clone(),
        visibility: mesh.visibility,
        block_entities: mesh.block_entities.clone(),
        lights: mesh.lights.clone(),
        sprite_layers: mesh.sprite_layers.clone(),
        bounds: mesh.bounds,
        keep: mesh.keep,
    }
}

/// A capture asked for: where to write it, and the game's settings the renderer does not hold.
pub struct Request {
    pub dir: PathBuf,
    pub settings: Settings,
}

/// The game's settings that change GPU work, as the game applied them to the renderer.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Settings {
    /// `GraphicsApi`'s variant name.
    pub graphics_api: String,
    pub translucent_batching: bool,
    /// `textureFiltering` and `maxAnisotropyBit` (`Renderer::set_texture_filtering`).
    pub texture_filtering: i32,
    pub anisotropy_bit: i32,
    pub mipmap_levels: u32,
    /// `Renderer::set_world_post`'s arguments.
    pub resolution_scale: f32,
    pub fxaa: bool,
    pub taa: bool,
    pub upscale: f32,
    /// Chunks.
    pub render_distance: i32,
    pub lod_distance: u32,
    /// The game's prepared resources, where the replay loads the block sprites from.
    pub resources: PathBuf,
    /// The block sprites the meshes were made against.
    pub atlas: AtlasCheck,
}

impl Settings {
    pub fn graphics_api(&self) -> GraphicsApi {
        GraphicsApi::ALL
            .into_iter()
            .find(|api| format!("{api:?}") == self.graphics_api)
            .unwrap_or_default()
    }
}

/// Which block sprite layers the meshes' vertices name: the replay's must be the same.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AtlasCheck {
    pub sprite_size: u32,
    pub texture_layers: u32,
    /// FNV-1a of every sprite's name and layer.
    pub layers: u64,
}

impl AtlasCheck {
    pub fn of(render: &crate::assets::RenderAssets) -> Self {
        let mut hash = 0xcbf2_9ce4_8422_2325u64;
        for (name, layer) in &render.sprite_layers {
            for byte in name.bytes().chain(layer.to_le_bytes()) {
                hash = (hash ^ u64::from(byte)).wrapping_mul(0x0000_0100_0000_01b3);
            }
        }
        Self {
            sprite_size: render.sprite_size,
            texture_layers: render.texture_layers,
            layers: hash,
        }
    }
}

/// The renderer's own state that changes GPU work.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RendererState {
    pub adapter: String,
    pub backend: String,
    /// The frame's size in pixels.
    pub size: [u32; 2],
    /// The column layout: lowest block y and sections.
    pub min_y: i32,
    pub sections: usize,
    pub flat_draws: bool,
    pub section_cull: bool,
    pub section_sides: bool,
    pub gpu_culling: bool,
    pub gamma: f32,
    pub fullbright: bool,
    pub improved_transparency: bool,
    pub animated_textures: bool,
    /// A shader pack was running; the replay draws without it.
    pub shader_pack: bool,
}

/// The cloud inputs of the frame (`clouds::Input`), with the clouds texture.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Clouds {
    pub texture: (u32, u32, Vec<bool>),
    pub uv: [f32; 2],
    pub camera: [f64; 3],
    pub time: i64,
    pub partial_tick: f64,
    pub range: i32,
    pub fast: bool,
    pub color: [f32; 4],
}

/// The frame's camera and the inputs of its globals, sky, fog and lightmap.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FrameInputs {
    pub camera: [f64; 3],
    pub yaw: f32,
    pub pitch: f32,
    pub fov_degrees: f32,
    pub eye: [f64; 3],
    pub eye_rotation: [f32; 2],
    pub first_person: bool,
    pub bob: [f32; 16],
    pub spin: [f32; 16],
    pub sky: Sky,
    pub distance: f32,
    pub detail_distance: f32,
    pub depth_far: f32,
    pub underwater: bool,
    pub water_plane: Option<f32>,
    pub cave_culling: bool,
    pub sky_blocked: bool,
    pub skybox: bool,
    pub end_sky: bool,
    pub end_sky_sprite: Option<[f32; 4]>,
    pub environment_fog: Option<(f32, f32)>,
    pub water_fog: [f32; 3],
    pub fog: Option<FogData>,
    pub dark_disc: bool,
    pub nether_lighting: bool,
    pub glint_alpha: f32,
    pub glint_ticks: f32,
    pub hand_fov_degrees: f32,
    pub stars: Option<([f32; 16], f32)>,
    pub clouds: Option<Clouds>,
    pub game_time: f32,
    pub shader_game_time: f32,
    pub end_portal_sprites: [[f32; 4]; 2],
    pub crumbling_sprite: [f32; 4],
    pub ticks: u64,
    pub darkness: (f32, f32),
    pub night_vision: f32,
    pub block_flicker: f32,
    pub boss_darken: f32,
}

/// `scene.json`.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Scene {
    pub settings: Settings,
    pub renderer: RendererState,
    pub frame: FrameInputs,
    pub sections: usize,
    pub tiles: usize,
    /// The chunk columns far tiles leave out for full-detail sections.
    pub lod_cutout: Vec<(i32, i32)>,
    /// `celestial.bin`.
    #[serde(skip)]
    pub celestial: Vec<Vertex>,
}

/// One mesh of `sections.json` or `tiles.json`; its vertices follow the previous mesh's in
/// the `.bin` file. `key` is (x, section y, z) for a section, (x, z, level) for a tile.
#[derive(Serialize, Deserialize)]
struct MeshRecord {
    key: [i32; 3],
    solid: usize,
    translucent: usize,
    sides: Sides,
    visibility: u64,
    block_entities: Vec<([i32; 3], u32)>,
    lights: Vec<([i32; 3], u32)>,
    sprite_layers: Vec<u16>,
    bounds: Option<[[f32; 3]; 2]>,
    keep: bool,
}

/// Writes `name.json` and `name.bin`: (meshes, vertex bytes).
fn write_meshes<'m>(
    dir: &Path,
    name: &str,
    meshes: impl Iterator<Item = ([i32; 3], &'m SectionMesh)>,
) -> Result<(usize, u64)> {
    let mut blob = BufWriter::new(File::create(dir.join(format!("{name}.bin")))?);
    let mut records = Vec::new();
    let mut bytes = 0u64;
    for (key, mesh) in meshes {
        for part in [&mesh.solid, &mesh.translucent] {
            let raw: &[u8] = bytemuck::cast_slice(part.as_slice());
            blob.write_all(raw)?;
            bytes += raw.len() as u64;
        }
        records.push(MeshRecord {
            key,
            solid: mesh.solid.len(),
            translucent: mesh.translucent.len(),
            sides: mesh.sides,
            visibility: mesh.visibility,
            block_entities: mesh.block_entities.clone(),
            lights: mesh.lights.clone(),
            sprite_layers: mesh.sprite_layers.clone(),
            bounds: mesh
                .bounds
                .map(|[min, max]| [min.to_array(), max.to_array()]),
            keep: mesh.keep,
        });
    }
    blob.flush()?;
    let mut list = BufWriter::new(File::create(dir.join(format!("{name}.json")))?);
    serde_json::to_writer(&mut list, &records)?;
    list.flush()?;
    Ok((records.len(), bytes))
}

/// Reads `name.json` and `name.bin`, handing each mesh to `each` as it is read.
pub fn read_meshes(
    dir: &Path,
    name: &str,
    mut each: impl FnMut([i32; 3], SectionMesh),
) -> Result<()> {
    let records: Vec<MeshRecord> = serde_json::from_reader(BufReader::new(File::open(
        dir.join(format!("{name}.json")),
    )?))?;
    let mut blob = BufReader::new(File::open(dir.join(format!("{name}.bin")))?);
    for record in records {
        let mut solid = vec![<TerrainVertex as bytemuck::Zeroable>::zeroed(); record.solid];
        blob.read_exact(bytemuck::cast_slice_mut::<TerrainVertex, u8>(
            solid.as_mut_slice(),
        ))?;
        let mut translucent =
            vec![<TerrainVertex as bytemuck::Zeroable>::zeroed(); record.translucent];
        blob.read_exact(bytemuck::cast_slice_mut::<TerrainVertex, u8>(
            translucent.as_mut_slice(),
        ))?;
        each(
            record.key,
            SectionMesh {
                solid,
                sides: record.sides,
                translucent,
                visibility: record.visibility,
                block_entities: record.block_entities,
                lights: record.lights,
                sprite_layers: record.sprite_layers,
                bounds: record
                    .bounds
                    .map(|[min, max]| [Vec3::from_array(min), Vec3::from_array(max)]),
                keep: record.keep,
            },
        );
    }
    Ok(())
}

impl Renderer {
    /// Keeps CPU copies of the meshes uploaded from now on, for `render-capture`, or drops
    /// them. They start empty: the caller meshes everything again for a complete capture.
    pub fn keep_capture_copies(&mut self, on: bool) {
        if on != self.capture_copies.is_some() {
            self.capture_copies = on.then(Copies::default);
        }
    }
    pub fn keeps_capture_copies(&self) -> bool {
        self.capture_copies.is_some()
    }

    /// Writes the scene `frame` draws to `request.dir` (`render` calls it for `capture`).
    pub(super) fn write_capture(
        &self,
        request: &Request,
        frame: &Frame,
    ) -> Result<serde_json::Value> {
        let copies = self
            .capture_copies
            .as_ref()
            .ok_or("no mesh copies kept: render-capture on first")?;
        let dir = &request.dir;
        fs::create_dir_all(dir)?;
        let (sections, section_bytes) = write_meshes(
            dir,
            "sections",
            copies
                .sections
                .iter()
                .map(|(&(x, y, z), mesh)| ([x, y, z], mesh)),
        )?;
        let standing = self.standing_lods();
        let (tiles, tile_bytes) = write_meshes(
            dir,
            "tiles",
            copies
                .tiles
                .iter()
                .filter(|(key, _)| standing.contains(*key))
                .map(|(key, mesh)| ([key.x, key.z, i32::from(key.level)], mesh)),
        )?;
        fs::write(
            dir.join("celestial.bin"),
            bytemuck::cast_slice::<Vertex, u8>(frame.celestial),
        )?;
        let scene = Scene {
            settings: request.settings.clone(),
            renderer: RendererState {
                adapter: self.info.name.clone(),
                backend: format!("{:?}", self.info.backend),
                size: [self.config.width, self.config.height],
                min_y: self.layout.min_y(),
                sections: self.layout.sections(),
                flat_draws: self.flat_draws(),
                section_cull: self.section_cull(),
                section_sides: self.section_sides(),
                gpu_culling: self.gpu_culling(),
                gamma: self.gamma,
                fullbright: self.fullbright,
                improved_transparency: self.improved_transparency,
                animated_textures: self.animated_textures,
                shader_pack: self.pack_post.is_some() || self.native.is_some(),
            },
            frame: FrameInputs {
                camera: frame.camera.to_array(),
                yaw: frame.yaw,
                pitch: frame.pitch,
                fov_degrees: frame.fov_degrees,
                eye: frame.eye.to_array(),
                eye_rotation: frame.eye_rotation,
                first_person: frame.first_person,
                bob: frame.bob.to_cols_array(),
                spin: frame.spin.to_cols_array(),
                sky: frame.sky,
                distance: frame.distance,
                detail_distance: frame.detail_distance,
                depth_far: frame.depth_far,
                underwater: frame.underwater,
                water_plane: frame.water_plane,
                cave_culling: frame.cave_culling,
                sky_blocked: frame.sky_blocked,
                skybox: frame.skybox,
                end_sky: frame.end_sky,
                end_sky_sprite: frame.end_sky_sprite,
                environment_fog: frame.environment_fog,
                water_fog: frame.water_fog,
                fog: frame.fog,
                dark_disc: frame.dark_disc,
                nether_lighting: frame.nether_lighting,
                glint_alpha: frame.glint_alpha,
                glint_ticks: frame.glint_ticks,
                hand_fov_degrees: frame.hand_fov_degrees,
                stars: frame
                    .stars
                    .map(|(rotation, brightness)| (rotation.to_cols_array(), brightness)),
                clouds: frame.clouds.as_ref().map(|c| Clouds {
                    texture: c.texture.clone(),
                    uv: c.uv,
                    camera: c.camera.to_array(),
                    time: c.time,
                    partial_tick: c.partial_tick,
                    range: c.range,
                    fast: c.fast,
                    color: c.color,
                }),
                game_time: frame.game_time,
                shader_game_time: frame.shader_game_time,
                end_portal_sprites: frame.end_portal_sprites,
                crumbling_sprite: frame.crumbling_sprite,
                ticks: frame.ticks,
                darkness: frame.darkness,
                night_vision: frame.night_vision,
                block_flicker: frame.block_flicker,
                boss_darken: frame.boss_darken,
            },
            sections,
            tiles,
            lod_cutout: self.lod_cutout.iter().copied().collect(),
            celestial: Vec::new(),
        };
        let mut out = BufWriter::new(File::create(dir.join("scene.json"))?);
        serde_json::to_writer_pretty(&mut out, &scene)?;
        out.flush()?;
        Ok(serde_json::json!({
            "ok": true,
            "dir": dir.display().to_string(),
            "sections": sections,
            "tiles": tiles,
            "vertex_bytes": section_bytes + tile_bytes,
            "shader_pack_left_out": scene.renderer.shader_pack,
        }))
    }
}

impl Scene {
    /// Reads `scene.json` and `celestial.bin` (the meshes: `read_meshes`).
    pub fn read(dir: &Path) -> Result<Self> {
        let mut scene: Scene =
            serde_json::from_reader(BufReader::new(File::open(dir.join("scene.json"))?))?;
        let bytes = fs::read(dir.join("celestial.bin"))?;
        let size = std::mem::size_of::<Vertex>();
        let mut celestial = vec![<Vertex as bytemuck::Zeroable>::zeroed(); bytes.len() / size];
        let len = celestial.len() * size;
        bytemuck::cast_slice_mut::<Vertex, u8>(celestial.as_mut_slice())
            .copy_from_slice(&bytes[..len]);
        scene.celestial = celestial;
        Ok(scene)
    }

    /// The captured frame: its camera, sky, fog, clouds, sun, moon and stars, without
    /// entities, particles, weather, the hand, the outline or the HUD.
    pub fn frame<'a>(&'a self, hud: &'a HudBatch) -> Frame<'a> {
        let f = &self.frame;
        Frame {
            weather: &[],
            weather_time: crate::weather::Time::new(0, 0.0),
            camera: DVec3::from_array(f.camera),
            yaw: f.yaw,
            pitch: f.pitch,
            fov_degrees: f.fov_degrees,
            eye: DVec3::from_array(f.eye),
            eye_rotation: f.eye_rotation,
            first_person: f.first_person,
            bob: Mat4::from_cols_array(&f.bob),
            spin: Mat4::from_cols_array(&f.spin),
            sky: f.sky,
            shadow_scene_revision: (0, 0),
            shadow_cache: false,
            distance: f.distance,
            detail_distance: f.detail_distance,
            depth_far: f.depth_far,
            underwater: f.underwater,
            water_plane: f.water_plane,
            cave_culling: f.cave_culling,
            title: false,
            sky_blocked: f.sky_blocked,
            skybox: f.skybox,
            end_sky: f.end_sky,
            end_sky_sprite: f.end_sky_sprite,
            environment_fog: f.environment_fog,
            water_fog: f.water_fog,
            fog: f.fog,
            dark_disc: f.dark_disc,
            outline: Vec::new(),
            outline_after_terrain: false,
            outline_high_contrast: false,
            debug_lines: &[],
            debug_lines_on_top: &[],
            debug_crosshair: &[],
            hud,
            glint_alpha: f.glint_alpha,
            glint_ticks: f.glint_ticks,
            nether_lighting: f.nether_lighting,
            entities: &[],
            static_block_entities: &[],
            visible_static_block_entity_sections: &[],
            static_block_entity_stats: Default::default(),
            entity_motion: &[],
            entity_spans: &[],
            skin: None,
            glowing: &[],
            dynamic_lights: &[],
            colored_lights: &[],
            emissive_layers: &[],
            clouds: f.clouds.as_ref().map(|c| crate::clouds::Input {
                texture: &c.texture,
                uv: c.uv,
                camera: DVec3::from_array(c.camera),
                time: c.time,
                partial_tick: c.partial_tick,
                range: c.range,
                fast: c.fast,
                color: c.color,
            }),
            lightning: Default::default(),
            additive: &[],
            see_through: &[],
            shadow_only: &[],
            celestial: &self.celestial,
            stars: f
                .stars
                .map(|(rotation, brightness)| (Mat4::from_cols_array(&rotation), brightness)),
            hand: &[],
            hand_fov_degrees: f.hand_fov_degrees,
            screen: &[],
            pack: Default::default(),
            border: None,
            game_time: f.game_time,
            shader_game_time: f.shader_game_time,
            end_portal_sprites: f.end_portal_sprites,
            crumbling_sprite: f.crumbling_sprite,
            ticks: f.ticks,
            darkness: f.darkness,
            night_vision: f.night_vision,
            block_flicker: f.block_flicker,
            boss_darken: f.boss_darken,
            chunk_fade: 0.0,
            post_effects: crate::post_chain::Request::none(),
            menu_blur: None,
        }
    }
}
