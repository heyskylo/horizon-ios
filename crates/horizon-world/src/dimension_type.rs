//! The parts of vanilla's `DimensionType` Horizon reads, and the attributes keyed on the level today.

use std::sync::LazyLock;

use horizon_core::level_key::LevelKey;
use serde_json::Value;

use crate::worldgen::overrides::LevelHeight;
use crate::worldgen::value_provider::IntProvider;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Skybox {
    None,
    Overworld,
    End,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CardinalLight {
    Default,
    Nether,
}

/// `BedRule.Rule`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BedRuleTest {
    Always,
    WhenDark,
    Never,
}

impl BedRuleTest {
    /// `BedRule.Rule.test`.
    pub fn test(self, dark_outside: bool) -> bool {
        match self {
            Self::Always => true,
            Self::WhenDark => dark_outside,
            Self::Never => false,
        }
    }

    fn parse(name: &str) -> Result<Self, String> {
        match name {
            "always" => Ok(Self::Always),
            "when_dark" => Ok(Self::WhenDark),
            "never" => Ok(Self::Never),
            _ => Err(format!("unknown bed rule {name:?}")),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BedRule {
    pub can_sleep: BedRuleTest,
    pub can_set_spawn: BedRuleTest,
    pub destroy_on_use: bool,
    pub destroy_on_leave: bool,
}

impl BedRule {
    pub const CAN_SLEEP_WHEN_DARK: Self = Self {
        can_sleep: BedRuleTest::WhenDark,
        can_set_spawn: BedRuleTest::Always,
        destroy_on_use: false,
        destroy_on_leave: false,
    };
    pub const DESTROY_ON_USE: Self = Self {
        can_sleep: BedRuleTest::Never,
        can_set_spawn: BedRuleTest::Never,
        destroy_on_use: true,
        destroy_on_leave: false,
    };
    pub const DESTROY_ON_LEAVE: Self = Self {
        can_sleep: BedRuleTest::WhenDark,
        can_set_spawn: BedRuleTest::Never,
        destroy_on_use: false,
        destroy_on_leave: true,
    };

    fn from_json(v: &Value) -> Result<Self, String> {
        let rule = |k: &str| {
            BedRuleTest::parse(v[k].as_str().ok_or_else(|| format!("bed rule needs {k}"))?)
        };
        Ok(Self {
            can_sleep: rule("can_sleep")?,
            can_set_spawn: rule("can_set_spawn")?,
            destroy_on_use: v["destroy_on_use"].as_bool().unwrap_or(false),
            destroy_on_leave: v["destroy_on_leave"].as_bool().unwrap_or(false),
        })
    }
}

/// A dimension type, with the gameplay attributes `EnvironmentAttributes` defaults fill in.
#[derive(Clone, Debug)]
pub struct DimensionFacts {
    pub id: String,
    pub has_fixed_time: bool,
    pub has_ender_dragon_fight: bool,
    pub coordinate_scale: f64,
    pub height: LevelHeight,
    pub infiniburn: String,
    pub ambient_light: f32,
    pub monster_spawn_light_level: IntProvider,
    pub monster_spawn_block_light_limit: i32,
    pub skybox: Skybox,
    pub cardinal_light: CardinalLight,
    pub default_clock: Option<String>,
    pub timelines: Vec<String>,
    pub water_evaporates: bool,
    pub fast_lava: bool,
    pub respawn_anchor_works: bool,
    pub nether_portal_spawns_piglin: bool,
    pub piglins_zombify: bool,
    pub snow_golem_melts: bool,
    pub can_start_raid: bool,
    pub sky_light_level: f32,
    pub bed_rule: BedRule,
    pub straw_bed_rule: BedRule,
}

const VANILLA_TYPES: [&str; 4] = [
    "minecraft:overworld",
    "minecraft:overworld_caves",
    "minecraft:the_nether",
    "minecraft:the_end",
];

static VANILLA: LazyLock<[DimensionFacts; 4]> = LazyLock::new(|| {
    let overworld = |id: &str, has_ceiling: bool| DimensionFacts {
        id: id.to_owned(),
        has_fixed_time: false,
        has_ender_dragon_fight: false,
        coordinate_scale: 1.0,
        height: LevelHeight {
            min_y: -64,
            height: 384,
            logical_height: 384,
            has_skylight: true,
            has_ceiling,
        },
        infiniburn: "#minecraft:infiniburn_overworld".to_owned(),
        ambient_light: 0.0,
        monster_spawn_light_level: IntProvider::Uniform { min: 0, max: 7 },
        monster_spawn_block_light_limit: 0,
        skybox: Skybox::Overworld,
        cardinal_light: CardinalLight::Default,
        default_clock: Some("minecraft:overworld".to_owned()),
        timelines: [
            "minecraft:villager_schedule",
            "minecraft:day",
            "minecraft:moon",
            "minecraft:early_game",
        ]
        .map(str::to_owned)
        .to_vec(),
        water_evaporates: false,
        fast_lava: false,
        respawn_anchor_works: false,
        nether_portal_spawns_piglin: true,
        piglins_zombify: true,
        snow_golem_melts: false,
        can_start_raid: true,
        sky_light_level: 15.0,
        bed_rule: BedRule::CAN_SLEEP_WHEN_DARK,
        straw_bed_rule: BedRule::DESTROY_ON_LEAVE,
    };
    let overworld_type = overworld("minecraft:overworld", false);
    let caves = overworld("minecraft:overworld_caves", true);
    let nether = DimensionFacts {
        id: "minecraft:the_nether".to_owned(),
        has_fixed_time: true,
        coordinate_scale: 8.0,
        height: LevelHeight {
            min_y: 0,
            height: 256,
            logical_height: 128,
            has_skylight: false,
            has_ceiling: true,
        },
        infiniburn: "#minecraft:infiniburn_nether".to_owned(),
        ambient_light: 0.1,
        monster_spawn_light_level: IntProvider::Constant(7),
        monster_spawn_block_light_limit: 15,
        skybox: Skybox::None,
        cardinal_light: CardinalLight::Nether,
        default_clock: None,
        timelines: vec!["minecraft:villager_schedule".to_owned()],
        water_evaporates: true,
        fast_lava: true,
        respawn_anchor_works: true,
        nether_portal_spawns_piglin: false,
        piglins_zombify: false,
        snow_golem_melts: true,
        can_start_raid: false,
        sky_light_level: 4.0,
        bed_rule: BedRule::DESTROY_ON_USE,
        straw_bed_rule: BedRule::DESTROY_ON_USE,
        ..overworld_type.clone()
    };
    let end = DimensionFacts {
        id: "minecraft:the_end".to_owned(),
        has_fixed_time: true,
        has_ender_dragon_fight: true,
        height: LevelHeight {
            min_y: 0,
            height: 256,
            logical_height: 256,
            has_skylight: true,
            has_ceiling: false,
        },
        infiniburn: "#minecraft:infiniburn_end".to_owned(),
        ambient_light: 0.25,
        monster_spawn_light_level: IntProvider::Constant(15),
        skybox: Skybox::End,
        default_clock: Some("minecraft:the_end".to_owned()),
        timelines: vec!["minecraft:villager_schedule".to_owned()],
        nether_portal_spawns_piglin: false,
        bed_rule: BedRule::DESTROY_ON_USE,
        straw_bed_rule: BedRule::DESTROY_ON_USE,
        ..overworld_type.clone()
    };
    [overworld_type, caves, nether, end]
});

impl DimensionFacts {
    /// A vanilla dimension type by id.
    pub fn vanilla(type_id: &str) -> Option<&'static Self> {
        VANILLA_TYPES
            .iter()
            .position(|id| *id == type_id)
            .map(|i| &VANILLA[i])
    }

    /// The type of a built-in level; any other key gets the Overworld's.
    pub fn of_builtin_level(level: LevelKey) -> &'static Self {
        match level {
            LevelKey::NETHER => &VANILLA[2],
            LevelKey::END => &VANILLA[3],
            _ => Self::overworld(),
        }
    }

    pub fn overworld() -> &'static Self {
        &VANILLA[0]
    }

    /// Reads a dimension type's JSON; `timeline_tag` expands a timeline tag id to its timelines.
    pub fn from_json(
        id: &str,
        json: &Value,
        timeline_tag: &dyn Fn(&str) -> Vec<String>,
    ) -> Result<Self, String> {
        let height = LevelHeight::of_json(json).ok_or("invalid level height")?;
        let flag = |k: &str, default: bool| json[k].as_bool().unwrap_or(default);
        let name = |k: &str| json[k].as_str();
        let number = |k: &str| json[k].as_f64().ok_or_else(|| format!("{id} needs {k}"));
        let attributes = &json["attributes"];
        let attribute = |k: &str| &attributes[format!("minecraft:gameplay/{k}")];
        let bool_attr = |k: &str, default: bool| attribute(k).as_bool().unwrap_or(default);
        let bed = |k: &str, default: BedRule| match attribute(k) {
            Value::Null => Ok(default),
            v => BedRule::from_json(v),
        };
        let timelines = match &json["timelines"] {
            Value::Null => Vec::new(),
            Value::String(s) => match s.strip_prefix('#') {
                Some(tag) => timeline_tag(tag),
                None => vec![s.clone()],
            },
            Value::Array(items) => items
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect(),
            _ => return Err(format!("{id} has invalid timelines")),
        };
        let default_clock = match &json["default_clock"] {
            Value::String(s) => Some(s.clone()),
            Value::Object(o) => o.get("id").and_then(Value::as_str).map(str::to_owned),
            _ => None,
        };
        Ok(Self {
            id: id.to_owned(),
            has_fixed_time: flag("has_fixed_time", false),
            has_ender_dragon_fight: flag("has_ender_dragon_fight", false),
            coordinate_scale: number("coordinate_scale")?,
            height,
            infiniburn: name("infiniburn")
                .ok_or_else(|| format!("{id} needs infiniburn"))?
                .to_owned(),
            ambient_light: number("ambient_light")? as f32,
            monster_spawn_light_level: IntProvider::from_json(
                &json["monster_spawn_light_level"].to_string(),
            )
            .map_err(|e| format!("{id} monster_spawn_light_level: {e:?}"))?,
            monster_spawn_block_light_limit: json["monster_spawn_block_light_limit"]
                .as_i64()
                .and_then(|n| i32::try_from(n).ok())
                .ok_or_else(|| format!("{id} needs monster_spawn_block_light_limit"))?,
            skybox: match name("skybox").unwrap_or("overworld") {
                "none" => Skybox::None,
                "overworld" => Skybox::Overworld,
                "end" => Skybox::End,
                other => return Err(format!("unknown skybox {other:?}")),
            },
            cardinal_light: match name("cardinal_light").unwrap_or("default") {
                "default" => CardinalLight::Default,
                "nether" => CardinalLight::Nether,
                other => return Err(format!("unknown cardinal light {other:?}")),
            },
            default_clock,
            timelines,
            water_evaporates: bool_attr("water_evaporates", false),
            fast_lava: bool_attr("fast_lava", false),
            respawn_anchor_works: bool_attr("respawn_anchor_works", false),
            nether_portal_spawns_piglin: bool_attr("nether_portal_spawns_piglin", false),
            piglins_zombify: bool_attr("piglins_zombify", true),
            snow_golem_melts: bool_attr("snow_golem_melts", false),
            can_start_raid: bool_attr("can_start_raid", true),
            sky_light_level: attribute("sky_light_level").as_f64().unwrap_or(15.0) as f32,
            bed_rule: bed("bed_rule", BedRule::CAN_SLEEP_WHEN_DARK)?,
            straw_bed_rule: bed("straw_bed_rule", BedRule::DESTROY_ON_LEAVE)?,
        })
    }

    pub fn has_timeline(&self, id: &str) -> bool {
        self.timelines.iter().any(|t| t == id)
    }

    /// `DimensionType.getTeleportationScale`.
    pub fn teleportation_scale(from: &Self, to: &Self) -> f64 {
        from.coordinate_scale / to.coordinate_scale
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::{Path, PathBuf};

    fn resources() -> Option<PathBuf> {
        let root = std::env::var_os("HORIZON_RESOURCES")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.local/horizon-client/26.3")
            });
        root.join("data/minecraft/dimension_type")
            .is_dir()
            .then_some(root)
    }

    fn read(path: &Path) -> Value {
        serde_json::from_str(&std::fs::read_to_string(path).expect("file")).expect("json")
    }

    fn bounds(p: &IntProvider) -> (i32, i32) {
        match p {
            IntProvider::Constant(n) => (*n, *n),
            IntProvider::Uniform { min, max } => (*min, *max),
            other => panic!("unexpected provider {other:?}"),
        }
    }

    #[test]
    fn vanilla_facts_match_the_prepared_files() {
        let Some(root) = resources() else {
            eprintln!("skipping: no prepared data");
            return;
        };
        let tags = root.join("data/minecraft/tags/timeline");
        fn expand(dir: &Path, tag: &str, out: &mut Vec<String>) {
            let name = tag.strip_prefix("minecraft:").unwrap_or(tag);
            let json = read(&dir.join(format!("{name}.json")));
            for value in json["values"].as_array().expect("values") {
                let value = value.as_str().expect("id");
                match value.strip_prefix('#') {
                    Some(inner) => expand(dir, inner, out),
                    None => out.push(value.to_owned()),
                }
            }
        }
        let timeline_tag = |tag: &str| {
            let mut out = Vec::new();
            expand(&tags, tag, &mut out);
            out
        };
        for id in VANILLA_TYPES {
            let file = id.strip_prefix("minecraft:").unwrap();
            let json = read(&root.join(format!("data/minecraft/dimension_type/{file}.json")));
            let parsed = DimensionFacts::from_json(id, &json, &timeline_tag).expect(id);
            let built = DimensionFacts::vanilla(id).expect(id);
            let (mut a, mut b) = (parsed.clone(), built.clone());
            assert_eq!(
                bounds(&a.monster_spawn_light_level),
                bounds(&b.monster_spawn_light_level),
                "{id}"
            );
            a.monster_spawn_light_level = IntProvider::Constant(0);
            b.monster_spawn_light_level = IntProvider::Constant(0);
            assert_eq!(format!("{a:?}"), format!("{b:?}"), "{id}");
        }
    }

    #[test]
    fn built_in_levels_use_their_own_type() {
        let ty = |k| DimensionFacts::of_builtin_level(k).id.as_str();
        assert_eq!(ty(LevelKey::OVERWORLD), "minecraft:overworld");
        assert_eq!(ty(LevelKey::NETHER), "minecraft:the_nether");
        assert_eq!(ty(LevelKey::END), "minecraft:the_end");
        assert_eq!(
            ty(LevelKey::parse("example:sky").unwrap()),
            "minecraft:overworld"
        );
        assert!(DimensionFacts::vanilla("example:sky").is_none());
    }

    #[test]
    fn portals_scale_by_the_coordinate_scales() {
        let (over, nether) = (
            DimensionFacts::of_builtin_level(LevelKey::OVERWORLD),
            DimensionFacts::of_builtin_level(LevelKey::NETHER),
        );
        assert_eq!(DimensionFacts::teleportation_scale(nether, over), 8.0);
        assert_eq!(DimensionFacts::teleportation_scale(over, nether), 0.125);
    }

    #[test]
    fn bed_rules_test_the_light() {
        assert!(BedRuleTest::WhenDark.test(true));
        assert!(!BedRuleTest::WhenDark.test(false));
        assert!(BedRuleTest::Always.test(false));
        assert!(!BedRuleTest::Never.test(true));
    }

    #[test]
    fn timelines_come_from_the_type() {
        assert!(DimensionFacts::overworld().has_timeline("minecraft:day"));
        let end = DimensionFacts::of_builtin_level(LevelKey::END);
        assert!(!end.has_timeline("minecraft:day"));
        assert!(end.has_timeline("minecraft:villager_schedule"));
    }
}
