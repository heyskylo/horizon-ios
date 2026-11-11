//! What the configuration phase sends: the synchronized registries (names in
//! network order, with their data for clients that do not know the vanilla
//! pack) and the tags of every registry, loaded from the prepared resources
//! (`data/minecraft/...`) and the registries report.
use crate::chunk::LevelShapes;
use horizon_protocol::{
    entity_data,
    packets::configuration::{RegistryData, RegistryEntry, RegistryTags, TagEntry, UpdateTags},
    registry::{Builtin, Dynamic},
};
use horizon_world::nbt::{JavaString, Tag};
use serde_json::Value;
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    path::{Path, PathBuf},
};

/// RegistryDataLoader.SYNCHRONIZED_REGISTRIES, in vanilla's order.
pub const SYNCHRONIZED: &[&str] = &[
    "minecraft:worldgen/biome",
    "minecraft:chat_type",
    "minecraft:trim_pattern",
    "minecraft:trim_material",
    "minecraft:wolf_variant",
    "minecraft:wolf_sound_variant",
    "minecraft:pig_variant",
    "minecraft:pig_sound_variant",
    "minecraft:frog_variant",
    "minecraft:cat_variant",
    "minecraft:cat_sound_variant",
    "minecraft:cow_sound_variant",
    "minecraft:cow_variant",
    "minecraft:chicken_sound_variant",
    "minecraft:chicken_variant",
    "minecraft:zombie_nautilus_variant",
    "minecraft:painting_variant",
    "minecraft:sulfur_cube_archetype",
    "minecraft:dimension_type",
    "minecraft:damage_type",
    "minecraft:banner_pattern",
    "minecraft:enchantment",
    "minecraft:jukebox_song",
    "minecraft:instrument",
    "minecraft:test_environment",
    "minecraft:test_instance",
    "minecraft:dialog",
    "minecraft:world_clock",
    "minecraft:timeline",
    "minecraft:decorated_pot_pattern",
    "minecraft:block_transformer",
    "minecraft:worldgen/block_state_provider",
];
pub const BIOME_REGISTRY: &str = "minecraft:worldgen/biome";
/// The table a client plays a server with: `known` and the biomes of the
/// server's biome registry that it lacks, built from their data; None when
/// there are none.
pub fn client_biomes(
    known: &crate::biome::Biomes,
    registry: &[(String, Option<Tag>)],
) -> Option<crate::biome::Biomes> {
    let files: Vec<(&str, Value)> = registry
        .iter()
        .filter(|(name, _)| !known.contains(name))
        .filter_map(|(name, data)| {
            Some((
                name.as_str(),
                crate::scoreboard::tag_to_json(data.as_ref()?),
            ))
        })
        .collect();
    if files.is_empty() {
        return None;
    }
    log::info!(
        "Horizon: the server has {} biomes the game lacks",
        files.len()
    );
    Some(known.with_packs(files.iter().map(|(n, v)| (*n, v))))
}
/// KnownPack.vanilla("core") for this version.
pub const CORE_PACK: (&str, &str, &str) = ("minecraft", "core", horizon_protocol::GAME_VERSION);

/// Synchronized registries a world's data packs add entries to (or replace
/// them in). The others stay the game's own: Horizon's gameplay reads them
/// from the prepared resources only.
const PACK_REGISTRIES: &[&str] = &["minecraft:dialog"];

/// A data-driven registry: entry names in network order and their NBT.
#[derive(Clone)]
struct Entries {
    names: Vec<String>,
    data: Vec<Tag>,
    /// The entry comes from a world's data pack, which no client knows, so
    /// its data is always sent.
    from_pack: Vec<bool>,
}

/// The damage registry already bound for gameplay at world entry. Keeping
/// only these entries lets unrelated pack data remain lazy until a hub needs it.
pub struct FrozenDamage(Entries);

impl FrozenDamage {
    pub fn new(
        catalog: &horizon_world::damage_source::Catalog,
        overlay: &horizon_world::loot::PackOverlay<'_>,
    ) -> Self {
        let packed: HashSet<_> = overlay.kind("damage_type").map(|(id, _)| id).collect();
        let mut definitions: Vec<_> = catalog.entries().map(|(_, d)| d).collect();
        definitions.sort_by_cached_key(|d| {
            let (ns, path) = d.name.split_once(':').expect("canonical damage identifier");
            (format!("{path}.json"), ns.to_owned())
        });
        Self(Entries {
            names: definitions.iter().map(|d| d.name.clone()).collect(),
            data: definitions.iter().map(|d| d.to_nbt()).collect(),
            from_pack: definitions
                .iter()
                .map(|d| !d.name.starts_with("minecraft:") || packed.contains(d.name.as_str()))
                .collect(),
        })
    }
}

pub struct SyncData {
    pub content_identity: Option<[u8; 32]>,
    pub builtin: Builtin,
    /// Entry names per synchronized registry (for id lookups).
    pub dynamic: Dynamic,
    registries: Vec<(String, Entries)>,
    pub tags: UpdateTags,
    pub features: Vec<String>,
    /// Vanilla's command tree (the commands report), when found.
    pub commands: Option<super::commands::CommandTree>,
    /// ClientboundUpdateRecipesPacket's payload.
    pub recipes: Vec<u8>,
    pub advancements: super::advancements::ServerAdvancements,
}

use crate::crafting::qualified;

/// Every `*.json` under `dir`, as (path without extension relative to dir, value), sorted;
/// a file that can't be read or doesn't parse is left out. Through `json_folder`: from the
/// prepared data file, and read once for the tables a load builds from the same folder.
fn walk(dir: &Path, out: &mut BTreeMap<String, Value>) {
    let Ok(folder) = horizon_world::json_folder::read(dir) else {
        return;
    };
    for file in &folder.files {
        if let Ok(value) = serde_json::from_slice::<Value>(file.bytes().unwrap_or_default()) {
            out.insert(file.name.clone(), value);
        }
    }
}

/// A synchronized registry's entries from the world's data packs (`<dir>/`
/// files, the top pack's for each id) over the game's own. Each entry is
/// ((namespace, path), JSON, from a pack); `file_order` sorts them.
fn pack_entries(
    dir: &str,
    overlay: &horizon_world::loot::PackOverlay,
    entries: &mut Vec<((String, String), Value, bool)>,
) {
    for (id, files) in overlay.kind(dir) {
        let Some((ns, path)) = id.split_once(':') else {
            continue;
        };
        let top = horizon_world::loot::PackOverlay::top(files);
        let value = match serde_json::from_slice::<Value>(top) {
            Ok(value) => value,
            Err(e) => {
                log::warn!("Horizon: data pack {dir} {id} is not read: {e}");
                continue;
            }
        };
        let key = (ns.to_owned(), path.to_owned());
        match entries.iter_mut().find(|(k, ..)| *k == key) {
            Some(entry) => {
                entry.1 = value;
                entry.2 = true;
            }
            None => entries.push((key, value, true)),
        }
    }
}

/// ResourceManagerRegistryLoadTask.load: the entries register sorted by the
/// file `Identifier` FileToIdConverter.listMatchingResources keys them by
/// (`<dir>/<path>.json`), path first, then namespace (Identifier.compareTo).
/// The `.json` counts: `x-y` comes before `x`, as `-` sorts before `.`.
fn file_order<T>(entries: &mut [((String, String), T, bool)]) {
    entries.sort_by_cached_key(|((ns, path), ..)| (format!("{path}.json"), ns.clone()));
}

/// A JSON value as the NBT that vanilla's NbtOps would produce from the codec
/// (integers are ints, other numbers doubles, booleans bytes).
pub fn json_to_nbt(v: &Value) -> Tag {
    match v {
        Value::Null => Tag::compound(Vec::<(&str, Tag)>::new()),
        Value::Bool(b) => Tag::Byte(i8::from(*b)),
        Value::Number(n) => match n.as_i64() {
            Some(i) => match i32::try_from(i) {
                Ok(i) => Tag::Int(i),
                Err(_) => Tag::Long(i),
            },
            None => Tag::Double(n.as_f64().unwrap_or(0.0).to_bits()),
        },
        Value::String(s) => Tag::String(JavaString::from(s.as_str())),
        Value::Array(a) => Tag::List(a.iter().map(json_to_nbt).collect()),
        Value::Object(o) => Tag::compound(o.iter().map(|(k, v)| (k.as_str(), json_to_nbt(v)))),
    }
}

/// A component's JSON as NbtOps writes it. A list whose elements differ in type
/// (translation arguments mixing numbers and components) stays one list: the NBT
/// writer wraps each element that isn't a compound under the empty key, as
/// `ListTag.wrapIfNeeded` does, so wrapping it here too would leave the client a
/// `{"": 1}` argument it cannot read.
pub fn component_nbt(v: &Value) -> Tag {
    match v {
        Value::Array(a) => Tag::List(a.iter().map(component_nbt).collect()),
        Value::Object(o) => Tag::compound(o.iter().map(|(k, v)| (k.as_str(), component_nbt(v)))),
        other => json_to_nbt(other),
    }
}

impl SyncData {
    /// No registries, tags or recipes: enough for tests of the login phase.
    pub fn empty() -> Self {
        Self {
            builtin: Builtin::default(),
            dynamic: Dynamic::default(),
            registries: Vec::new(),
            tags: UpdateTags {
                registries: Vec::new(),
            },
            features: Vec::new(),
            commands: None,
            recipes: Vec::new(),
            advancements: super::advancements::ServerAdvancements::load(
                Path::new(""),
                &horizon_world::loot::PackOverlay::NONE,
                BTreeMap::new(),
                HashMap::new(),
            ),
            content_identity: None,
        }
    }
    pub fn load(
        resources: &Path,
        overlay: &horizon_world::loot::PackOverlay,
    ) -> Result<Self, String> {
        Self::load_with(resources, overlay, None)
    }

    /// `load` with the open world's biomes: when its data packs added or
    /// replaced some, the biome registry is that table's, in its order.
    pub fn load_with(
        resources: &Path,
        overlay: &horizon_world::loot::PackOverlay,
        biomes: Option<&crate::biome::Biomes>,
    ) -> Result<Self, String> {
        Self::load_with_kept(resources, overlay, biomes, None)
    }

    /// Reload tags and reloadable data against the registries frozen at open.
    pub fn reload(
        resources: &Path,
        overlay: &horizon_world::loot::PackOverlay,
        kept: &Self,
    ) -> Result<Self, String> {
        Self::load_with_kept(resources, overlay, None, Some(kept))
    }

    /// Load reloadable data with frozen synchronized registries; an explicit
    /// biome table supplies the world's already-loaded biome registry.
    pub fn load_with_kept(
        resources: &Path,
        overlay: &horizon_world::loot::PackOverlay,
        biomes: Option<&crate::biome::Biomes>,
        kept: Option<&Self>,
    ) -> Result<Self, String> {
        Self::load_with_frozen(resources, overlay, biomes, kept, None, false)
    }

    /// [`Self::load_with_kept`] over the packs `kept` was itself loaded with (a world's own
    /// biomes or flags over the data its packs gave): the recipes and advancements, read
    /// from the same files, are `kept`'s rather than read again (~0.7 s with a large modpack).
    pub fn load_over(
        resources: &Path,
        overlay: &horizon_world::loot::PackOverlay,
        biomes: Option<&crate::biome::Biomes>,
        kept: &Self,
    ) -> Result<Self, String> {
        Self::load_with_frozen(resources, overlay, biomes, Some(kept), None, true)
    }

    /// First lazy load after world entry: damage definitions are the same
    /// snapshot gameplay uses, even if `/reload` happened before the first hub.
    pub fn load_with_frozen_damage(
        resources: &Path,
        overlay: &horizon_world::loot::PackOverlay,
        damage: &FrozenDamage,
    ) -> Result<Self, String> {
        Self::load_with_frozen(resources, overlay, None, None, Some(damage), false)
    }

    fn load_with_frozen(
        resources: &Path,
        overlay: &horizon_world::loot::PackOverlay,
        biomes: Option<&crate::biome::Biomes>,
        kept: Option<&Self>,
        damage: Option<&FrozenDamage>,
        same_files: bool,
    ) -> Result<Self, String> {
        use crate::spans::startup;
        // The recipe book, the recipe sync and the advancements read the
        // recipe folder: held here, they share one read of it.
        let _recipe_files = startup::time("sync.recipe_folder", || {
            horizon_world::json_folder::read(&resources.join("data/minecraft/recipe"))
        });
        let registries_step = startup::step("sync.registries");
        let builtin = Builtin::find(resources)
            .and_then(|p| Builtin::load(&p).ok())
            .ok_or("registries.json not found next to the resources")?;
        let root: PathBuf = resources.join("data/minecraft");
        let mut registries = Vec::new();
        let mut dynamic = Dynamic {
            custom_components: horizon_protocol::item::custom::entries().next().is_some(),
            ..Default::default()
        };
        for key in SYNCHRONIZED {
            if let Some(kept) = kept.filter(|_| *key != BIOME_REGISTRY || biomes.is_none()) {
                if let Some((_, entries)) = kept.registries.iter().find(|(name, _)| name == key) {
                    dynamic
                        .registries
                        .insert((*key).into(), kept.dynamic.registries[*key].clone());
                    registries.push(((*key).into(), entries.clone()));
                }
                continue;
            }
            if *key == "minecraft:damage_type"
                && let Some(damage) = damage
            {
                dynamic.registries.insert(
                    (*key).into(),
                    damage
                        .0
                        .names
                        .iter()
                        .map(|name| (name.clone(), None))
                        .collect(),
                );
                registries.push(((*key).into(), damage.0.clone()));
                continue;
            }
            let path = key.trim_start_matches("minecraft:");
            let mut found = BTreeMap::new();
            walk(&root.join(path), &mut found);
            let mut entries: Vec<_> = found
                .into_iter()
                .map(|(p, v)| (("minecraft".to_owned(), p), v, false))
                .collect();
            let mut damage_entries = None;
            if *key == "minecraft:damage_type" {
                // Validate the actual resource bytes before publishing any
                // registry data. Small tools can supply just these files,
                // without the gameplay-only exported damage catalog.
                let mut files = horizon_world::damage_source::resource_files(resources, path)
                    .map_err(|e| e.to_string())?;
                let packed: HashSet<_> = overlay.kind(path).map(|(id, _)| id).collect();
                for (id, versions) in overlay.kind(path) {
                    files.insert(
                        id.into(),
                        horizon_world::loot::PackOverlay::top(versions).to_vec(),
                    );
                }
                let mut decoded = Vec::new();
                for (id, bytes) in files {
                    let definition =
                        horizon_world::damage_source::Definition::from_json(&id, &bytes)
                            .map_err(|e| format!("damage type {id}: {e}"))?;
                    let (ns, p) = id.split_once(':').ok_or("invalid damage identifier")?;
                    decoded.push((
                        (ns.into(), p.into()),
                        definition.to_nbt(),
                        ns != "minecraft" || packed.contains(id.as_str()),
                    ));
                }
                damage_entries = Some(decoded);
            } else if PACK_REGISTRIES.contains(key) {
                pack_entries(path, overlay, &mut entries);
            }
            if *key == BIOME_REGISTRY
                && let Some(table) = biomes.filter(|b| b.has_packs())
            {
                // A pack's biome goes with its data: no client knows it.
                let found: HashMap<_, _> = entries
                    .drain(..)
                    .map(|((ns, p), v, _)| (format!("{ns}:{p}"), v))
                    .collect();
                for name in table.registry_order() {
                    let (ns, p) = name.split_once(':').unwrap_or(("minecraft", name));
                    let pack = table.pack_file(name);
                    let json = pack.or_else(|| found.get(name)).cloned();
                    let json = json.unwrap_or_else(|| Value::Object(Default::default()));
                    entries.push(((ns.to_owned(), p.to_owned()), json, pack.is_some()));
                }
            }
            let mut entries = damage_entries.unwrap_or_else(|| {
                entries
                    .into_iter()
                    .map(|(key, value, packed)| (key, json_to_nbt(&value), packed))
                    .collect()
            });
            file_order(&mut entries);
            let mut e = Entries {
                names: Vec::new(),
                data: Vec::new(),
                from_pack: Vec::new(),
            };
            for ((ns, p), v, from_pack) in entries {
                e.names.push(format!("{ns}:{p}"));
                e.data.push(v);
                e.from_pack.push(from_pack);
            }
            dynamic.registries.insert(
                (*key).to_owned(),
                e.names.iter().map(|n| (n.clone(), None)).collect(),
            );
            registries.push(((*key).to_owned(), e));
        }
        // The world's packs' dialogs joined the registry above
        // (`pack_entries`); their tags join the root's here, the dialogs'
        // (the Quick Actions key and `show_dialog`) and every other
        // registry's a pack has tag files for (a block made climbable).
        drop(registries_step);
        let tags_step = startup::step("sync.tags");
        let mut tags = Self::load_tags(&root, &builtin, &dynamic);
        Self::merge_pack_tags(&mut tags, &root, overlay, &builtin, &dynamic);
        Self::merge_damage_tags(&mut tags, resources, overlay, &builtin, &dynamic)?;
        drop(tags_step);
        let content_identity = if resources.join(crate::content_identity::MARKER).exists()
            || horizon_protocol::item::custom::entries().next().is_some()
        {
            Some(
                startup::time("sync.content_identity", || {
                    crate::content_identity::identity(resources)
                })
                .map_err(|error| error.to_string())?,
            )
        } else {
            None
        };
        let mut sync = Self {
            content_identity,
            builtin,
            dynamic,
            registries,
            tags,
            features: vec!["minecraft:vanilla".to_owned()],
            commands: startup::time("sync.commands", || {
                super::commands::CommandTree::load(resources)
            }),
            recipes: Vec::new(),
            advancements: match kept.filter(|_| same_files) {
                Some(kept) => kept.advancements.clone(),
                None => {
                    let (recipes, groups) = startup::time("sync.network_recipes", || {
                        super::recipes::network_recipes(resources, overlay)
                    });
                    startup::time("sync.advancements", || {
                        super::advancements::ServerAdvancements::load(
                            resources, overlay, recipes, groups,
                        )
                    })
                }
            },
        };
        sync.recipes = startup::time("sync.update_recipes", || {
            super::recipes::update_recipes(&sync.advancements.recipes, &sync)
        });
        Ok(sync)
    }

    /// MinecraftServer.reloadResources: `/reload` keeps the synchronized
    /// registries the server started with (`kept`'s, its
    /// LayeredRegistryAccess) and reads the tags against them
    /// (TagLoader.loadTagsForExistingRegistries), the world's packs' as well
    /// (`overlay`); the rest stays reloaded.
    pub fn keep_registries(
        &mut self,
        resources: &Path,
        kept: &SyncData,
        overlay: &horizon_world::loot::PackOverlay,
    ) {
        self.dynamic = kept.dynamic.clone();
        self.registries = kept.registries.clone();
        let root = resources.join("data/minecraft");
        self.tags = Self::load_tags(&root, &self.builtin, &self.dynamic);
        Self::merge_pack_tags(&mut self.tags, &root, overlay, &self.builtin, &self.dynamic);
    }

    /// Whether a synchronized registry holds an entry from a world's pack.
    pub fn has_pack_entries(&self) -> bool {
        self.registries
            .iter()
            .any(|(_, e)| e.from_pack.contains(&true))
    }

    /// `LivingEntity.updateSynchronizedMobEffectParticles`: DATA_EFFECT_PARTICLES
    /// (10) and DATA_EFFECT_AMBIENCE_ID (11) for the visible `effects`, as
    /// (effect, ambient). The entries are left out when there is nothing to
    /// show, which is their default.
    pub fn effect_meta<'a>(
        &self,
        effects: impl IntoIterator<Item = (&'a str, bool)>,
    ) -> Vec<(u8, i32, entity_data::Value)> {
        let mut particles = Vec::new();
        let mut all_ambient = true;
        for (effect, ambient) in effects {
            let effect = effect.trim_start_matches("minecraft:");
            // MobEffect.particleFactory: the omens have their own particles,
            // the others an entity effect in their colour (faint if ambient).
            let (name, data) = match effect {
                "trial_omen" | "raid_omen" => (effect, None),
                _ => {
                    let Some(def) = crate::brewing::effect(effect) else {
                        continue;
                    };
                    let alpha = if ambient { 38 } else { 255 };
                    let argb = (alpha << 24) | def.shown_color() as i32;
                    (
                        "entity_effect",
                        Some(entity_data::ParticleData::Color(argb)),
                    )
                }
            };
            let Some(id) = self.id("minecraft:particle_type", &format!("minecraft:{name}")) else {
                continue;
            };
            particles.push(entity_data::Particle { id, data });
            all_ambient &= ambient;
        }
        if particles.is_empty() {
            return Vec::new();
        }
        let mut meta = vec![(
            10,
            entity_data::serializer::PARTICLES,
            entity_data::Value::Particles(particles),
        )];
        if all_ambient {
            meta.push((
                11,
                entity_data::serializer::BOOLEAN,
                entity_data::Value::Bool(true),
            ));
        }
        meta
    }

    /// The id of a name in a built-in or synchronized registry.
    pub fn id(&self, registry: &str, name: &str) -> Option<i32> {
        if registry == "minecraft:data_component_type"
            && self.dynamic.custom_components
            && let Some(id) = horizon_protocol::item::custom::id(name)
        {
            return Some(id);
        }
        if let Some(ids) = self.builtin.get(registry) {
            return ids.id(name);
        }
        self.dynamic
            .registries
            .get(registry)?
            .iter()
            .position(|(n, _)| n == name)
            .map(|i| i as i32)
    }

    /// ClientboundRegistryDataPacket per registry. With `known_pack` the
    /// client rebuilds the entries from its own data, so none is sent, the
    /// dialogs among them (`RegistrySynchronization.packRegistry`): only an
    /// entry a world's pack added or replaced (`from_pack`) carries its data.
    /// The registries for a client, with the data of a dimension type the
    /// world's data packs changed (`levels`) even when it knows the vanilla
    /// pack.
    pub fn registry_packets(&self, known_pack: bool, levels: &LevelShapes) -> Vec<RegistryData> {
        self.registries
            .iter()
            .map(|(key, e)| RegistryData {
                registry: key.clone(),
                entries: e
                    .names
                    .iter()
                    .zip(&e.data)
                    .zip(&e.from_pack)
                    .map(|((name, data), &from_pack)| {
                        let changed = (key == "minecraft:dimension_type")
                            .then(|| changed_dimension_type(name, data, levels))
                            .flatten();
                        RegistryEntry {
                            id: name.clone(),
                            data: match changed {
                                Some(changed) => Some(changed),
                                None if known_pack && !from_pack => None,
                                None => Some(data.clone()),
                            },
                        }
                    })
                    .collect(),
            })
            .collect()
    }
    fn merge_damage_tags(
        tags: &mut UpdateTags,
        resources: &Path,
        overlay: &horizon_world::loot::PackOverlay,
        builtin: &Builtin,
        dynamic: &Dynamic,
    ) -> Result<(), String> {
        let Some(names) = Self::registry_names(builtin, dynamic, "minecraft:damage_type") else {
            return Ok(());
        };
        let resolved = horizon_world::damage_source::resource_tags(resources, overlay, |id, _| {
            names.get(id).copied()
        })
        .map_err(|e| e.to_string())?;
        tags.registries
            .retain(|r| r.registry != "minecraft:damage_type");
        tags.registries.push(RegistryTags {
            registry: "minecraft:damage_type".into(),
            tags: resolved
                .into_iter()
                .map(|(name, ids)| TagEntry { name, ids })
                .collect(),
        });
        Ok(())
    }

    /// The tags the server sends (`TagNetworkSerialization.serializeTagsToNetwork`
    /// reads the registries' tags as `TagLoader` loaded them from the whole
    /// pack stack): every synchronized registry the world's packs have tag
    /// files for is merged again over the resource root's, and
    /// `minecraft:dialog` always (its tags name the packs' dialogs too).
    /// Without packs, the root's tags stand.
    fn merge_pack_tags(
        tags: &mut UpdateTags,
        root: &Path,
        overlay: &horizon_world::loot::PackOverlay,
        builtin: &Builtin,
        dynamic: &Dynamic,
    ) {
        let mut keys: Vec<&String> = builtin
            .registries
            .keys()
            .chain(dynamic.registries.keys())
            .collect();
        keys.sort();
        keys.dedup();
        for key in keys {
            // Registries.tagsDirPath: `tags/<path>`, a mod's registry under
            // `tags/<namespace>/<path>`.
            let kind = match key.split_once(':') {
                Some(("minecraft", path)) => path.to_owned(),
                Some((ns, path)) => format!("{ns}/{path}"),
                None => key.clone(),
            };
            if key != "minecraft:dialog" && overlay.tag_kind(&kind).next().is_none() {
                continue;
            }
            Self::merge_registry_tags(tags, root, overlay, builtin, dynamic, key, &kind);
        }
    }
    /// One registry's tags of the pack stack: the resource root's files, then
    /// each world pack's in order (`TagLoader`: a file adds its values, or
    /// replaces the earlier ones with `"replace": true`), nested tags expanded
    /// to the registry's ids.
    fn merge_registry_tags(
        tags: &mut UpdateTags,
        root: &Path,
        overlay: &horizon_world::loot::PackOverlay,
        builtin: &Builtin,
        dynamic: &Dynamic,
        key: &str,
        kind: &str,
    ) {
        let Some(names) = Self::registry_names(builtin, dynamic, key) else {
            return;
        };
        let mut files: BTreeMap<String, Vec<Value>> = BTreeMap::new();
        let mut own = BTreeMap::new();
        walk(&root.join("tags").join(kind), &mut own);
        for (name, value) in own {
            Self::add_tag_values(files.entry(qualified(&name)).or_default(), &value);
        }
        for (id, packs) in overlay.tag_kind(kind) {
            for bytes in packs {
                if let Ok(value) = serde_json::from_slice::<Value>(bytes) {
                    Self::add_tag_values(files.entry(id.clone()).or_default(), &value);
                }
            }
        }
        let mut entries = Vec::new();
        for name in files.keys() {
            let mut ids = Vec::new();
            let mut seen = HashSet::new();
            Self::expand_pack_tag(&files, name, &names, &mut ids, &mut seen);
            entries.push(TagEntry {
                name: name.clone(),
                ids,
            });
        }
        tags.registries.retain(|r| r.registry != key);
        if !entries.is_empty() {
            tags.registries.push(RegistryTags {
                registry: key.into(),
                tags: entries,
            });
        }
    }

    /// A tag file's values, after `"replace": true` empties the earlier ones.
    fn add_tag_values(entries: &mut Vec<Value>, json: &Value) {
        if json["replace"].as_bool() == Some(true) {
            entries.clear();
        }
        if let Some(values) = json["values"].as_array() {
            entries.extend(values.iter().cloned());
        }
    }

    /// A tag's entries as registry ids, nested tags expanded once.
    fn expand_pack_tag(
        files: &BTreeMap<String, Vec<Value>>,
        tag: &str,
        names: &HashMap<&str, i32>,
        out: &mut Vec<i32>,
        seen: &mut HashSet<String>,
    ) {
        if !seen.insert(tag.to_owned()) {
            return;
        }
        let Some(values) = files.get(tag) else {
            return;
        };
        for value in values {
            let id = value
                .as_str()
                .or_else(|| value["id"].as_str())
                .unwrap_or_default();
            if let Some(reference) = id.strip_prefix('#') {
                Self::expand_pack_tag(files, &qualified(reference), names, out, seen);
            } else if let Some(&n) = names.get(qualified(id).as_str())
                && !out.contains(&n)
            {
                out.push(n);
            }
        }
    }

    fn registry_names<'a>(
        builtin: &'a Builtin,
        dynamic: &'a Dynamic,
        key: &str,
    ) -> Option<HashMap<&'a str, i32>> {
        if let Some(ids) = builtin.get(key) {
            return Some(
                ids.names
                    .iter()
                    .enumerate()
                    .map(|(i, n)| (n.as_str(), i as i32))
                    .collect(),
            );
        }
        dynamic.registries.get(key).map(|v| {
            v.iter()
                .enumerate()
                .map(|(i, (n, _))| (n.as_str(), i as i32))
                .collect()
        })
    }

    /// TagNetworkSerialization.serializeTagsToNetwork from the tag files.
    fn load_tags(root: &Path, builtin: &Builtin, dynamic: &Dynamic) -> UpdateTags {
        let tag_root = root.join("tags");
        let mut registries = Vec::new();
        let mut keys: Vec<String> = builtin
            .registries
            .keys()
            .chain(dynamic.registries.keys())
            .cloned()
            .collect();
        keys.sort();
        for key in keys {
            let dir = tag_root.join(key.trim_start_matches("minecraft:"));
            if !dir.is_dir() {
                continue;
            }
            let mut files = BTreeMap::new();
            walk(&dir, &mut files);
            if files.is_empty() {
                continue;
            }
            let Some(names) = Self::registry_names(builtin, dynamic, &key) else {
                continue;
            };
            let mut tags = Vec::new();
            for name in files.keys() {
                let mut ids = Vec::new();
                let mut seen = HashSet::new();
                Self::expand(&files, name, &names, &mut ids, &mut seen);
                tags.push(TagEntry {
                    name: format!("minecraft:{name}"),
                    ids,
                });
            }
            registries.push(RegistryTags {
                registry: key,
                tags,
            });
        }
        UpdateTags { registries }
    }

    fn expand(
        files: &BTreeMap<String, Value>,
        tag: &str,
        names: &HashMap<&str, i32>,
        out: &mut Vec<i32>,
        seen: &mut HashSet<String>,
    ) {
        if !seen.insert(tag.to_owned()) {
            return;
        }
        let Some(values) = files.get(tag).and_then(|v| v["values"].as_array()) else {
            return;
        };
        for value in values {
            let id = value
                .as_str()
                .or_else(|| value["id"].as_str())
                .unwrap_or_default();
            if let Some(reference) = id.strip_prefix('#') {
                let reference = reference.trim_start_matches("minecraft:");
                Self::expand(files, reference, names, out, seen);
            } else if let Some(&n) = names.get(id)
                && !out.contains(&n)
            {
                out.push(n);
            }
        }
    }
}

/// The vanilla dimension type `name` (its NBT `data`) with the shape the
/// world gives the level of that name, if its data packs changed it.
fn changed_dimension_type(name: &str, data: &Tag, levels: &LevelShapes) -> Option<Tag> {
    if !matches!(
        name,
        "minecraft:overworld" | "minecraft:the_nether" | "minecraft:the_end"
    ) {
        return None;
    }
    let shape = levels.get(name);
    if shape == LevelShapes::VANILLA.get(name) {
        return None;
    }
    let mut data = data.clone();
    let Tag::Compound(fields) = &mut data else {
        return None;
    };
    fields.insert("min_y".into(), Tag::Int(shape.min_y));
    fields.insert("height".into(), Tag::Int(shape.height));
    fields.insert("logical_height".into(), Tag::Int(shape.logical_height));
    fields.insert("has_skylight".into(), Tag::Byte(i8::from(shape.sky_light)));
    fields.insert("has_ceiling".into(), Tag::Byte(i8::from(shape.has_ceiling)));
    Some(data)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "requires prepared vanilla resources"]
    fn datapack_damage_registry_reload_keeps_wire_ids_and_reloads_tags_before_future_join() {
        use horizon_world::loot::PackOverlay;
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../.local/horizon-client")
            .join(horizon_core::VANILLA_VERSION);
        let files = vec![
            ("damage_type/demo:heat".into(), vec![br#"{"message_id":"heat","scaling":"never","exhaustion":0.1,"effects":"burning"}"#.to_vec()]),
            ("damage_type/demo:infinite".into(), vec![br#"{"message_id":"infinite","scaling":"never","exhaustion":1e400}"#.to_vec()]),
            ("tags/minecraft:damage_type/is_fire".into(), vec![br#"{"values":["demo:heat"]}"#.to_vec()]),
        ];
        let open = SyncData::load(&root, &PackOverlay { files: &files }).unwrap();
        let heat = open.id("minecraft:damage_type", "demo:heat").unwrap();
        let packet = open.registry_packets(true, &LevelShapes::VANILLA);
        let packet = packet
            .iter()
            .find(|p| p.registry == "minecraft:damage_type")
            .unwrap();
        assert!(
            packet.entries[heat as usize].data.is_some(),
            "custom definitions are sent even with known vanilla packs"
        );
        let infinite = open.id("minecraft:damage_type", "demo:infinite").unwrap();
        let data = packet.entries[infinite as usize].data.as_ref().unwrap();
        assert_eq!(
            data.get("exhaustion"),
            Some(&Tag::Float(f32::INFINITY.to_bits()))
        );
        assert!(data.get("effects").is_none() && data.get("death_message_type").is_none());
        let changed = vec![
            (
                "damage_type/demo:heat".into(),
                vec![br#"{"invalid":true}"#.to_vec()],
            ),
            (
                "damage_type/demo:unregistered".into(),
                vec![br#"{"invalid":true}"#.to_vec()],
            ),
            (
                "tags/demo:damage_type/current".into(),
                vec![
                    br#"{"values":["demo:heat",{"id":"demo:unregistered","required":false}]}"#
                        .to_vec(),
                ],
            ),
        ];
        let reloaded = SyncData::reload(&root, &PackOverlay { files: &changed }, &open).unwrap();
        assert_eq!(
            reloaded.id("minecraft:damage_type", "demo:heat"),
            Some(heat)
        );
        assert!(
            reloaded
                .id("minecraft:damage_type", "demo:unregistered")
                .is_none()
        );
        let tags = reloaded
            .tags
            .registries
            .iter()
            .find(|r| r.registry == "minecraft:damage_type")
            .unwrap();
        assert_eq!(
            tags.tags
                .iter()
                .find(|t| t.name == "demo:current")
                .unwrap()
                .ids,
            [heat]
        );
        assert!(
            !tags
                .tags
                .iter()
                .find(|t| t.name == "minecraft:is_fire")
                .unwrap()
                .ids
                .contains(&heat)
        );
        // Another world performs registry loading again and refuses malformed definitions.
        assert!(SyncData::load(&root, &PackOverlay { files: &changed }).is_err());
        let fresh = SyncData::load(&root, &PackOverlay::NONE).unwrap();
        assert!(fresh.id("minecraft:damage_type", "demo:heat").is_none());
    }

    #[test]
    fn world_packs_supply_dialog_entries_and_tags() {
        use horizon_world::loot::PackOverlay;
        let vanilla = |p: &str| {
            (
                ("minecraft".to_owned(), p.to_owned()),
                serde_json::json!({ "type": "minecraft:dialog_list" }),
                false,
            )
        };
        let mut found = vec![vanilla("quick_actions"), vanilla("server_links")];
        let files = vec![
            (
                "dialog/test:menu".to_owned(),
                vec![br#"{"type":"minecraft:multi_action","title":"Menu"}"#.to_vec()],
            ),
            (
                "dialog/minecraft:quick_actions".to_owned(),
                vec![br#"{"type":"minecraft:dialog_list","title":"Quick"}"#.to_vec()],
            ),
            (
                "tags/minecraft:dialog/quick_actions".to_owned(),
                vec![br##"{"values":["test:menu","#minecraft:more"]}"##.to_vec()],
            ),
            (
                "tags/minecraft:dialog/more".to_owned(),
                vec![br#"{"values":["minecraft:server_links"]}"#.to_vec()],
            ),
        ];
        let overlay = PackOverlay { files: &files };
        pack_entries("dialog", &overlay, &mut found);
        file_order(&mut found);
        let names: Vec<String> = found
            .iter()
            .map(|((ns, p), _, _)| format!("{ns}:{p}"))
            .collect();
        // Identifier order: by path, then namespace.
        assert_eq!(
            names,
            [
                "test:menu",
                "minecraft:quick_actions",
                "minecraft:server_links"
            ]
        );
        assert_eq!(found[1].1["title"], "Quick", "a pack overrides an entry");
        let dynamic = Dynamic {
            registries: HashMap::from([(
                "minecraft:dialog".to_owned(),
                names.iter().map(|n| (n.clone(), None)).collect(),
            )]),
            ..Default::default()
        };
        let mut tags = UpdateTags {
            registries: Vec::new(),
        };
        SyncData::merge_pack_tags(
            &mut tags,
            Path::new("a-resource-root-without-tags"),
            &overlay,
            &Builtin::default(),
            &dynamic,
        );
        let dialog = tags
            .registries
            .iter()
            .find(|r| r.registry == "minecraft:dialog")
            .expect("the dialog tags are sent");
        let quick = dialog
            .tags
            .iter()
            .find(|t| t.name == "minecraft:quick_actions")
            .unwrap();
        assert_eq!(
            quick.ids,
            [0, 2],
            "the pack's dialog, then the nested tag's"
        );
    }

    /// #7843: the server sends its tags as `TagLoader` merged them from the
    /// whole pack stack. A world pack that adds dirt to `climbable` and
    /// replaces `mineable/pickaxe` reaches the block tags sent, over the
    /// resource root's; a nested tag expands; a registry no pack has tag files
    /// for keeps what `load_tags` read.
    #[test]
    fn world_packs_tags_reach_every_synchronized_registry() {
        let root = std::env::temp_dir().join(format!("horizon-pack-tags-{}", std::process::id()));
        let block_tags = root.join("tags/block");
        std::fs::create_dir_all(block_tags.join("mineable")).unwrap();
        std::fs::write(
            block_tags.join("climbable.json"),
            r#"{"values":["minecraft:ladder"]}"#,
        )
        .unwrap();
        std::fs::write(
            block_tags.join("mineable/pickaxe.json"),
            r#"{"values":["minecraft:stone"]}"#,
        )
        .unwrap();
        let names = |list: &[&str]| {
            horizon_protocol::registry::Ids::from_names(
                list.iter().map(|s| (*s).to_owned()).collect(),
            )
        };
        let builtin = Builtin {
            registries: HashMap::from([
                (
                    "minecraft:block".to_owned(),
                    names(&["minecraft:stone", "minecraft:dirt", "minecraft:ladder"]),
                ),
                ("minecraft:item".to_owned(), names(&["minecraft:stick"])),
            ]),
        };
        let dynamic = Dynamic::default();
        let mut tags = SyncData::load_tags(&root, &builtin, &dynamic);
        let item_tags = RegistryTags {
            registry: "minecraft:item".into(),
            tags: vec![TagEntry {
                name: "minecraft:sticks".into(),
                ids: vec![0],
            }],
        };
        tags.registries.push(item_tags.clone());
        let files = vec![
            (
                "tags/minecraft:block/climbable".to_owned(),
                vec![br##"{"values":["minecraft:dirt","#test:more"]}"##.to_vec()],
            ),
            (
                "tags/test:block/more".to_owned(),
                vec![br#"{"values":["minecraft:stone"]}"#.to_vec()],
            ),
            (
                "tags/minecraft:block/mineable/pickaxe".to_owned(),
                vec![br#"{"replace":true,"values":["minecraft:dirt"]}"#.to_vec()],
            ),
        ];
        let overlay = horizon_world::loot::PackOverlay { files: &files };
        SyncData::merge_pack_tags(&mut tags, &root, &overlay, &builtin, &dynamic);
        let _ = std::fs::remove_dir_all(&root);
        let sent = |registry: &str, tag: &str| -> Vec<i32> {
            tags.registries
                .iter()
                .find(|r| r.registry == registry)
                .and_then(|r| r.tags.iter().find(|t| t.name == tag))
                .map(|t| t.ids.clone())
                .unwrap_or_else(|| panic!("{registry} {tag} is sent"))
        };
        // The root's ladder, then the pack's dirt and its nested tag's stone.
        assert_eq!(sent("minecraft:block", "minecraft:climbable"), [2, 1, 0]);
        // `"replace": true` drops the root's stone.
        assert_eq!(sent("minecraft:block", "minecraft:mineable/pickaxe"), [1]);
        assert_eq!(sent("minecraft:block", "test:more"), [0]);
        // No pack tags for items: the registry stays as it was read.
        let items = tags
            .registries
            .iter()
            .find(|r| r.registry == "minecraft:item")
            .unwrap();
        assert_eq!(items.tags.len(), 1);
        assert_eq!(items.tags[0].ids, item_tags.tags[0].ids);
        assert_eq!(
            tags.registries
                .iter()
                .filter(|r| r.registry == "minecraft:block")
                .count(),
            1,
            "one entry per registry"
        );
    }

    /// RegistrySynchronization.packRegistry: a client that knows the core
    /// pack gets the vanilla dialogs without data; a pack's dialog has it.
    #[test]
    fn known_pack_dialogs_are_sent_without_data() {
        let mut sync = SyncData::empty();
        sync.registries.push((
            "minecraft:dialog".to_owned(),
            Entries {
                names: vec!["minecraft:quick_actions".into(), "test:menu".into()],
                data: vec![json_to_nbt(&serde_json::json!({ "type": "minecraft:dialog_list" })); 2],
                from_pack: vec![false, true],
            },
        ));
        let data = |known_pack| -> Vec<bool> {
            sync.registry_packets(known_pack, &LevelShapes::VANILLA)[0]
                .entries
                .iter()
                .map(|e| e.data.is_some())
                .collect()
        };
        assert_eq!(data(true), [false, true]);
        assert_eq!(data(false), [true, true]);
    }

    #[test]
    fn cached_dialog_nbt_keeps_the_existing_wire_payload() {
        let data = json_to_nbt(&serde_json::json!({"type":"minecraft:dialog_list"}));
        let mut sync = SyncData::empty();
        sync.registries.push((
            "minecraft:dialog".into(),
            Entries {
                names: vec!["minecraft:quick_actions".into()],
                data: vec![data],
                from_pack: vec![false],
            },
        ));
        let packets = sync.registry_packets(false, &LevelShapes::VANILLA);
        let data = packets[0].entries[0].data.as_ref().unwrap();
        let encoded =
            horizon_world::nbt::write_network(data, horizon_world::nbt::Limits::default()).unwrap();
        // Compound, string key/type, 21-byte value, compound end.
        let mut expected = vec![10, 8, 0, 4, b't', b'y', b'p', b'e', 0, 21];
        expected.extend_from_slice(b"minecraft:dialog_list");
        expected.push(0);
        assert_eq!(encoded, expected);
    }

    #[test]
    fn json_becomes_nbt() {
        let v: Value = serde_json::from_str(r#"{"a":1,"b":[true,2.5],"c":"x"}"#).unwrap();
        let t = json_to_nbt(&v);
        assert!(matches!(t.get("a"), Some(Tag::Int(1))));
        assert!(matches!(t.get("c"), Some(Tag::String(_))));
        assert!(matches!(t.get("b"), Some(Tag::List(l)) if l.len() == 2));
    }

    #[test]
    fn a_changed_dimension_type_is_sent_with_its_shape() {
        let vanilla: Value =
            serde_json::from_str(r#"{"min_y":-64,"height":384,"logical_height":384,"has_skylight":true,"ultrawarm":false}"#)
                .unwrap();
        let vanilla = json_to_nbt(&vanilla);
        let mut levels = LevelShapes::VANILLA;
        assert_eq!(
            changed_dimension_type("minecraft:overworld", &vanilla, &levels),
            None
        );
        levels.overworld = crate::chunk::LevelShape::new(-64, 2096, 2096, true, true).unwrap();
        let sent = changed_dimension_type("minecraft:overworld", &vanilla, &levels).unwrap();
        assert_eq!(
            (sent.get("min_y"), sent.get("height")),
            (Some(&Tag::Int(-64)), Some(&Tag::Int(2096)))
        );
        assert_eq!(sent.get("has_ceiling"), Some(&Tag::Byte(1)));
        assert_eq!(
            sent.get("ultrawarm"),
            Some(&Tag::Byte(0)),
            "the rest of the type is kept"
        );
        assert_eq!(
            changed_dimension_type("minecraft:the_nether", &vanilla, &levels),
            None
        );
        assert_eq!(
            changed_dimension_type("minecraft:overworld_caves", &vanilla, &levels),
            None
        );
    }
}

#[cfg(test)]
mod component_tests {
    use super::*;
    use horizon_world::nbt::{Limits, read_network, write_network};

    /// `/give`'s reply: a count, then components. On the wire each element is wrapped
    /// once, and read back (ListTag.addAndUnwrap) it is the number again.
    #[test]
    fn mixed_translation_arguments_are_wrapped_once() {
        let json = serde_json::json!({
            "translate": "commands.give.success.single",
            "with": [1, {"text": "Raw Iron"}, {"text": "Tester"}],
        });
        let tag = component_nbt(&json);
        let Some(Tag::List(args)) = tag.get("with") else {
            panic!("no arguments: {tag:?}");
        };
        assert_eq!(args[0], Tag::Int(1));
        // Read back as ListTag.addAndUnwrap does: a wrapper wrapped twice stays a compound.
        let bytes = write_network(&tag, Limits::default()).unwrap();
        assert_eq!(read_network(&bytes, Limits::default()).unwrap().0, tag);
    }
}
