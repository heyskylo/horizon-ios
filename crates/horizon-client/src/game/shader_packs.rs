//! Shader packs: the one picked on Options > Horizon Settings > Shader Packs, or
//! `HORIZON_SHADER_PACK=<name>` (development; it wins over the pick), runs `shaderpacks/<name>`
//! (a folder or a zip beside `options.txt`) over the world; Horizon's own packs go to
//! `game/native_shaders.rs`. The programs of the current dimension's folder are loaded when the
//! dimension changes (spec 1.2).

use horizon_client::camera::CameraFluid;
use horizon_client::gfx::pack_uniforms::PackInputs;
use horizon_client::text::Text;
use horizon_shaders::{BlockIds, Macros, Pack, Properties, dimension_folder, load_post};

use super::{Dim, Game};
use horizon_client::gfx::pack_draw::EntitySpan;
use std::cell::RefCell;
use std::collections::HashMap;

use serde_json::{Value, json};

pub(super) const HELP: &[&str] =
    &["shader-pack [list|off|NAME]    list or select a shader pack in shaderpacks/"];
thread_local! {
    static REPORT: RefCell<Option<Value>> = const { RefCell::new(None) };
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
fn chosen(settings: &horizon_client::settings::Settings) -> &str {
    static DEV_PACK: std::sync::OnceLock<Option<String>> = std::sync::OnceLock::new();
    DEV_PACK
        .get_or_init(|| std::env::var("HORIZON_SHADER_PACK").ok())
        .as_deref()
        .unwrap_or(&settings.horizon.shader_pack)
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
    /// `oldHandLight` (3.2): the first held item and light are those of the brighter hand.
    old_hand_light: bool,
}

#[derive(Clone, PartialEq, Eq)]
struct LoadKey {
    path: std::path::PathBuf,
    dimension: i32,
    context: (u64, u64),
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

fn start_parser(key: LoadKey, inputs: horizon_shaders::GameInputs) -> Result<(), String> {
    let path = key.path.clone();
    let dimension = key.dimension;
    let (sender, result) = std::sync::mpsc::channel();
    let _worker = std::thread::Builder::new()
        .name("Shader source".into())
        .stack_size(16 * 1024 * 1024)
        .spawn(move || {
            let parsed = parse_source(|| {
                let pack = Pack::open(&path).map_err(|error| error.to_string())?;
                let macros = Macros::standard();
                let properties =
                    Properties::from_pack(&pack, &dimension_folder(&pack, dimension), &macros)
                        .in_dimension(dimension);
                let mut post = load_post(
                    &pack,
                    &dimension_folder(&pack, dimension),
                    &properties,
                    &macros,
                    &inputs,
                );
                post.notes.extend(
                    properties
                        .notes
                        .iter()
                        .map(|note| format!("shaders.properties:{}: {}", note.line, note.message)),
                );
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
                    items: item_ids.plain(),
                    missing_item: if item_text.is_some() { -1 } else { 0 },
                    old_hand_light: properties
                        .get("oldHandLight")
                        .is_none_or(|value| value.trim() != "false"),
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
                }
                record_report(&self.native_shaders.picked, dimension, &programs, &notes);
                for note in &notes {
                    log::warn!("shader pack: {note}");
                }
            }
            let desired = LoadKey {
                path: Self::shader_packs_dir().join(chosen(&self.settings)),
                dimension,
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
    }

    /// Loads the pack for the current dimension when it changed, and returns this frame's
    /// inputs to the pack's uniforms.
    pub(super) fn shader_pack_inputs(&mut self, camera: glam::DVec3, partial: f32) -> PackInputs {
        self.poll_shader_workers();
        let completed = poll_parser();
        let picked = chosen(&self.settings);
        if self.native_shaders.picked != picked {
            REPORT.with_borrow_mut(Option::take);
            // Another pack, or none: drop what ran.
            self.native_shaders = super::native_shaders::NativeShaders::picking(picked);
            self.pack_ids = None;
            if let Some(renderer) = &mut self.renderer {
                renderer.drop_shader_packs();
            }
        }
        if picked.is_empty() {
            return PackInputs::default();
        }
        let name = picked.to_owned();
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
            let key = LoadKey {
                path: path.clone(),
                dimension,
                context: renderer.shader_context_key(),
            };
            let ready = completed.and_then(|(at, result)| (at == key).then_some(result));
            if let Some(result) = ready {
                match result {
                    Ok(mut parsed) => {
                        let biomes = self.res.biomes();
                        let names: Vec<&str> = biomes
                            .list
                            .iter()
                            .map(|biome| biome.name.as_str())
                            .collect();
                        parsed.post.custom.set_biomes(&names);
                        let table = block_id_table(&self.pack, &parsed.ids);
                        self.pack_ids = parsed.post.complete.then_some(parsed.entity_ids);
                        if let Some((programs, notes)) =
                            renderer.set_shader_pack(dimension, Some(parsed.post), &table)
                        {
                            record_report(&name, dimension, &programs, &notes);
                        }
                    }
                    Err(error) => {
                        if error.starts_with(SOURCE_PANIC) {
                            self.shader_pack_failed(&name, dimension, &error);
                        } else {
                            self.pack_ids = None;
                            renderer.set_shader_pack(dimension, None, &[]);
                        }
                        record_report(&name, dimension, &[], &[error]);
                    }
                }
            } else if renderer.shader_pack_dimension() != Some(dimension) && !parser_busy() {
                let inputs = renderer.pack_inputs();
                match start_parser(key,inputs) {
                    Ok(())=>record_report(&name,dimension,&[],&["Processing shader sources in the background. Normal rendering remains active until ready.".into()]),
                    Err(error)=>{renderer.set_shader_pack(dimension,None,&[]);record_report(&name,dimension,&[],&[error]);}
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
        let info = biome.map(|b| self.res.biomes().get(b));
        let (end_flash_position, end_flash_intensity) = self.end_flash_for_pack(partial);
        PackInputs {
            day_time: self.world.day_time,
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
            biome: biome.map_or(-1, i32::from),
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
            // Same cloud plane as clouds::Input::uniform; independent of the Clouds option.
            cloud_height: if self.dim == Dim::Overworld {
                192.33
            } else {
                f32::NAN
            },
            heavy_fog: self.boss_overlay.should_create_world_fog(),
            end_flash_position,
            end_flash_intensity,
        }
    }

    /// Turns off the pack `name` after loading it panicked: nothing runs in its place, it is no
    /// longer the picked pack (so the next start does not load it again), and the chat says so.
    fn shader_pack_failed(&mut self, name: &str, dimension: i32, error: &str) {
        log::error!("shader pack {name}: turned off: {error}");
        self.pack_ids = None;
        if let Some(renderer) = &mut self.renderer {
            // Marks this dimension as loaded, without a pack, so it is not tried again.
            renderer.set_shader_pack(dimension, None, &[]);
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

    /// `heldItemId(2)` and `heldBlockLightValue(2)`: main hand, then off hand (5.2, 5.3).
    fn held_for_pack(&mut self) -> ([i32; 2], [i32; 2]) {
        if self.pack_ids.is_none() {
            return ([0; 2], [0; 2]);
        }
        if self.luminance.is_none() {
            self.luminance = Some(horizon_client::dynamic_light::Luminance::new(
                self.res.facts(),
                self.pack.assets.registry,
            ));
        }
        let (Some(ids), Some(lum)) = (&self.pack_ids, &self.luminance) else {
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
    /// Resolves the rendered stack, using block IDs for block items as required
    /// by the shader API. This is unrelated to either currently equipped hand.
    pub(super) fn rendered_item_id(&self, item: &str) -> i32 {
        let Some(ids) = &self.pack_ids else { return 0 };
        if let Some(state) = self.res.items().get(item).and_then(|item| item.block) {
            return self
                .pack
                .assets
                .registry
                .state(state)
                .and_then(|state| {
                    ids.blocks.id(&state.name, |key| {
                        state.properties.get(key).map(String::as_str)
                    })
                })
                .unwrap_or(ids.missing_block);
        }
        ids.items.get(item).copied().unwrap_or(ids.missing_item)
    }

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
        if let Some(ids) = &self.pack_ids {
            let id = ids.entities.get(name).copied();
            self.push_span(start, "ENTITIES", id.unwrap_or(ids.missing_entity));
            if let Some(last) = self.entity_spans.borrow_mut().last_mut() {
                last.translucent = translucent;
            }
        }
    }

    /// Starts a span for the block entity of block state `state`.
    pub(super) fn mark_block_entity(&self, start: usize, state: u32) {
        let Some(ids) = &self.pack_ids else { return };
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
        }
    }

    /// Starts the span of the local player's model; `hidden` in first person, where only a
    /// pack's shadow pass draws it.
    pub(super) fn mark_player(&self, start: usize, hidden: bool) {
        self.mark_entity(start, "minecraft:player");
        if self.pack_ids.is_some()
            && let Some(last) = self.entity_spans.borrow_mut().last_mut()
        {
            last.player = true;
            last.hidden = hidden;
        }
    }

    /// Marks vertices `start..end` (one entity) as hidden from the camera, for an entity
    /// Horizon's culling would drop but a pack's shadow pass still draws (`EntitySpan::hide`).
    pub(super) fn hide_spans_from(&self, start: usize, end: usize) -> bool {
        EntitySpan::hide(&mut self.entity_spans.borrow_mut(), start, end)
    }

    /// Starts a span of geometry that is not an entity (`PARTICLES`, `DESTROY`...).
    pub(super) fn mark_stage(&self, start: usize, stage: &str) {
        if self.pack_ids.is_some() {
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
        // A span that drew nothing gives way to the next.
        match spans.last_mut() {
            Some(last) if last.start == span.start => *last = span,
            _ => spans.push(span),
        }
    }
}

/// `mc_Entity` (x, y) of each block sprite layer, as (id, fluid) pairs: terrain vertices carry
/// their sprite layer, not their block. A layer takes the id of the block named like its sprite
/// (`block/oak_leaves` for `oak_leaves`), else the id most blocks drawing it share (ties: the
/// lowest); -1 when no block drawing it has one. y is 1 for the fluid sprites, -1 otherwise.
fn block_id_table(pack: &super::PackResources, ids: &BlockIds) -> Vec<i32> {
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
                    dimension: 0,
                    context: (0, 0),
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
                    properties,
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
        assert_eq!(&table[..10], &[101, -1, -1, 1, 12, 202, -1, -1, 1, 3]);
        assert_eq!(&table[10..15], &[505, -1, 303, 1, 3]);
        assert_eq!(&table[(4 + 7) * 5..(4 + 8) * 5], &[404, -1, 404, 1, 3]);
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
            dimension: 0,
            context: (1, 2),
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
