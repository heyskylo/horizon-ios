//! Environment attribute layers (wiki "Environment Attributes"): a map from attribute id to a
//! value, which sets the attribute, a `{argument, modifier}` object, which modifies the value the
//! earlier layers gave, or `REMOVE`, which drops the game's own layer for it.
use serde_json::{Map, Value};
use std::collections::BTreeMap;

/// The vanilla `modifier` operations.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Modifier {
    Override,
    Add,
    Subtract,
    Multiply,
    Minimum,
    Maximum,
    AlphaBlend,
    BlendToGray,
    And,
    Nand,
    Or,
    Nor,
    Xor,
    Xnor,
}

impl Modifier {
    /// The `modifier` field, in any case (the wiki writes both `add` and `ADD`).
    fn parse(s: &str) -> Option<Self> {
        Some(match s.to_ascii_lowercase().as_str() {
            "override" => Self::Override,
            "add" => Self::Add,
            "subtract" => Self::Subtract,
            "multiply" => Self::Multiply,
            "minimum" => Self::Minimum,
            "maximum" => Self::Maximum,
            "alpha_blend" => Self::AlphaBlend,
            "blend_to_gray" => Self::BlendToGray,
            "and" => Self::And,
            "nand" => Self::Nand,
            "or" => Self::Or,
            "nor" => Self::Nor,
            "xor" => Self::Xor,
            "xnor" => Self::Xnor,
            _ => return None,
        })
    }
}

/// A layer's value or `argument`.
#[derive(Clone, Debug, PartialEq)]
pub enum Argument {
    /// A number: a float, an angle, or a packed color, as the attribute's type reads it.
    Number(f64),
    Bool(bool),
    /// A `#rrggbb` / `#aarrggbb` string (the `#` optional), packed ARGB; opaque without alpha.
    Color(u32),
    /// A `namespace:path` string: a colormap sampled for the value.
    Colormap(String),
    /// Any other string: a scripting expression, kept as source.
    Expression(String),
    /// Vanilla's structured arguments, such as `alpha_blend`'s `{value, alpha}`.
    Object(Map<String, Value>),
}

impl Argument {
    fn parse(v: &Value) -> Result<Self, String> {
        Ok(match v {
            Value::Number(n) => Self::Number(n.as_f64().unwrap_or_default()),
            Value::Bool(b) => Self::Bool(*b),
            Value::Object(o) => Self::Object(o.clone()),
            Value::String(s) => {
                let s = s.trim();
                let hex = s.strip_prefix('#').unwrap_or(s);
                let color =
                    matches!(hex.len(), 6 | 8) && hex.bytes().all(|b| b.is_ascii_hexdigit());
                if let (true, Ok(packed)) = (color, u32::from_str_radix(hex, 16)) {
                    Self::Color(if hex.len() == 6 {
                        0xff00_0000 | packed
                    } else {
                        packed
                    })
                } else if s.contains(':') && crate::target::parse_id(s).is_some() {
                    Self::Colormap(s.to_owned())
                } else {
                    Self::Expression(s.to_owned())
                }
            }
            _ => return Err(format!("{v} is no attribute value")),
        })
    }
}

/// What one modifier file does to one attribute.
#[derive(Clone, Debug, PartialEq)]
pub enum Layer {
    /// `REMOVE`: the game's own layer for the attribute in this group is dropped.
    Remove,
    /// A value (`Override`) or a `{argument, modifier}` object. `biome_blend` and `time_blend`
    /// (default `true`) say whether a computed value is mixed across biome borders and faded
    /// over time when it jumps.
    Apply {
        modifier: Modifier,
        argument: Argument,
        biome_blend: bool,
        time_blend: bool,
    },
}

impl Layer {
    fn parse(v: &Value) -> Result<Self, String> {
        if v.as_str().is_some_and(|s| s.trim() == "REMOVE") {
            return Ok(Self::Remove);
        }
        let object = v.as_object().filter(|o| o.contains_key("modifier"));
        let Some(o) = object else {
            return Ok(Self::Apply {
                modifier: Modifier::Override,
                argument: Argument::parse(v)?,
                biome_blend: true,
                time_blend: true,
            });
        };
        let name = o["modifier"].as_str().ok_or("modifier is not a string")?;
        let flag = |key: &str| o.get(key).and_then(Value::as_bool).unwrap_or(true);
        Ok(Self::Apply {
            modifier: Modifier::parse(name).ok_or_else(|| format!("unknown modifier {name:?}"))?,
            argument: Argument::parse(o.get("argument").ok_or("modifier without an argument")?)?,
            biome_blend: flag("biome_blend"),
            time_blend: flag("time_blend"),
        })
    }
}

/// `visual/fog_color` and `minecraft:visual/fog_color` are the same attribute.
pub fn normalize_attribute(id: &str) -> String {
    if id.contains(':') {
        id.to_owned()
    } else {
        format!("minecraft:{id}")
    }
}

/// One group's layers, by attribute id (with its namespace).
pub type Attributes = BTreeMap<String, Layer>;

pub(crate) fn parse_attributes(v: &Value) -> Result<Attributes, String> {
    let map = v.as_object().ok_or("attributes are not an object")?;
    map.iter()
        .map(|(id, layer)| {
            let layer = Layer::parse(layer).map_err(|e| format!("{id}: {e}"))?;
            Ok((normalize_attribute(id), layer))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn layer(v: Value) -> Layer {
        Layer::parse(&v).unwrap()
    }
    fn set(argument: Argument) -> Layer {
        Layer::Apply {
            modifier: Modifier::Override,
            argument,
            biome_blend: true,
            time_blend: true,
        }
    }

    #[test]
    fn plain_values_set_the_attribute() {
        assert_eq!(layer(json!(192.5)), set(Argument::Number(192.5)));
        assert_eq!(layer(json!(false)), set(Argument::Bool(false)));
        assert_eq!(layer(json!("REMOVE")), Layer::Remove);
    }

    #[test]
    fn color_strings_pack_to_argb_with_or_without_the_hash() {
        assert_eq!(layer(json!("4cb1a8")), set(Argument::Color(0xff4c_b1a8)));
        assert_eq!(layer(json!("#332211")), set(Argument::Color(0xff33_2211)));
        assert_eq!(layer(json!("#867a7aff")), set(Argument::Color(0x867a_7aff)));
        assert_eq!(layer(json!("#00000000")), set(Argument::Color(0)));
    }

    #[test]
    fn ids_name_colormaps_and_other_strings_are_expressions() {
        let colormap = Argument::Colormap("my_pack:swamp_sky_color".into());
        assert_eq!(layer(json!("my_pack:swamp_sky_color")), set(colormap));
        let expression = Argument::Expression("r.rand() + 0.1".into());
        assert_eq!(layer(json!("r.rand() + 0.1")), set(expression));
        // Not hex, so no color, though six letters long.
        let word = Argument::Expression("g.rain".into());
        assert_eq!(layer(json!("g.rain")), set(word));
    }

    #[test]
    fn modifier_objects_read_any_case_and_the_blend_flags() {
        let l = layer(json!({"argument": "0.2 * g.rain", "modifier": "ADD", "biome_blend": false}));
        assert_eq!(
            l,
            Layer::Apply {
                modifier: Modifier::Add,
                argument: Argument::Expression("0.2 * g.rain".into()),
                biome_blend: false,
                time_blend: true,
            }
        );
        let blend =
            layer(json!({"argument": {"value": 4.0, "alpha": 0.5}, "modifier": "alpha_blend"}));
        assert!(matches!(
            blend,
            Layer::Apply {
                modifier: Modifier::AlphaBlend,
                argument: Argument::Object(_),
                ..
            }
        ));
    }

    #[test]
    fn unknown_modifiers_and_missing_arguments_are_errors() {
        assert!(Layer::parse(&json!({"argument": 1, "modifier": "divide"})).is_err());
        assert!(Layer::parse(&json!({"modifier": "add"})).is_err());
        assert!(Layer::parse(&json!(null)).is_err());
    }

    #[test]
    fn attribute_ids_get_the_minecraft_namespace() {
        let a =
            parse_attributes(&json!({"visual/fog_end_distance": 64, "mod:visual/x": 1})).unwrap();
        let ids: Vec<&str> = a.keys().map(String::as_str).collect();
        assert_eq!(ids, ["minecraft:visual/fog_end_distance", "mod:visual/x"]);
    }
}
