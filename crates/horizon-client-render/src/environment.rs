//! A dimension's environment attributes (`EnvironmentAttributeSystem.addDefaultLayers`): the
//! dimension type's attributes, the biome layer, the dimension's timelines and, where it can
//! have weather, the rain and thunder layers, sampled into the sky state the renderer draws.
use crate::Result;
use horizon_world::environment_attributes::{
    Attribute, AttributeMap, Entry, Kind, Value, attribute,
};
use horizon_world::environment_system::{BiomeBlend, Builder, Conditions, System};
use horizon_world::environment_timeline::Timeline;
use serde_json::Value as Json;
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::{Arc, RwLock};

pub struct Environment {
    system: System,
    /// What the packs' expressions read; the frame sets it before sampling.
    frame: Arc<RwLock<Frame>>,
    /// Biome attribute maps by biome id, where resource packs changed them; the biome layer
    /// reads these instead of the biome's own.
    biomes: HashMap<String, AttributeMap>,
    /// Packs' biome `post_process` maps by biome id: a biome layer after the weather.
    post: HashMap<String, AttributeMap>,
}

#[derive(Clone, Copy, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct Sky {
    pub sky_color: [f32; 3],
    pub fog_color: [f32; 3],
    pub sky_light_factor: f32,
    pub sky_light_color: [f32; 3],
    pub block_light_tint: [f32; 3],
    pub ambient_light_color: [f32; 3],
    pub sun_angle: f32,
    pub moon_angle: f32,
    pub star_angle: f32,
    pub star_brightness: f32,
    pub sunrise_color: [f32; 4],
    pub cloud_color: [f32; 4],
    /// `gameplay/sky_light_level`, the server-side sky darkening.
    pub sky_light_level: f32,
    /// Environmental fog (`visual/fog_start_distance` / `fog_end_distance`).
    pub fog_start: f32,
    pub fog_end: f32,
    /// `visual/cloud_fog_end_distance`.
    pub cloud_fog_end: f32,
    /// `visual/cloud_height`.
    pub cloud_height: f32,
}

/// Where and when the sky is sampled.
#[derive(Clone, Copy, Default)]
pub struct Sample<'a> {
    /// The day clock's total ticks.
    pub day_time: i64,
    /// `Level.getRainLevel` and `getThunderLevel`, 0 where the dimension has no weather.
    pub rain: f32,
    pub thunder: f32,
    /// The biomes around the camera (`EnvironmentAttributeProbe`'s Gaussian mix).
    pub blend: Option<&'a BiomeBlend<'a>>,
    /// The biome at the camera, for attributes that don't blend.
    pub biome: Option<&'a AttributeMap>,
    /// The same for the packs' biome post-process group.
    pub post_blend: Option<&'a BiomeBlend<'a>>,
    pub post_biome: Option<&'a AttributeMap>,
}

/// Resource packs' Polytone biome and dimension modifiers (#11486): layers they add to, change
/// in or remove from the attribute stack.
#[derive(Default)]
pub struct PackModifiers {
    biomes: Vec<horizon_polytone::BiomeModifier>,
    dimensions: Vec<horizon_polytone::DimensionModifier>,
    /// The packs' config values, for `config(id)` in expressions.
    configs: horizon_polytone::pack::Configs,
}

impl PackModifiers {
    /// Every `assets/<namespace>/polytone/{biome,dimension}_modifiers/**.json` under a pack
    /// overlay root. A file that fails to parse is skipped with a warning.
    pub fn read(pack_root: &Path) -> Self {
        let mut out = Self::default();
        let Ok(namespaces) = std::fs::read_dir(pack_root.join("assets")) else {
            return out;
        };
        for namespace in namespaces.flatten() {
            for kind in ["biome_modifiers", "dimension_modifiers"] {
                let mut stack = vec![namespace.path().join("polytone").join(kind)];
                while let Some(dir) = stack.pop() {
                    for entry in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
                        let path = entry.path();
                        if path.is_dir() {
                            stack.push(path);
                            continue;
                        }
                        let Ok(relative) = path.strip_prefix(pack_root) else {
                            continue;
                        };
                        let relative = relative.to_string_lossy();
                        let implicit = horizon_polytone::implicit_id(&relative, kind);
                        let json = match horizon_world::json_file::read::<Json>(&path) {
                            Ok(json) => json,
                            Err(e) => {
                                log::warn!("{relative}: {e}");
                                continue;
                            }
                        };
                        let parsed = if kind == "biome_modifiers" {
                            horizon_polytone::BiomeModifier::parse(&json, implicit.as_deref())
                                .map(|m| m.map(|m| out.biomes.push(m)))
                        } else {
                            horizon_polytone::DimensionModifier::parse(&json, implicit.as_deref())
                                .map(|m| m.map(|m| out.dimensions.push(m)))
                        };
                        if let Err(e) = parsed {
                            log::warn!("{relative}: {e}");
                        }
                    }
                }
            }
        }
        out.biomes.sort_by_key(|m| m.priority);
        out.dimensions.sort_by_key(|m| m.priority);
        out
    }

    pub fn is_empty(&self) -> bool {
        self.biomes.is_empty() && self.dimensions.is_empty()
    }

    /// The config values expressions read.
    pub fn with_configs(mut self, configs: horizon_polytone::pack::Configs) -> Self {
        self.configs = configs;
        self
    }
}

/// What a pack's expression can ask about the frame (wiki "Scripting Expressions": the global,
/// player, camera and positional contexts), set before the frame samples the sky.
#[derive(Clone, Debug, Default)]
pub struct Frame {
    pub rain: f32,
    pub thunder: f32,
    pub game_time: i64,
    pub day_time: i64,
    /// The render distance in chunks.
    pub render_distance: i32,
    /// `skyType`: 1 for the overworld's sky, 2 for the End's, 0 for none.
    pub sky_type: i32,
    pub dimension: String,
    pub player: [f64; 3],
    pub in_water: bool,
    pub camera: [f64; 3],
    /// The sky and block light at the camera.
    pub sky_light: i32,
    pub block_light: i32,
}

/// The game side of a pack expression in an attribute value: `g`, `p`, `c`, `o` (the camera's
/// position) and `r`, `v` (the value so far) and the pack's `config`.
struct FrameHost<'a> {
    frame: &'a Frame,
    configs: &'a horizon_polytone::pack::Configs,
    value: f64,
    seed: u64,
}

impl horizon_polytone::expr::Host for FrameHost<'_> {
    fn root(&mut self, root: horizon_polytone::expr::Root) -> horizon_polytone::expr::Value {
        use horizon_polytone::expr::{Root, Value as V};
        match root {
            Root::Value => V::Num(self.value),
            other => V::Object(other as u32),
        }
    }
    fn method(
        &mut self,
        target: &horizon_polytone::expr::Value,
        name: &str,
        _args: &[horizon_polytone::expr::Value],
    ) -> std::result::Result<horizon_polytone::expr::Value, String> {
        use horizon_polytone::expr::{Root, Value as V};
        let f = self.frame;
        let on = |root: Root| *target == V::Object(root as u32);
        let num = |n: f64| Ok(V::Num(n));
        match name {
            "rain" if on(Root::Global) => num(f64::from(f.rain)),
            "time" if on(Root::Global) => num(f.game_time as f64),
            "dayTime" if on(Root::Global) => num(f.day_time as f64),
            "skyType" if on(Root::Global) => num(f64::from(f.sky_type)),
            "dimensionType" if on(Root::Global) => Ok(V::Str(f.dimension.as_str().into())),
            "renderDistance" if on(Root::Global) => num(f64::from(f.render_distance)),
            "moonPhase" if on(Root::Global) => {
                num(f.day_time.div_euclid(24_000).rem_euclid(8) as f64)
            }
            "seaLevel" if on(Root::Global) => num(63.0),
            "viewDistance" if on(Root::Camera) => num(f64::from(f.render_distance * 16)),
            "x" | "y" | "z" if on(Root::Player) || on(Root::Camera) || on(Root::Object) => {
                let at = if on(Root::Player) { f.player } else { f.camera };
                num(at[usize::from(name.as_bytes()[0] - b'x')])
            }
            "inWater" if on(Root::Player) => Ok(V::Bool(f.in_water)),
            "skyLight" if on(Root::Object) || on(Root::Player) => num(f64::from(f.sky_light)),
            "blockLight" if on(Root::Object) || on(Root::Player) => num(f64::from(f.block_light)),
            "rand" if on(Root::Random) => {
                // A splitmix64 step: the frame's own sequence.
                self.seed = self.seed.wrapping_add(0x9e37_79b9_7f4a_7c15);
                let mut z = self.seed;
                z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
                z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
                num(((z ^ (z >> 31)) >> 11) as f64 / (1u64 << 53) as f64)
            }
            _ => Err(format!(
                "{name} is not available in an attribute expression"
            )),
        }
    }
    fn function(
        &mut self,
        name: &str,
        args: &[horizon_polytone::expr::Value],
    ) -> std::result::Result<horizon_polytone::expr::Value, String> {
        match (name, args) {
            ("config", [horizon_polytone::expr::Value::Str(id)]) => self
                .configs
                .get(id)
                .map(horizon_polytone::pack::ConfigValue::to_value)
                .ok_or_else(|| format!("no config {id}")),
            _ => Err(format!(
                "{name} is not available in an attribute expression"
            )),
        }
    }
}

/// A value as an expression's `v`: floats as they are, colours packed ARGB (`ARGB.as8BitChannel`).
fn as_number(value: &Value) -> f64 {
    let byte = |c: f32| ((c.clamp(0.0, 1.0) * 255.0).floor() as u32) & 255;
    match value {
        Value::Float(f) => f64::from(*f),
        Value::Bool(b) => f64::from(u8::from(*b)),
        Value::Rgb([r, g, b]) => {
            f64::from((0xff00_0000 | byte(*r) << 16 | byte(*g) << 8 | byte(*b)) as i32)
        }
        Value::Argb([r, g, b, a]) => {
            f64::from((byte(*a) << 24 | byte(*r) << 16 | byte(*g) << 8 | byte(*b)) as i32)
        }
        Value::Other(_) => 0.0,
    }
}

/// Turns Polytone layers into vanilla entries, compiling expressions against the frame.
struct Resolver {
    frame: Arc<RwLock<Frame>>,
    configs: Arc<horizon_polytone::pack::Configs>,
}

impl Resolver {
    /// A Polytone layer as a vanilla entry: `Some(None)` removes the attribute's entry, `None`
    /// is a value not supported yet (colormaps) or a broken one.
    fn entry(
        &self,
        attribute: &'static Attribute,
        layer: &horizon_polytone::Layer,
    ) -> Option<Option<Entry>> {
        use horizon_polytone::{Argument, Layer, Modifier};
        let (modifier, argument) = match layer {
            Layer::Remove => return Some(None),
            Layer::Apply {
                modifier, argument, ..
            } => (modifier, argument),
        };
        let op = match modifier {
            Modifier::Override => "override",
            Modifier::Add => "add",
            Modifier::Subtract => "subtract",
            Modifier::Multiply => "multiply",
            Modifier::Minimum => "minimum",
            Modifier::Maximum => "maximum",
            Modifier::AlphaBlend => "alpha_blend",
            Modifier::BlendToGray => "blend_to_gray",
            Modifier::And => "and",
            Modifier::Nand => "nand",
            Modifier::Or => "or",
            Modifier::Nor => "nor",
            Modifier::Xor => "xor",
            Modifier::Xnor => "xnor",
        };
        let json = match argument {
            Argument::Number(n) => Json::from(*n),
            Argument::Bool(b) => Json::Bool(*b),
            // Polytone colours may leave out the `#`; vanilla's codec wants it, 6 digits for RGB.
            Argument::Color(c) if attribute.kind == Kind::RgbColor => {
                Json::String(format!("#{:06x}", c & 0x00ff_ffff))
            }
            Argument::Color(c) => Json::String(format!("#{c:08x}")),
            Argument::Object(o) => Json::Object(o.clone()),
            Argument::Expression(src) => return self.expression(attribute, op, src).map(Some),
            Argument::Colormap(_) => return None,
        };
        match Entry::with_modifier(attribute, op, &json) {
            Ok(entry) => Some(Some(entry)),
            Err(e) => {
                log::warn!("{}: {e}", attribute.id);
                None
            }
        }
    }

    /// An expression value: compiled once, evaluated against the frame each time the layer
    /// applies, its result read as the modifier's argument (a number, a boolean, or a packed
    /// colour for colour attributes).
    fn expression(
        &self,
        attribute: &'static Attribute,
        op: &'static str,
        src: &str,
    ) -> Option<Entry> {
        let program = match horizon_polytone::expr::Program::compile(src) {
            Ok(program) => program,
            Err(e) => {
                log::warn!("{}: {e}", attribute.id);
                return None;
            }
        };
        let (frame, configs) = (self.frame.clone(), self.configs.clone());
        let f = move |value: &Value| -> Option<Entry> {
            let frame = frame.read().ok()?;
            let mut host = FrameHost {
                frame: &frame,
                configs: &configs,
                value: as_number(value),
                seed: 0,
            };
            let json = match program.eval(&mut host).ok()? {
                horizon_polytone::expr::Value::Bool(b) => Json::Bool(b),
                result => Json::from(result.num()),
            };
            Entry::with_modifier(attribute, op, &json).ok()
        };
        Some(Entry::Dynamic(
            horizon_world::environment_attributes::Dynamic(Arc::new(f)),
        ))
    }

    /// Applies a group of Polytone layers to `map`: entries set or replaced, `REMOVE` drops
    /// them.
    fn apply(&self, map: &mut AttributeMap, group: &horizon_polytone::Attributes) {
        for (id, layer) in group {
            let Some(attribute) = attribute(id) else {
                log::warn!("unknown attribute {id}");
                continue;
            };
            match self.entry(attribute, layer) {
                Some(Some(entry)) => map.set(attribute, entry),
                Some(None) => map.remove(attribute),
                None => {}
            }
        }
    }
}

/// The biome ids a `#tag` (`data/<namespace>/tags/worldgen/biome/<path>.json`) holds, nested
/// tags included.
fn biome_tag(
    root: &Path,
    tag: &str,
    cache: &mut HashMap<String, HashSet<String>>,
) -> HashSet<String> {
    if let Some(hit) = cache.get(tag) {
        return hit.clone();
    }
    cache.insert(tag.to_owned(), HashSet::new());
    let (namespace, path) = tag.split_once(':').unwrap_or(("minecraft", tag));
    let file = root.join(format!("data/{namespace}/tags/worldgen/biome/{path}.json"));
    let mut out = HashSet::new();
    if let Ok(json) = horizon_world::json_file::read::<Json>(&file) {
        for value in json["values"].as_array().into_iter().flatten() {
            let id = value
                .as_str()
                .or_else(|| value["id"].as_str())
                .unwrap_or_default();
            match id.strip_prefix('#') {
                Some(inner) => out.extend(biome_tag(root, inner, cache)),
                None if !id.is_empty() => {
                    out.insert(if id.contains(':') {
                        id.to_owned()
                    } else {
                        format!("minecraft:{id}")
                    });
                }
                None => {}
            }
        }
    }
    cache.insert(tag.to_owned(), out.clone());
    out
}

/// The timeline ids a dimension type's `timelines` names: an id, a `#tag` or a list,
/// tags resolved through `tags/timeline/` (their entries may be `{"id", "required"}`).
fn timeline_ids(root: &Path, json: &Json, out: &mut Vec<String>, depth: u32) {
    let entry = |e: &Json| {
        e.as_str()
            .or_else(|| e.get("id").and_then(Json::as_str))
            .map(str::to_owned)
    };
    let items: Vec<String> = match json {
        Json::Array(list) => list.iter().filter_map(entry).collect(),
        other => entry(other).into_iter().collect(),
    };
    for item in items {
        if let Some(tag) = item.strip_prefix('#') {
            let (namespace, path) = tag.split_once(':').unwrap_or(("minecraft", tag));
            let file = root.join(format!("data/{namespace}/tags/timeline/{path}.json"));
            if depth < 8
                && let Ok(tag) = horizon_world::json_file::read::<Json>(&file)
            {
                timeline_ids(root, &tag["values"], out, depth + 1);
            }
        } else if !out.contains(&item) {
            out.push(item);
        }
    }
}

impl Environment {
    pub fn load(root: &Path) -> Result<Self> {
        Self::load_dimension(root, "overworld")
    }

    /// `EnvironmentAttributeSystem.addDefaultLayers` for a dimension type: its attributes, a
    /// biome layer for every attribute some biome sets, its timelines (in tag order), and the
    /// weather where `Level.canHaveWeather` (sky light, no ceiling, not the End).
    pub fn load_dimension(root: &Path, dimension: &str) -> Result<Self> {
        Self::load_dimension_with(root, dimension, &PackModifiers::default())
    }

    /// The same with resource packs' Polytone modifiers (wiki "Environment Attributes"): a
    /// dimension modifier's `base` changes the dimension's own entries, `rain` and `thunder`
    /// the weather's, `post_process` adds a layer after all others; a biome modifier's `base`
    /// changes the biome's entries, `post_process` adds a biome layer after the weather.
    /// Modifiers apply lowest priority first, so the highest wins.
    pub fn load_dimension_with(
        root: &Path,
        dimension: &str,
        packs: &PackModifiers,
    ) -> Result<Self> {
        let json: Json = horizon_world::json_file::read(
            &root.join(format!("data/minecraft/dimension_type/{dimension}.json")),
        )?;
        let id = format!("minecraft:{dimension}");
        let mine: Vec<_> = packs
            .dimensions
            .iter()
            .filter(|m| m.targets.matches(&id, |_| false))
            .collect();
        let frame = Arc::new(RwLock::new(Frame::default()));
        let resolver = Resolver {
            frame: frame.clone(),
            configs: Arc::new(packs.configs.clone()),
        };
        let mut dimension_map = AttributeMap::parse(&json["attributes"])?;
        let (mut rain, mut thunder) = horizon_world::environment_system::weather_maps();
        let mut dimension_post = AttributeMap::default();
        for m in &mine {
            resolver.apply(&mut dimension_map, &m.base);
            resolver.apply(&mut rain, &m.rain);
            resolver.apply(&mut thunder, &m.thunder);
            resolver.apply(&mut dimension_post, &m.post_process);
        }
        let mut builder = Builder::default();
        builder.constant_layer(&dimension_map);
        // Without biome data (a bare dimension type), there is no biome layer.
        let mut biomes = HashMap::new();
        let mut post = HashMap::new();
        let tags = std::cell::RefCell::new(HashMap::new());
        if let Ok(folder) =
            horizon_world::json_folder::read(&root.join("data/minecraft/worldgen/biome"))
        {
            for file in folder.files.iter().filter(|f| !f.name.contains('/')) {
                let biome: Json = horizon_world::json_file::parse(&file.path, file.bytes()?)?;
                let name = format!("minecraft:{}", file.name);
                let mut map = match AttributeMap::parse(&biome["attributes"]) {
                    Ok(map) => map,
                    Err(e) => {
                        log::warn!("biome {}: {e}", file.name);
                        continue;
                    }
                };
                let mut post_map = AttributeMap::default();
                for m in &packs.biomes {
                    let in_tag =
                        |tag: &str| biome_tag(root, tag, &mut tags.borrow_mut()).contains(&name);
                    if m.targets.matches(&name, in_tag) {
                        resolver.apply(&mut map, &m.base);
                        resolver.apply(&mut post_map, &m.post_process);
                    }
                }
                if !post_map.is_empty() {
                    post.insert(name.clone(), post_map);
                }
                biomes.insert(name, map);
            }
        }
        builder.biome_layer(biomes.values());
        let mut ids = Vec::new();
        timeline_ids(root, &json["timelines"], &mut ids, 0);
        for id in ids {
            let (namespace, path) = id.split_once(':').unwrap_or(("minecraft", &id));
            let file = root.join(format!("data/{namespace}/timeline/{path}.json"));
            match horizon_world::json_file::read::<Json>(&file)
                .map_err(|e| e.to_string())
                .and_then(|t| Timeline::parse(&t))
            {
                Ok(timeline) => {
                    builder.timeline_layer(&timeline);
                }
                Err(e) => log::warn!("timeline {id}: {e}"),
            }
        }
        let weather = json["has_skylight"].as_bool() == Some(true)
            && json["has_ceiling"].as_bool() != Some(true)
            && dimension != "the_end";
        if weather {
            builder.weather_layers_with(&rain, &thunder);
        }
        builder.biome_post_layer(post.values());
        builder.constant_layer(&dimension_post);
        Ok(Self {
            system: builder.build(),
            frame,
            biomes,
            post,
        })
    }

    /// Sets what the packs' expressions read for the next samples.
    pub fn set_frame(&self, frame: Frame) {
        if let Ok(mut f) = self.frame.write() {
            *f = frame;
        }
    }

    /// A biome's attribute map (by id, `minecraft:plains`), as the packs left it.
    pub fn biome(&self, id: &str) -> Option<&AttributeMap> {
        self.biomes.get(id)
    }

    /// A biome's post-process map from the packs, if any sets one.
    pub fn biome_post(&self, id: &str) -> Option<&AttributeMap> {
        self.post.get(id)
    }

    /// Whether packs added post-process biome layers (the frame then mixes them too).
    pub fn has_biome_post(&self) -> bool {
        !self.post.is_empty()
    }

    /// `EnvironmentAttributeSystem.getValue` of one attribute, or its dimension value without
    /// a biome.
    pub fn value(&self, id: &str, at: &Sample) -> Option<Value> {
        let attribute = attribute(id)?;
        let biome = |_full_resolution: bool| at.biome;
        let clock = |_clock: &str| at.day_time;
        let post_biome = |_full_resolution: bool| at.post_biome;
        let conditions = Conditions {
            rain: at.rain,
            thunder: at.thunder,
            blend: at.blend,
            biome: &biome,
            clock: &clock,
            post_blend: at.post_blend,
            post_biome: Some(&post_biome),
        };
        match (at.blend, at.biome) {
            (None, None) => self.system.dimension_value(attribute, &conditions),
            _ => self.system.value(attribute, &conditions),
        }
    }

    /// The sky state the renderer draws.
    pub fn sample(&self, at: &Sample) -> Sky {
        let float = |id: &str| match self.value(id, at) {
            Some(Value::Float(f)) => f,
            _ => 0.0,
        };
        let rgb = |id: &str| match self.value(id, at) {
            Some(Value::Rgb(c)) => c,
            _ => [0.0; 3],
        };
        let argb = |id: &str| match self.value(id, at) {
            Some(Value::Argb(c)) => c,
            _ => [0.0; 4],
        };
        Sky {
            sky_color: rgb("visual/sky_color"),
            fog_color: rgb("visual/fog_color"),
            sky_light_factor: float("visual/sky_light_factor"),
            sky_light_color: rgb("visual/sky_light_color"),
            block_light_tint: rgb("visual/block_light_tint"),
            ambient_light_color: rgb("visual/ambient_light_color"),
            sun_angle: float("visual/sun_angle"),
            moon_angle: float("visual/moon_angle"),
            star_angle: float("visual/star_angle"),
            star_brightness: float("visual/star_brightness"),
            sunrise_color: argb("visual/sunrise_sunset_color"),
            cloud_color: argb("visual/cloud_color"),
            sky_light_level: float("gameplay/sky_light_level") / 15.0,
            fog_start: float("visual/fog_start_distance"),
            fog_end: float("visual/fog_end_distance"),
            cloud_fog_end: float("visual/cloud_fog_end_distance"),
            cloud_height: float("visual/cloud_height"),
        }
    }
}

/// `Level.updateSkyBrightness`'s darkening: from the sky light level, a dimension value.
pub fn sky_darken(environment: &Environment, day_time: i64, rain: f32, thunder: f32) -> i32 {
    let sky = environment.sample(&Sample {
        day_time,
        rain,
        thunder,
        ..Sample::default()
    });
    // Level.updateSkyBrightness: `(int)(15.0F - skyLightLevel)`, the float
    // truncated, not rounded (#11323): the darkening steps up only once the
    // light has fallen a whole level. `sky_light_level` is the attribute as a
    // fraction of 15, so the product is its raw value in `f32`.
    (15.0 - 15.0 * sky.sky_light_level) as i32
}

#[cfg(test)]
mod tests {
    use super::*;

    fn overworld() -> Option<Environment> {
        let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../.local/horizon-client/26.3");
        root.join("data/minecraft/timeline/day.json")
            .exists()
            .then(|| Environment::load(&root).unwrap())
    }

    #[test]
    fn dark_outside_follows_time_and_weather_levels() {
        let Some(env) = overworld() else {
            return;
        };
        let dark = |time, rain, thunder| sky_darken(&env, time, rain, thunder) >= 4;
        assert!(!dark(6000, 0.0, 0.0), "clear noon");
        assert!(!dark(6000, 1.0, 0.0), "rainy noon");
        assert!(dark(6000, 1.0, 1.0), "thunderstorm at noon");
        assert!(dark(18000, 0.0, 0.0), "clear midnight");
        assert!(!dark(1000, 0.0, 0.0), "clear morning");
    }

    /// A world with one sky-lit dimension and two biomes, one of them in a tag, and a pack with
    /// Polytone modifiers.
    fn with_packs(pack: &[(&str, &str)]) -> Environment {
        static NEXT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        let dir = std::env::temp_dir().join(format!(
            "horizon-env-packs-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&dir);
        let write = |path: &str, text: &str| {
            let path = dir.join(path);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, text).unwrap();
        };
        write(
            "world/data/minecraft/dimension_type/overworld.json",
            r##"{"has_skylight": true, "has_ceiling": false,
                "attributes": {"minecraft:visual/sky_color": "#808080", "minecraft:visual/fog_color": "#000000"}}"##,
        );
        write(
            "world/data/minecraft/worldgen/biome/plains.json",
            r##"{"attributes": {"minecraft:visual/sky_color": "#404040"}}"##,
        );
        write(
            "world/data/minecraft/worldgen/biome/desert.json",
            r#"{"attributes": {}}"#,
        );
        write(
            "world/data/minecraft/tags/worldgen/biome/dry.json",
            r#"{"values": ["minecraft:desert"]}"#,
        );
        for (path, text) in pack {
            write(&format!("pack/{path}"), text);
        }
        let modifiers = PackModifiers::read(&dir.join("pack")).with_configs(
            crate::packs::polytone_configs(&crate::packs::Source::Dir(dir.join("pack"))),
        );
        let env =
            Environment::load_dimension_with(&dir.join("world"), "overworld", &modifiers).unwrap();
        std::fs::remove_dir_all(&dir).unwrap();
        env
    }

    fn sky_at(env: &Environment, biome: &str, rain: f32) -> Sky {
        let map = env.biome(biome).unwrap();
        env.sample(&Sample {
            rain,
            biome: Some(map),
            post_biome: env.biome_post(biome),
            ..Sample::default()
        })
    }

    const GRAY_40: f32 = 64.0 / 255.0;

    #[test]
    fn pack_biome_modifiers_change_their_targets_only() {
        let env = with_packs(&[(
            "assets/mypack/polytone/biome_modifiers/dry_fog.json",
            r##"{"targets": "#minecraft:dry", "attributes_modifiers": {"visual/fog_color": "ff0000"}}"##,
        )]);
        assert_eq!(
            sky_at(&env, "minecraft:desert", 0.0).fog_color,
            [1.0, 0.0, 0.0]
        );
        assert_eq!(sky_at(&env, "minecraft:plains", 0.0).fog_color, [0.0; 3]);
        // Without packs the stack is vanilla's: the biome's own sky.
        let plain = with_packs(&[]);
        assert_eq!(
            sky_at(&plain, "minecraft:plains", 0.0).sky_color,
            [GRAY_40; 3]
        );
    }

    #[test]
    fn dimension_modifiers_remove_weather_layers_and_post_process_last() {
        let raining = |env: &Environment| sky_at(env, "minecraft:plains", 1.0).sky_color;
        // Vanilla's rain grays the sky toward a darker gray.
        assert_ne!(raining(&with_packs(&[])), [GRAY_40; 3]);
        let no_rain = with_packs(&[(
            "assets/minecraft/polytone/dimension_modifiers/overworld.json",
            r#"{"attributes_modifiers": {"rain": {"visual/sky_color": "REMOVE"}}}"#,
        )]);
        assert_eq!(raining(&no_rain), [GRAY_40; 3]);
        let post = with_packs(&[(
            "assets/minecraft/polytone/dimension_modifiers/overworld.json",
            r##"{"attributes_modifiers": {"post_process": {"visual/sky_color": "#0000ff"}}}"##,
        )]);
        assert_eq!(raining(&post), [0.0, 0.0, 1.0]);
    }

    #[test]
    fn biome_post_process_and_priority() {
        let env = with_packs(&[
            (
                "assets/a/polytone/biome_modifiers/wins.json",
                r##"{"targets": "minecraft:plains", "priority": 5,
                    "attributes_modifiers": {"base": {"visual/fog_color": "#00ff00"},
                                             "post_process": {"visual/sky_color": "#ffffff"}}}"##,
            ),
            (
                "assets/b/polytone/biome_modifiers/loses.json",
                r##"{"targets": "minecraft:plains", "priority": -1,
                    "attributes_modifiers": {"visual/fog_color": "#0000ff"}}"##,
            ),
        ]);
        // The higher priority's fog wins.
        assert_eq!(
            sky_at(&env, "minecraft:plains", 0.0).fog_color,
            [0.0, 1.0, 0.0]
        );
        // The post-process sky comes after the weather, so the rain doesn't gray it.
        assert_eq!(
            sky_at(&env, "minecraft:plains", 1.0).sky_color,
            [1.0, 1.0, 1.0]
        );
        assert!(env.biome_post("minecraft:desert").is_none());
    }

    #[test]
    fn expression_values_read_the_frame_and_the_value_so_far() {
        let env = with_packs(&[
            (
                "assets/minecraft/polytone/dimension_modifiers/overworld.json",
                r#"{"attributes_modifiers": {
                    "visual/fog_end_distance": "14 * (c.viewDistance / 16) + min(g.rain * 2, 1) * 100",
                    "visual/fog_start_distance": {"argument": "v + config('mypack:shift')", "modifier": "override"},
                    "visual/cloud_height": "v * 0.5"}}"#,
            ),
            (
                "assets/mypack/polytone/config_entries/shift.json",
                r#"{"default_value": 7}"#,
            ),
        ]);
        let at = |rain: f32, chunks: i32| {
            env.set_frame(Frame {
                rain,
                render_distance: chunks,
                ..Frame::default()
            });
            sky_at(&env, "minecraft:plains", 0.0)
        };
        // `c.viewDistance` is in blocks: 16 chunks are 256.
        assert_eq!(at(0.0, 16).fog_end, 224.0);
        assert_eq!(at(0.25, 32).fog_end, 498.0);
        // `v` is the value the layers before gave: the default start 0, and 192.33 for clouds.
        assert_eq!(at(0.0, 16).fog_start, 7.0);
        assert_eq!(at(0.0, 16).cloud_height, 192.33 * 0.5);
    }
}
