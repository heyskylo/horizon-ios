//! Component data shared by item resolution and model conditions.
use horizon_world::nbt::{Tag, parse_snbt};
use serde_json::Value;

/// An NBT value as the JSON its component codec reads.
pub fn tag_json(tag: &Tag) -> Value {
    match tag {
        Tag::End => Value::Null,
        Tag::Byte(v) => Value::from(*v),
        Tag::Short(v) => Value::from(*v),
        Tag::Int(v) => Value::from(*v),
        Tag::Long(v) => Value::from(*v),
        Tag::Float(bits) => serde_json::Number::from_f64(f64::from(f32::from_bits(*bits)))
            .map_or(Value::Null, Value::Number),
        Tag::Double(bits) => {
            serde_json::Number::from_f64(f64::from_bits(*bits)).map_or(Value::Null, Value::Number)
        }
        Tag::ByteArray(a) => Value::Array(a.iter().map(|b| Value::from(*b as i8)).collect()),
        Tag::String(s) => Value::String(s.to_string_lossy()),
        Tag::List(l) => Value::Array(l.iter().map(tag_json).collect()),
        Tag::Compound(c) => Value::Object(
            c.iter()
                .map(|(k, v)| (k.to_string_lossy(), tag_json(v)))
                .collect(),
        ),
        Tag::IntArray(a) => Value::Array(a.iter().map(|v| Value::from(*v)).collect()),
        Tag::LongArray(a) => Value::Array(a.iter().map(|v| Value::from(*v)).collect()),
    }
}

/// Custom model flags use Codec.BOOL even though NBT stores them as numbers.
/// Keep the other custom-model fields, and generic NBT JSON, numeric.
pub(super) fn custom_model_data_json(tag: &Tag) -> Value {
    let mut value = tag_json(tag);
    let flags: Option<Vec<Value>> = match tag.get("flags") {
        Some(Tag::List(flags)) => flags
            .iter()
            .map(|flag| crate::item_data::component_bool(flag).map(Value::Bool))
            .collect(),
        Some(Tag::ByteArray(flags)) => flags
            .iter()
            .map(|flag| crate::item_data::component_bool(&Tag::Byte(*flag as i8)).map(Value::Bool))
            .collect(),
        Some(Tag::IntArray(flags)) => flags
            .iter()
            .map(|flag| crate::item_data::component_bool(&Tag::Int(*flag)).map(Value::Bool))
            .collect(),
        Some(Tag::LongArray(flags)) => flags
            .iter()
            .map(|flag| crate::item_data::component_bool(&Tag::Long(*flag)).map(Value::Bool))
            .collect(),
        _ => None,
    };
    if let (Some(flags), Some(object)) = (flags, value.as_object_mut()) {
        object.insert("flags".into(), Value::Array(flags));
    }
    value
}

/// Match the custom-data condition against the resolver's JSON component view.
/// SNBT booleans become byte values, just as stack_view converts network NBT.
pub fn custom_data_matches(actual: &Value, pattern: &Value) -> bool {
    let expected = match pattern {
        Value::String(text) => match parse_snbt(text) {
            Ok((tag @ Tag::Compound(_), used)) if text[used..].trim().is_empty() => tag_json(&tag),
            _ => return false,
        },
        Value::Object(_) => pattern.clone(),
        _ => return false,
    };
    partial_match(&expected, actual)
}
fn partial_match(expected: &Value, actual: &Value) -> bool {
    match (expected, actual) {
        (Value::Object(e), Value::Object(a)) => e
            .iter()
            .all(|(k, v)| a.get(k).is_some_and(|a| partial_match(v, a))),
        (Value::Array(e), Value::Array(a)) => {
            if e.is_empty() {
                a.is_empty()
            } else {
                e.iter().all(|e| a.iter().any(|a| partial_match(e, a)))
            }
        }
        _ => expected == actual,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn snbt_conditions_use_partial_components_and_byte_booleans() {
        let (actual, _) =
            parse_snbt("{td_attune_mode:1,model:true,extra:{values:[1,2,3]}}").unwrap();
        let actual = tag_json(&actual);
        for pattern in ["{td_attune_mode:1}", "{model:true}", "{extra:{values:[2]}}"] {
            assert!(
                custom_data_matches(&actual, &Value::from(pattern)),
                "{pattern}"
            );
        }
        for pattern in [
            "{td_attune_mode:2}",
            "{failed:true}",
            "{model:false}",
            "{extra:{values:[]}}",
            "{model:true} trailing",
            "malformed",
        ] {
            assert!(
                !custom_data_matches(&actual, &Value::from(pattern)),
                "{pattern}"
            );
        }
    }
}
