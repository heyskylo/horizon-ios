//! Pinned blockstate resource definitions, before registry binding and baking.
pub mod bind;
pub mod choice;
use crate::json::*;
use serde::{
    Deserializer,
    de::{MapAccess, Visitor},
};
use serde_json::value::RawValue;
use std::{collections::BTreeMap, fmt};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Variant {
    pub model: String,
    /// X, Y, Z rotations in quarter turns.
    pub rotations: [u8; 3],
    pub uv_lock: bool,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum VariantSet {
    Single(Variant),
    Weighted(Vec<(Variant, i32)>),
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Term {
    pub value: String,
    pub negated: bool,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Condition {
    Or(Vec<Condition>),
    And(Vec<Condition>),
    Properties(BTreeMap<String, Vec<Term>>),
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Selector {
    pub condition: Option<Condition>,
    pub variant: VariantSet,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BlockStateDefinition {
    /// Preserve resource order: overlapping variant selectors have order-sensitive
    /// native recovery behavior when bound to a registry.
    pub variants: Option<Vec<(String, VariantSet)>>,
    pub multipart: Option<Vec<Selector>>,
}
fn optional<'a>(fields: &Object<'a>, name: &str) -> Option<&'a RawValue> {
    fields.get(name).copied().filter(|v| v.get() != "null")
}
fn ordered_object(raw: &RawValue) -> Result<Vec<(String, &RawValue)>> {
    struct Ordered;
    impl<'de> Visitor<'de> for Ordered {
        type Value = Vec<(String, &'de RawValue)>;
        fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
            formatter.write_str("an object")
        }
        fn visit_map<M: MapAccess<'de>>(
            self,
            mut map: M,
        ) -> std::result::Result<Self::Value, M::Error> {
            let mut values: Self::Value = Vec::new();
            let mut indices = BTreeMap::new();
            while let Some((name, value)) = map.next_entry::<String, &'de RawValue>()? {
                if let Some(&index) = indices.get(&name) {
                    values[index] = (name, value);
                } else {
                    indices.insert(name.clone(), values.len());
                    values.push((name, value));
                }
            }
            Ok(values)
        }
    }
    Ok(serde_json::Deserializer::from_str(raw.get()).deserialize_map(Ordered)?)
}
impl BlockStateDefinition {
    pub fn from_json(text: &str) -> Result<Self> {
        let raw: &RawValue = serde_json::from_str(text.strip_prefix('\u{feff}').unwrap_or(text))?;
        validate_number_tokens(raw)?;
        let fields = object(raw)?;
        let variants = optional(&fields, "variants")
            .map(|v| {
                let values = ordered_object(v)?;
                if values.is_empty() {
                    return Err(error("empty variant map"));
                }
                values
                    .into_iter()
                    .map(|(key, value)| Ok((key, VariantSet::decode(value)?)))
                    .collect::<Result<Vec<_>>>()
            })
            .transpose()?;
        let multipart = optional(&fields, "multipart")
            .map(|v| {
                let values = array(v)?;
                if values.is_empty() {
                    return Err(error("empty multipart list"));
                }
                values
                    .into_iter()
                    .map(|v| {
                        let fields = object(v)?;
                        Ok(Selector {
                            condition: optional(&fields, "when")
                                .map(Condition::decode)
                                .transpose()?,
                            variant: VariantSet::decode(required(&fields, "apply")?)?,
                        })
                    })
                    .collect::<Result<Vec<_>>>()
            })
            .transpose()?;
        if variants.is_none() && multipart.is_none() {
            return Err(error("no variants or multipart"));
        }
        Ok(Self {
            variants,
            multipart,
        })
    }
}
impl Variant {
    fn decode(raw: &RawValue) -> Result<Self> {
        let fields = object(raw)?;
        let mut rotations = [0; 3];
        for (target, name) in rotations.iter_mut().zip(["x", "y", "z"]) {
            let angle = optional(&fields, name)
                .map(integer)
                .transpose()?
                .unwrap_or(0)
                .rem_euclid(360);
            if angle % 90 != 0 {
                return Err(error("model rotation must be a multiple of 90"));
            }
            *target = (angle / 90) as u8;
        }
        Ok(Self {
            model: identifier(&string(required(&fields, "model")?)?)?,
            rotations,
            uv_lock: optional(&fields, "uvlock")
                .map(|v| serde_json::from_str(v.get()))
                .transpose()?
                .unwrap_or(false),
        })
    }
}
impl VariantSet {
    fn decode(raw: &RawValue) -> Result<Self> {
        if !raw.get().starts_with('[') {
            return Ok(Self::Single(Variant::decode(raw)?));
        }
        let values = array(raw)?;
        if values.is_empty() {
            return Err(error("empty weighted variants"));
        }
        let mut total = 0i32;
        let entries = values
            .into_iter()
            .map(|v| {
                let fields = object(v)?;
                let weight = optional(&fields, "weight")
                    .map(integer)
                    .transpose()?
                    .unwrap_or(1);
                if weight <= 0 {
                    return Err(error("nonpositive variant weight"));
                }
                total = total
                    .checked_add(weight)
                    .ok_or_else(|| error("variant weight overflow"))?;
                Ok((Variant::decode(v)?, weight))
            })
            .collect::<Result<Vec<_>>>()?;
        Ok(Self::Weighted(entries))
    }
}
impl Condition {
    fn decode(raw: &RawValue) -> Result<Self> {
        let fields = object(raw)?;
        if fields.len() == 1 {
            for name in ["OR", "AND"] {
                if let Some(raw) = fields.get(name)
                    && let Ok(values) = array(raw).and_then(|values| {
                        values
                            .into_iter()
                            .map(Self::decode)
                            .collect::<Result<Vec<_>>>()
                    })
                {
                    return Ok(if name == "OR" {
                        Self::Or(values)
                    } else {
                        Self::And(values)
                    });
                }
            }
        }
        if fields.is_empty() {
            return Err(error("empty property conditions"));
        }
        let tests = fields
            .into_iter()
            .map(|(key, raw)| {
                let value = match raw.get().as_bytes().first() {
                    Some(b'"') => string(raw)?,
                    Some(b't' | b'f') => raw.get().to_owned(),
                    _ => integer(raw)?.to_string(),
                };
                let terms = value
                    .split('|')
                    .map(|v| {
                        let negated = v.starts_with('!');
                        let value = v.strip_prefix('!').unwrap_or(v);
                        if value.is_empty() {
                            return Err(error("empty property term"));
                        }
                        Ok(Term {
                            value: value.to_owned(),
                            negated,
                        })
                    })
                    .collect::<Result<Vec<_>>>()?;
                Ok((key, terms))
            })
            .collect::<Result<_>>()?;
        Ok(Self::Properties(tests))
    }
}
