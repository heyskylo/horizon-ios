//! Original comparison transport for material conditions and cache behavior.
use horizon_core::{
    density::{ConstantSampler, DensitySampler, SampleError, SamplerContext},
    noise::{DensityVolume, NanRange, Noise},
    position::BlockPos,
    random::{LegacyRandom, PositionalRandom, RandomSource, XoroshiroRandom},
};
use horizon_world::worldgen::{
    anchor::{VerticalAnchor, WorldGenerationContext},
    biome_temperature::{BiomeTemperature, BiomeTemperatureSettings},
    decode_noise,
    density::DensityRegistry,
    material_condition::{
        MaterialCondition, MaterialConditionRegistry, MaterialConditions, MaterialDensitySources,
        MaterialNoiseSources, MaterialRule, MaterialRuleError, MaterialRuleLibrary,
        MaterialSurfaceQueries,
    },
    material_surface::{ClayBandBlocks, MaterialSurface},
    tags::TagDefinitions,
};
use horizon_world::{
    DATA_VERSION,
    nbt::Tag,
    registry::{BlockRegistry, BlockStateId, BlockStateJsonCodec},
};
use serde_json::Value;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::{self, Write},
    path::Path,
    sync::{Arc, Mutex},
};
// Canonicalize numeric values as double bits, avoiding differences in JSON
// number formatting while preserving every encoded int32/double value.
fn encoded(value: &MaterialCondition) -> String {
    let Ok(text) = value.to_json() else {
        return "error".to_owned();
    };
    let bytes = text.as_bytes();
    let mut transformed = Vec::new();
    let mut i = 0;
    let mut quoted = false;
    let mut escaped = false;
    while i < bytes.len() {
        if !quoted {
            let special = [
                ("-Infinity", 0xfff0000000000000u64),
                ("Infinity", 0x7ff0000000000000),
                ("NaN", 0x7ff8000000000000),
            ]
            .into_iter()
            .find(|(token, _)| bytes[i..].starts_with(token.as_bytes()));
            if let Some((token, bits)) = special {
                transformed.extend_from_slice(format!("\"number:{bits:x}\"").as_bytes());
                i += token.len();
                continue;
            }
        }
        let c = bytes[i];
        transformed.push(c);
        i += 1;
        if escaped {
            escaped = false;
        } else if quoted && c == b'\\' {
            escaped = true;
        } else if c == b'"' {
            quoted = !quoted;
        }
    }
    fn normalize(value: &mut Value) {
        match value {
            Value::Number(n) => {
                let v = n.as_f64().unwrap();
                *value = Value::String(format!("number:{:x}", v.to_bits()));
            }
            Value::Array(a) => a.iter_mut().for_each(normalize),
            Value::Object(o) => o.values_mut().for_each(normalize),
            _ => {}
        }
    }
    let mut value: Value = serde_json::from_slice(&transformed).unwrap();
    normalize(&mut value);
    serde_json::to_string(&value).unwrap()
}
fn anchor(v: &Value) -> VerticalAnchor {
    let n = v[1].as_i64().unwrap() as i32;
    match v[0].as_u64().unwrap() {
        0 => VerticalAnchor::Absolute(n),
        1 => VerticalAnchor::AboveBottom(n),
        2 => VerticalAnchor::BelowTop(n),
        3 => VerticalAnchor::RelativeToSeaLevel(n),
        _ => panic!("unknown anchor"),
    }
}
fn condition(v: &Value, registry: &MaterialConditionRegistry) -> MaterialCondition {
    match v[0].as_str().unwrap() {
        "json" => registry.decode(v[1].as_str().unwrap()).unwrap(),
        "noise" => MaterialCondition::NoiseThreshold {
            noise: v[1].as_str().unwrap().to_owned(),
            min: f64::from_bits(u64::from_str_radix(v[2].as_str().unwrap(), 16).unwrap()),
            max: f64::from_bits(u64::from_str_radix(v[3].as_str().unwrap(), 16).unwrap()),
            is_3d: v[4].as_bool().unwrap(),
        },
        "temperature" => MaterialCondition::Temperature,
        "above" => MaterialCondition::AbovePreliminarySurface,
        "biome" => MaterialCondition::Biome(
            v[1].as_array()
                .unwrap()
                .iter()
                .map(|name| name.as_str().unwrap().to_owned())
                .collect(),
        ),
        "hole" => MaterialCondition::Hole,
        "steep" => MaterialCondition::Steep,
        "y" => MaterialCondition::YAbove {
            anchor: anchor(&v[1]),
            surface_depth_multiplier: v[2].as_i64().unwrap() as i32,
            add_stone_depth: v[3].as_bool().unwrap(),
        },
        "water" => MaterialCondition::Water {
            offset: v[1].as_i64().unwrap() as i32,
            surface_depth_multiplier: v[2].as_i64().unwrap() as i32,
            add_stone_depth: v[3].as_bool().unwrap(),
        },
        "stone" => MaterialCondition::StoneDepth {
            offset: v[1].as_i64().unwrap() as i32,
            add_surface_depth: v[2].as_bool().unwrap(),
            secondary_depth_range: v[3].as_i64().unwrap() as i32,
            ceiling: v[4].as_bool().unwrap(),
        },
        "gradient" => MaterialCondition::VerticalGradient {
            random_name: v[1].as_str().unwrap().to_owned(),
            true_at_and_below: anchor(&v[2]),
            false_at_and_above: anchor(&v[3]),
        },
        "not" => MaterialCondition::Not(Box::new(condition(&v[1], registry))),
        _ => panic!("unknown condition"),
    }
}
#[derive(Default, Debug)]
struct Inputs {
    depth: i32,
    secondary: f32,
    queries: Vec<String>,
    biome: Option<String>,
    density: f32,
    density_remaining: i32,
    ore_values: [f32; 3],
    ore_remaining: [i32; 3],
}
#[derive(Debug)]
struct TracedDensity {
    inputs: Arc<Mutex<Inputs>>,
    channel: Option<usize>,
}
impl TracedDensity {
    fn label(&self) -> String {
        self.channel
            .map(|i| format!("ore {i}"))
            .unwrap_or_else(|| "density".to_owned())
    }
}
impl DensitySampler for TracedDensity {
    fn sample_value(&self, _: &mut SamplerContext, position: [i32; 3]) -> Result<f32, SampleError> {
        let [x, y, z] = position;
        let mut inputs = self.inputs.lock().unwrap();
        inputs
            .queries
            .push(format!("{} query {x} {y} {z}", self.label()));
        let remaining = match self.channel {
            Some(i) => &mut inputs.ore_remaining[i],
            None => &mut inputs.density_remaining,
        };
        if *remaining == 0 {
            return Err(SampleError::SurfaceSearchOverflow);
        }
        if *remaining > 0 {
            *remaining -= 1;
        }
        Ok(self
            .channel
            .map(|i| inputs.ore_values[i])
            .unwrap_or(inputs.density))
    }
    fn sample_volume(
        &self,
        context: &mut SamplerContext,
        buffer: &mut [f32],
        volume: DensityVolume,
    ) -> Result<(), SampleError> {
        let [sx, sy, sz] = volume.sizes();
        let [x, y, z] = volume.minimum();
        let [dx, dy, dz] = volume.steps();
        self.inputs.lock().unwrap().queries.push(format!(
            "{} volume {sx} {sy} {sz} {x} {y} {z} {dx} {dy} {dz}",
            self.label()
        ));
        for iz in 0..sz {
            for ix in 0..sx {
                for iy in 0..sy {
                    buffer[iy + (ix + iz * sx) * sy] = self.sample_value(
                        context,
                        [
                            x.wrapping_add((ix as i32).wrapping_mul(dx)),
                            y.wrapping_add((iy as i32).wrapping_mul(dy)),
                            z.wrapping_add((iz as i32).wrapping_mul(dz)),
                        ],
                    )?;
                }
            }
        }
        Ok(())
    }
}
struct Surface<P: PositionalRandom> {
    inputs: Arc<Mutex<Inputs>>,
    noises: MaterialNoiseSources<P, MaterialSurface<P>>,
    real_noise: bool,
    biomes: Arc<Mutex<BTreeMap<String, BiomeTemperature>>>,
    possible: Option<BTreeSet<String>>,
}
#[derive(Debug)]
struct TracedNoise {
    name: String,
    inputs: Arc<Mutex<Inputs>>,
}
impl Noise for TracedNoise {
    fn range(&self) -> Result<[f32; 2], NanRange> {
        Ok([f32::NEG_INFINITY, f32::INFINITY])
    }
    fn get2(&self, _: f64, _: f64) -> f32 {
        panic!("unexpected 2D noise call")
    }
    fn get3(&self, x: f64, y: f64, z: f64) -> f32 {
        let mut inputs = self.inputs.lock().unwrap();
        inputs.queries.push(format!(
            "noise query {} {} {} {}",
            self.name, x as i32, y as i32, z as i32
        ));
        inputs.secondary
    }
}
impl<P: PositionalRandom> MaterialSurfaceQueries for Surface<P> {
    fn possible_biomes(&self) -> Option<BTreeSet<String>> {
        self.possible.clone()
    }
    fn biome(&mut self, p: BlockPos) -> Result<Option<Arc<str>>, MaterialRuleError> {
        let mut inputs = self.inputs.lock().unwrap();
        inputs
            .queries
            .push(format!("biome query {} {} {}", p.x, p.y, p.z));
        if inputs.biome.as_deref() == Some("#throw") {
            return Err(MaterialRuleError::BiomeQueryFailed);
        }
        Ok(inputs.biome.as_deref().map(Arc::from))
    }
    fn cold_enough_to_snow(
        &mut self,
        name: &str,
        p: BlockPos,
        sea: i32,
    ) -> Result<bool, MaterialRuleError> {
        Ok(self
            .biomes
            .lock()
            .unwrap()
            .get_mut(name)
            .ok_or(MaterialRuleError::MissingBiome)?
            .cold_enough_to_snow(p, sea))
    }
    fn bind_noise(
        &mut self,
        name: &str,
    ) -> Result<Arc<dyn Noise>, horizon_world::worldgen::DecodeError> {
        let real = self.noises.bind_noise(name)?;
        if self.real_noise {
            Ok(real)
        } else {
            Ok(Arc::new(TracedNoise {
                name: name.to_owned(),
                inputs: self.inputs.clone(),
            }))
        }
    }
    fn band(&mut self, position: BlockPos) -> Result<BlockStateId, MaterialRuleError> {
        self.noises.band(position)
    }
    fn surface_depth(&mut self, _: i32, _: i32) -> i32 {
        self.inputs.lock().unwrap().depth
    }
    fn surface_secondary(&mut self, x: i32, z: i32) -> f64 {
        let mut i = self.inputs.lock().unwrap();
        i.queries.push(format!("secondary query {x} {z}"));
        f64::from(i.secondary)
    }
}
fn rule(
    v: &Value,
    registry: &MaterialConditionRegistry,
    inputs: &Arc<Mutex<Inputs>>,
) -> MaterialRule {
    match v[0].as_str().unwrap() {
        "block" => MaterialRule::Block(BlockStateId(v[1].as_u64().unwrap() as u32)),
        "band" => MaterialRule::Bandlands,
        "ore" => MaterialRule::OreVein {
            ore: BlockStateId(7),
            raw_ore: BlockStateId(8),
            filler: BlockStateId(9),
            raw_ore_chance: f32::from_bits(
                u32::from_str_radix(v[1].as_str().unwrap(), 16).unwrap(),
            ),
            density: Arc::new(TracedDensity {
                inputs: inputs.clone(),
                channel: Some(0),
            }),
            richness: Arc::new(TracedDensity {
                inputs: inputs.clone(),
                channel: Some(1),
            }),
            filler_gap: Arc::new(TracedDensity {
                inputs: inputs.clone(),
                channel: Some(2),
            }),
            positive_y: [i32::MIN, i32::MAX],
        },
        "if" => MaterialRule::Condition {
            if_true: condition(&v[1], registry),
            then_run: Box::new(rule(&v[2], registry, inputs)),
        },
        "sequence" => MaterialRule::Sequence(
            v[1].as_array()
                .unwrap()
                .iter()
                .map(|v| rule(v, registry, inputs))
                .collect(),
        ),
        _ => panic!("unknown rule"),
    }
}
fn queries(inputs: &Mutex<Inputs>, out: &mut impl Write) -> io::Result<()> {
    for query in inputs.lock().unwrap().queries.drain(..) {
        writeln!(out, "{query}")?;
    }
    Ok(())
}
struct RuleData {
    blocks: BlockRegistry,
    integers: BTreeSet<(String, String)>,
    densities: BTreeMap<String, String>,
    definitions: BTreeMap<String, String>,
}
fn metadata_tag(v: &Value) -> Tag {
    match v {
        Value::String(s) => Tag::String(s.as_str().into()),
        Value::Number(n) => Tag::Int(n.as_i64().unwrap() as i32),
        Value::Array(a) => Tag::List(a.iter().map(metadata_tag).collect()),
        Value::Object(o) => Tag::Compound(
            o.iter()
                .map(|(k, v)| (k.as_str().into(), metadata_tag(v)))
                .collect(),
        ),
        _ => panic!("invalid registry metadata"),
    }
}
impl RuleData {
    fn library(
        &self,
        f: &Value,
        conditions: &MaterialConditionRegistry,
        noises: &BTreeMap<String, String>,
        query: Option<&str>,
    ) -> Result<MaterialRuleLibrary, Box<dyn std::error::Error>> {
        let mut definitions = self.definitions.clone();
        if let Some(query) = query {
            definitions.insert("horizon:query".to_owned(), query.to_owned());
        }
        let blocks = BlockStateJsonCodec::new(&self.blocks, self.integers.clone())?;
        let densities = DensityRegistry::from_json(noises, &self.densities)?;
        Ok(MaterialRuleLibrary::from_json(
            &definitions,
            conditions,
            &blocks,
            densities,
            f["seed"].as_i64().unwrap(),
            f["legacy"].as_bool().unwrap(),
        )?)
    }
}
fn fixture<R: RandomSource>(
    f: &Value,
    registry: &MaterialConditionRegistry,
    noises: &BTreeMap<String, String>,
    biomes: Arc<Mutex<BTreeMap<String, BiomeTemperature>>>,
    rules_data: Option<&RuleData>,
    mut random: R,
    out: &mut impl Write,
) -> Result<(), Box<dyn std::error::Error>>
where
    R::Positional: Clone,
{
    let [min, height, sea] = std::array::from_fn(|i| f["context"][i].as_i64().unwrap() as i32);
    let generation = WorldGenerationContext::new(min, height, sea);
    let inputs = Arc::new(Mutex::new(Inputs {
        biome: Some("minecraft:plains".to_owned()),
        density_remaining: -1,
        ore_values: f
            .get("ore_values")
            .map(|v| {
                std::array::from_fn(|i| {
                    f32::from_bits(u32::from_str_radix(v[i].as_str().unwrap(), 16).unwrap())
                })
            })
            .unwrap_or([1.0, 1.0, -1.0]),
        ore_remaining: [-1; 3],
        ..Inputs::default()
    }));
    let root = random.fork_positional();
    let mut samplers: Vec<Arc<dyn Noise>> = Vec::new();
    for name in ["clay_bands_offset", "surface", "surface_secondary"] {
        let id = format!("minecraft:{name}");
        samplers.push(Arc::new(
            decode_noise(&noises[&id])?.create(&mut root.from_hash_of(&id))?,
        ));
    }
    let mut library = rules_data
        .map(|data| data.library(f, registry, noises, None))
        .transpose()?;
    if let Some(data) = rules_data {
        if let Some(values) = f.get("library_json") {
            for value in values.as_array().unwrap() {
                let result = data.library(f, registry, noises, Some(value.as_str().unwrap()));
                writeln!(
                    out,
                    "rule decode {}",
                    if result.is_ok() { "ok" } else { "error" }
                )?;
            }
        }
    }
    let block_id = |name: &str, fallback| {
        rules_data
            .map(|data| {
                data.blocks
                    .default_state(&format!("minecraft:{name}"))
                    .unwrap()
            })
            .unwrap_or(BlockStateId(fallback))
    };
    let blocks = ClayBandBlocks {
        terracotta: block_id("terracotta", 0),
        orange: block_id("orange_terracotta", 1),
        yellow: block_id("yellow_terracotta", 2),
        brown: block_id("brown_terracotta", 3),
        red: block_id("red_terracotta", 4),
        white: block_id("white_terracotta", 5),
        light_gray: block_id("light_gray_terracotta", 6),
    };
    let surface = MaterialSurface::new(
        root.clone(),
        blocks,
        samplers[0].clone(),
        samplers[1].clone(),
        samplers[2].clone(),
    );
    let sources = MaterialNoiseSources::from_json(root.clone(), surface, noises)?;
    let surface = Surface {
        inputs: inputs.clone(),
        noises: sources,
        real_noise: f["real_noise"].as_bool().unwrap_or(false),
        biomes,
        possible: f.get("possible_biomes").filter(|v| !v.is_null()).map(|v| {
            v.as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_str().unwrap().to_owned())
                .collect()
        }),
    };
    let dims: [i32; 9] = f
        .get("volume")
        .map(|v| std::array::from_fn(|i| v[i].as_i64().unwrap() as i32))
        .unwrap_or([16, 1, 16, 0, 0, 0, 1, 1, 1]);
    let expected = DensityVolume::new(
        dims[..3].try_into().unwrap(),
        dims[3..6].try_into().unwrap(),
        dims[6..].try_into().unwrap(),
    )?;
    let surface = MaterialDensitySources::new(
        surface,
        expected,
        if library.is_some() {
            Arc::new(ConstantSampler(0.0))
        } else {
            Arc::new(TracedDensity {
                inputs: inputs.clone(),
                channel: None,
            })
        },
        if library.is_some() {
            SamplerContext::cached()
        } else {
            SamplerContext::default()
        },
    )?;
    let mut worker = MaterialConditions::<R, _>::new(generation, root, surface);
    let mut count = 0;
    for c in f["conditions"].as_array().unwrap() {
        let value = condition(c, registry);
        writeln!(out, "runtime {}", encoded(&value))?;
        worker.add(value)?;
        count += 1;
    }
    if let Some(values) = f.get("json") {
        for value in values.as_array().unwrap() {
            match registry.decode(value.as_str().unwrap()) {
                Err(_) => writeln!(out, "decode error")?,
                Ok(condition) => {
                    writeln!(out, "decode {}", encoded(&condition))?;
                    match worker.add(condition) {
                        Ok(_) => count += 1,
                        Err(_) => writeln!(out, "compile error")?,
                    }
                }
            }
        }
    }
    let mut rule_count = 0;
    if let Some(values) = f.get("rules") {
        for value in values.as_array().unwrap() {
            worker.add_rule(rule(value, registry, &inputs))?;
            queries(&inputs, out)?;
            rule_count += 1;
        }
    }
    if let Some(library) = &mut library {
        for name in f["library_rules"].as_array().unwrap() {
            worker.add_rule(library.rule(name.as_str().unwrap())?)?;
            queries(&inputs, out)?;
            rule_count += 1;
        }
    }
    for op in f["ops"].as_array().unwrap() {
        match op[0].as_str().unwrap() {
            "ore_input" => {
                let mut i = inputs.lock().unwrap();
                let channel = op[1].as_u64().unwrap() as usize;
                i.ore_values[channel] =
                    f32::from_bits(u32::from_str_radix(op[2].as_str().unwrap(), 16)?);
                i.ore_remaining[channel] = op[3].as_i64().unwrap() as i32;
            }
            "density" => {
                let mut i = inputs.lock().unwrap();
                i.density = f32::from_bits(u32::from_str_radix(op[1].as_str().unwrap(), 16)?);
                i.density_remaining = op[2].as_i64().unwrap() as i32;
            }
            "minimum" => {
                let value = worker.minimum_surface_level();
                queries(&inputs, out)?;
                match value {
                    Ok(v) => writeln!(out, "minimum {v}")?,
                    Err(_) => writeln!(out, "minimum error")?,
                }
            }
            "biome" => inputs.lock().unwrap().biome = op[1].as_str().map(str::to_owned),
            "rule" => {
                let result = worker.add_rule(rule(&op[1], registry, &inputs));
                queries(&inputs, out)?;
                match result {
                    Ok(index) => {
                        rule_count += 1;
                        writeln!(out, "rule added {index}")?;
                    }
                    Err(_) => writeln!(out, "rule compile error")?,
                }
            }
            "apply" => {
                let position = BlockPos::new(
                    op[1].as_i64().unwrap() as i32,
                    op[2].as_i64().unwrap() as i32,
                    op[3].as_i64().unwrap() as i32,
                );
                for index in 0..rule_count {
                    let result = worker.try_apply(index, position);
                    queries(&inputs, out)?;
                    match result {
                        Ok(Some(block)) => writeln!(out, "rule {index} {}", block.0)?,
                        Ok(None) => writeln!(out, "rule {index} none")?,
                        Err(_) => writeln!(out, "rule {index} error")?,
                    }
                }
            }
            "input" => {
                let mut i = inputs.lock().unwrap();
                i.depth = op[1].as_i64().unwrap() as i32;
                i.secondary = f32::from_bits(u32::from_str_radix(op[2].as_str().unwrap(), 16)?);
            }
            "xz" => worker.update_xz(
                op[1].as_i64().unwrap() as i32,
                op[2].as_i64().unwrap() as i32,
                op[3].as_i64().unwrap() as i32,
                op[4].as_i64().unwrap() as i32,
            ),
            "y" => worker.update_y(
                op[1].as_i64().unwrap() as i32,
                op[2].as_i64().unwrap() as i32,
                op[3].as_i64().unwrap() as i32,
                op[4].as_i64().unwrap() as i32,
            ),
            "add" => {
                let index = worker.add(condition(&op[1], registry))?;
                writeln!(out, "added {index}")?;
                count += 1;
            }
            "eval" => {
                let indices = if op.as_array().unwrap().len() == 1 {
                    (0..count).collect()
                } else {
                    op[1]
                        .as_array()
                        .unwrap()
                        .iter()
                        .map(|i| i.as_u64().unwrap() as usize)
                        .collect::<Vec<_>>()
                };
                for index in indices {
                    let result = worker.test(index);
                    queries(&inputs, out)?;
                    match result {
                        Ok(value) => writeln!(out, "eval {index} {value}")?,
                        Err(_) => writeln!(out, "eval {index} error")?,
                    }
                }
            }
            "secondary" => {
                let value = worker.surface_secondary();
                queries(&inputs, out)?;
                let bits = if value.is_nan() {
                    0x7ff8000000000000
                } else {
                    value.to_bits()
                };
                writeln!(out, "secondary {bits:x}")?;
            }
            _ => panic!("unknown operation"),
        }
    }
    Ok(())
}
pub fn main() -> Result<(), Box<dyn std::error::Error>> {
    let arg = crate::args().nth(1).expect("private directory");
    let dir = Path::new(&arg);
    let mut out = io::BufWriter::new(fs::File::create(dir.join("rust.txt"))?);
    let definitions = serde_json::from_str(&fs::read_to_string(dir.join("conditions.json"))?)?;
    let biome_definitions: BTreeMap<String, String> =
        serde_json::from_str(&fs::read_to_string(dir.join("biomes.json"))?)?;
    let mut temperatures = BTreeMap::new();
    for (name, json) in &biome_definitions {
        temperatures.insert(
            name.clone(),
            BiomeTemperature::new(BiomeTemperatureSettings::from_json(json)?),
        );
    }
    let biomes = Arc::new(Mutex::new(temperatures));
    let tag_json: BTreeMap<String, String> =
        serde_json::from_str(&fs::read_to_string(dir.join("biome_tags.json"))?)?;
    let mut tag_definitions = TagDefinitions::default();
    for (name, json) in tag_json {
        tag_definitions.apply_json(&name, "vanilla", &json)?;
    }
    let tags = tag_definitions
        .build(|id, _required| biome_definitions.contains_key(id).then(|| id.to_owned()))?;
    assert!(tags.missing.is_empty());
    let registry = MaterialConditionRegistry::from_json_with_biome_tags(
        &definitions,
        &biome_definitions.keys().cloned().collect(),
        &tags.tags,
    )?;
    let noises = serde_json::from_str(&fs::read_to_string(dir.join("noises.json"))?)?;
    let rules_data = if dir.join("rules.json").exists() {
        let facts: Value =
            serde_json::from_str(&fs::read_to_string(dir.join("facts-server.json"))?)?;
        Some(RuleData {
            blocks: BlockRegistry::from_nbt(&Tag::compound([
                ("DataVersion", Tag::Int(DATA_VERSION)),
                ("states", metadata_tag(&facts["states"])),
                ("defaults", metadata_tag(&facts["defaults"])),
            ]))?,
            integers: facts["integers"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| {
                    (
                        v[0].as_str().unwrap().to_owned(),
                        v[1].as_str().unwrap().to_owned(),
                    )
                })
                .collect(),
            densities: serde_json::from_str(&fs::read_to_string(dir.join("densities.json"))?)?,
            definitions: serde_json::from_str(&fs::read_to_string(dir.join("rules.json"))?)?,
        })
    } else {
        None
    };
    for (name, value) in registry.entries() {
        writeln!(out, "registry {name} {}", encoded(value))?;
    }
    for line in fs::read_to_string(dir.join("inputs.txt"))?.lines() {
        let f: Value = serde_json::from_str(line)?;
        let seed = f["seed"].as_i64().unwrap();
        if f["legacy"].as_bool().unwrap() {
            fixture(
                &f,
                &registry,
                &noises,
                biomes.clone(),
                rules_data.as_ref(),
                LegacyRandom::new(seed),
                &mut out,
            )?
        } else {
            fixture(
                &f,
                &registry,
                &noises,
                biomes.clone(),
                rules_data.as_ref(),
                XoroshiroRandom::new(seed),
                &mut out,
            )?
        }
    }
    out.flush()?;
    Ok(())
}
