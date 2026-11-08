//! DamageType.DIRECT_CODEC resources and TagLoader's world pack stack.
use super::{Catalog, DamageTypeId, Definition, Scaling, invalid, string};
use crate::{
    loot::PackOverlay,
    worldgen::tags::{TagDefinitions, identifier},
};
use serde_json::Value;
use std::{collections::BTreeMap, fs, io, path::Path};

/// The resource root's files of one damage registry kind, in every namespace.
/// `json_folder` also reads prepared vanilla files from horizon-data.bin.
pub fn resource_files(root: &Path, kind: &str) -> io::Result<BTreeMap<String, Vec<u8>>> {
    let mut files = BTreeMap::new();
    let namespaces = match fs::read_dir(root.join("data")) {
        Ok(entries) => entries,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(files),
        Err(e) => return Err(e),
    };
    for namespace in namespaces {
        let namespace = namespace?;
        if !namespace.path().is_dir() {
            continue;
        }
        let ns = namespace.file_name().to_string_lossy().into_owned();
        let folder = match crate::json_folder::read(&namespace.path().join(kind)) {
            Ok(folder) => folder,
            Err(e) if e.kind() == io::ErrorKind::NotFound => continue,
            Err(e) => return Err(e),
        };
        for file in &folder.files {
            files.insert(format!("{ns}:{}", file.name), file.bytes()?.to_vec());
        }
    }
    Ok(files)
}

/// TagLoader.loadTagsForExistingRegistries: resources resolve against the
/// supplied frozen registry. A missing required entry invalidates its tag;
/// unrelated tags survive, while optional missing entries are ignored.
pub fn resource_tags<T: Clone + Ord>(
    root: &Path,
    overlay: &PackOverlay<'_>,
    lookup: impl FnMut(&str, bool) -> Option<T>,
) -> io::Result<BTreeMap<String, Vec<T>>> {
    let mut files: BTreeMap<String, Vec<Vec<u8>>> = BTreeMap::new();
    let exported = root.join("damage-tags.json");
    if exported.exists() {
        let value: Value = serde_json::from_slice(&fs::read(exported)?)?;
        for (id, json) in value
            .as_object()
            .ok_or_else(|| invalid("expected damage tags"))?
        {
            files
                .entry(id.clone())
                .or_default()
                .push(string(json)?.as_bytes().to_vec());
        }
    }
    for (id, bytes) in resource_files(root, "tags/damage_type")? {
        files.entry(id).or_default().push(bytes);
    }
    for (id, versions) in overlay.tag_kind("damage_type") {
        files
            .entry(id)
            .or_default()
            .extend(versions.iter().cloned());
    }
    // ResourceManager lists stacks in Identifier file order (path first,
    // namespace second). Insertion order matters when tag cycles are pruned.
    let mut files: Vec<_> = files.into_iter().collect();
    files.sort_by_cached_key(|(id, _)| {
        let (ns, path) = id.split_once(':').unwrap_or(("minecraft", id));
        (format!("{path}.json"), ns.to_owned())
    });
    let mut tags = TagDefinitions::default();
    for (id, versions) in files {
        for bytes in versions {
            let result = std::str::from_utf8(&bytes)
                .map_err(|e| e.to_string())
                .and_then(|text| {
                    tags.apply_json(&id, "pack", text)
                        .map_err(|e| e.to_string())
                });
            if let Err(e) = result {
                log::warn!("Horizon: damage tag {id} is not read: {e}");
            }
        }
    }
    let built = tags.build(lookup).map_err(|e| invalid(e.to_string()))?;
    for id in built.missing.keys() {
        log::warn!("Horizon: damage tag {id} has unresolved required entries");
    }
    Ok(built.tags)
}

impl Definition {
    /// DamageType.DIRECT_CODEC with NbtOps: float exhaustion, and optional
    /// fields omitted at their codec defaults. FloatTag.valueOf shares ZERO
    /// for either sign of zero; the resident definition still keeps its bits.
    pub fn to_nbt(&self) -> crate::nbt::Tag {
        use crate::nbt::Tag;
        let mut fields = vec![
            ("message_id", Tag::String(self.message_id.as_str().into())),
            ("scaling", Tag::String(self.scaling.name().into())),
            (
                "exhaustion",
                Tag::Float(if self.exhaustion == 0.0 {
                    0
                } else {
                    self.exhaustion.to_bits()
                }),
            ),
        ];
        if self.effects != "hurt" {
            fields.push(("effects", Tag::String(self.effects.as_str().into())));
        }
        if self.death_message_type != "default" {
            fields.push((
                "death_message_type",
                Tag::String(self.death_message_type.as_str().into()),
            ));
        }
        Tag::compound(fields)
    }

    /// DamageType.DIRECT_CODEC: three required fields, two optional enums.
    pub fn from_json(name: &str, bytes: &[u8]) -> io::Result<Self> {
        let value: BTreeMap<String, &serde_json::value::RawValue> = serde_json::from_slice(bytes)?;
        let text = |key: &str, default: Option<&str>| -> io::Result<String> {
            match value.get(key).filter(|v| v.get() != "null") {
                Some(v) => serde_json::from_str(v.get()).map_err(|e| invalid(e.to_string())),
                None => default
                    .map(str::to_owned)
                    .ok_or_else(|| invalid(format!("missing damage {key}"))),
            }
        };
        if identifier(name).map_err(|e| invalid(e.to_string()))? != name {
            return Err(invalid("noncanonical damage type"));
        }
        let scaling = match text("scaling", None)?.as_str() {
            "never" => Scaling::Never,
            "when_caused_by_living_non_player" => Scaling::LivingNonPlayer,
            "always" => Scaling::Always,
            _ => return Err(invalid("unknown damage scaling")),
        };
        let effects = text("effects", Some("hurt"))?;
        if ![
            "hurt", "thorns", "drowning", "burning", "poking", "freezing",
        ]
        .contains(&effects.as_str())
        {
            return Err(invalid("unknown damage effects"));
        }
        let death = text("death_message_type", Some("default"))?;
        if !["default", "fall_variants", "intentional_game_design"].contains(&death.as_str()) {
            return Err(invalid("unknown death message type"));
        }
        Ok(Self {
            name: name.into(),
            message_id: text("message_id", None)?,
            // JsonOps parses directly to float; an intermediate f64 can
            // round the other way at a float's halfway boundary.
            exhaustion: value
                .get("exhaustion")
                .ok_or_else(|| invalid("missing damage exhaustion"))?
                .get()
                .parse::<f32>()
                .map_err(|_| invalid("expected damage exhaustion"))?,
            scaling,
            effects,
            death_message_type: death,
        })
    }
}

impl Catalog {
    /// The exported vanilla definitions plus ordinary resource-root and
    /// enabled world-pack files. No definition is published on codec failure.
    pub fn load(root: &Path, overlay: &PackOverlay<'_>) -> io::Result<Self> {
        let catalog = serde_json::from_slice(&fs::read(root.join("damage-catalog.json"))?)?;
        // Resolve tags only after custom definitions have joined the registry.
        Self::from_json(&catalog, &serde_json::json!({}))?.with_packs(root, overlay)
    }

    /// Names in a resolved tag, independently of local or network indices.
    pub fn tag_members(&self, tag: &str) -> impl Iterator<Item = &str> {
        self.tags
            .get(tag)
            .into_iter()
            .flatten()
            .map(|id| self.definitions[id.0].name.as_str())
    }

    /// RegistryDataLoader's world-load snapshot. Existing local definition
    /// indices stay stable; new names append in sorted order. Wire IDs remain
    /// the separately synchronized registry's, looked up by name.
    pub fn with_packs(&self, root: &Path, overlay: &PackOverlay<'_>) -> io::Result<Self> {
        let mut files = resource_files(root, "damage_type")?;
        for (id, versions) in overlay.kind("damage_type") {
            files.insert(id.into(), PackOverlay::top(versions).to_vec());
        }
        let mut catalog = self.clone();
        for (id, bytes) in files {
            let definition = Definition::from_json(&id, &bytes)
                .map_err(|e| invalid(format!("damage type {id}: {e}")))?;
            match catalog.ids.get(&id) {
                Some(index) => catalog.definitions[index.0] = definition,
                None => {
                    catalog
                        .ids
                        .insert(id, DamageTypeId(catalog.definitions.len()));
                    catalog.definitions.push(definition);
                }
            }
        }
        catalog.reload_tags(root, overlay)
    }

    /// MinecraftServer.reloadResources retains the frozen damage registry;
    /// only tags change, including when a pack containing a type is disabled.
    pub fn reload_tags(&self, root: &Path, overlay: &PackOverlay<'_>) -> io::Result<Self> {
        let mut catalog = self.clone();
        catalog.tags = resource_tags(root, overlay, |id, _| catalog.ids.get(id).copied())?
            .into_iter()
            .map(|(id, values)| (id, values.into_iter().collect()))
            .collect();
        Ok(catalog)
    }
}

#[cfg(test)]
mod tests;
