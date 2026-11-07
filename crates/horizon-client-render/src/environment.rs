//! A dimension's environment attributes (`EnvironmentAttributeSystem.addDefaultLayers`): the
//! dimension type's attributes, the biome layer, the dimension's timelines and, where it can
//! have weather, the rain and thunder layers, sampled into the sky state the renderer draws.
use crate::Result;
use horizon_world::environment_attributes::{AttributeMap, Value, attribute};
use horizon_world::environment_system::{BiomeBlend, Builder, Conditions, System};
use horizon_world::environment_timeline::Timeline;
use serde_json::Value as Json;
use std::path::Path;

pub struct Environment {
    system: System,
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
        let json: Json = horizon_world::json_file::read(
            &root.join(format!("data/minecraft/dimension_type/{dimension}.json")),
        )?;
        let mut builder = Builder::default();
        builder.constant_layer(&AttributeMap::parse(&json["attributes"])?);
        // Without biome data (a bare dimension type), there is no biome layer.
        let mut biomes = Vec::new();
        if let Ok(folder) =
            horizon_world::json_folder::read(&root.join("data/minecraft/worldgen/biome"))
        {
            for file in folder.files.iter().filter(|f| !f.name.contains('/')) {
                let biome: Json = horizon_world::json_file::parse(&file.path, file.bytes()?)?;
                match AttributeMap::parse(&biome["attributes"]) {
                    Ok(map) => biomes.push(map),
                    Err(e) => log::warn!("biome {}: {e}", file.name),
                }
            }
        }
        builder.biome_layer(&biomes);
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
            builder.weather_layers();
        }
        Ok(Self {
            system: builder.build(),
        })
    }

    /// `EnvironmentAttributeSystem.getValue` of one attribute, or its dimension value without
    /// a biome.
    pub fn value(&self, id: &str, at: &Sample) -> Option<Value> {
        let attribute = attribute(id)?;
        let biome = |_full_resolution: bool| at.biome;
        let clock = |_clock: &str| at.day_time;
        let conditions = Conditions {
            rain: at.rain,
            thunder: at.thunder,
            blend: at.blend,
            biome: &biome,
            clock: &clock,
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
}
