//! Registry-bound biome-source definitions. Inline full biome objects are separate.
use crate::worldgen::{
    DecodeError, Object, RawValue,
    climate::{ParameterPoint, decode_parameter_point, unquantize},
    density::identifier,
    int_range, optional, required,
};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BiomeSet {
    Direct(Vec<String>),
    Tag { name: String, members: Vec<String> },
}
impl BiomeSet {
    pub fn members(&self) -> &[String] {
        match self {
            Self::Direct(values) => values,
            Self::Tag { members, .. } => members,
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BiomeSourceDefinition {
    Fixed(String),
    Checkerboard { biomes: BiomeSet, scale: i32 },
    End,
    MultiNoise(Vec<(ParameterPoint, String)>),
    Preset { name: String, inline: bool },
}
#[derive(Clone, Debug)]
pub struct BiomeSourceCatalog {
    biomes: BTreeSet<String>,
    tags: BTreeMap<String, Vec<String>>,
    presets: BTreeSet<String>,
}
impl BiomeSourceCatalog {
    pub fn new(
        biomes: &BTreeSet<String>,
        tags: &BTreeMap<String, Vec<String>>,
        presets: &BTreeSet<String>,
    ) -> Result<Self, DecodeError> {
        if biomes.len() > 8192
            || tags.len() > 8192
            || presets.len() > 8192
            || tags.values().map(Vec::len).sum::<usize>() > 65536
        {
            return Err(DecodeError::Field("biome source catalog size"));
        }
        let biomes = biomes
            .iter()
            .map(|v| identifier(v))
            .collect::<Result<BTreeSet<_>, _>>()?;
        let presets = presets
            .iter()
            .map(|v| identifier(v))
            .collect::<Result<BTreeSet<_>, _>>()?;
        let mut resolved = BTreeMap::new();
        for (name, members) in tags {
            let members = members
                .iter()
                .map(|v| identifier(v))
                .collect::<Result<Vec<_>, _>>()?;
            if members.iter().any(|v| !biomes.contains(v)) {
                return Err(DecodeError::Field("unknown biome tag member"));
            }
            if resolved.insert(identifier(name)?, members).is_some() {
                return Err(DecodeError::Field("duplicate biome tag"));
            }
        }
        Ok(Self {
            biomes,
            tags: resolved,
            presets,
        })
    }
    fn biome(&self, raw: &RawValue) -> Result<String, DecodeError> {
        let name = identifier(&serde_json::from_str::<String>(raw.get())?)?;
        if !self.biomes.contains(&name) {
            return Err(DecodeError::Field("unknown biome"));
        }
        Ok(name)
    }
    fn set(&self, raw: &RawValue) -> Result<BiomeSet, DecodeError> {
        if raw.get().starts_with('"') {
            let name: String = serde_json::from_str(raw.get())?;
            if let Some(name) = name.strip_prefix('#') {
                let name = identifier(name)?;
                let members = self
                    .tags
                    .get(&name)
                    .ok_or(DecodeError::Field("unknown biome tag"))?
                    .clone();
                return Ok(BiomeSet::Tag { name, members });
            }
            return Ok(BiomeSet::Direct(vec![self.biome(raw)?]));
        }
        let values: Vec<Box<RawValue>> = serde_json::from_str(raw.get())?;
        if values.len() > 8192 {
            return Err(DecodeError::Field("biome holder set size"));
        }
        Ok(BiomeSet::Direct(
            values
                .iter()
                .map(|v| self.biome(v))
                .collect::<Result<_, _>>()?,
        ))
    }
    fn inline(&self, raw: &RawValue) -> Result<BiomeSourceDefinition, DecodeError> {
        let entries: Vec<Object> = serde_json::from_str(raw.get())?;
        if entries.is_empty() || entries.len() > 65536 {
            return Err(DecodeError::Field("multi-noise entry count"));
        }
        let mut result = Vec::with_capacity(entries.len());
        for entry in entries {
            result.push((
                decode_parameter_point(required(&entry, "parameters")?.get())?,
                self.biome(required(&entry, "biome")?)?,
            ));
        }
        Ok(BiomeSourceDefinition::MultiNoise(result))
    }
    pub fn decode(&self, json: &str) -> Result<BiomeSourceDefinition, DecodeError> {
        let object: Object = serde_json::from_str(json)?;
        let kind = identifier(&serde_json::from_str::<String>(
            required(&object, "type")?.get(),
        )?)?;
        Ok(match kind.as_str() {
            "minecraft:fixed" => {
                BiomeSourceDefinition::Fixed(self.biome(required(&object, "biome")?)?)
            }
            "minecraft:checkerboard" => BiomeSourceDefinition::Checkerboard {
                biomes: self.set(required(&object, "biomes")?)?,
                scale: optional(&object, "scale")
                    .map_or(Ok(2), |v| int_range(v, "biome scale", 0, 62))?,
            },
            "minecraft:the_end" => {
                for name in [
                    "the_end",
                    "end_highlands",
                    "end_midlands",
                    "small_end_islands",
                    "end_barrens",
                ] {
                    if !self.biomes.contains(&format!("minecraft:{name}")) {
                        return Err(DecodeError::Field("missing End biome"));
                    }
                }
                BiomeSourceDefinition::End
            }
            "minecraft:multi_noise" => {
                // mapEither prefers a successful inline list, then tries preset.
                if let Some(raw) = optional(&object, "biomes")
                    && let Ok(value) = self.inline(raw)
                {
                    return Ok(value);
                }
                let raw = required(&object, "preset")?;
                if raw.get().starts_with('"') {
                    let name = identifier(&serde_json::from_str::<String>(raw.get())?)?;
                    if !self.presets.contains(&name) {
                        return Err(DecodeError::Field("unknown biome preset holder"));
                    }
                    BiomeSourceDefinition::Preset {
                        name,
                        inline: false,
                    }
                } else {
                    let preset: Object = serde_json::from_str(raw.get())?;
                    let name = identifier(&serde_json::from_str::<String>(
                        required(&preset, "preset")?.get(),
                    )?)?;
                    if !matches!(name.as_str(), "minecraft:overworld" | "minecraft:nether") {
                        return Err(DecodeError::Field("unknown built-in biome preset"));
                    }
                    BiomeSourceDefinition::Preset { name, inline: true }
                }
            }
            _ => return Err(DecodeError::Field("unknown biome source")),
        })
    }
    /// Encode definitions in the vanilla canonical shape; decoding the result
    /// validates registry references and bounds before publishing JSON.
    pub fn encode(&self, definition: &BiomeSourceDefinition) -> Result<String, DecodeError> {
        let (kind, fields) = match definition {
            BiomeSourceDefinition::Fixed(name) => ("fixed", format!(",\"biome\":{}", quoted(name))),
            BiomeSourceDefinition::End => ("the_end", String::new()),
            BiomeSourceDefinition::Checkerboard { biomes, scale } => {
                let values = match biomes {
                    BiomeSet::Tag { name, .. } => quoted(&format!("#{name}")),
                    BiomeSet::Direct(values) if values.len() == 1 => quoted(&values[0]),
                    BiomeSet::Direct(values) => serde_json::to_string(values)?,
                };
                (
                    "checkerboard",
                    format!(
                        ",\"biomes\":{values}{}",
                        if *scale == 2 {
                            String::new()
                        } else {
                            format!(",\"scale\":{scale}")
                        }
                    ),
                )
            }
            BiomeSourceDefinition::MultiNoise(entries) => {
                let values = entries
                    .iter()
                    .map(|(point, name)| {
                        format!(
                            "{{\"parameters\":{},\"biome\":{}}}",
                            point_json(point),
                            quoted(name)
                        )
                    })
                    .collect::<Vec<_>>()
                    .join(",");
                ("multi_noise", format!(",\"biomes\":[{values}]"))
            }
            BiomeSourceDefinition::Preset { name, inline } => {
                let value = if *inline {
                    format!("{{\"preset\":{}}}", quoted(name))
                } else {
                    quoted(name)
                };
                ("multi_noise", format!(",\"preset\":{value}"))
            }
        };
        let json = format!("{{\"type\":\"minecraft:{kind}\"{fields}}}");
        self.decode(&json)?;
        Ok(json)
    }
}
fn quoted(value: &str) -> String {
    serde_json::to_string(value).expect("string serializes")
}
fn point_json(point: &ParameterPoint) -> String {
    let names = [
        "temperature",
        "humidity",
        "continentalness",
        "erosion",
        "depth",
        "weirdness",
    ];
    let mut fields = Vec::with_capacity(7);
    for (name, p) in names.into_iter().zip(point.coordinates) {
        let min = serde_json::to_string(&unquantize(p.min)).expect("finite coordinate");
        let max = serde_json::to_string(&unquantize(p.max)).expect("finite coordinate");
        let value = if min == max {
            min
        } else {
            format!("[{min},{max}]")
        };
        fields.push(format!("{}:{value}", quoted(name)));
    }
    fields.push(format!(
        "\"offset\":{}",
        serde_json::to_string(&unquantize(point.offset)).expect("finite offset")
    ));
    format!("{{{}}}", fields.join(","))
}
