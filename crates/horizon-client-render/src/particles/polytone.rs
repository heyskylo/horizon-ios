//! Resource packs' Polytone custom particles and block particle emitters (#11486, wiki "Custom
//! Particle Types" and "Block Properties Modifiers"): a particle whose start, motion, look and
//! death are expressions, and emitters that spawn particles from blocks and from particles. Each
//! custom particle is an ordinary engine particle (sprite set, interpolation, rendering, the
//! total limit) with a controller that runs the type's ticker.
use super::{
    Behaviour, Fluid, Particle, ParticleEngine, ParticleLevel, ParticleOptions, ParticleType,
    ParticleUpdate, Sprite,
};
use crate::environment::Frame;
use horizon_core::noise::SimplexNoise;
use horizon_core::random::{LegacyRandom, RandomSource};
use horizon_polytone::expr::{Host, Program, Root, Value as V};
use horizon_polytone::particles::{
    BlockEmitters, CustomParticle, Emitter, EntityEmitters, LiquidAffinity, RenderType,
};
use horizon_world::registry::BlockRegistry;
use serde_json::Value as Json;
use std::collections::{BTreeMap, HashMap};
use std::path::Path;
use std::sync::{Arc, Mutex, RwLock};

/// The packs' custom particle types and block emitters, and what their expressions read beyond
/// the particle and its blocks.
pub struct Registry {
    types: HashMap<String, Arc<CustomParticle>>,
    /// Block state id → the highest priority block modifiers with emitters aimed at its block.
    block_emitters: Vec<Option<Arc<[Arc<BlockEmitters>]>>>,
    /// The entity modifiers with emitters, and by entity type the ones aimed at it.
    entity_modifiers: Vec<Arc<EntityEmitters>>,
    entity_emitters: Mutex<HashMap<String, Arc<[Arc<EntityEmitters>]>>>,
    /// The world's biomes, for `o.biome()` and emitters' `biomes` (what the game last set).
    biomes: RwLock<Option<Arc<crate::biome::Biomes>>>,
    /// The frame's global, player and camera context (what the game last set).
    pub frame: Arc<RwLock<Frame>>,
    configs: horizon_polytone::pack::Configs,
    blocks: &'static BlockRegistry,
    noise: SimplexNoise,
}

/// Every `.json` under `dir`, with its path relative to `root`.
fn json_files(root: &Path, dir: &Path) -> Vec<(String, Json)> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            if path.extension().is_none_or(|e| e != "json") {
                continue;
            }
            let Ok(relative) = path.strip_prefix(root) else {
                continue;
            };
            let relative = relative.to_string_lossy().into_owned();
            match horizon_world::json_file::read::<Json>(&path) {
                Ok(json) => out.push((relative, json)),
                Err(e) => log::warn!("{relative}: {e}"),
            }
        }
    }
    out
}

impl Registry {
    /// Every `assets/<namespace>/polytone/{custom_particles,block_modifiers,entity_modifiers}/**.json` under a
    /// pack overlay root.
    pub fn read(
        pack_root: &Path,
        configs: horizon_polytone::pack::Configs,
        blocks: &'static BlockRegistry,
    ) -> Self {
        let mut types = HashMap::new();
        let mut modifiers = Vec::new();
        let mut entity_modifiers = Vec::new();
        let namespaces = std::fs::read_dir(pack_root.join("assets"))
            .into_iter()
            .flatten()
            .flatten();
        for namespace in namespaces {
            let polytone = namespace.path().join("polytone");
            for (relative, json) in json_files(pack_root, &polytone.join("custom_particles")) {
                let Some(id) = horizon_polytone::implicit_id(&relative, "custom_particles") else {
                    continue;
                };
                match CustomParticle::parse(&json) {
                    Ok(particle) => {
                        types.insert(id, Arc::new(particle));
                    }
                    Err(e) => log::warn!("{relative}: {e}"),
                }
            }
            for (relative, json) in json_files(pack_root, &polytone.join("entity_modifiers")) {
                let implicit = horizon_polytone::implicit_id(&relative, "entity_modifiers");
                match EntityEmitters::parse(&json, implicit.as_deref()) {
                    Ok(Some(m)) if !m.emitters.is_empty() => entity_modifiers.push(Arc::new(m)),
                    Ok(_) => {}
                    Err(e) => log::warn!("{relative}: {e}"),
                }
            }
            for (relative, json) in json_files(pack_root, &polytone.join("block_modifiers")) {
                let implicit = horizon_polytone::implicit_id(&relative, "block_modifiers");
                match BlockEmitters::parse(&json, implicit.as_deref()) {
                    Ok(Some(m)) if !m.particle_emitters.is_empty() => modifiers.push(Arc::new(m)),
                    Ok(_) => {}
                    Err(e) => log::warn!("{relative}: {e}"),
                }
            }
        }
        // Each block's modifiers: the highest priority ones (block tags aren't resolved here).
        let mut by_name: HashMap<&str, Option<Arc<[Arc<BlockEmitters>]>>> = HashMap::new();
        let block_emitters = (0..blocks.len() as u32)
            .map(|i| {
                let name = blocks
                    .state(horizon_world::registry::BlockStateId(i))?
                    .name
                    .as_str();
                by_name
                    .entry(name)
                    .or_insert_with(|| {
                        let aimed: Vec<_> = modifiers
                            .iter()
                            .filter(|m| m.targets.matches(name, |_| false))
                            .collect();
                        let top = aimed.iter().map(|m| m.priority).max()?;
                        Some(
                            aimed
                                .into_iter()
                                .filter(|m| m.priority == top)
                                .cloned()
                                .collect(),
                        )
                    })
                    .clone()
            })
            .collect();
        Self {
            types,
            block_emitters,
            entity_modifiers,
            entity_emitters: Mutex::default(),
            biomes: RwLock::default(),
            frame: Arc::new(RwLock::new(Frame::default())),
            configs,
            blocks,
            noise: SimplexNoise::new(&mut LegacyRandom::new(0)),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.types.is_empty()
            && self.block_emitters.iter().all(Option::is_none)
            && self.entity_modifiers.is_empty()
    }

    pub fn get(&self, id: &str) -> Option<&Arc<CustomParticle>> {
        self.types.get(id)
    }

    /// Sets the world's biome table (the world's own, or the default one).
    pub fn set_biomes(&self, biomes: &Arc<crate::biome::Biomes>) {
        let same = self
            .biomes
            .read()
            .is_ok_and(|b| b.as_ref().is_some_and(|b| Arc::ptr_eq(b, biomes)));
        if !same && let Ok(mut b) = self.biomes.write() {
            *b = Some(biomes.clone());
        }
    }

    /// The entity modifiers aimed at an entity type (`minecraft:player`); entity tags aren't
    /// resolved here.
    fn entity_emitters(&self, entity: &str) -> Arc<[Arc<EntityEmitters>]> {
        let mut cache = self
            .entity_emitters
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        cache
            .entry(entity.to_owned())
            .or_insert_with(|| {
                self.entity_modifiers
                    .iter()
                    .filter(|m| m.targets.matches(entity, |_| false))
                    .cloned()
                    .collect()
            })
            .clone()
    }
}

impl std::fmt::Debug for Registry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Registry({} types, {} emitting states)",
            self.types.len(),
            self.block_emitters.iter().filter(|e| e.is_some()).count()
        )
    }
}

/// What an emitter seeds on the custom particle it spawns: its own `red`... `custom` and customs.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Seed {
    pub fields: BTreeMap<String, f64>,
    pub customs: BTreeMap<String, f64>,
}

/// A custom particle's state beyond the engine particle's.
#[derive(Clone)]
struct Controller {
    def: Arc<CustomParticle>,
    registry: Arc<Registry>,
    custom: f64,
    customs: BTreeMap<String, f64>,
    ticks: u32,
    /// Sprites in its set, for the age-driven frame.
    frames: usize,
}

impl std::fmt::Debug for Controller {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Controller")
            .field("custom", &self.custom)
            .field("customs", &self.customs)
            .finish()
    }
}

const SELF: u32 = 1;
const PLAYER: u32 = 2;
const CAMERA: u32 = 3;
const GLOBAL: u32 = 4;
const RANDOM: u32 = 5;
/// Block objects (`o.relative(...)`) are ids from here, indexes into `Ctx::blocks`.
const BLOCKS: u32 = 16;

/// What `o` is: a custom particle (tickers, particle emitters), a block (block emitters) or an
/// entity (entity emitters).
enum Subject<'a> {
    Particle(&'a mut Particle),
    Block([i32; 3]),
    Entity([f64; 3]),
}

/// What one expression runs against: its subject, the frame and the level.
struct Ctx<'a> {
    subject: Subject<'a>,
    random: &'a mut LegacyRandom,
    custom: &'a mut f64,
    customs: &'a mut BTreeMap<String, f64>,
    frame: &'a Frame,
    registry: &'a Registry,
    level: Option<&'a ParticleLevel<'a>>,
    blocks: Vec<[i32; 3]>,
}

/// The fluid at `at` (none for a state the facts lack).
fn fluid_at(level: &ParticleLevel, at: [i32; 3]) -> Fluid {
    let state = level.world.block(at[0], at[1], at[2]);
    level
        .facts
        .states
        .get(state.0 as usize)
        .map_or(Fluid::None, |f| f.fluid)
}

fn axis(name: &str) -> usize {
    usize::from(name.as_bytes()[0] - b'x')
}

impl Ctx<'_> {
    fn position(&self) -> [f64; 3] {
        match &self.subject {
            Subject::Particle(p) => p.position,
            Subject::Block(b) => b.map(f64::from),
            Subject::Entity(at) => *at,
        }
    }
    /// The biome at a block (`minecraft:plains`), when the level and biomes are known.
    fn biome(&self, at: [i32; 3]) -> Option<String> {
        let chunk = self.level?.world.chunks.get(&(at[0] >> 4, at[2] >> 4))?;
        let id = chunk.biome(at[0], at[1], at[2]);
        let biomes = self.registry.biomes.read().ok()?;
        Some(biomes.as_ref()?.get(id).name.to_string())
    }
    fn block_object(&mut self, at: [i32; 3]) -> V {
        self.blocks.push(at);
        V::Object(BLOCKS + self.blocks.len() as u32 - 1)
    }
    fn rand(&mut self) -> f64 {
        f64::from(self.random.next_float())
    }
    /// The positional methods of the subject's block and of block objects.
    fn positional(&mut self, at: [i32; 3], name: &str, args: &[V]) -> Result<V, String> {
        let num = |n: f64| Ok(V::Num(n));
        if name == "biome" {
            return Ok(V::Str(self.biome(at).unwrap_or_default().into()));
        }
        if name == "temperature" {
            return num(f64::from(self.frame.temperature));
        }
        if name == "downfall" {
            return num(f64::from(self.frame.downfall));
        }
        let arg = |i: usize| args.get(i).map_or(0.0, V::num) as i32;
        let neighbour = match name {
            "relative" => Some([at[0] + arg(0), at[1] + arg(1), at[2] + arg(2)]),
            "north" => Some([at[0], at[1], at[2] - 1]),
            "south" => Some([at[0], at[1], at[2] + 1]),
            "west" => Some([at[0] - 1, at[1], at[2]]),
            "east" => Some([at[0] + 1, at[1], at[2]]),
            "up" => Some([at[0], at[1] + 1, at[2]]),
            "down" => Some([at[0], at[1] - 1, at[2]]),
            _ => None,
        };
        if let Some(n) = neighbour {
            return Ok(self.block_object(n));
        }
        let level = self.level.ok_or("no level here")?;
        let state = level.world.block(at[0], at[1], at[2]);
        let fluid = fluid_at(level, at);
        let light = level.world.light(at[0], at[1], at[2]);
        match name {
            "block" => Ok(V::Str(
                self.registry
                    .blocks
                    .state(state)
                    .map_or("minecraft:air", |s| s.name.as_str())
                    .into(),
            )),
            "hasAirAt" => Ok(V::Bool(state == level.facts.air)),
            "hasFluid" => Ok(V::Bool(fluid != Fluid::None)),
            "fluid" => Ok(V::Str(
                match fluid {
                    Fluid::Water => "minecraft:water",
                    Fluid::Lava => "minecraft:lava",
                    Fluid::None => "minecraft:empty",
                }
                .into(),
            )),
            "skyLight" => num(f64::from(light >> 4)),
            "blockLight" => num(f64::from(light & 15)),
            "canSeeSky" => Ok(V::Bool(light >> 4 == 15)),
            "blockStateValue" => {
                let Some(V::Str(property)) = args.first() else {
                    return Err("blockStateValue needs a property".into());
                };
                let value = self
                    .registry
                    .blocks
                    .state(state)
                    .and_then(|s| s.properties.get(property.as_ref()));
                Ok(match value {
                    Some(v) => v
                        .parse::<f64>()
                        .map_or_else(|_| V::Str(v.as_str().into()), V::Num),
                    None => V::Null,
                })
            }
            _ => Err(format!("{name} is not a block method")),
        }
    }
    /// `o.<name>(...)` on a custom particle: its fields, setters and named customs.
    fn particle_method(&mut self, name: &str, args: &[V]) -> Option<Result<V, String>> {
        let value = args.first().map(V::num);
        let custom = &mut *self.custom;
        let customs = &mut *self.customs;
        let Subject::Particle(p) = &mut self.subject else {
            return None;
        };
        let f = |n: f64| Some(Ok(V::Num(n)));
        let set = |slot: &mut f32| {
            if let Some(v) = value {
                *slot = v as f32;
            }
            Some(Ok(V::Num(f64::from(*slot))))
        };
        match name {
            "age" => f(f64::from(p.age)),
            "life" => f(f64::from(p.lifetime)),
            "x" | "y" | "z" => f(p.position[axis(name)]),
            "xd" | "yd" | "zd" => f(p.velocity[axis(name)]),
            "setX" | "setY" | "setZ" => {
                p.position[usize::from(name.as_bytes()[3] - b'X')] = value?;
                f(value?)
            }
            "setXd" | "setYd" | "setZd" => {
                p.velocity[usize::from(name.as_bytes()[3] - b'X')] = value?;
                f(value?)
            }
            "red" | "setRed" => set(&mut p.rgba[0]),
            "green" | "setGreen" => set(&mut p.rgba[1]),
            "blue" | "setBlue" => set(&mut p.rgba[2]),
            "alpha" | "setAlpha" => set(&mut p.rgba[3]),
            "roll" | "setRoll" => set(&mut p.roll),
            "size" | "setSize" => set(&mut p.size),
            "color" => {
                let c = |i: usize| (p.rgba[i].clamp(0.0, 1.0) * 255.0) as u32;
                f(f64::from(c(0) << 16 | c(1) << 8 | c(2)))
            }
            "setRemoved" | "removed" => {
                p.remove();
                Some(Ok(V::Bool(true)))
            }
            "custom" | "setCustom" => Some(match args {
                [] => Ok(V::Num(*custom)),
                [V::Str(n)] => Ok(V::Num(
                    customs.get(&n.to_lowercase()).copied().unwrap_or(0.0),
                )),
                [V::Str(n), v] => {
                    customs.insert(n.to_lowercase(), v.num());
                    Ok(V::Num(v.num()))
                }
                [v] => {
                    *custom = v.num();
                    Ok(V::Num(v.num()))
                }
                _ => Err("custom takes a name and a value at most".into()),
            }),
            _ => None,
        }
    }
}

impl Host for Ctx<'_> {
    fn root(&mut self, root: Root) -> V {
        match root {
            Root::Object => V::Object(SELF),
            Root::Player => V::Object(PLAYER),
            Root::Camera => V::Object(CAMERA),
            Root::Global => V::Object(GLOBAL),
            Root::Random => V::Object(RANDOM),
            Root::Value => V::Num(0.0),
        }
    }

    fn method(&mut self, target: &V, name: &str, args: &[V]) -> Result<V, String> {
        let V::Object(id) = target else {
            return Err(format!("{name} on a value that is no object"));
        };
        let num = |n: f64| Ok(V::Num(n));
        let frame = self.frame;
        let other = |v: Option<&V>| match v {
            Some(V::Object(PLAYER)) => Some(frame.player),
            Some(V::Object(CAMERA)) => Some(frame.camera),
            _ => None,
        };
        // Positional objects all measure distances the same way.
        if matches!(name, "distSq" | "dist") {
            let from = match *id {
                SELF => self.position(),
                PLAYER => frame.player,
                CAMERA => frame.camera,
                b if b >= BLOCKS => self
                    .blocks
                    .get((b - BLOCKS) as usize)
                    .ok_or("no such block object")?
                    .map(f64::from),
                _ => return Err(format!("{name} on no position")),
            };
            let to =
                other(args.first()).ok_or_else(|| format!("{name} needs the player or camera"))?;
            let d2: f64 = (0..3).map(|i| (from[i] - to[i]).powi(2)).sum();
            return num(if name == "dist" { d2.sqrt() } else { d2 });
        }
        match (*id, name) {
            (SELF, _) => {
                if let Some(result) = self.particle_method(name, args) {
                    return result;
                }
                if matches!(name, "x" | "y" | "z") && !matches!(self.subject, Subject::Particle(_))
                {
                    return num(self.position()[axis(name)]);
                }
                let at = ParticleLevel::block_at(self.position());
                self.positional(at, name, args)
            }
            (PLAYER, "x" | "y" | "z") => num(frame.player[axis(name)]),
            (PLAYER, "inWater") => Ok(V::Bool(frame.in_water)),
            (PLAYER, "onGround") => Ok(V::Bool(frame.player_on_ground)),
            (PLAYER, "speedSq") => num(frame.player_velocity.iter().map(|v| v * v).sum()),
            (PLAYER, "speed") => num(frame
                .player_velocity
                .iter()
                .map(|v| v * v)
                .sum::<f64>()
                .sqrt()),
            (PLAYER, _) => self.positional(ParticleLevel::block_at(frame.player), name, args),
            (CAMERA, "x" | "y" | "z") => num(frame.camera[axis(name)]),
            (CAMERA, "viewDistance") => num(f64::from(frame.render_distance * 16)),
            (CAMERA, _) => self.positional(ParticleLevel::block_at(frame.camera), name, args),
            (GLOBAL, "rain") => num(f64::from(frame.rain)),
            (GLOBAL, "time") => num(frame.game_time as f64),
            (GLOBAL, "dayTime") => num(frame.day_time as f64),
            (GLOBAL, "skyType") => num(f64::from(frame.sky_type)),
            (GLOBAL, "dimensionType") => Ok(V::Str(frame.dimension.as_str().into())),
            (GLOBAL, "renderDistance") => num(f64::from(frame.render_distance)),
            (RANDOM, "rand") => {
                let r = self.rand();
                let a = |i: usize| args.get(i).map_or(0.0, V::num);
                num(match args.len() {
                    0 => r,
                    1 => r * a(0),
                    _ => a(0) + r * (a(1) - a(0)),
                })
            }
            (RANDOM, "gaussian") => {
                let g = self.random.next_gaussian();
                let a = |i: usize| args.get(i).map_or(0.0, V::num);
                num(if args.len() >= 2 { a(0) + g * a(1) } else { g })
            }
            (RANDOM, "noise") => {
                // `r.noise(x, y)`: one simplex noise (named noises aren't read yet).
                let mut coords = args.iter().filter(|v| !matches!(v, V::Str(_))).map(V::num);
                let (x, y) = (coords.next().unwrap_or(0.0), coords.next().unwrap_or(0.0));
                num(f64::from(self.registry.noise.get2(x, y)))
            }
            (id, _) if id >= BLOCKS => {
                let at = *self
                    .blocks
                    .get((id - BLOCKS) as usize)
                    .ok_or("no such block object")?;
                match name {
                    "x" | "y" | "z" => num(f64::from(at[axis(name)])),
                    _ => self.positional(at, name, args),
                }
            }
            _ => Err(format!("{name} is not available here")),
        }
    }

    fn function(&mut self, name: &str, args: &[V]) -> Result<V, String> {
        match (name, args) {
            ("config", [V::Str(id)]) => self
                .registry
                .configs
                .get(id)
                .map(horizon_polytone::pack::ConfigValue::to_value)
                .ok_or_else(|| format!("no config {id}")),
            _ => Err(format!("{name} is not available here")),
        }
    }
}

fn eval(program: &Program, ctx: &mut Ctx) -> Option<f64> {
    program.eval(ctx).ok().map(|v| v.num())
}

/// A rule test on a block state (`state_predicate`): `always_true`, `block_match` and
/// `blockstate_match`; other tests don't hold.
fn state_matches(predicate: &Json, ctx: &Ctx, at: [i32; 3]) -> bool {
    let Some(level) = ctx.level else {
        return false;
    };
    let state = level.world.block(at[0], at[1], at[2]);
    let kind = predicate["predicate_type"]
        .as_str()
        .unwrap_or("")
        .trim_start_matches("minecraft:");
    let name = |id: &str| {
        if id.contains(':') {
            id.to_owned()
        } else {
            format!("minecraft:{id}")
        }
    };
    let Some(block) = ctx.registry.blocks.state(state) else {
        return false;
    };
    match kind {
        "always_true" => true,
        "block_match" => predicate["block"]
            .as_str()
            .is_some_and(|b| name(b) == block.name),
        "blockstate_match" => {
            let wanted = &predicate["block_state"];
            let id = wanted["Name"].as_str().unwrap_or("");
            name(id) == block.name
                && wanted["Properties"]
                    .as_object()
                    .into_iter()
                    .flatten()
                    .all(|(k, v)| {
                        block.properties.get(k.as_str()).map(String::as_str) == v.as_str()
                    })
        }
        _ => false,
    }
}

/// Whether `biomes` (an id, a list of ids, or a tag, which isn't resolved here) holds the biome
/// at `at`.
fn biome_matches(biomes: &Json, ctx: &Ctx, at: [i32; 3]) -> bool {
    let Some(biome) = ctx.biome(at) else {
        return true;
    };
    let id = |s: &str| {
        if s.contains(':') {
            s.to_owned()
        } else {
            format!("minecraft:{s}")
        }
    };
    match biomes {
        Json::String(s) => id(s) == biome,
        Json::Array(list) => list.iter().filter_map(Json::as_str).any(|s| id(s) == biome),
        _ => true,
    }
}

/// The options to spawn `id` with: a pack's custom particle, a vanilla particle, or a mod's
/// sprite-set particle.
fn options_for(id: &str, registry: &Registry) -> ParticleOptions {
    if registry.types.contains_key(id) {
        return ParticleOptions::named(id);
    }
    match ParticleType::from_name(id) {
        Some(ty) if id.starts_with("minecraft:") => ParticleOptions::simple(ty),
        _ => ParticleOptions::named(id),
    }
}

/// Runs `emitters` from the subject: each fires `count` times when its `chance` beats a random
/// draw, its position relative to `base` (the particle, or the block's spawn location).
fn emit(emitters: &[Emitter], ctx: &mut Ctx, out: &mut Vec<(ParticleOptions, [f64; 3], [f64; 3])>) {
    for emitter in emitters {
        if emitter
            .tick_source
            .as_deref()
            .is_some_and(|s| s.trim_start_matches("minecraft:") != "animate_tick")
        {
            continue;
        }
        // No `chance` always fires; one that fails to run never does.
        let chance = emitter
            .fields
            .get("chance")
            .map_or(Some(1.0), |c| eval(c, ctx))
            .unwrap_or(0.0);
        if ctx.rand() >= chance {
            continue;
        }
        let field = |ctx: &mut Ctx, key: &str| emitter.fields.get(key).and_then(|p| eval(p, ctx));
        let base = match &ctx.subject {
            Subject::Particle(p) => p.position,
            // Bones aren't resolved yet: offsets count from the entity's feet, in world axes.
            Subject::Entity(at) => *at,
            Subject::Block(b) => {
                let corner = b.map(f64::from);
                match emitter.spawn_location.as_deref() {
                    Some("lower_corner") => corner,
                    _ => corner.map(|c| c + 0.5),
                }
            }
        };
        if let Some(biomes) = &emitter.biomes
            && !biome_matches(biomes, ctx, ParticleLevel::block_at(base))
        {
            continue;
        }
        let count = field(ctx, "count").map_or(1, |c| c.max(0.0) as u32).min(64);
        for _ in 0..count {
            let at = [
                base[0] + field(ctx, "x").unwrap_or(0.0),
                base[1] + field(ctx, "y").unwrap_or(0.0),
                base[2] + field(ctx, "z").unwrap_or(0.0),
            ];
            if let Some(predicate) = &emitter.state_predicate
                && !state_matches(predicate, ctx, ParticleLevel::block_at(at))
            {
                continue;
            }
            let velocity = [
                field(ctx, "dx").unwrap_or(0.0),
                field(ctx, "dy").unwrap_or(0.0),
                field(ctx, "dz").unwrap_or(0.0),
            ];
            let id = emitter.particle.id();
            let mut options = options_for(&id, ctx.registry);
            if ctx.registry.types.contains_key(&id) {
                let mut seed = Seed::default();
                for (key, short) in [
                    ("red", "r"),
                    ("green", "g"),
                    ("blue", "b"),
                    ("alpha", "a"),
                    ("roll", ""),
                    ("size", ""),
                    ("custom", ""),
                ] {
                    let program = emitter.fields.get(key).or(emitter.fields.get(short));
                    if let Some(v) = program.and_then(|p| eval(p, ctx)) {
                        seed.fields.insert(key.to_owned(), v);
                    }
                }
                for (name, p) in &emitter.customs {
                    if let Some(v) = eval(p, ctx) {
                        seed.customs.insert(name.to_lowercase(), v);
                    }
                }
                options.polytone_seed = (seed != Seed::default()).then(|| Arc::new(seed));
            }
            out.push((options, at, velocity));
        }
    }
}

/// Sets each `keys` field the program map holds on `p` through `apply`.
fn run_fields(
    programs: &BTreeMap<String, Program>,
    ctx: &mut Ctx,
    keys: &[&str],
    mut apply: impl FnMut(&mut Ctx, &str, f64),
) {
    for &key in keys {
        if let Some(v) = programs.get(key).and_then(|p| eval(p, ctx)) {
            apply(ctx, key, v);
        }
    }
}

/// Writes a particle field a ticker or initializer computed.
fn set_field(p: &mut Particle, custom: &mut f64, key: &str, v: f64) {
    let v32 = v as f32;
    match key {
        "x" => p.position[0] = v,
        "y" => p.position[1] = v,
        "z" => p.position[2] = v,
        "dx" => p.velocity[0] = v,
        "dy" => p.velocity[1] = v,
        "dz" => p.velocity[2] = v,
        "red" => p.rgba[0] = v32,
        "green" => p.rgba[1] = v32,
        "blue" => p.rgba[2] = v32,
        "alpha" => p.rgba[3] = v32,
        "size" => p.size = v32,
        "roll" => p.roll = v32,
        "custom" => *custom = v,
        "lifetime" => p.lifetime = v as i32,
        "friction" => p.friction = v32,
        "hitbox_size" => {
            p.bb_width = v32;
            p.bb_height = v32;
        }
        _ => {}
    }
}

const TICKED: [&str; 13] = [
    "dx", "dy", "dz", "x", "y", "z", "red", "green", "blue", "alpha", "size", "roll", "custom",
];
const INITIALIZED: [&str; 13] = [
    "lifetime",
    "size",
    "hitbox_size",
    "red",
    "green",
    "blue",
    "alpha",
    "roll",
    "friction",
    "custom",
    "dx",
    "dy",
    "dz",
];

impl super::ParticleController for Controller {
    fn tick(&mut self, p: &mut Particle, level: &ParticleLevel<'_>) -> ParticleUpdate {
        let mut update = ParticleUpdate::default();
        if !p.advance_age() {
            return update;
        }
        let registry = self.registry.clone();
        let def = self.def.clone();
        self.ticks += 1;
        if self.ticks.is_multiple_of(def.tick_interval.max(1)) {
            let Ok(frame) = registry.frame.read() else {
                return update;
            };
            let mut random = std::mem::replace(&mut p.random, LegacyRandom::new(0));
            let mut ctx = Ctx {
                subject: Subject::Particle(p),
                random: &mut random,
                custom: &mut self.custom,
                customs: &mut self.customs,
                frame: &frame,
                registry: &registry,
                level: Some(level),
                blocks: Vec::new(),
            };
            let removed = def
                .ticker
                .get("remove_condition")
                .and_then(|prog| prog.eval(&mut ctx).ok())
                .is_some_and(|v| v.num() != 0.0);
            if !removed {
                run_fields(&def.ticker, &mut ctx, &TICKED, |ctx, key, v| {
                    let custom = &mut *ctx.custom;
                    if let Subject::Particle(p) = &mut ctx.subject {
                        set_field(p, custom, key, v);
                    }
                });
                for (name, prog) in &def.ticker_customs {
                    if let Some(v) = eval(prog, &mut ctx) {
                        ctx.customs.insert(name.to_lowercase(), v);
                    }
                }
                emit(&def.emitters, &mut ctx, &mut update.spawned);
            }
            drop(ctx);
            p.random = random;
            if removed {
                p.remove();
                return update;
            }
        }
        // Moves with collision when it has physics, else freely; then the friction.
        if def.has_physics {
            let before = p.position;
            p.move_by(level, p.velocity);
            let still = (0..3).all(|i| (p.position[i] - before[i]).abs() < 1.0e-7);
            if (def.kill_on_contact && (p.on_ground || p.stopped_by_collision))
                || (def.kill_when_still && still)
            {
                p.remove();
            }
        } else {
            for i in 0..3 {
                p.position[i] += p.velocity[i];
            }
            p.bb = super::aabb_at(p.position, p.bb_width, p.bb_height);
        }
        let f = f64::from(p.friction);
        for c in &mut p.velocity {
            *c *= f;
        }
        if def.liquid_affinity != LiquidAffinity::Any {
            let wet = fluid_at(level, ParticleLevel::block_at(p.position)) != Fluid::None;
            if wet != (def.liquid_affinity == LiquidAffinity::Liquids) {
                p.remove();
            }
        }
        if !def.random_sprite && p.lifetime > 0 {
            // Through the sprite set over the particle's life, as vanilla's setSpriteFromAge.
            let last = self.frames.saturating_sub(1);
            update.frame = Some(p.age.max(0) as usize * last / p.lifetime.max(1) as usize);
        }
        update
    }
}

/// A custom particle at `position` moving at `velocity`, its initializer run (seeded by the
/// emitter that spawned it); `None` without a sprite set for a visible type.
#[allow(clippy::too_many_arguments)]
pub(super) fn make(
    engine: &ParticleEngine,
    registry: &Arc<Registry>,
    name: &str,
    def: &Arc<CustomParticle>,
    options: &ParticleOptions,
    position: [f64; 3],
    velocity: [f64; 3],
    seed: i64,
) -> Option<Particle> {
    // `limit`: at most that many of the type alive at once.
    // (A GPU particle's `limit` is its ring of records instead.)
    if let Some(limit) = def.limit
        && def.gpu.is_none()
    {
        let alive = engine
            .particles
            .iter()
            .chain(&engine.pending)
            .filter(|p| !p.removed && p.polytone_type.as_deref() == Some(name))
            .count();
        if alive >= limit as usize {
            return None;
        }
    }
    let mut random = LegacyRandom::new(seed);
    let mut particle = Particle::at(&mut random, ParticleType::Smoke, position);
    particle.velocity = velocity;
    particle.gravity = 0.0;
    particle.has_physics = def.has_physics;
    let mut frames = 0;
    // A model particle (`model`) isn't drawn yet: it still ticks and emits. A GPU particle is
    // drawn from its record instead (`polytone_gpu`).
    let hidden = def.render_type == RenderType::Invisible
        || def.render.contains_key("model")
        || def.gpu.is_some();
    match engine.mod_sets.get(name) {
        Some(&(set, _)) => {
            let sprites = engine.set_sprites.get(set as usize)?;
            frames = sprites.len();
            particle.set = Some(set);
            let pick = if def.random_sprite {
                random.next_int_bound(frames.max(1) as i32).unwrap_or(0) as usize
            } else {
                0
            };
            particle.sprite = Sprite::Index(*sprites.get(pick).or(sprites.first())?);
        }
        None if hidden => {}
        None => return None,
    }
    particle.no_render = hidden;
    particle.orientation = super::polytone_gpu::Orientation::parse(&def.rotation_mode);
    particle.additive = def.render_type == RenderType::AdditiveTranslucent;
    particle.behaviour = Behaviour::Controlled {
        translucent: def.render_type != RenderType::Opaque,
    };
    let (mut custom, mut customs) = (0.0, BTreeMap::new());
    if let Ok(frame) = registry.frame.read() {
        let mut ctx = Ctx {
            subject: Subject::Particle(&mut particle),
            random: &mut random,
            custom: &mut custom,
            customs: &mut customs,
            frame: &frame,
            registry,
            level: None,
            blocks: Vec::new(),
        };
        run_fields(&def.initializer, &mut ctx, &INITIALIZED, |ctx, key, v| {
            let custom = &mut *ctx.custom;
            if let Subject::Particle(p) = &mut ctx.subject {
                set_field(p, custom, key, v);
            }
        });
        for (name, p) in &def.initializer_customs {
            if let Some(v) = eval(p, &mut ctx) {
                ctx.customs.insert(name.to_lowercase(), v);
            }
        }
    }
    particle.o_roll = particle.roll;
    // The emitter's own values win over the initializer's.
    if let Some(seed) = &options.polytone_seed {
        for (key, v) in &seed.fields {
            set_field(&mut particle, &mut custom, key, *v);
        }
        customs.extend(seed.customs.iter().map(|(k, v)| (k.clone(), *v)));
    }
    particle.random = random;
    particle.polytone_type = def.limit.map(|_| Arc::from(name));
    particle.bb = super::aabb_at(position, particle.bb_width, particle.bb_height);
    particle.controller = Some(Box::new(Controller {
        def: def.clone(),
        registry: registry.clone(),
        custom,
        customs,
        ticks: 0,
        frames,
    }));
    Some(particle)
}

impl ParticleEngine {
    /// The emitters of the packs' entity modifiers aimed at `entity` (its type, as
    /// `minecraft:player`) standing at `position`, run once a client tick.
    pub fn polytone_entity(&mut self, level: &ParticleLevel<'_>, entity: &str, position: [f64; 3]) {
        let Some(registry) = self.polytone.clone() else {
            return;
        };
        let modifiers = registry.entity_emitters(entity);
        if modifiers.is_empty() {
            return;
        }
        let Ok(frame) = registry.frame.read() else {
            return;
        };
        let camera = self.camera;
        let distance_sq: f64 = (0..3).map(|i| (position[i] - camera[i]).powi(2)).sum();
        let mut out = Vec::new();
        let (mut custom, mut customs) = (0.0, BTreeMap::new());
        let mut ctx = Ctx {
            subject: Subject::Entity(position),
            random: &mut self.random,
            custom: &mut custom,
            customs: &mut customs,
            frame: &frame,
            registry: &registry,
            level: Some(level),
            blocks: Vec::new(),
        };
        for modifier in modifiers.iter() {
            for emitter in &modifier.emitters {
                // `max_distance`: 32 blocks from the camera unless the emitter says otherwise.
                if distance_sq <= emitter.max_distance.unwrap_or(32.0).powi(2) {
                    emit(std::slice::from_ref(emitter), &mut ctx, &mut out);
                }
            }
        }
        drop(ctx);
        drop(frame);
        for (options, position, velocity) in out {
            self.add(&options, position, velocity);
        }
    }

    /// The block emitters of the packs' block modifiers aimed at the block at `at`, run from a
    /// block animate tick (`Block.animateTick`, as torches spawn their flames).
    pub fn polytone_animate(&mut self, level: &ParticleLevel<'_>, at: [i32; 3]) {
        let Some(registry) = self.polytone.clone() else {
            return;
        };
        let state = level.world.block(at[0], at[1], at[2]);
        let Some(Some(modifiers)) = registry.block_emitters.get(state.0 as usize) else {
            return;
        };
        let modifiers = modifiers.clone();
        let Ok(frame) = registry.frame.read() else {
            return;
        };
        let mut out = Vec::new();
        let (mut custom, mut customs) = (0.0, BTreeMap::new());
        let mut ctx = Ctx {
            subject: Subject::Block(at),
            random: &mut self.random,
            custom: &mut custom,
            customs: &mut customs,
            frame: &frame,
            registry: &registry,
            level: Some(level),
            blocks: Vec::new(),
        };
        for modifier in modifiers.iter() {
            emit(&modifier.particle_emitters, &mut ctx, &mut out);
        }
        drop(ctx);
        drop(frame);
        for (options, position, velocity) in out {
            self.add(&options, position, velocity);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chunk::Chunk;
    use crate::world::World;
    use glam::DVec3;
    use horizon_world::DATA_VERSION;
    use horizon_world::block_collision::BlockCollisionRegistry;
    use horizon_world::collision::CachedCollisionRegistry;
    use horizon_world::nbt::Tag;
    use horizon_world::registry::BlockStateId;
    use std::collections::HashSet;

    const STATES: [&str; 2] = ["minecraft:air", "minecraft:torch"];

    fn blocks() -> &'static BlockRegistry {
        let names = || Tag::List(STATES.iter().map(|s| Tag::String((*s).into())).collect());
        Box::leak(Box::new(
            BlockRegistry::from_nbt(&Tag::compound([
                ("DataVersion", Tag::Int(DATA_VERSION)),
                ("states", names()),
                (
                    "defaults",
                    Tag::compound([
                        ("minecraft:air", Tag::Int(0)),
                        ("minecraft:torch", Tag::Int(1)),
                    ]),
                ),
            ]))
            .unwrap(),
        ))
    }

    /// A pack overlay root holding `files` (path, JSON).
    fn pack(files: &[(&str, serde_json::Value)]) -> std::path::PathBuf {
        static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "horizon-polytone-particles-{}-{n}",
            std::process::id()
        ));
        for (path, json) in files {
            let path = root.join(path);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, json.to_string()).unwrap();
        }
        root
    }

    struct Scene {
        world: World,
        facts: crate::blocks::BlockFacts,
        collision: BlockCollisionRegistry<'static>,
    }

    fn scene(blocks: &'static BlockRegistry) -> Scene {
        let names = || Tag::List(STATES.iter().map(|s| Tag::String((*s).into())).collect());
        let cached = CachedCollisionRegistry::from_nbt(
            &Tag::compound([
                ("DataVersion", Tag::Int(DATA_VERSION)),
                ("states", names()),
                ("shapes", Tag::List(vec![])),
                ("cached_shapes", Tag::IntArray(vec![-1, -1])),
            ]),
            blocks,
        )
        .unwrap();
        let mut world = World::new(BlockStateId(0), 0);
        world.chunks.insert((0, 0), Arc::new(Chunk::empty(0, 0)));
        world.set(1, 64, 1, BlockStateId(1)).unwrap();
        Scene {
            world,
            facts: crate::blocks::BlockFacts {
                states: vec![],
                air: BlockStateId(0),
                woods: HashMap::new(),
            },
            collision: BlockCollisionRegistry::new(Arc::new(cached)).unwrap(),
        }
    }

    fn engine(registry: Registry) -> ParticleEngine {
        let mut engine = ParticleEngine::new(Path::new(""), &HashMap::new(), &HashSet::new());
        engine.mod_sets.insert("test:spark".into(), (0, None));
        engine.set_sprites.push(vec![10, 11, 12]);
        engine.camera = [1.0, 65.0, 1.0];
        engine.set_polytone(Some(Arc::new(registry)));
        engine
    }

    fn spark_pack() -> std::path::PathBuf {
        pack(&[
            (
                "assets/test/polytone/custom_particles/spark.json",
                serde_json::json!({
                    "has_physics": false,
                    "initializer": {"lifetime": "10", "size": "0.5", "red": "0.25", "green": "0.5"},
                    "ticker": {"dx": "o.custom('heat')", "remove_condition": "o.age >= 4"}
                }),
            ),
            (
                "assets/test/polytone/block_modifiers/torch.json",
                serde_json::json!({
                    "targets": ["minecraft:torch"],
                    "particle_emitters": [
                        {
                            "particle": "test:spark", "chance": "1", "spawn_location": "lower_corner",
                            "x": "0.5", "y": "1", "z": "0.5", "red": "0.75", "customs": {"Heat": "2"}
                        },
                        {"particle": "minecraft:flame", "chance": "0"}
                    ]
                }),
            ),
            (
                // A lower priority modifier of the same block: the torch's above wins.
                "assets/test/polytone/block_modifiers/low.json",
                serde_json::json!({
                    "targets": ["minecraft:torch"],
                    "priority": -1,
                    "particle_emitters": [{"particle": "minecraft:flame", "chance": "1"}]
                }),
            ),
        ])
    }

    #[test]
    fn a_block_emitter_spawns_a_custom_particle_its_values_seed() {
        let blocks = blocks();
        let root = spark_pack();
        let scene = scene(blocks);
        let level = ParticleLevel {
            world: &scene.world,
            facts: &scene.facts,
            collision: &scene.collision,
            player: None,
        };
        let mut engine = engine(Registry::read(&root, Default::default(), blocks));
        // Air has no emitters; the torch fires its first emitter only.
        engine.polytone_animate(&level, [1, 64, 2]);
        assert!(engine.pending.is_empty());
        engine.polytone_animate(&level, [1, 64, 1]);
        assert_eq!(engine.pending.len(), 1);
        let p = &engine.pending[0];
        assert!(p.controller.is_some());
        assert_eq!(p.position, [1.5, 65.0, 1.5]);
        // The initializer's values, the emitter's red over its own.
        assert_eq!((p.lifetime, p.size), (10, 0.5));
        assert_eq!(&p.rgba[..2], &[0.75, 0.5]);
        assert!(matches!(p.sprite, Sprite::Index(10)));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn the_ticker_moves_the_particle_and_removes_it() {
        let blocks = blocks();
        let root = spark_pack();
        let scene = scene(blocks);
        let level = ParticleLevel {
            world: &scene.world,
            facts: &scene.facts,
            collision: &scene.collision,
            player: None,
        };
        let mut engine = engine(Registry::read(&root, Default::default(), blocks));
        engine.polytone_animate(&level, [1, 64, 1]);
        let mut p = engine.pending.pop().unwrap();
        // `dx` is the emitter's named custom each tick: two blocks east per tick.
        for tick in 1..=3 {
            engine.tick_particle(&mut p, &level);
            assert_eq!(p.position[0], 1.5 + 2.0 * f64::from(tick));
            assert!(!p.removed);
        }
        // Through the sprite set over its life: age 3 of 10 over 3 sprites.
        assert!(matches!(p.sprite, Sprite::Index(10)));
        engine.tick_particle(&mut p, &level);
        assert!(p.removed, "removed once o.age >= 4");
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn entity_emitters_run_on_their_entity_near_the_camera_up_to_the_limit() {
        let blocks = blocks();
        let root = pack(&[
            (
                "assets/test/polytone/custom_particles/spark.json",
                serde_json::json!({"has_physics": false, "limit": 1}),
            ),
            (
                "assets/test/polytone/custom_particles/dot.json",
                serde_json::json!({"render_type": "invisible"}),
            ),
            (
                // No `targets`: the file name, `minecraft:player`.
                "assets/minecraft/polytone/entity_modifiers/player.json",
                serde_json::json!({"emitters": [
                    {"particle": "test:spark", "chance": "p.onGround() ? 1 : 0", "x": "1", "y": "2"},
                    // A chance that fails to run never fires.
                    {"particle": "test:dot", "chance": "o.nope()"}
                ]}),
            ),
        ]);
        let scene = scene(blocks);
        let level = ParticleLevel {
            world: &scene.world,
            facts: &scene.facts,
            collision: &scene.collision,
            player: None,
        };
        let registry = Registry::read(&root, Default::default(), blocks);
        let frame = registry.frame.clone();
        let mut engine = engine(registry);
        let feet = [1.0, 64.0, 1.0];
        // In the air the chance is 0.
        engine.polytone_entity(&level, "minecraft:player", feet);
        assert!(engine.pending.is_empty());
        frame.write().unwrap().player_on_ground = true;
        engine.polytone_entity(&level, "minecraft:zombie", feet);
        // Beyond the default 32 blocks from the camera.
        engine.polytone_entity(&level, "minecraft:player", [1.0, 64.0, 40.0]);
        assert!(engine.pending.is_empty());
        engine.polytone_entity(&level, "minecraft:player", feet);
        assert_eq!(engine.pending.len(), 1);
        assert_eq!(engine.pending[0].position, [2.0, 66.0, 1.0]);
        // `limit` 1: no second one while the first lives.
        engine.polytone_entity(&level, "minecraft:player", feet);
        assert_eq!(engine.pending.len(), 1);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_gpu_particle_is_a_record_drawn_until_its_life_ends() {
        let blocks = blocks();
        let root = pack(&[
            (
                "assets/test/polytone/custom_particles/rain.json",
                serde_json::json!({
                    "type": "gpu", "texture": "test:textures/particle/rain.png",
                    "render_type": "translucent", "rotation_mode": "movement_aligned",
                    "initializer": {"lifetime": "3", "size": "0.5"},
                    "gravity": 0.1, "area": {"size": [4, 4, 4], "count": 3}
                }),
            ),
            (
                // Additive: drawn in the frame's additive pass instead.
                "assets/test/polytone/custom_particles/glow.json",
                serde_json::json!({
                    "type": "gpu", "texture": "test:textures/particle/rain.png",
                    "render_type": "additive_translucent", "initializer": {"lifetime": "3"}
                }),
            ),
            (
                "assets/minecraft/polytone/entity_modifiers/player.json",
                serde_json::json!({"emitters": [{"particle": "test:rain"}, {"particle": "test:glow"}]}),
            ),
        ]);
        let scene = scene(blocks);
        let level = ParticleLevel {
            world: &scene.world,
            facts: &scene.facts,
            collision: &scene.collision,
            player: None,
        };
        let mut engine = engine(Registry::read(&root, Default::default(), blocks));
        let rect = crate::hud::Rect {
            u0: 0.0,
            v0: 0.0,
            u1: 1.0,
            v1: 1.0,
            w: 16,
            h: 16,
        };
        engine
            .atlas
            .insert("test:particle/rain".into(), (rect, true));
        engine.polytone_entity(&level, "minecraft:player", [1.0, 64.0, 1.0]);
        // No engine particle: a record each; the rain drawn as its area's three quads.
        assert!(engine.pending.is_empty());
        assert_eq!(engine.polytone_gpu_records(), 2);
        let mut out = Vec::new();
        engine.render(
            DVec3::new(1.0, 70.0, 1.0),
            0.0,
            0.0,
            0.0,
            &|_| 0xF0,
            &mut out,
        );
        assert_eq!(out.len(), 3 * 4);
        assert_eq!(engine.take_additive().len(), 4);
        assert!(
            out.iter()
                .all(|v| v.layer == crate::gfx::TRANSLUCENT_PARTICLE_OVERLAY)
        );
        for _ in 0..3 {
            engine.tick(&level);
        }
        assert_eq!(
            engine.polytone_gpu_records(),
            0,
            "gone once its 3 ticks are over"
        );
        std::fs::remove_dir_all(root).unwrap();
    }
}
