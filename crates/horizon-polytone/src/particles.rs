//! Custom particle types (`polytone/custom_particles/`, wiki "Custom Particle Types") and the
//! particle emitters blocks and entities carry (`block_modifiers`, `entity_modifiers`): what a
//! particle does is data, its numbers expressions compiled when the pack loads.
use crate::expr::Program;
use crate::target::Targets;
use serde_json::{Map, Value as Json};
use std::collections::BTreeMap;

/// An expression field: a string, or a plain number or boolean written as one.
fn program(v: &Json) -> Result<Program, String> {
    match v {
        Json::String(s) => Program::compile(s),
        Json::Number(n) => Program::compile(&n.to_string()),
        Json::Bool(b) => Program::compile(if *b { "1" } else { "0" }),
        _ => Err(format!("{v} is not an expression")),
    }
}

/// The expression fields of `object` named in `keys`, and its `customs` map.
fn expressions(
    object: &Map<String, Json>,
    keys: &[&str],
) -> Result<(BTreeMap<String, Program>, BTreeMap<String, Program>), String> {
    let mut fields = BTreeMap::new();
    for key in keys {
        if let Some(v) = object.get(*key) {
            fields.insert(
                (*key).to_owned(),
                program(v).map_err(|e| format!("{key}: {e}"))?,
            );
        }
    }
    let mut customs = BTreeMap::new();
    for (name, v) in object
        .get("customs")
        .and_then(Json::as_object)
        .into_iter()
        .flatten()
    {
        // Named variables are case insensitive.
        customs.insert(
            name.to_lowercase(),
            program(v).map_err(|e| format!("customs.{name}: {e}"))?,
        );
    }
    Ok((fields, customs))
}

/// The particle an emitter spawns: an id, or an object with that particle's own options
/// (`{"type": "minecraft:block", "block_state": ...}`).
#[derive(Clone, Debug, PartialEq)]
pub enum ParticleRef {
    Id(String),
    Options(Map<String, Json>),
}

impl ParticleRef {
    fn parse(v: &Json) -> Result<Self, String> {
        let id = |s: &str| {
            if s.contains(':') {
                s.to_owned()
            } else {
                format!("minecraft:{s}")
            }
        };
        match v {
            Json::String(s) => Ok(Self::Id(id(s))),
            Json::Object(o) if o.get("type").is_some_and(Json::is_string) => {
                Ok(Self::Options(o.clone()))
            }
            _ => Err(format!("{v} is no particle")),
        }
    }

    /// The particle type's id.
    pub fn id(&self) -> String {
        match self {
            Self::Id(id) => id.clone(),
            Self::Options(o) => {
                let t = o["type"].as_str().unwrap_or_default();
                if t.contains(':') {
                    t.to_owned()
                } else {
                    format!("minecraft:{t}")
                }
            }
        }
    }
}

/// A particle emitter: what it spawns and, as expressions, how often (`chance`, and for entity
/// emitters `count`), where (`x`, `y`, `z`) and with what motion and look.
pub struct Emitter {
    pub particle: ParticleRef,
    /// `chance`, `count`, `x`, `y`, `z`, `dx`, `dy`, `dz`, the colour (`red`... or `r`...),
    /// `roll`, `size`, `custom`: those present.
    pub fields: BTreeMap<String, Program>,
    /// Named variables seeded on the particle spawned.
    pub customs: BTreeMap<String, Program>,
    /// A rule test on the block state at the spawn position, kept as JSON.
    pub state_predicate: Option<Json>,
    /// Biome ids or tags where it spawns, kept as JSON.
    pub biomes: Option<Json>,
    /// Block emitters: where `x`, `y`, `z` count from (`lower_corner`, `center`...).
    pub spawn_location: Option<String>,
    /// Entity emitters: the model bone it sits on, and how far from the camera it still runs.
    pub bone: Option<String>,
    pub max_distance: Option<f64>,
    /// Block emitters: `animate_tick` (the default), `block_broken` or `block_cracking`.
    pub tick_source: Option<String>,
}

const EMITTER_FIELDS: [&str; 19] = [
    "chance", "count", "x", "y", "z", "dx", "dy", "dz", "red", "green", "blue", "alpha", "r", "g",
    "b", "a", "roll", "size", "custom",
];

impl Emitter {
    pub fn parse(v: &Json) -> Result<Self, String> {
        let o = v.as_object().ok_or("an emitter is not an object")?;
        let particle =
            ParticleRef::parse(o.get("particle").ok_or("an emitter without a particle")?)?;
        let (fields, customs) = expressions(o, &EMITTER_FIELDS)?;
        Ok(Self {
            particle,
            fields,
            customs,
            state_predicate: o.get("state_predicate").cloned(),
            biomes: o.get("biomes").cloned(),
            spawn_location: o
                .get("spawn_location")
                .and_then(Json::as_str)
                .map(str::to_owned),
            bone: o.get("bone").and_then(Json::as_str).map(str::to_owned),
            max_distance: o.get("max_distance").and_then(Json::as_f64),
            tick_source: o
                .get("tick_source")
                .and_then(Json::as_str)
                .map(str::to_owned),
        })
    }
}

fn emitters(v: Option<&Json>) -> Result<Vec<Emitter>, String> {
    v.and_then(Json::as_array)
        .into_iter()
        .flatten()
        .enumerate()
        .map(|(i, e)| Emitter::parse(e).map_err(|err| format!("emitter {i}: {err}")))
        .collect()
}

/// `type`: a full custom particle, one copying a vanilla particle's behaviour, or a GPU one.
#[derive(Clone, Debug, PartialEq)]
pub enum ParticleKind {
    Custom,
    SemiCustom { copy_from: String },
    Gpu,
}

/// `render_type`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RenderType {
    Opaque,
    Translucent,
    AdditiveTranslucent,
    /// Drawn nowhere: a particle that only runs logic (its emitters, its sounds).
    Invisible,
}

/// `liquid_affinity`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LiquidAffinity {
    Any,
    Liquids,
    NonLiquids,
}

/// A custom particle type.
pub struct CustomParticle {
    pub kind: ParticleKind,
    pub render_type: RenderType,
    /// Expressions setting the particle up when it spawns (`size`, `lifetime`, colour,
    /// `roll`, `friction`, `custom`, `hitbox_size`...), and its named variables.
    pub initializer: BTreeMap<String, Program>,
    pub initializer_customs: BTreeMap<String, Program>,
    /// Expressions run each tick (`x`... `dz`, `size`, colour, `roll`, `custom`,
    /// `remove_condition`), and named variables reassigned.
    pub ticker: BTreeMap<String, Program>,
    pub ticker_customs: BTreeMap<String, Program>,
    pub emitters: Vec<Emitter>,
    /// Sound emitters, kept as JSON.
    pub sound_emitters: Vec<Json>,
    pub has_physics: bool,
    pub kill_on_contact: bool,
    pub kill_when_still: bool,
    pub kill_when_not_in_view: bool,
    pub random_sprite: bool,
    pub force_spawn: bool,
    pub liquid_affinity: LiquidAffinity,
    /// The most of this type alive at once.
    pub limit: Option<u32>,
    /// Ticks between two runs of the tick logic (`tick_interval`, or `tick_rate`).
    pub tick_interval: u32,
    /// No spawn this close to another particle of the type.
    pub exclusion_radius: f64,
    pub light_level: u8,
    pub rotation_mode: String,
    /// Fields kept as JSON for the renderer: `offset`, `colormap`, `model`, a GPU particle's
    /// `texture`, `shader` and `uniforms`.
    pub render: Map<String, Json>,
}

const INITIALIZER: [&str; 13] = [
    "size",
    "hitbox_size",
    "lifetime",
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
const TICKER: [&str; 14] = [
    "x",
    "y",
    "z",
    "dx",
    "dy",
    "dz",
    "size",
    "red",
    "green",
    "blue",
    "alpha",
    "roll",
    "custom",
    "remove_condition",
];

impl CustomParticle {
    pub fn parse(json: &Json) -> Result<Self, String> {
        let o = json.as_object().ok_or("a particle is not an object")?;
        let text = |key: &str| o.get(key).and_then(Json::as_str);
        let flag = |key: &str, default: bool| o.get(key).and_then(Json::as_bool).unwrap_or(default);
        let kind = match (text("type"), text("copy_from")) {
            (Some("gpu"), _) => ParticleKind::Gpu,
            (Some("semi_custom") | None, Some(from)) => ParticleKind::SemiCustom {
                copy_from: from.to_owned(),
            },
            (Some("custom") | None, _) => ParticleKind::Custom,
            (Some(other), _) => return Err(format!("unknown particle type {other}")),
        };
        let render_type = match text("render_type").unwrap_or("opaque") {
            "opaque" => RenderType::Opaque,
            "translucent" => RenderType::Translucent,
            "additive_translucent" => RenderType::AdditiveTranslucent,
            "invisible" => RenderType::Invisible,
            other => return Err(format!("unknown render_type {other}")),
        };
        let liquid_affinity = match text("liquid_affinity").unwrap_or("any") {
            "any" => LiquidAffinity::Any,
            "liquids" => LiquidAffinity::Liquids,
            "non_liquids" => LiquidAffinity::NonLiquids,
            other => return Err(format!("unknown liquid_affinity {other}")),
        };
        let empty = Map::new();
        let section = |key: &str| o.get(key).and_then(Json::as_object).unwrap_or(&empty);
        let (initializer, initializer_customs) = expressions(section("initializer"), &INITIALIZER)
            .map_err(|e| format!("initializer.{e}"))?;
        let (ticker, ticker_customs) =
            expressions(section("ticker"), &TICKER).map_err(|e| format!("ticker.{e}"))?;
        let count = |key: &str| {
            o.get(key)
                .and_then(Json::as_u64)
                .and_then(|n| u32::try_from(n).ok())
        };
        let render = [
            "offset", "colormap", "model", "texture", "shader", "uniforms", "lit",
        ]
        .into_iter()
        .filter_map(|k| Some((k.to_owned(), o.get(k)?.clone())))
        .collect();
        Ok(Self {
            kind,
            render_type,
            initializer,
            initializer_customs,
            ticker,
            ticker_customs,
            emitters: emitters(o.get("particle_emitters"))?,
            sound_emitters: o
                .get("sound_emitters")
                .and_then(Json::as_array)
                .cloned()
                .unwrap_or_default(),
            has_physics: flag("has_physics", true),
            kill_on_contact: flag("kill_on_contact", false),
            kill_when_still: flag("kill_when_still", true),
            kill_when_not_in_view: flag("kill_when_not_in_view", true),
            random_sprite: flag("random_sprite", false),
            force_spawn: flag("force_spawn", false),
            liquid_affinity,
            limit: count("limit"),
            tick_interval: count("tick_interval")
                .or(count("tick_rate"))
                .unwrap_or(1)
                .max(1),
            exclusion_radius: o
                .get("exclusion_radius")
                .and_then(Json::as_f64)
                .unwrap_or(0.0),
            light_level: count("light_level").map_or(0, |l| l.min(15) as u8),
            rotation_mode: text("rotation_mode").unwrap_or("look_at_xyz").to_owned(),
            render,
        })
    }
}

/// A block modifier's particle side: the blocks it targets and their emitters.
pub struct BlockEmitters {
    pub targets: Targets,
    pub priority: i32,
    pub particle_emitters: Vec<Emitter>,
    pub sound_emitters: Vec<Json>,
}

impl BlockEmitters {
    /// `None` when the file has no emitters (it modifies other things) or is switched off.
    pub fn parse(json: &Json, implicit: Option<&str>) -> Result<Option<Self>, String> {
        if json.get("polytone_ignore").and_then(Json::as_bool) == Some(true) {
            return Ok(None);
        }
        let particle_emitters = emitters(json.get("particle_emitters"))?;
        let sound_emitters = json
            .get("sound_emitters")
            .and_then(Json::as_array)
            .cloned()
            .unwrap_or_default();
        if particle_emitters.is_empty() && sound_emitters.is_empty() {
            return Ok(None);
        }
        Ok(Some(Self {
            targets: Targets::parse(json.get("targets"), implicit)?,
            priority: json.get("priority").and_then(Json::as_i64).unwrap_or(0) as i32,
            particle_emitters,
            sound_emitters,
        }))
    }
}

/// An entity modifier: the entities it targets and the emitters on their model bones.
pub struct EntityEmitters {
    pub targets: Targets,
    pub emitters: Vec<Emitter>,
}

impl EntityEmitters {
    pub fn parse(json: &Json, implicit: Option<&str>) -> Result<Option<Self>, String> {
        if json.get("polytone_ignore").and_then(Json::as_bool) == Some(true) {
            return Ok(None);
        }
        Ok(Some(Self {
            targets: Targets::parse(json.get("targets"), implicit)?,
            emitters: emitters(json.get("emitters"))?,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_custom_particle_reads_its_defaults_and_expressions() {
        let p = CustomParticle::parse(&json!({
            "initializer": {"lifetime": "10+r.rand()*10", "size": 4.0, "customs": {"Phase": "1"}},
            "ticker": {"dx": "o.xd*1.01", "remove_condition": "o.age > 20"},
            "render_type": "translucent",
            "liquid_affinity": "non_liquids",
            "limit": 512,
            "tick_rate": 2
        }))
        .unwrap();
        assert_eq!(p.kind, ParticleKind::Custom);
        assert_eq!(p.render_type, RenderType::Translucent);
        assert_eq!(p.liquid_affinity, LiquidAffinity::NonLiquids);
        assert_eq!((p.limit, p.tick_interval), (Some(512), 2));
        assert!(p.has_physics && p.kill_when_still && !p.kill_on_contact);
        let keys: Vec<&str> = p.initializer.keys().map(String::as_str).collect();
        assert_eq!(keys, ["lifetime", "size"]);
        // Named variables are case insensitive.
        assert!(p.initializer_customs.contains_key("phase"));
        assert!(p.ticker.contains_key("remove_condition"));
    }

    #[test]
    fn particle_kinds_and_render_types() {
        let kind = |v: Json| CustomParticle::parse(&v).map(|p| p.kind);
        assert_eq!(
            kind(json!({"copy_from": "minecraft:flame"})).unwrap(),
            ParticleKind::SemiCustom {
                copy_from: "minecraft:flame".into()
            }
        );
        assert_eq!(
            kind(json!({"type": "gpu", "render_type": "invisible"})).unwrap(),
            ParticleKind::Gpu
        );
        assert!(kind(json!({"type": "laser"})).is_err());
        assert!(CustomParticle::parse(&json!({"render_type": "neon"})).is_err());
    }

    #[test]
    fn emitters_spawn_ids_or_particles_with_options() {
        let p = CustomParticle::parse(&json!({"particle_emitters": [
            {"particle": "flame", "chance": "(o.age == 1)", "y": 2, "state_predicate": {"predicate_type": "block_match", "block": "minecraft:air"}},
            {"particle": {"type": "minecraft:block", "block_state": "minecraft:stone"}, "chance": 1}
        ]}))
        .unwrap();
        assert_eq!(
            p.emitters[0].particle,
            ParticleRef::Id("minecraft:flame".into())
        );
        assert!(
            p.emitters[0].fields.contains_key("chance") && p.emitters[0].fields.contains_key("y")
        );
        assert!(p.emitters[0].state_predicate.is_some());
        assert_eq!(p.emitters[1].particle.id(), "minecraft:block");
        // A broken expression fails the file when the pack loads.
        assert!(
            CustomParticle::parse(
                &json!({"particle_emitters": [{"particle": "flame", "chance": "1 +"}]})
            )
            .is_err()
        );
    }

    #[test]
    fn block_and_entity_modifiers_keep_their_emitters() {
        let block = BlockEmitters::parse(
            &json!({"targets": ["#minecraft:leaves"], "particle_emitters": [
                {"particle": "mypack:leaf", "chance": "0.015 + g.rain*0.015", "spawn_location": "lower_corner"}]}),
            None,
        )
        .unwrap()
        .unwrap();
        assert!(
            block
                .targets
                .matches("minecraft:oak_leaves", |t| t == "minecraft:leaves")
        );
        assert_eq!(
            block.particle_emitters[0].spawn_location.as_deref(),
            Some("lower_corner")
        );
        // A block modifier that only changes other things has no emitters.
        assert!(
            BlockEmitters::parse(&json!({"targets": "stone", "sound_type": "metal"}), None)
                .unwrap()
                .is_none()
        );
        let entity = EntityEmitters::parse(
            &json!({"targets": ["minecraft:player"], "emitters": [
                {"particle": "mypack:fog", "max_distance": 32, "chance": 0.1, "count": "30", "x": "r.rand(-25.0, 25.0)"}]}),
            None,
        )
        .unwrap()
        .unwrap();
        assert_eq!(entity.emitters[0].max_distance, Some(32.0));
        assert!(entity.emitters[0].fields.contains_key("count"));
    }
}
