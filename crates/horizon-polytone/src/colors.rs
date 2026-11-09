//! Custom colours (`assets/<namespace>/polytone/colors.json` or `color.properties`, wiki "Custom
//! Colors"): hardcoded colours and a few switches a pack replaces. JSON objects nest the keys the
//! properties form joins with dots (`{"text": {"xpbar": ...}}` is `text.xpbar`); a value is a
//! number or a hex string.

use std::collections::BTreeMap;
use std::path::Path;

use serde_json::Value as Json;

/// The `environment` fog settings vanilla hardcodes, as the packs set them (vanilla's by default).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Environment {
    /// `powder_snow_fog`: the fog colour inside powder snow (RGB).
    pub powder_snow_fog: Option<u32>,
    /// `blindness_fog_distance`, `darkness_fog_distance`, `boss_fog_distance`: the fog ends of
    /// Blindness (5), Darkness (15) and bosses that add world fog (96).
    pub blindness_fog_distance: f32,
    pub darkness_fog_distance: f32,
    pub boss_fog_distance: f32,
    /// `water_fog_brightening`: how much the underwater fog colour washes out (1 vanilla, 0 none).
    pub water_fog_brightening: f32,
    /// `water_fog_brightening_time`: the ticks under water it takes (vanilla's 600).
    pub water_fog_brightening_time: f32,
}

impl Default for Environment {
    fn default() -> Self {
        Self {
            powder_snow_fog: None,
            blindness_fog_distance: 5.0,
            darkness_fog_distance: 15.0,
            boss_fog_distance: 96.0,
            water_fog_brightening: 1.0,
            water_fog_brightening_time: 600.0,
        }
    }
}

/// A `biome_colors` entry: what replaces one of vanilla's biome colour functions.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BiomeColor {
    /// A flat colour (RGB).
    Flat(u32),
    /// A colormap id (`polytone/colormaps/<path>`).
    Colormap(String),
}

/// Every pack's custom colours, by dotted key.
#[derive(Clone, Debug, Default)]
pub struct PackColors {
    entries: BTreeMap<String, Json>,
    /// [`Self::environment`], read once with the packs: the fog reads it every frame.
    pub env: Environment,
}

impl PackColors {
    /// The colours of every namespace's file under a pack overlay root (`colors.json` over
    /// `color.properties` in the same namespace).
    pub fn read(root: &Path) -> Self {
        let mut colors = Self::default();
        let mut namespaces: Vec<_> = std::fs::read_dir(root.join("assets"))
            .into_iter()
            .flatten()
            .flatten()
            .map(|e| e.path())
            .collect();
        namespaces.sort();
        for namespace in namespaces {
            let polytone = namespace.join("polytone");
            if let Ok(text) = std::fs::read_to_string(polytone.join("color.properties")) {
                colors.add_properties(&text);
            }
            if let Ok(text) = std::fs::read_to_string(polytone.join("colors.json")) {
                match serde_json::from_str::<Json>(&text) {
                    Ok(json) => colors.add_json("", &json),
                    Err(e) => log::warn!("{}: {e}", polytone.join("colors.json").display()),
                }
            }
        }
        colors.env = colors.environment();
        colors
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    fn add_json(&mut self, prefix: &str, json: &Json) {
        match json {
            Json::Object(map) => {
                for (key, value) in map {
                    let key = if prefix.is_empty() {
                        key.clone()
                    } else {
                        format!("{prefix}.{key}")
                    };
                    self.add_json(&key, value);
                }
            }
            value if !prefix.is_empty() => {
                self.entries.insert(prefix.to_owned(), value.clone());
            }
            _ => {}
        }
    }

    fn add_properties(&mut self, text: &str) {
        for line in text.lines() {
            let line = line.trim();
            if line.starts_with('#') || line.starts_with('!') {
                continue;
            }
            if let Some((key, value)) = line.split_once(['=', ':']) {
                self.entries
                    .insert(key.trim().to_owned(), Json::String(value.trim().to_owned()));
            }
        }
    }

    /// A colour, ARGB as written (`0xff00ffff`, `#00ff22`, `ff00ff`, or a decimal number).
    pub fn color(&self, key: &str) -> Option<u32> {
        parse_color(self.entries.get(key)?)
    }

    /// `dye.<name>.<kind>`: a dye's `diffuse` (also plain `dye.<name>`), `text` or `firework` colour.
    pub fn dye(&self, name: &str, kind: &str) -> Option<u32> {
        let plain = || (kind == "diffuse").then(|| self.color(&format!("dye.{name}")))?;
        self.color(&format!("dye.{name}.{kind}")).or_else(plain)
    }

    /// `sheep.<dye name>`: the wool colour of a sheep of that colour, over the dye's.
    pub fn sheep(&self, name: &str) -> Option<u32> {
        self.color(&format!("sheep.{name}"))
    }

    /// `redstone.<power>`: the wire's and its dust particles' colour at that power (0 to 15).
    pub fn redstone(&self, power: u8) -> Option<u32> {
        self.color(&format!("redstone.{power}"))
    }

    /// `effect.<id>` or `effect.<id>.color` (the deprecated `potion.<id>` too): mob effects'
    /// colours by id without vanilla's namespace, as the effects table names them. `water` and
    /// `empty` are left out: they change nothing on the real mod.
    pub fn effects(&self) -> Vec<(String, u32)> {
        let mut out = Vec::new();
        for (key, value) in &self.entries {
            let Some(rest) = key
                .strip_prefix("effect.")
                .or_else(|| key.strip_prefix("potion."))
            else {
                continue;
            };
            let id = rest.strip_suffix(".color").unwrap_or(rest);
            if id.ends_with(".particle") || matches!(id, "water" | "empty" | "minecraft:water") {
                continue;
            }
            if let Some(color) = parse_color(value) {
                let id = id.strip_prefix("minecraft:").unwrap_or(id);
                // `effect.<id>.color` wins over the plain key whichever comes first.
                out.retain(|(other, _)| other != id || !key.ends_with(".color"));
                if !out.iter().any(|(other, _)| other == id) {
                    out.push((id.to_owned(), color));
                }
            }
        }
        out
    }

    /// `particle.<id>`: a colour or an expression each new particle of that type takes, by
    /// namespaced type id.
    pub fn particles(&self) -> Vec<(String, String)> {
        self.entries
            .iter()
            .filter_map(|(key, value)| {
                let id = key.strip_prefix("particle.")?;
                let id = if id.contains(':') {
                    id.to_owned()
                } else {
                    format!("minecraft:{id}")
                };
                let value = match value {
                    Json::String(s) => s.trim().to_owned(),
                    Json::Number(n) => n.to_string(),
                    _ => return None,
                };
                Some((id, value))
            })
            .collect()
    }

    /// `environment.*`: the fog settings a pack replaces.
    pub fn environment(&self) -> Environment {
        let number = |key: &str, default: f32| {
            match self.entries.get(&format!("environment.{key}")) {
                Some(Json::Number(n)) => n.as_f64().map(|v| v as f32),
                Some(Json::String(s)) => s.trim().parse().ok(),
                _ => None,
            }
            .unwrap_or(default)
        };
        let vanilla = Environment::default();
        Environment {
            powder_snow_fog: self
                .color("environment.powder_snow_fog")
                .map(|c| c & 0xFF_FFFF),
            blindness_fog_distance: number(
                "blindness_fog_distance",
                vanilla.blindness_fog_distance,
            ),
            darkness_fog_distance: number("darkness_fog_distance", vanilla.darkness_fog_distance),
            boss_fog_distance: number("boss_fog_distance", vanilla.boss_fog_distance),
            water_fog_brightening: number("water_fog_brightening", vanilla.water_fog_brightening),
            water_fog_brightening_time: number(
                "water_fog_brightening_time",
                vanilla.water_fog_brightening_time,
            )
            .max(1.0),
        }
    }

    /// `biome_colors.<function>` (`grass`, `foliage`, `dry_foliage`, `water`): a colormap id or
    /// a flat colour. Inline colormaps aren't read.
    pub fn biome_color(&self, function: &str) -> Option<BiomeColor> {
        let value = self.entries.get(&format!("biome_colors.{function}"))?;
        if let Json::String(s) = value {
            let s = s.trim();
            if s.contains(':') {
                return Some(BiomeColor::Colormap(s.to_owned()));
            }
            if parse_color(value).is_none() {
                return Some(BiomeColor::Colormap(format!("minecraft:{s}")));
            }
        }
        parse_color(value).map(|c| BiomeColor::Flat(c & 0xFF_FFFF))
    }

    /// `entity_shadows`: false hides every entity's round shadow, whatever the video option says.
    pub fn entity_shadows(&self) -> bool {
        match self.entries.get("entity_shadows") {
            Some(Json::Bool(on)) => *on,
            Some(Json::String(s)) => s.trim() != "false",
            _ => true,
        }
    }

    /// `text.xpbar`: the experience level number's colour, or its `foreground` and `background`
    /// (the outline); `None` where the pack leaves vanilla's.
    pub fn xp_bar(&self) -> [Option<u32>; 2] {
        match self.color("text.xpbar") {
            Some(color) => [Some(color), None],
            None => [
                self.color("text.xpbar.foreground"),
                self.color("text.xpbar.background"),
            ],
        }
    }
}

fn parse_color(value: &Json) -> Option<u32> {
    match value {
        Json::Number(n) => n.as_i64().map(|v| v as u32),
        Json::String(s) => {
            let s = s.trim();
            let hex = s
                .strip_prefix("0x")
                .or_else(|| s.strip_prefix("0X"))
                .or_else(|| s.strip_prefix('#'));
            match hex {
                Some(hex) => u32::from_str_radix(hex, 16).ok(),
                None => s
                    .parse::<i64>()
                    .ok()
                    .filter(|_| !s.chars().any(|c| c.is_ascii_alphabetic()))
                    .map(|v| v as u32)
                    .or_else(|| u32::from_str_radix(s, 16).ok()),
            }
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn json_and_properties_name_the_same_keys() {
        let mut c = PackColors::default();
        c.add_json(
            "",
            &json!({"entity_shadows": false, "text": {"xpbar": "ff00ff"}, "map": {"grass": 7}}),
        );
        assert!(!c.entity_shadows());
        assert_eq!(c.xp_bar(), [Some(0xff00ff), None]);
        assert_eq!(c.color("map.grass"), Some(7));
        let mut p = PackColors::default();
        p.add_properties(
            "# a comment\ntext.xpbar.foreground = 0x00ff22\ntext.xpbar.background=-1\n",
        );
        assert!(p.entity_shadows());
        assert_eq!(p.xp_bar(), [Some(0x00ff22), Some(u32::MAX)]);
    }

    #[test]
    fn effects_particles_and_redstone_by_id() {
        let mut c = PackColors::default();
        c.add_json(
            "",
            &json!({
                "effect": {"minecraft:speed": {"color": "ff0000", "particle": "minecraft:poof"}, "water": "00ff00", "poison": 7},
                "particle": {"minecraft:flame": "0x0000ff", "poof": 0},
                "redstone": {"15": "00ff00"}
            }),
        );
        let mut effects = c.effects();
        effects.sort();
        assert_eq!(
            effects,
            [("poison".to_owned(), 7), ("speed".to_owned(), 0xff0000)]
        );
        let mut particles = c.particles();
        particles.sort();
        assert_eq!(
            particles,
            [
                ("minecraft:flame".to_owned(), "0x0000ff".to_owned()),
                ("minecraft:poof".to_owned(), "0".to_owned())
            ]
        );
        assert_eq!((c.redstone(15), c.redstone(0)), (Some(0x00ff00), None));
    }

    #[test]
    fn environment_keeps_vanillas_where_unset() {
        let mut c = PackColors::default();
        assert_eq!(c.environment(), Environment::default());
        c.add_json(
            "",
            &json!({"environment": {"powder_snow_fog": "#c0c0d0", "blindness_fog_distance": 40, "water_fog_brightening_time": "200"}}),
        );
        let e = c.environment();
        assert_eq!(e.powder_snow_fog, Some(0xc0c0d0));
        assert_eq!(
            (e.blindness_fog_distance, e.darkness_fog_distance),
            (40.0, 15.0)
        );
        assert_eq!(
            (e.water_fog_brightening, e.water_fog_brightening_time),
            (1.0, 200.0)
        );
    }

    #[test]
    fn biome_colours_are_colormaps_or_flat() {
        let mut c = PackColors::default();
        c.add_json(
            "",
            &json!({"biome_colors": {"grass": "my_pack:grass", "foliage": "0x0000ff", "water": "lake"}}),
        );
        assert_eq!(
            c.biome_color("grass"),
            Some(BiomeColor::Colormap("my_pack:grass".into()))
        );
        assert_eq!(c.biome_color("foliage"), Some(BiomeColor::Flat(0x0000ff)));
        assert_eq!(
            c.biome_color("water"),
            Some(BiomeColor::Colormap("minecraft:lake".into()))
        );
        assert_eq!(c.biome_color("dry_foliage"), None);
    }

    #[test]
    fn dyes_name_their_colour_kind() {
        let mut c = PackColors::default();
        c.add_json(
            "",
            &json!({"dye": {"red": {"diffuse": "00ff00", "text": "0000ff"}, "blue": "ff0000"}, "sheep": {"white": 5}}),
        );
        assert_eq!(c.dye("red", "diffuse"), Some(0x00ff00));
        assert_eq!(c.dye("red", "text"), Some(0x0000ff));
        assert_eq!(c.dye("red", "firework"), None);
        assert_eq!(c.dye("blue", "diffuse"), Some(0xff0000));
        assert_eq!(c.dye("blue", "text"), None);
        assert_eq!(c.sheep("white"), Some(5));
        assert_eq!(c.sheep("red"), None);
    }

    #[test]
    fn colours_read_hex_and_decimal() {
        let color = |v: Json| parse_color(&v);
        assert_eq!(color(json!("0xff00ffff")), Some(0xff00ffff));
        assert_eq!(color(json!("#c0c0d0")), Some(0xc0c0d0));
        assert_eq!(color(json!("62803d")), Some(0x62803d));
        assert_eq!(color(json!("4259712")), Some(4_259_712));
        assert_eq!(color(json!(23223)), Some(23223));
        assert_eq!(color(json!("no")), None);
    }
}
