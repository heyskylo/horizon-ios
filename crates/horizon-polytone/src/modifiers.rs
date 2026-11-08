//! Biome modifiers (`polytone/biome_modifiers/`) and dimension modifiers
//! (`polytone/dimension_modifiers/`): their targets, priority and `attributes_modifiers` groups.
use crate::attributes::{Attributes, parse_attributes};
use crate::target::Targets;
use serde_json::Value;

/// The fields every modifier shares. `None` when the file is switched off (`polytone_ignore`).
fn common(json: &Value, implicit: Option<&str>) -> Result<Option<(Targets, i32)>, String> {
    if json.get("polytone_ignore").and_then(Value::as_bool) == Some(true) {
        return Ok(None);
    }
    let targets = Targets::parse(json.get("targets"), implicit)?;
    let priority = json.get("priority").and_then(Value::as_i64).unwrap_or(0);
    let priority = i32::try_from(priority).map_err(|_| "priority out of range")?;
    Ok(Some((targets, priority)))
}

/// `attributes_modifiers` split into its groups. Without any group key, the whole object is the
/// `base` group.
fn groups<const N: usize>(json: &Value, names: [&str; N]) -> Result<[Attributes; N], String> {
    let mut out = std::array::from_fn(|_| Attributes::new());
    let Some(field) = json.get("attributes_modifiers") else {
        return Ok(out);
    };
    let object = field
        .as_object()
        .ok_or("attributes_modifiers is not an object")?;
    if !object.keys().any(|k| names.contains(&k.as_str())) {
        out[0] = parse_attributes(field)?;
        return Ok(out);
    }
    // A key beside the groups that is none of them is left alone, so a pack Polytone loads
    // isn't refused over an entry it doesn't read either.
    for (key, value) in object {
        let Some(i) = names.iter().position(|n| n == key) else {
            continue;
        };
        out[i] = parse_attributes(value).map_err(|e| format!("{key}: {e}"))?;
    }
    Ok(out)
}

/// A biome modifier's attribute layers: `base` applies in the biome group, before the dimension
/// timeline and the weather; `post_process` after them.
#[derive(Clone, Debug)]
pub struct BiomeModifier {
    pub targets: Targets,
    /// Higher wins where several modifiers apply.
    pub priority: i32,
    pub base: Attributes,
    pub post_process: Attributes,
}

impl BiomeModifier {
    /// `implicit` is the id the file's path names ([`crate::implicit_id`]).
    pub fn parse(json: &Value, implicit: Option<&str>) -> Result<Option<Self>, String> {
        let Some((targets, priority)) = common(json, implicit)? else {
            return Ok(None);
        };
        let [base, post_process] = groups(json, ["base", "post_process"])?;
        Ok(Some(Self {
            targets,
            priority,
            base,
            post_process,
        }))
    }
}

/// A dimension modifier's attribute layers: `base` in the dimension group (first of all), `rain`
/// and `thunder` in the weather groups, `post_process` last.
#[derive(Clone, Debug)]
pub struct DimensionModifier {
    pub targets: Targets,
    pub priority: i32,
    pub base: Attributes,
    pub rain: Attributes,
    pub thunder: Attributes,
    pub post_process: Attributes,
}

impl DimensionModifier {
    pub fn parse(json: &Value, implicit: Option<&str>) -> Result<Option<Self>, String> {
        let Some((targets, priority)) = common(json, implicit)? else {
            return Ok(None);
        };
        let [base, rain, thunder, post_process] =
            groups(json, ["base", "rain", "thunder", "post_process"])?;
        Ok(Some(Self {
            targets,
            priority,
            base,
            rain,
            thunder,
            post_process,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::attributes::{Argument, Layer, Modifier};
    use serde_json::json;

    const FOG: &str = "minecraft:visual/fog_color";

    #[test]
    fn a_flat_biome_modifier_fills_the_base_group() {
        let json = json!({"attributes_modifiers": {"visual/fog_color": "#102030"}});
        let m = BiomeModifier::parse(&json, Some("minecraft:forest"))
            .unwrap()
            .unwrap();
        assert!(m.targets.matches("minecraft:forest", |_| false));
        assert_eq!(m.priority, 0);
        assert!(m.post_process.is_empty());
        assert!(matches!(
            m.base[FOG],
            Layer::Apply {
                modifier: Modifier::Override,
                argument: Argument::Color(0xff10_2030),
                ..
            }
        ));
    }

    #[test]
    fn a_grouped_biome_modifier_keeps_its_groups_apart() {
        let json = json!({
            "targets": ["minecraft:swamp", "minecraft:mangrove_swamp"],
            "priority": -2,
            "attributes_modifiers": {
                "base": {"minecraft:visual/sky_color": "my_pack:sky"},
                "post_process": {"visual/sunrise_sunset_color": "REMOVE"}
            }
        });
        let m = BiomeModifier::parse(&json, None).unwrap().unwrap();
        assert_eq!(m.priority, -2);
        assert!(m.targets.matches("minecraft:mangrove_swamp", |_| false));
        assert_eq!(m.base.len(), 1);
        assert_eq!(
            m.post_process["minecraft:visual/sunrise_sunset_color"],
            Layer::Remove
        );
    }

    #[test]
    fn a_key_beside_the_groups_that_is_none_of_them_is_left_alone() {
        let json = json!({
            "attributes_modifiers": {
                "base": {"visual/sky_color": "#FF0000"},
                "comment": "the groups' neighbour"
            }
        });
        let m = BiomeModifier::parse(&json, Some("minecraft:plains"))
            .unwrap()
            .unwrap();
        assert_eq!(m.base.len(), 1);
        assert!(m.post_process.is_empty());
    }

    #[test]
    fn dimension_modifiers_read_the_weather_groups() {
        let json = json!({
            "targets": "minecraft:overworld",
            "attributes_modifiers": {
                "base": {"visual/fog_start_distance": "REMOVE"},
                "rain": {"visual/cloud_color": "REMOVE", "visual/fog_color": "#3f3f4c"},
                "thunder": {"visual/sky_color": {"argument": 0.5, "modifier": "multiply"}}
            }
        });
        let m = DimensionModifier::parse(&json, None).unwrap().unwrap();
        assert_eq!(m.base.len(), 1);
        assert_eq!(m.rain.len(), 2);
        assert!(matches!(
            m.thunder["minecraft:visual/sky_color"],
            Layer::Apply {
                modifier: Modifier::Multiply,
                argument: Argument::Number(0.5),
                ..
            }
        ));
        assert!(m.post_process.is_empty());
        // Without groups, a dimension modifier's attributes are its base group too.
        let flat = json!({"targets": "minecraft:overworld", "attributes_modifiers": {"visual/sky_color": -1}});
        let flat = DimensionModifier::parse(&flat, None).unwrap().unwrap();
        assert_eq!(flat.base.len(), 1);
        assert!(flat.rain.is_empty());
    }

    #[test]
    fn ignored_files_load_nothing_and_broken_layers_fail() {
        let ignored = json!({"polytone_ignore": true, "attributes_modifiers": {"x": "REMOVE"}});
        assert!(
            BiomeModifier::parse(&ignored, Some("minecraft:forest"))
                .unwrap()
                .is_none()
        );
        // `rain` is a dimension group, not a biome one: a biome modifier leaves it alone.
        let wrong_group = json!({"attributes_modifiers": {"base": {}, "rain": {"x": "REMOVE"}}});
        let m = BiomeModifier::parse(&wrong_group, Some("minecraft:forest"))
            .unwrap()
            .unwrap();
        assert!(m.base.is_empty() && m.post_process.is_empty());
        let bad_layer = json!({"attributes_modifiers": {"visual/fog_color": null}});
        assert!(DimensionModifier::parse(&bad_layer, Some("minecraft:overworld")).is_err());
    }
}
