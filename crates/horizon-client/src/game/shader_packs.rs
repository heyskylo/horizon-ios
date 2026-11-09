//! Shader packs: the one picked on Options > Horizon Settings > Shader Packs, or
//! `HORIZON_SHADER_PACK=<name>` (development; it wins over the pick), runs `shaderpacks/<name>`
//! (a folder or a zip beside `options.txt`) over the world; Horizon's own packs go to
//! `game/native_shaders.rs`. The programs of the current dimension's folder are loaded when the
//! dimension changes (spec 1.2).

use horizon_client::camera::CameraFluid;
use horizon_client::gfx::pack_uniforms::{PackInputs, PlayerStatus};
use horizon_client::shader_options::ShaderOptions;
use horizon_client::text::Text;
use horizon_shaders::{BlockIds, Macros, Pack, Properties, dimension_folder_named, load_post};

use super::{Dim, Game};
use horizon_client::gfx::pack_draw::EntitySpan;
use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, HashMap};

/// The values the player chose for pack `name`'s options (`shaderpacks/<name>.txt`).
fn chosen_shader_options(name: &str) -> BTreeMap<String, String> {
    let file = Game::shader_packs_dir().join(horizon_shaders::chosen_file(name));
    std::fs::read_to_string(file)
        .map(|text| horizon_shaders::parse_chosen(&text))
        .unwrap_or_default()
}

/// Pack `name`'s options, screens and names in `language`, with the values the player chose.
pub(super) fn read_shader_options(
    name: &str,
    language: &str,
    macros: &Macros,
) -> Option<ShaderOptions> {
    let pack = match Pack::open(&Game::shader_packs_dir().join(name)) {
        Ok(pack) => pack,
        Err(e) => {
            log::warn!("shader pack {name}: {e}");
            return None;
        }
    };
    let options = horizon_shaders::options(&pack);
    let lang = horizon_shaders::Lang::load(&pack, language);
    let chosen = chosen_shader_options(name);
    let pack = pack.with_chosen(chosen.clone());
    let text = pack.read_text("shaders.properties").unwrap_or_default();
    let properties = Properties::parse(&text, &horizon_shaders::properties_macros(&pack, macros));
    let screens = horizon_shaders::Screens::parse(&properties, &options);
    Some(ShaderOptions::new(
        name.to_owned(),
        options,
        screens,
        lang,
        chosen,
    ))
}

/// Saves the values chosen on the Shader Options page; none chosen removes the file.
pub(super) fn write_shader_options(options: &ShaderOptions) {
    let file = Game::shader_packs_dir().join(horizon_shaders::chosen_file(&options.pack));
    let saved = if options.chosen.is_empty() {
        std::fs::remove_file(&file).or_else(|e| match e.kind() {
            std::io::ErrorKind::NotFound => Ok(()),
            _ => Err(e),
        })
    } else {
        std::fs::write(&file, horizon_shaders::chosen_text(&options.chosen))
    };
    if let Err(e) = saved {
        log::warn!("shader pack {}: {}: {e}", options.pack, file.display());
    }
}

/// The pack that runs over the world ("": none): `HORIZON_SHADER_PACK`, else the pick
/// (`active_pack`).
fn picked_pack(settings: &horizon_client::settings::Settings) -> &str {
    static DEV_PACK: std::sync::OnceLock<Option<String>> = std::sync::OnceLock::new();
    let dev = DEV_PACK.get_or_init(|| std::env::var("HORIZON_SHADER_PACK").ok());
    active_pack(
        dev.as_deref().unwrap_or(&settings.horizon.shader_pack),
        &settings.mods,
    )
}

/// Whether the pack picked (`picked_pack`) draws flat where the device can (`pack_draws_flat`):
/// a renderer made now lays the sections out for it before the first world frame loads it
/// (`Renderer::new`).
pub(super) fn picked_pack_draws_flat(settings: &horizon_client::settings::Settings) -> bool {
    let picked = picked_pack(settings);
    !picked.is_empty() && pack_draws_flat(&Game::shader_packs_dir().join(picked))
}

/// Whether the pack at `path` has passes that draw the terrain flat where the device can,
/// reading translucent quads beside their section's solid ones: a `shaders/` pack, taken to
/// draw terrain, or one of Horizon's own with a `[shadow]` pass.
fn pack_draws_flat(path: &std::path::Path) -> bool {
    !horizon_shader_api::package::is_pack(path)
        || horizon_shader_api::Pack::read(path).is_ok_and(|p| p.manifest.shadow.is_some())
}

// A native mod can bundle a pack as `native-<mod-id>`; its Mods toggle controls it too.
fn active_pack<'a>(picked: &'a str, mods: &horizon_client::mod_list::ModList) -> &'a str {
    if picked.strip_prefix("native-").is_some_and(|id| {
        mods.installed
            .iter()
            .any(|m| m.id == id && m.native.is_some() && mods.disabled.contains(id))
    }) {
        ""
    } else {
        picked
    }
}

/// A selected pack may read resource-pack material maps, whether native or GLSL.
pub(super) fn picks_material_pack(settings: &horizon_client::settings::Settings) -> bool {
    let dev = std::env::var("HORIZON_SHADER_PACK").ok();
    let picked = active_pack(
        dev.as_deref().unwrap_or(&settings.horizon.shader_pack),
        &settings.mods,
    );
    !picked.is_empty()
}

use serde_json::{Value, json};

pub(super) const HELP: &[&str] =
    &["shader-pack [list|off|NAME]    list or select a shader pack in shaderpacks/"];
thread_local! {
    static REPORT: RefCell<Option<Value>> = const { RefCell::new(None) };
    static OPTIONS_REVISION: Cell<u64> = const { Cell::new(0) };
}
pub(super) fn record_report(name: &str, dimension: i32, programs: &[String], notes: &[String]) {
    REPORT.with_borrow_mut(|report| {
        *report = Some(json!({
            "pack": name, "dimension": dimension, "programs": programs, "notes": notes,
        }))
    });
}
pub(super) fn record_reload_error(name: &str, dimension: i32, errors: &[String]) {
    let previous: Vec<String> = REPORT.with_borrow(|report| {
        report
            .as_ref()
            .filter(|r| r["pack"].as_str() == Some(name))
            .and_then(|r| r["programs"].as_array())
            .map(|programs| {
                programs
                    .iter()
                    .filter_map(|value| value.as_str().map(str::to_owned))
                    .collect()
            })
            .unwrap_or_default()
    });
    let mut notes = errors.to_vec();
    if !previous.is_empty() {
        notes.insert(
            0,
            "Reload failed; the previous shader programs remain active.".into(),
        );
    }
    record_report(name, dimension, &previous, &notes);
}
pub(super) fn command(
    settings: &mut horizon_client::settings::Settings,
    words: &[&str],
    line: &str,
) -> horizon_client::Result<Option<Value>> {
    if words.first() != Some(&"shader-pack") {
        return Ok(None);
    }
    let root = std::env::current_dir()?;
    let packs = horizon_client::shader_pack_list::scan(&root.join("shaderpacks"));
    let arg = line
        .trim_start()
        .strip_prefix("shader-pack")
        .unwrap_or("")
        .trim();
    match arg {
        "" | "list" => {}
        "off" => {
            settings.horizon.shader_pack.clear();
            settings.horizon.save(&root);
        }
        name => {
            if !packs.iter().any(|pack| pack.file == name) {
                return Err(format!("no shader pack {name} in shaderpacks/").into());
            }
            settings.horizon.shader_pack = name.to_owned();
            settings.horizon.save(&root);
        }
    }
    settings.shader_packs = packs;
    let selected =
        (!settings.horizon.shader_pack.is_empty()).then_some(settings.horizon.shader_pack.as_str());
    let report = REPORT.with_borrow(|report| {
        report
            .as_ref()
            .filter(|r| r["pack"].as_str() == selected)
            .cloned()
    });
    let list: Vec<Value> = settings
        .shader_packs
        .iter()
        .map(|pack| json!({"name": pack.file, "title":pack.title, "problem":null}))
        .collect();
    Ok(Some(
        json!({"packs":list,"selected":selected,"runtime":report}),
    ))
}

/// The ids a pack gives entities and block entities (4.1, 5.3), for the spans of the entity
/// geometry.
pub(super) struct PackIds {
    entities: HashMap<String, i32>,
    blocks: BlockIds,
    /// `blockEntityId` by block state, filled as states are drawn.
    block_states: RefCell<HashMap<u32, i32>>,
    /// The id of a name the file lacks: 65535, or 0 without the file.
    missing_entity: i32,
    missing_block: i32,
    /// `item.properties` (4.1) and the id of a held item the file lacks, or of an empty hand:
    /// −1, or 0 without the file (5.3).
    items: HashMap<String, i32>,
    missing_item: i32,
    /// `currentRenderedItemId` of each block item (`StateFacts::item` of its block's default
    /// state): that state's `block.properties` id, or 0.
    block_items: HashMap<String, i32>,
    /// Whether the pack has `item.properties` (`currentRenderedItemId` of an item it lacks).
    item_file: bool,
    /// `oldHandLight` (3.2): the first held item and light are those of the brighter hand.
    old_hand_light: bool,
    /// `vignette`: `false` hides the game's vignette over the pack's image.
    vignette: bool,
    /// `oldLighting`: `true` keeps the game's face shading in the block colours.
    old_lighting: bool,
    /// `separateAo`: `true` passes AO factor in vertex alpha and keeps pure tint in RGB.
    pub(super) separate_ao: bool,
}

#[derive(Clone, PartialEq, Eq)]
struct LoadKey {
    path: std::path::PathBuf,
    resources: std::path::PathBuf,
    dimension: i32,
    dimension_name: String,
    context: (u64, u64),
    options_revision: u64,
}
struct ParsedPack {
    post: horizon_shaders::Post,
    ids: BlockIds,
    entity_ids: PackIds,
}
struct PendingLoad {
    worker: Option<std::thread::JoinHandle<()>>,
    key: LoadKey,
    result: std::sync::mpsc::Receiver<Result<ParsedPack, String>>,
}
thread_local! {
    // One parser at a time. A changed selection waits for obsolete work to
    // finish; it never spawns an unbounded group of large-stack workers.
    static PENDING_LOAD: RefCell<Option<PendingLoad>> = const { RefCell::new(None) };
}
pub(super) fn shutdown_parser() -> bool {
    PENDING_LOAD.with_borrow_mut(|pending| {
        if pending.as_ref().is_some_and(|load| {
            load.worker
                .as_ref()
                .is_some_and(|worker| !worker.is_finished())
        }) {
            return false;
        }
        if let Some(load) = pending.take()
            && let Some(worker) = load.worker
        {
            let _ = worker.join();
        }
        true
    })
}
fn parser_busy() -> bool {
    PENDING_LOAD.with_borrow(|pending| pending.is_some())
}
fn poll_parser() -> Option<(LoadKey, Result<ParsedPack, String>)> {
    PENDING_LOAD.with_borrow_mut(|pending| {
        let load = pending.as_ref()?;
        if load
            .worker
            .as_ref()
            .is_some_and(|worker| !worker.is_finished())
        {
            return None;
        }
        let result = match load.result.try_recv() {
            Ok(result) => result,
            Err(std::sync::mpsc::TryRecvError::Empty) => return None,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                Err("Shader parser worker failed".into())
            }
        };
        let key = pending.take().unwrap().key;
        Some((key, result))
    })
}
const SOURCE_PANIC: &str = "Shader source compiler panicked: ";
fn parse_source<T>(work: impl FnOnce() -> Result<T, String>) -> Result<T, String> {
    horizon_client::crash::contain(work)
        .unwrap_or_else(|panic| Err(format!("{SOURCE_PANIC}{panic}")))
}

fn start_parser(
    key: LoadKey,
    inputs: horizon_shaders::GameInputs,
    mut macros: Macros,
) -> Result<(), String> {
    let path = key.path.clone();
    let dimension = key.dimension;
    let dimension_name = key.dimension_name.clone();
    let resources = key.resources.clone();
    let options = chosen_shader_options(path.file_name().and_then(|n| n.to_str()).unwrap_or(""));
    let (sender, result) = std::sync::mpsc::channel();
    let _worker =
        std::thread::Builder::new()
            .name("Shader source".into())
            .stack_size(16 * 1024 * 1024)
            .spawn(move || {
                let parsed =
                    parse_source(|| {
                        let pack = Pack::open(&path)
                            .map_err(|error| error.to_string())?
                            .with_chosen(options);
                        let folder = dimension_folder_named(&pack, &dimension_name, dimension);
                        let properties =
                            Properties::from_pack(&pack, &folder, &macros).in_dimension(dimension);
                        let feature_notes = macros.define_features(&properties);
                        let mut post = load_post(&pack, &folder, &properties, &macros, &inputs);
                        let read = |namespace: &str, path: &str| {
                            std::fs::read(resources.join("assets").join(namespace).join(path)).ok()
                        };
                        post.notes.extend(horizon_shaders::resolve_resources(
                            &mut post.textures,
                            &read,
                        ));
                        post.notes.extend(feature_notes);
                        post.notes.extend(properties.notes.iter().map(|note| {
                            format!("shaders.properties:{}: {}", note.line, note.message)
                        }));
                        let ids = BlockIds::parse(&Properties::parse(
                            &pack.read_text("block.properties").unwrap_or_default(),
                            &macros,
                        ));
                        post.notes.extend(
                            ids.notes
                                .iter()
                                .map(|note| format!("block.properties: {note}")),
                        );
                        let entity_text = pack.read_text("entity.properties");
                        let entity_ids = BlockIds::parse_kind(
                            &Properties::parse(entity_text.as_deref().unwrap_or_default(), &macros),
                            "entity",
                        );
                        let item_text = pack.read_text("item.properties");
                        let item_ids = BlockIds::parse_kind(
                            &Properties::parse(item_text.as_deref().unwrap_or_default(), &macros),
                            "item",
                        );
                        let entity_ids = PackIds {
                            block_items: HashMap::new(),
                            item_file: item_text.is_some(),
                            items: item_ids.plain(),
                            missing_item: if item_text.is_some() { -1 } else { 0 },
                            old_hand_light: properties
                                .get("oldHandLight")
                                .is_none_or(|value| value.trim() != "false"),
                            vignette: properties
                                .get("vignette")
                                .is_none_or(|v| v.trim() != "false"),
                            old_lighting: properties
                                .get("oldLighting")
                                .is_some_and(|v| v.trim() == "true"),
                            separate_ao: properties
                                .get("separateAo")
                                .is_some_and(|v| v.trim() == "true"),
                            entities: entity_ids.plain(),
                            missing_entity: if entity_text.is_some() { 65535 } else { 0 },
                            missing_block: if ids.is_empty() { 0 } else { 65535 },
                            blocks: ids.clone(),
                            block_states: RefCell::default(),
                        };
                        Ok(ParsedPack {
                            post,
                            ids,
                            entity_ids,
                        })
                    });
                let _ = sender.send(parsed);
            })
            .map_err(|error| format!("Could not start shader parser: {error}"))?;
    PENDING_LOAD.with_borrow_mut(|pending| {
        *pending = Some(PendingLoad {
            key,
            result,
            worker: Some(_worker),
        })
    });
    Ok(())
}

impl Game<'_> {
    pub(super) fn shader_pack_control(
        &mut self,
        words: &[&str],
        line: &str,
    ) -> horizon_client::Result<Option<Value>> {
        let response = command(&mut self.settings, words, line)?;
        let argument = line
            .trim_start()
            .strip_prefix("shader-pack")
            .unwrap_or("")
            .trim();
        if response.is_some() && !matches!(argument, "" | "list") {
            if let Some(renderer) = &mut self.renderer {
                renderer.drop_shader_packs();
            }
            self.native_shaders = super::native_shaders::NativeShaders::picking("");
            self.pack_ids = None;
            REPORT.with_borrow_mut(Option::take);
        }
        Ok(response)
    }

    /// Settle shader jobs without rendering, including while the launcher covers the game.
    pub(super) fn poll_shader_workers(&mut self) {
        let dimension = match self.dim {
            Dim::Overworld => 0,
            Dim::Nether => -1,
            Dim::End => 1,
        };
        let mut failed = None;
        if let Some(renderer) = &mut self.renderer {
            // A dimension transition must stop both active and in-flight old programs
            // before accepting any worker result or forming the next parser key.
            if renderer
                .shader_pack_dimension()
                .is_some_and(|old| old != dimension)
            {
                renderer.drop_shader_packs();
                self.pack_ids = None;
            }
            if let Some((dimension, programs, notes)) = renderer.take_shader_report() {
                if programs.is_empty() {
                    self.pack_ids = None;
                    failed = Some((self.native_shaders.picked.clone(), dimension, notes.clone()));
                }
                record_report(&self.native_shaders.picked, dimension, &programs, &notes);
                for note in &notes {
                    log::warn!("shader pack: {note}");
                }
            }
            let desired = LoadKey {
                path: Self::shader_packs_dir().join(picked_pack(&self.settings)),
                resources: self.options.resources.clone(),
                options_revision: OPTIONS_REVISION.with(Cell::get),
                dimension,
                dimension_name: self.dim.id().to_owned(),
                context: renderer.shader_context_key(),
            };
            PENDING_LOAD.with_borrow_mut(|pending| {
                if let Some(load) = pending.as_ref()
                    && load.key != desired
                    && load
                        .worker
                        .as_ref()
                        .is_none_or(|worker| worker.is_finished())
                {
                    match load.result.try_recv() {
                        Err(std::sync::mpsc::TryRecvError::Empty) => {}
                        _ => {
                            *pending = None;
                        }
                    }
                }
            });
        }
        if let Some((name, dimension, notes)) = failed {
            // This report is a completed GPU job, never the pending notice.
            // Apply the same quarantine as the synchronous loader so a pack
            // that the device refuses is not retried on every app launch.
            self.shader_pack_failed(&name, dimension, &notes.join("; "));
        }
    }

    /// Loads the pack for the current dimension when it changed, and returns this frame's
    /// inputs to the pack's uniforms.
    pub(super) fn shader_pack_inputs(
        &mut self,
        camera: glam::DVec3,
        partial: f32,
        sky: &horizon_client::environment::Sky,
    ) -> PackInputs {
        // A `shaders/` pack lights the faces itself unless `oldLighting=true`: sections mesh
        // without the game's face shading (the pack loaded or dropped below shows next frame).
        let pack_lighting = self.pack_ids.as_ref().is_some_and(|ids| !ids.old_lighting);
        if self.streaming.shader_pack_lighting != pack_lighting {
            self.streaming.shader_pack_lighting = pack_lighting;
            self.all_sections_changed();
        }
        self.poll_shader_workers();
        let completed = poll_parser();
        // Any `shaders/` pack (its `oldLighting` too) draws fluid tops split as Iris does.
        let shader_pack = self.pack_ids.is_some();
        if self.streaming.shader_pack != shader_pack {
            self.streaming.shader_pack = shader_pack;
            self.all_sections_changed();
        }
        let separate_ao = self.pack_ids.as_ref().is_some_and(|ids| ids.separate_ao);
        if self.streaming.separate_ao != separate_ao {
            self.streaming.separate_ao = separate_ao;
            self.all_sections_changed();
        }
        let picked = picked_pack(&self.settings).to_owned();
        if self.native_shaders.picked != picked {
            REPORT.with_borrow_mut(Option::take);
            // Another pack, or none: drop what ran.
            self.native_shaders = super::native_shaders::NativeShaders::picking(&picked);
            self.set_translucent_layers(Vec::new());
            self.pack_ids = None;
            if let Some(renderer) = &mut self.renderer {
                renderer.drop_shader_packs();
            }
        }
        if picked.is_empty() {
            return PackInputs::default();
        }
        if !horizon_client::sprites::labpbr_maps() {
            horizon_client::sprites::set_labpbr_maps(true);
            self.reload_packs();
            return PackInputs::default();
        }
        let name = picked;
        let dimension = match self.dim {
            Dim::Overworld => 0,
            Dim::Nether => -1,
            Dim::End => 1,
        };
        let path = Self::shader_packs_dir().join(&name);
        if horizon_shader_api::package::is_pack(&path) {
            // One of Horizon's own packs (game/native_shaders.rs).
            self.tick_native_shader_pack(&path);
        } else if let Some(renderer) = &mut self.renderer {
            // Lava uses the solid terrain program. Supply its sprite layers before
            // taking the renderer inputs handed to the background source parser.
            renderer.set_pack_lava_layers(
                ["lava_still", "lava_flow"]
                    .iter()
                    .filter_map(|f| {
                        let name = format!("minecraft:block/{f}");
                        self.pack.render.sprite_layers.get(&name).map(|&l| l as u16)
                    })
                    .collect(),
            );
            let key = LoadKey {
                path: path.clone(),
                resources: self.options.resources.clone(),
                options_revision: OPTIONS_REVISION.with(Cell::get),
                dimension,
                dimension_name: self.dim.id().to_owned(),
                context: renderer.shader_context_key(),
            };
            let ready = completed.and_then(|(at, result)| (at == key).then_some(result));
            if let Some(result) = ready {
                match result {
                    Ok(mut parsed) => {
                        let biomes = world_biomes!(self);
                        let names: Vec<&str> = biomes
                            .list
                            .iter()
                            .map(|biome| biome.name.as_str())
                            .collect();
                        parsed.post.custom.set_biomes(&names);
                        let table = state_material_table(&self.pack, &parsed.ids);
                        parsed.entity_ids.block_items = block_item_ids(
                            self.res.facts(),
                            self.pack.assets.registry,
                            &parsed.ids,
                        );
                        renderer.set_separate_ao(parsed.entity_ids.separate_ao);
                        self.pack_ids = parsed.post.complete.then_some(parsed.entity_ids);
                        match renderer.set_shader_pack(dimension, Some(parsed.post), &table) {
                            Ok(Some((programs, notes))) => {
                                record_report(&name, dimension, &programs, &notes)
                            }
                            Ok(None) => {}
                            Err(refused) => {
                                let mut notes = refused.notes;
                                notes.push(refused.reason.clone());
                                self.shader_pack_failed(&name, dimension, &refused.reason);
                                record_report(&name, dimension, &[], &notes);
                            }
                        }
                    }
                    Err(error) => {
                        if error.starts_with(SOURCE_PANIC) {
                            self.shader_pack_failed(&name, dimension, &error);
                        } else {
                            self.pack_ids = None;
                            let _ = renderer.set_shader_pack(dimension, None, &[]);
                        }
                        record_report(&name, dimension, &[], &[error]);
                    }
                }
            } else if renderer.shader_pack_dimension() != Some(dimension) && !parser_busy() {
                let inputs = renderer.pack_inputs();
                let mut macros = Macros::standard();
                renderer.define_pack_macros(&mut macros);
                match start_parser(key,inputs,macros) {
                    Ok(())=>record_report(&name,dimension,&[],&["Processing shader sources in the background. Normal rendering remains active until ready.".into()]),
                    Err(error)=>{let _ = renderer.set_shader_pack(dimension,None,&[]);record_report(&name,dimension,&[],&[error]);}
                }
            }
        }
        let lerp = |(old, new): (f32, f32)| old + (new - old) * partial;
        let at = camera.floor().as_ivec3();
        let light = self.world.light(at.x, at.y, at.z);
        let biome = self
            .world
            .chunks
            .get(&(at.x >> 4, at.z >> 4))
            .map(|c| c.biome(at.x, at.y, at.z));
        let (held_items, held_light) = self.held_for_pack();
        let status = self.pack_player_status();
        let info = biome.map(|b| world_biomes!(self).get(b));
        let (end_flash_position, end_flash_intensity) = self.end_flash_for_pack(partial);
        let dimension = self.remote.as_ref().map(|r| &r.dimension);
        PackInputs {
            // `Level.getDefaultClockTime`: 0 in a dimension type with no default clock (the
            // Nether's has none), as Iris 1.11.7 reads `worldTime` there (probe pack).
            day_time: if dimension.is_some_and(|d| d.default_clock.is_none()) {
                0
            } else {
                self.world.day_time
            },
            rain: lerp(self.world.weather.rain_level),
            thunder: lerp(self.world.weather.thunder_level),
            eye_in: match self.camera_fluid(camera.to_array()) {
                CameraFluid::None => 0,
                CameraFluid::Water => 1,
                CameraFluid::Lava => 2,
                CameraFluid::PowderSnow => 3,
            },
            blindness: f32::from(u8::from(self.potions.effects.get("blindness").is_some())),
            eye_light: [i32::from(light & 15), i32::from(light >> 4)],
            hide_gui: self.ui.hide_gui,
            brightness: self.settings.gamma,
            biome: info.map_or(-1, |b| {
                let names = world_biomes!(self).list.iter().map(|b| b.name.as_str());
                horizon_shaders::biome_pack_id(&b.name, names)
            }),
            biome_known: biome.is_some(),
            biome_category: info.map_or(0, |b| horizon_shaders::biome_category(&b.name)),
            biome_precipitation: info.map_or(0, |b| {
                horizon_shaders::biome_precipitation(b.precipitation, b.temperature)
            }),
            temperature: info.map_or(0.0, |b| b.temperature),
            rainfall: info.map_or(0.0, |b| b.downfall),
            held_items,
            held_light,
            eye_position: if self.camera_type.first_person() {
                camera
            } else {
                self.player.as_ref().map_or(camera, |p| {
                    let alpha = self.clock.camera_fraction();
                    let feet = self.previous_position.lerp(p.position, alpha);
                    glam::DVec3::new(
                        feet.x,
                        feet.y
                            + f64::from(
                                self.view.eye_height_old
                                    + (self.view.eye_height - self.view.eye_height_old)
                                        * alpha as f32,
                            ),
                        feet.z,
                    )
                })
            },
            player_look: glam::DVec3::from_array(self.forward()).as_vec3(),
            elytra_flying: self.player.as_ref().is_some_and(|p| p.fall_flying),
            player_mood: self.ambience.moodiness,
            heavy_fog: self.boss_overlay.should_create_world_fog(),
            end_flash_position,
            end_flash_intensity,
            right_handed: !self.settings.main_hand_left,
            on_ground: self.player.as_ref().is_some_and(|p| p.ground.on_ground),
            min_y: self.world.shape.min_y,
            height: self.world.shape.height,
            logical_height: self.world.shape.logical_height,
            has_skylight: self.world.shape.sky_light,
            has_ceiling: self.world.shape.has_ceiling,
            cloud_height: horizon_client::gfx::pack_uniforms::cloud_height(sky),
            // Vanilla's drift in float, times its 0.030000001F blocks a tick.
            cloud_time: horizon_client::clouds::drift(
                self.world.game_time,
                f64::from(partial),
                self.pack.clouds.0,
            ) as f32
                * 0.030_000_001,
            mod_params: self.native_shader_params_now(partial),
            status,
            ambient_light: dimension.map_or(0.0, |d| d.ambient_light),
            still_ticks: self.clock.still().then(|| self.visual_ticks()),
        }
    }

    /// The local player's state for Iris's status uniforms, read as the HUD reads it.
    fn pack_player_status(&mut self) -> PlayerStatus {
        let spectator = self.spectator();
        let survival = !self.creative && !spectator;
        let invisible = self.own_invisible();
        let riding = self.riding.is_some();
        let Some(p) = &mut self.player else {
            return PlayerStatus::default();
        };
        PlayerStatus {
            survival,
            health: p.health.health(),
            max_health: p.attribute("minecraft:max_health").unwrap_or(20.0) as f32,
            food: p.health.food.food as f32,
            air: p.air as f32,
            armor: p
                .attribute("minecraft:armor")
                .or_else(|_| p.attribute("armor"))
                .unwrap_or(0.0) as f32,
            spectator,
            riding,
            feet_in_water: p.fluids.was_touching_water,
            swimming: p.pose == horizon_client::pose::Pose::Swimming,
            burning: p.fire_ticks > 0 && !spectator,
            hurt: p.health.hurt_animation_time > 0,
            invisible,
            sneaking: p.pose == horizon_client::pose::Pose::Crouching,
            sprinting: p.sprinting,
        }
    }

    /// The Shader Options page opened: the picked pack's options, screens and names.
    pub(super) fn open_shader_options(&mut self) {
        let mut macros = Macros::standard();
        if let Some(renderer) = &self.renderer {
            renderer.define_pack_macros(&mut macros);
        }
        let name = &self.settings.horizon.shader_pack;
        self.settings.shader_options = read_shader_options(name, &self.settings.language, &macros);
    }

    /// The Shader Options page closed with changes: they are saved, and the pack loads again.
    pub(super) fn save_shader_options(&mut self) {
        let Some(options) = self.settings.shader_options.take() else {
            return;
        };
        write_shader_options(&options);
        OPTIONS_REVISION.with(|revision| revision.set(revision.get().wrapping_add(1)));
        self.pack_ids = None;
        if let Some(renderer) = &mut self.renderer {
            renderer.drop_shader_packs();
        }
    }

    /// Turns off the pack `name` after loading it panicked or failed: nothing runs in its place,
    /// it is no longer the picked pack (so the next start does not load it again), and the chat
    /// says so.
    fn shader_pack_failed(&mut self, name: &str, dimension: i32, error: &str) {
        log::error!("shader pack {name}: turned off: {error}");
        self.pack_ids = None;
        if let Some(renderer) = &mut self.renderer {
            // Marks this dimension as loaded, without a pack, so it is not tried again (without
            // a pack this can't fail).
            let _ = renderer.set_shader_pack(dimension, None, &[]);
        }
        if self.settings.horizon.shader_pack == name {
            self.settings.horizon.shader_pack.clear();
            self.settings.save(&Self::options_path());
        }
        self.system_message(Text::colored(
            format!("Shader pack {name} could not be loaded and was turned off"),
            0xFF5555,
        ));
    }

    /// Whether the game's vignette draws: the option, unless the loaded pack turns it off.
    pub(super) fn vignette_shown(&self) -> bool {
        self.settings.vignette && self.pack_ids.as_ref().is_none_or(|ids| ids.vignette)
    }

    /// `currentRenderedItemId` of a held `item`, [`rendered_item_id`]'s; 0 without a pack.
    pub(super) fn rendered_item_id(&self, item: &str) -> i32 {
        self.pack_ids.as_ref().map_or(0, |ids| {
            rendered_item_id(
                ids.block_items.get(item).copied(),
                ids.items.get(item).copied(),
                ids.item_file,
            )
        })
    }

    /// `heldItemId(2)` and `heldBlockLightValue(2)`: main hand, then off hand (5.2, 5.3).
    fn held_for_pack(&mut self) -> ([i32; 2], [i32; 2]) {
        if self.pack_ids.is_none() {
            return ([0; 2], [0; 2]);
        }
        let lum = self.luminance();
        let Some(ids) = &self.pack_ids else {
            return ([0; 2], [0; 2]);
        };
        let hands = [self.selected, super::shield::OFFHAND_SLOT].map(|slot| {
            let item = self.stack_in(slot).map(|s| &*s.item);
            let id = item.and_then(|i| ids.items.get(i).copied());
            (
                id.unwrap_or(ids.missing_item),
                item.map_or(0, |i| i32::from(lum.of(i))),
            )
        });
        let first = if ids.old_hand_light && hands[1].1 > hands[0].1 {
            hands[1]
        } else {
            hands[0]
        };
        ([first.0, hands[1].0], [first.1, hands[1].1])
    }
}

impl Game<'_> {
    pub(super) fn mark_rendered_item(&self, start: usize, end: usize, item: &str) {
        if self.pack_ids.is_some() && self.emitting_entity_items.get() {
            EntitySpan::item_range(
                &mut self.entity_spans.borrow_mut(),
                start,
                end,
                self.rendered_item_id(item),
            );
        }
    }

    /// Starts a span of the entity geometry at vertex `start` for entity type `name`
    /// (`minecraft:zombie`; "" for one the pack cannot name). Nothing without a pack.
    pub(super) fn mark_entity(&self, start: usize, name: &str) {
        self.mark_entity_part(start, name, false);
    }

    /// Starts a span for entity geometry that blends (name tags, shadows).
    pub(super) fn mark_translucent_entity(&self, start: usize, name: &str) {
        self.mark_entity_part(start, name, true);
    }

    fn mark_entity_part(&self, start: usize, name: &str, translucent: bool) {
        let id = match &self.pack_ids {
            Some(ids) => ids
                .entities
                .get(name)
                .copied()
                .unwrap_or(ids.missing_entity),
            None if native_spans() => name_id(name),
            None => return,
        };
        self.push_span(start, "ENTITIES", id);
        if let Some(last) = self.entity_spans.borrow_mut().last_mut() {
            last.translucent = translucent;
            if let Some(gpu) = self.skin_frame.borrow_mut().spans.last_mut() {
                gpu.translucent = translucent;
            }
        }
    }

    /// Starts a span for the block entity of block state `state`.
    pub(super) fn mark_block_entity(&self, start: usize, state: u32) {
        let Some(ids) = &self.pack_ids else {
            if native_spans() {
                self.push_span(start, "BLOCK_ENTITIES", state as i32);
            }
            return;
        };
        let id = *ids
            .block_states
            .borrow_mut()
            .entry(state)
            .or_insert_with(|| {
                let registry = self.pack.assets.registry;
                registry
                    .state(horizon_world::registry::BlockStateId(state))
                    .and_then(|s| {
                        ids.blocks
                            .id(&s.name, |k| s.properties.get(k).map(String::as_str))
                    })
                    .unwrap_or(ids.missing_block)
            });
        self.push_span(start, "BLOCK_ENTITIES", id);
        let emission = self
            .res
            .facts()
            .get(horizon_world::registry::BlockStateId(state))
            .emission;
        if let Some(last) = self.entity_spans.borrow_mut().last_mut() {
            last.emissive = emission > 0;
            if let Some(gpu) = self.skin_frame.borrow_mut().spans.last_mut() {
                gpu.emissive = emission > 0;
            }
        }
    }

    /// Starts the span of the local player's model; `hidden` in first person, where only a
    /// pack's shadow pass draws it.
    pub(super) fn mark_player(&self, start: usize, hidden: bool) {
        self.mark_entity(start, "minecraft:player");
        if (self.pack_ids.is_some() || native_spans())
            && let Some(last) = self.entity_spans.borrow_mut().last_mut()
        {
            last.player = true;
            last.hidden = hidden;
            if let Some(gpu) = self.skin_frame.borrow_mut().spans.last_mut() {
                gpu.player = true;
                gpu.hidden = hidden;
            }
        }
    }

    /// Marks vertices `start..end` (one entity) as hidden from the camera, for an entity
    /// Horizon's culling would drop but a pack's shadow pass still draws (`EntitySpan::hide`).
    pub(super) fn hide_spans_from(&self, start: usize, end: usize) -> bool {
        EntitySpan::hide(&mut self.entity_spans.borrow_mut(), start, end)
    }

    /// Starts a span of geometry that is not an entity (`PARTICLES`, `DESTROY`...).
    pub(super) fn mark_stage(&self, start: usize, stage: &str) {
        if self.pack_ids.is_some() || native_spans() {
            self.push_span(start, stage, 0);
        }
    }

    fn push_span(&self, start: usize, stage: &str, id: i32) {
        let mut spans = self.entity_spans.borrow_mut();
        let span = EntitySpan {
            start: start as u32,
            stage: horizon_shaders::render_stage(stage),
            id,
            ..Default::default()
        };
        if self.pack_ids.is_some() && self.skinning() {
            let mut skin = self.skin_frame.borrow_mut();
            let gpu = EntitySpan {
                start: skin.vertices,
                ..span
            };
            match skin.spans.last_mut() {
                Some(last) if last.start == gpu.start => *last = gpu,
                _ => skin.spans.push(gpu),
            }
        }
        // A span that drew nothing gives way to the next.
        match spans.last_mut() {
            Some(last) if last.start == span.start => *last = span,
            _ => spans.push(span),
        }
    }
}

/// `currentRenderedItemId` of an item (Iris 1.11.7, observed with probe packs): a block item
/// (`block`: its block's `block.properties` id, 0 when the file lacks it) reads that, whatever
/// `item.properties` says of it; any other item its `item.properties` id (`item`), and one the
/// file lacks 65535, or 0 when the pack has no `item.properties` (`item_file`).
fn rendered_item_id(block: Option<i32>, item: Option<i32>, item_file: bool) -> i32 {
    block.or(item).unwrap_or(if item_file { 65535 } else { 0 })
}

/// The block items' `currentRenderedItemId`: for each block's default state, the item it gives
/// (`StateFacts::item`) takes the state's id in `ids`, or 0. A block named as its item wins over
/// another block giving that item.
fn block_item_ids(
    facts: &horizon_client::blocks::BlockFacts,
    registry: &horizon_world::registry::BlockRegistry,
    ids: &BlockIds,
) -> HashMap<String, i32> {
    let mut out = HashMap::new();
    for (i, state) in facts.states.iter().enumerate() {
        let id = horizon_world::registry::BlockStateId(i as u32);
        if &*state.item == "minecraft:air" || registry.default_state(&state.name) != Some(id) {
            continue;
        }
        let value = registry
            .state(id)
            .and_then(|s| ids.id(&s.name, |k| s.properties.get(k).map(String::as_str)))
            .unwrap_or(0);
        if state.item == state.name {
            out.insert(state.item.to_string(), value);
        } else {
            out.entry(state.item.to_string()).or_insert(value);
        }
    }
    out
}

/// Spans for a native mod (`native_render::RenderNeeds::entity_spans`), without a pack.
fn native_spans() -> bool {
    horizon_client::gfx::native_render::render_needs().entity_spans
}

/// A span's id for entity type `name` without a pack: stable, so a mod can match runs from one
/// frame to the next.
fn name_id(name: &str) -> i32 {
    // FNV-1a.
    let mut hash: u32 = 0x811c_9dc5;
    for byte in name.bytes() {
        hash = (hash ^ u32::from(byte)).wrapping_mul(0x0100_0193);
    }
    hash as i32
}

/// Exact state identities for the GLSL terrain adapter; distinct from native material
/// tags, which are indexed by sprite layer (`block_id_table`).
fn state_material_table(pack: &super::PackResources, ids: &BlockIds) -> Vec<i32> {
    state_block_id_table(pack.assets.registry, ids, |id| {
        pack.context.tables.facts(id).map_or(0, |f| f.emission)
    })
}
/// Exact model/fluid identities and emitted light for each source block state.
fn state_block_id_table(
    registry: &horizon_world::registry::BlockRegistry,
    ids: &BlockIds,
    emission: impl Fn(horizon_world::registry::BlockStateId) -> u8,
) -> Vec<i32> {
    use horizon_world::{
        fluid::{FluidBlockBindings, FluidKind, FluidState},
        registry::BlockStateId,
    };
    let fluids = FluidBlockBindings::new(registry).ok();
    let stride = horizon_client::gfx::TERRAIN_STATE_WORDS;
    let mut table = vec![-1; registry.len() * stride];
    for i in 0..registry.len() {
        let Some(state) = registry.state(BlockStateId(i as u32)) else {
            continue;
        };
        table[stride * i] = ids
            .id(&state.name, |k| state.properties.get(k).map(String::as_str))
            .unwrap_or(-1);
        table[stride * i + 1] = 0;
        table[stride * i + 3] = 1;
        table[stride * i + 4] = i32::from(emission(BlockStateId(i as u32)).min(15));
        if let (Some(bindings), Ok(fluid)) = (&fluids, FluidState::from_block(state))
            && fluid.kind() != FluidKind::Empty
            && let Some(state) = registry.state(bindings.block(fluid))
        {
            table[stride * i + 2] = ids
                .id(&state.name, |k| state.properties.get(k).map(String::as_str))
                .unwrap_or(-1);
        }
    }
    table
}

/// Sprite material IDs for native packs: named blocks win, then the most common
/// model use. GLSL uses the exact state table above instead.
/// Votes from distinct blocks, so adding states cannot change a shared sprite's material.
#[derive(Default)]
struct BlockIdVotes<'a>(BTreeMap<(u32, i32), (bool, std::collections::BTreeSet<&'a str>)>);

impl<'a> BlockIdVotes<'a> {
    fn add(&mut self, layer: u32, id: i32, name: &'a str, sprite: Option<&str>) {
        // Containers share the still sprite. Its fluid owns it just as a block
        // named exactly like its sprite does; a lower container id must not win.
        let owner = sprite
            .and_then(|s| s.strip_prefix("block/"))
            .map(|s| match s {
                "lava_still" | "lava_flow" => "lava",
                "water_still" | "water_flow" => "water",
                _ => s,
            });
        let named = owner == Some(name.strip_prefix("minecraft:").unwrap_or(name));
        let v = self.0.entry((layer, id)).or_default();
        v.0 |= named;
        v.1.insert(name);
    }

    fn write(self, table: &mut [i32]) {
        let mut best = BTreeMap::new();
        for ((layer, id), (named, blocks)) in self.0 {
            let candidate = (named, blocks.len(), -id);
            let b = best.entry(layer).or_insert(candidate);
            if candidate > *b {
                *b = candidate;
            }
        }
        for (layer, (_, _, id)) in best {
            if let Some(slot) = table.get_mut(layer as usize * 2) {
                *slot = -id;
            }
        }
    }
}

#[cfg(test)]
mod block_id_tests {
    use super::BlockIdVotes;

    #[test]
    fn fluid_sprites_keep_their_fluid_material_when_a_container_uses_them() {
        let mut votes = BlockIdVotes::default();
        // Our runtime probe: cauldron 17, lava 29. The lower container id used to
        // win the still sprite's tie, while the flow sprite already read 29.
        for (layer, sprite) in [(0, "lava_still"), (1, "lava_flow")] {
            votes.add(
                layer,
                29,
                "minecraft:lava",
                Some(&format!("block/{sprite}")),
            );
        }
        votes.add(0, 17, "minecraft:lava_cauldron", Some("block/lava_still"));
        // Water's still sprite is also shared by containers and waterlogged models.
        votes.add(2, 31, "minecraft:water", Some("block/water_still"));
        votes.add(2, 17, "minecraft:water_cauldron", Some("block/water_still"));
        votes.add(
            2,
            17,
            "minecraft:other_container",
            Some("block/water_still"),
        );
        let mut table = [-1; 8];
        votes.write(&mut table);
        assert_eq!(table, [29, -1, 29, -1, 31, -1, -1, -1]);
    }

    #[test]
    fn shared_model_sprites_still_use_distinct_blocks_and_the_named_owner() {
        let mut votes = BlockIdVotes::default();
        for _ in 0..20 {
            votes.add(0, 3, "minecraft:many_states", Some("block/shared"));
        }
        votes.add(0, 9, "minecraft:one", Some("block/shared"));
        votes.add(0, 9, "minecraft:two", Some("block/shared"));
        votes.add(1, 20, "minecraft:oak_leaves", Some("block/oak_leaves"));
        votes.add(1, 3, "minecraft:one", Some("block/oak_leaves"));
        votes.add(1, 3, "minecraft:two", Some("block/oak_leaves"));
        let mut table = [-1; 4];
        votes.write(&mut table);
        assert_eq!(table, [9, -1, 20, -1]);
    }
}

/// `mc_Entity` (x, y) of each block sprite layer, as (id, fluid) pairs: terrain vertices carry
/// their sprite layer, not their block. A layer takes the id of the block named like its sprite
/// (`block/oak_leaves` for `oak_leaves`, and still/flow sprites for their fluid), else the id
/// most blocks drawing it share (ties: the lowest); -1 when no block drawing it has one.
/// y is 1 for the fluid sprites, 0 otherwise (`fluid_pairs`).
pub(super) fn block_id_table(pack: &super::PackResources, ids: &BlockIds) -> Vec<i32> {
    let layers = pack
        .render
        .sprite_layers
        .values()
        .max()
        .map_or(0, |l| l + 1) as usize;
    let sprite = |name: &str| pack.render.sprite_layers.get(name).copied();
    let fluids = ["water_still", "water_flow", "lava_still", "lava_flow"]
        .map(|f| sprite(&format!("minecraft:block/{f}")));
    let mut table = fluid_pairs(layers, fluids.into_iter().flatten());
    if ids.is_empty() {
        return table;
    }
    let registry = pack.assets.registry;
    let catalog = &pack.render.model_catalog;
    // (layer, id) -> (a block named like the sprite, the blocks).
    let mut votes = BlockIdVotes::default();
    for i in 0..registry.len() {
        let state_id = horizon_world::registry::BlockStateId(i as u32);
        let Some(state) = registry.state(state_id) else {
            continue;
        };
        let Some(id) = ids.id(&state.name, |k| state.properties.get(k).map(String::as_str)) else {
            continue;
        };
        let block = state.name.strip_prefix("minecraft:").unwrap_or(&state.name);
        let mut vote = |layer: u32| {
            votes.add(
                layer,
                id,
                &state.name,
                pack.layer_names.get(&layer).map(String::as_str),
            );
        };
        // Fluids draw from their own sprites.
        let fluid = match block {
            "water" => ["water_still", "water_flow"],
            "lava" => ["lava_still", "lava_flow"],
            _ => ["", ""],
        };
        for f in fluid.iter().filter(|f| !f.is_empty()) {
            if let Some(l) = sprite(&format!("minecraft:block/{f}")) {
                vote(l);
            }
        }
        let origin = horizon_core::position::BlockPos::new(0, 0, 0);
        for (_, model) in catalog.collect(state_id, origin).unwrap_or_default() {
            for face in &model.faces {
                vote(face.layer);
            }
        }
    }
    votes.write(&mut table);
    table
}

/// `layers` (id, fluid) pairs before the ids are known: id -1, and `mc_Entity.y` 1 for the
/// `fluids` layers, 0 for the others. Iris 1.11.7, with probe packs of ours (#10452), gave
/// terrain y 1 on water and lava, and 0 on every other block, whether `block.properties`
/// maps it or not.
fn fluid_pairs(layers: usize, fluids: impl IntoIterator<Item = u32>) -> Vec<i32> {
    let mut table: Vec<i32> = std::iter::repeat_n([-1, 0], layers).flatten().collect();
    for l in fluids {
        if let Some(slot) = table.get_mut(l as usize * 2 + 1) {
            *slot = 1;
        }
    }
    table
}

#[cfg(test)]
mod integration_tests {
    use super::*;
    #[test]
    fn source_parser_panics_become_errors_without_losing_load_errors() {
        let error = parse_source::<()>(|| panic!("broken source")).unwrap_err();
        assert!(error.starts_with(SOURCE_PANIC));
        assert!(error.contains("broken source"));
        assert_eq!(
            parse_source::<()>(|| Err("missing pack".into())).unwrap_err(),
            "missing pack"
        );
        assert_eq!(parse_source(|| Ok(7)), Ok(7));
    }
    #[test]
    fn shutdown_waits_for_parser_and_discards_its_result() {
        let (release, wait) = std::sync::mpsc::channel();
        let (send, result) = std::sync::mpsc::channel();
        let worker = std::thread::spawn(move || {
            wait.recv().unwrap();
            send.send(Err("cancelled result".into())).unwrap();
        });
        PENDING_LOAD.with_borrow_mut(|pending| {
            *pending = Some(PendingLoad {
                key: LoadKey {
                    path: "shutdown.zip".into(),
                    resources: "resources".into(),
                    dimension: 0,
                    dimension_name: "minecraft:overworld".into(),
                    context: (0, 0),
                    options_revision: 0,
                },
                result,
                worker: Some(worker),
            })
        });
        assert!(!shutdown_parser());
        release.send(()).unwrap();
        while PENDING_LOAD.with_borrow(|pending| {
            !pending
                .as_ref()
                .unwrap()
                .worker
                .as_ref()
                .unwrap()
                .is_finished()
        }) {
            std::thread::yield_now();
        }
        assert!(shutdown_parser());
        assert!(!parser_busy());
        assert!(shutdown_parser());
    }
    #[test]
    fn block_materials_use_exact_states_and_waterlogged_fluid_identity() {
        use horizon_world::{chunk::BlockState, nbt::Tag, registry::BlockRegistry};
        use std::collections::BTreeMap;
        let mut states = Vec::new();
        let mut defaults = BTreeMap::new();
        let mut add = |name: &str, properties: BTreeMap<String, String>| {
            let id = states.len() as i32;
            defaults.entry(name.into()).or_insert(Tag::Int(id));
            states.push(
                BlockState {
                    name: name.into(),
                    properties: properties.into(),
                }
                .to_nbt(),
            );
        };
        add(
            "minecraft:oak_log",
            BTreeMap::from([("axis".into(), "x".into())]),
        );
        add(
            "minecraft:oak_log",
            BTreeMap::from([("axis".into(), "y".into())]),
        );
        add(
            "minecraft:oak_stairs",
            BTreeMap::from([("waterlogged".into(), "true".into())]),
        );
        add("minecraft:air", BTreeMap::new());
        for name in ["minecraft:water", "minecraft:lava"] {
            for level in 0..16 {
                add(name, BTreeMap::from([("level".into(), level.to_string())]));
            }
        }
        let registry = BlockRegistry::from_nbt(&Tag::Compound(BTreeMap::from([
            ("DataVersion".into(), Tag::Int(horizon_world::DATA_VERSION)),
            ("states".into(), Tag::List(states)),
            ("defaults".into(), Tag::Compound(defaults)),
        ])))
        .unwrap();
        let ids = BlockIds::parse(&Properties::parse(
            "block.101=minecraft:oak_log:axis=x\nblock.202=minecraft:oak_log:axis=y\nblock.303=minecraft:water:level=0\nblock.404=minecraft:water:level=7\nblock.505=minecraft:oak_stairs",
            &Macros::standard(),
        ));
        assert!(ids.notes.is_empty(), "{:?}", ids.notes);
        let table = state_block_id_table(&registry, &ids, |id| if id.0 == 0 { 12 } else { 3 });
        assert_eq!(table.len(), registry.len() * 5);
        assert!(
            table
                .chunks_exact(5)
                .all(|state| state[1] == 0 && state[3] == 1)
        );
        assert_eq!(&table[..10], &[101, 0, -1, 1, 12, 202, 0, -1, 1, 3]);
        assert_eq!(&table[10..15], &[505, 0, 303, 1, 3]);
        assert_eq!(&table[(4 + 7) * 5..(4 + 8) * 5], &[404, 0, 404, 1, 3]);
    }
    #[test]
    fn failed_native_reload_reports_the_active_previous_programs() {
        record_report("Example.hshader", 0, &["sky".into(), "post".into()], &[]);
        record_reload_error("Example.hshader", 0, &["bad shader".into()]);
        REPORT.with_borrow(|report| {
            let report = report.as_ref().unwrap();
            assert_eq!(report["programs"], json!(["sky", "post"]));
            assert!(
                report["notes"][0]
                    .as_str()
                    .unwrap()
                    .contains("remain active")
            );
        });
        REPORT.with_borrow_mut(Option::take);
        record_reload_error("Other.hshader", 0, &["bad initial shader".into()]);
        REPORT.with_borrow(|report| assert_eq!(report.as_ref().unwrap()["programs"], json!([])));
        REPORT.with_borrow_mut(Option::take);
    }
    #[test]
    fn parser_results_are_nonblocking_and_collected_once() {
        let (send, result) = std::sync::mpsc::channel();
        let key = LoadKey {
            path: "test.zip".into(),
            resources: "resources".into(),
            dimension: 0,
            dimension_name: "minecraft:overworld".into(),
            context: (1, 2),
            options_revision: 0,
        };
        PENDING_LOAD.with_borrow_mut(|pending| {
            *pending = Some(PendingLoad {
                key: key.clone(),
                result,
                worker: None,
            })
        });
        assert!(poll_parser().is_none());
        send.send(Err("test error".into()))
            .unwrap_or_else(|_| panic!("receiver dropped"));
        let (at, result) = poll_parser().unwrap();
        assert!(at == key && result.is_err());
        assert!(!parser_busy());
        assert!(poll_parser().is_none());
    }
}

#[cfg(test)]
mod rendered_item_tests {
    use super::rendered_item_id;

    /// Iris 1.11.7's values (probe packs of ours): a block item reads its block's id, and 0 when
    /// `block.properties` lacks it even if `item.properties` names it (oak planks as item 15);
    /// other items read their `item.properties` id, or 65535 when the file lacks them, and 0
    /// when the pack has no such file.
    #[test]
    fn a_held_item_reads_its_block_id_or_item_id() {
        // Stone: block 21 and item 12.
        assert_eq!(rendered_item_id(Some(21), Some(12), true), 21);
        assert_eq!(rendered_item_id(Some(0), Some(15), true), 0);
        assert_eq!(rendered_item_id(Some(0), None, true), 0);
        assert_eq!(rendered_item_id(Some(0), None, false), 0);
        assert_eq!(rendered_item_id(None, Some(11), true), 11);
        assert_eq!(rendered_item_id(None, None, true), 65535);
        assert_eq!(rendered_item_id(None, None, false), 0);
    }
}

#[cfg(test)]
mod native_pack_tests {
    use super::{active_pack, pack_draws_flat};
    use horizon_client::mod_list::{Installed, ModList, NativePage};

    #[test]
    fn shaders_packs_and_packs_with_a_shadow_pass_draw_flat() {
        let root = std::env::temp_dir().join(format!("horizon-flat-packs-{}", std::process::id()));
        // A folder without a manifest is a `shaders/` pack; with one, a pack of Horizon's own.
        let pack = |name: &str, manifest: Option<&str>| {
            let dir = root.join(name);
            std::fs::create_dir_all(&dir).unwrap();
            if let Some(extra) = manifest {
                std::fs::write(dir.join("material.wgsl"), "").unwrap();
                let text = format!(
                    "schema = 1\nid = \"{name}\"\nname = \"{name}\"\nversion = \"1\"\napi = \"{}\"\n\
                     [material]\nfile = \"material.wgsl\"\n{extra}",
                    horizon_shader_api::API
                );
                std::fs::write(dir.join(horizon_shader_api::MANIFEST), text).unwrap();
            }
            dir
        };
        assert!(pack_draws_flat(&pack("shaders_pack", None)));
        assert!(!pack_draws_flat(&pack("unshadowed", Some(""))));
        assert!(pack_draws_flat(&pack("shadowed", Some("[shadow]\n"))));
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn native_shader_follows_its_mod_without_affecting_other_packs() {
        let mut mods = ModList::default();
        mods.installed.push(Installed {
            id: "horizon_better_visuals".into(),
            title: "Better Visuals".into(),
            description: String::new(),
            problem: None,
            dev: false,
            native: Some(NativePage::default()),
        });
        let owned = "native-horizon_better_visuals";
        assert_eq!(active_pack(owned, &mods), owned);
        mods.disabled.insert("horizon_better_visuals".into());
        assert_eq!(active_pack(owned, &mods), "");
        assert_eq!(active_pack("golden_hour", &mods), "golden_hour");
        mods.installed[0].native = None;
        assert_eq!(active_pack(owned, &mods), owned);
    }
}

#[cfg(test)]
mod fluid_pair_tests {
    use super::fluid_pairs;
    #[test]
    fn terrain_reads_y_1_on_fluids_and_0_elsewhere() {
        assert_eq!(fluid_pairs(4, [1, 3]), [-1, 0, -1, 1, -1, 0, -1, 1]);
        assert_eq!(fluid_pairs(2, []), [-1, 0, -1, 0]);
    }
}
