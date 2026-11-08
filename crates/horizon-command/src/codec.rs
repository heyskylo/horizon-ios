//! ResourceOrIdArgument and the data-fixer-upper decoding of inline values
//! commands accept (loot predicates, item modifiers, slot sources, context
//! providers, dialogs), reproducing the codec error messages vanilla reports.
use super::args::{Registries, is_id_char, parse_identifier, slot_ranges};
use super::{CmdResult, CommandError, Reader};
use horizon_world::nbt::{Compound, JavaString, Tag};

pub fn snbt(t: &Tag) -> String {
    horizon_client_core::snbt::to_snbt(t)
}
pub fn not_map(t: &Tag) -> String {
    format!("Not a map: {}", snbt(t))
}
pub fn not_list(t: &Tag) -> String {
    format!("Not a list: {}", snbt(t))
}
pub fn no_key(key: &str, t: &Tag) -> String {
    format!("No key {key} in MapLike[{}]", snbt(t))
}
pub fn either(a: String, b: String) -> String {
    format!("Failed to parse either. First: {a}; Second: {b}")
}
pub fn get<'t>(map: &'t Compound, key: &str) -> Option<&'t Tag> {
    map.get(&JavaString::from(key))
}
pub fn is_number(t: &Tag) -> bool {
    matches!(
        t,
        Tag::Byte(_) | Tag::Short(_) | Tag::Int(_) | Tag::Long(_) | Tag::Float(_) | Tag::Double(_)
    )
}
pub fn as_string(t: &Tag) -> Option<String> {
    match t {
        Tag::String(s) => Some(s.to_string_lossy()),
        _ => None,
    }
}

/// KeyDispatchCodec over a type registry: the map, its `type` and the
/// type's own fields (`fields` checks them).
fn dispatch(
    t: &Tag,
    reg: &Registries,
    type_registry: &str,
    fields: &dyn Fn(&str, &Compound, &Tag) -> Result<(), String>,
) -> Result<(), String> {
    let Tag::Compound(map) = t else {
        return Err(not_map(t));
    };
    let Some(ty) = get(map, "type") else {
        return Err(no_key("type", t));
    };
    let Some(name) = as_string(ty) else {
        return Err("Not a string".into());
    };
    let Some(id) = parse_identifier(&name) else {
        return Err(format!("Not a valid resource location: {name}"));
    };
    let known = reg.ids(type_registry);
    if !known.is_empty() && !known.contains(&id) {
        return Err(format!(
            "Unknown registry key in ResourceKey[minecraft:root / {type_registry}]: {id}"
        ));
    }
    fields(&id, map, t)
}
/// Required fields, reported in vanilla's order when missing.
fn require(map: &Compound, t: &Tag, keys: &[&str]) -> Result<(), String> {
    let missing: Vec<String> = keys
        .iter()
        .filter(|k| get(map, k).is_none())
        .map(|k| no_key(k, t))
        .collect();
    if missing.is_empty() {
        Ok(())
    } else {
        Err(missing.join("; "))
    }
}
/// The required fields of each context number provider type, in the
/// order DataFixerUpper reports them missing (the reverse of the record's).
fn provider_fields(id: &str, map: &Compound, t: &Tag) -> Result<(), String> {
    match id.trim_start_matches("minecraft:") {
        "uniform" => require(map, t, &["max", "min"]),
        "constant" => require(map, t, &["value"]),
        "binomial" => require(map, t, &["p", "n"]),
        "add" | "mul" | "avg" | "max" | "min" | "length" => require(map, t, &["inputs"]),
        "sub" | "div" | "mod" | "floor_div" | "floor_mod" => require(map, t, &["right", "left"]),
        "pow" => require(map, t, &["exponent", "base"]),
        "abs" | "negate" | "from_float" | "from_int" | "floor" | "ceil" | "round" | "sqrt"
        | "sin" | "cos" | "truncate" => require(map, t, &["input"]),
        "conditional" => require(map, t, &["on_true", "condition"]),
        "number_dispatcher" => require(map, t, &["cases"]),
        "weighted_list" => require(map, t, &["distribution"]),
        "enchantment_level" => require(map, t, &["amount"]),
        "score" => require(map, t, &["score", "target"]),
        _ => Ok(()),
    }
}
/// A holder list codec: a single element, a `#tag`, or a list.
fn element_or_list(t: &Tag, element: &dyn Fn(&Tag) -> Result<(), String>) -> Result<(), String> {
    let first = match element(t) {
        Ok(()) => return Ok(()),
        Err(e) => e,
    };
    let tag_ref = match as_string(t) {
        Some(s) if s.starts_with('#') => return Ok(()),
        Some(_) => "Not a tag id".to_owned(),
        None => "Not a string".to_owned(),
    };
    let list = match t {
        Tag::List(items) => {
            let errors: Vec<String> = items.iter().filter_map(|i| element(i).err()).collect();
            if errors.is_empty() {
                return Ok(());
            }
            errors.join("; ")
        }
        _ => not_list(t),
    };
    let last = element(t).err().unwrap_or_default();
    Err(either(first, either(tag_ref, either(list, last))))
}

/// CompositeLootItemCondition.createCodec / InvertedLootItemCondition:
/// required inline combinator children must decode before command execution.
fn predicate_fields(id: &str, map: &Compound, t: &Tag, reg: &Registries) -> Result<(), String> {
    match id.trim_start_matches("minecraft:") {
        "all_of" | "any_of" => {
            require(map, t, &["terms"])?;
            let terms = get(map, "terms").expect("required field");
            let Tag::List(terms) = terms else {
                return Err(not_list(terms));
            };
            for term in terms {
                decode("minecraft:predicate", term, reg)?;
            }
            Ok(())
        }
        "inverted" => {
            require(map, t, &["term"])?;
            decode(
                "minecraft:predicate",
                get(map, "term").expect("required field"),
                reg,
            )
        }
        // Other predicate families retain their existing codec/evaluator paths.
        _ => Ok(()),
    }
}

/// Decode an inline value of a ResourceOrIdArgument registry.
pub fn decode(registry: &str, t: &Tag, reg: &Registries) -> Result<(), String> {
    match registry {
        "minecraft:context_int_provider" | "minecraft:context_float_provider" => {
            if is_number(t) {
                return Ok(());
            }
            let ty = format!("{registry}_type");
            let second = dispatch(t, reg, &ty, &provider_fields);
            second.map_err(|e| either("Not a number".into(), e))
        }
        "minecraft:predicate" => {
            dispatch(t, reg, "minecraft:loot_condition_type", &|id, map, t| {
                predicate_fields(id, map, t, reg)
            })
        }
        "minecraft:item_modifier" => element_or_list(t, &|x| {
            dispatch(x, reg, "minecraft:loot_function_type", &|_, _, _| Ok(()))
        }),
        "minecraft:slot_source" => element_or_list(t, &|x| {
            dispatch(x, reg, "minecraft:slot_source_type", &|id, map, t| {
                if id == "minecraft:contents" {
                    require(map, t, &["component"])?;
                    let value = get(map, "component").expect("required component");
                    let name = as_string(value).ok_or_else(|| "Not a string".to_owned())?;
                    let component = parse_identifier(&name)
                        .ok_or_else(|| format!("Not a valid resource location: {name}"))?;
                    let known = reg.ids("minecraft:data_component_type");
                    if !known.is_empty() && !known.contains(&component) {
                        return Err(format!(
                            "Unknown registry key in ResourceKey[minecraft:root / minecraft:data_component_type]: {component}"
                        ));
                    }
                    if !matches!(
                        component.as_str(),
                        "minecraft:container"
                            | "minecraft:bundle_contents"
                            | "minecraft:charged_projectiles"
                    ) {
                        return Err("No items in component".into());
                    }
                } else if id == "minecraft:slot_range" {
                    // RangeSlotSource.MAP_CODEC uses the global SlotRanges.CODEC,
                    // independently of the eventual target inventory's size.
                    require(map, t, &["slots"])?;
                    let value = get(map, "slots").expect("required slots");
                    let name = as_string(value).ok_or_else(|| "Not a string".to_owned())?;
                    if !slot_ranges().iter().any(|(known, _)| known == &name) {
                        return Err(format!("Unknown element name:{name}"));
                    }
                }
                Ok(())
            })
        }),
        // Dialog types: CommonDialogData's title, then each type's own.
        "minecraft:dialog" => dispatch(t, reg, "minecraft:dialog_type", &|id, map, t| {
            let own: &[&str] = match id {
                "minecraft:confirmation" => &["yes", "no"],
                "minecraft:multi_action" => &["actions"],
                "minecraft:dialog_list" => &["dialogs"],
                _ => &[],
            };
            let keys: Vec<&str> = std::iter::once("title")
                .chain(own.iter().copied())
                .collect();
            require(map, t, &keys)
        }),
        // The ConfiguredFeature codec, where the source has the live features.
        "minecraft:worldgen/feature" => match t {
            Tag::Compound(_) => match &reg.inline_feature {
                Some(codec) => codec.check(t),
                None => Ok(()),
            },
            _ => Err(not_map(t)),
        },
        "minecraft:loot_table" => match t {
            Tag::Compound(_) => Ok(()),
            _ => Err(not_map(t)),
        },
        _ => Ok(()),
    }
}

/// Identifier.readNonEmpty (None when empty or invalid).
fn read_non_empty(reader: &mut Reader) -> Option<String> {
    let start = reader.cursor;
    while reader.can_read() && is_id_char(reader.peek()) {
        reader.skip();
    }
    let raw = &reader.string[start..reader.cursor];
    let id = if raw.is_empty() {
        None
    } else {
        parse_identifier(raw)
    };
    if id.is_none() {
        reader.cursor = start;
    }
    id
}

/// ResourceOrIdArgument.parse: a registry id (checked) or an inline SNBT
/// value decoded with the registry's codec.
pub fn resource_or_id(reader: &mut Reader, reg: &Registries, registry: &str) -> CmdResult<()> {
    let start = reader.cursor;
    let mut r = reader.clone();
    r.skip_whitespace();
    if let Some(id) = read_non_empty(&mut r) {
        reader.cursor = r.cursor;
        if !reg.ids(registry).contains(&id) {
            return Err(CommandError::tr_at(
                "argument.resource_or_id.no_such_element",
                &[&id, &registry],
                reader,
            ));
        }
        return Ok(());
    }
    reader.cursor = start;
    let tag = super::snbt::parse_tag_argument(reader)?;
    decode(registry, &tag, reg)
        .map_err(|m| CommandError::tr_at("argument.resource_or_id.failed_to_parse", &[&m], reader))
}

#[cfg(test)]
mod predicate_tests {
    use super::*;

    #[test]
    fn inline_constant_combinators_require_decodable_children() {
        let reg = Registries::default();
        let decode_text = |text| {
            let (tag, _) = horizon_world::nbt::parse_snbt(text).unwrap();
            decode("minecraft:predicate", &tag, &reg)
        };
        assert!(decode_text(r#"{type:"minecraft:all_of",terms:[{type:"minecraft:inverted",term:{type:"minecraft:any_of",terms:[]}}]}"#).is_ok());
        assert_eq!(
            decode_text(r#"{type:"minecraft:all_of"}"#),
            Err(r#"No key terms in MapLike[{type:"minecraft:all_of"}]"#.into())
        );
        assert_eq!(
            decode_text(r#"{type:"minecraft:inverted"}"#),
            Err(r#"No key term in MapLike[{type:"minecraft:inverted"}]"#.into())
        );
        for bad in [
            r#"{type:"minecraft:all_of",terms:[{}]}"#,
            r#"{type:"minecraft:inverted",term:{}}"#,
        ] {
            assert_eq!(decode_text(bad), Err("No key type in MapLike[{}]".into()));
        }
    }
}

#[cfg(test)]
mod contents_component_tests {
    use super::*;

    #[test]
    fn only_component_collections_are_admitted_for_contents_sources() {
        let reg = Registries::default();
        for component in [
            "container",
            "bundle_contents",
            "charged_projectiles",
            "damage",
        ] {
            let raw = Tag::compound([
                ("type", Tag::String("contents".into())),
                ("component", Tag::String(component.into())),
                (
                    "slot_source",
                    Tag::compound([
                        ("type", Tag::String("slot_range".into())),
                        ("slots", Tag::String("container.0".into())),
                    ]),
                ),
            ]);
            let result = decode("minecraft:slot_source", &raw, &reg);
            if component == "damage" {
                // Exact outer codec error independently captured from Java.
                assert_eq!(result, Err(r#"Failed to parse either. First: No items in component; Second: Failed to parse either. First: Not a string; Second: Failed to parse either. First: Not a list: {component:"damage",slot_source:{slots:"container.0",type:"slot_range"},type:"contents"}; Second: No items in component"#.into()));
            } else {
                assert_eq!(result, Ok(()));
            }
        }
    }
}

#[cfg(test)]
mod slot_range_tests {
    use super::*;

    #[test]
    fn inline_range_names_use_the_global_slot_registry() {
        let reg = Registries::default();
        for name in ["container.0", "container.53", "container.*", "hotbar.8"] {
            let input = format!(r#"{{type:"minecraft:slot_range",slots:"{name}"}}"#);
            let mut reader = Reader::new(&input);
            assert!(
                resource_or_id(&mut reader, &reg, "minecraft:slot_source").is_ok(),
                "{name}"
            );
            assert_eq!(reader.cursor, input.len());
        }
        for name in ["container.54", "container.99"] {
            let input = format!(r#"{{type:"minecraft:slot_range",slots:"{name}"}}"#);
            let mut reader = Reader::new(&input);
            assert!(
                resource_or_id(&mut reader, &reg, "minecraft:slot_source").is_err(),
                "{name} crossed argument parsing"
            );
            let (raw, _) = horizon_world::nbt::parse_snbt(&input).unwrap();
            let message = format!("Unknown element name:{name}");
            assert_eq!(
                decode("minecraft:slot_source", &raw, &reg),
                Err(either(
                    message.clone(),
                    either("Not a string".into(), either(not_list(&raw), message))
                ))
            );
        }
    }
}
