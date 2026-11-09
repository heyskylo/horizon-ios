//! Runtime block-state IDs imported from a version-pinned registry data export.
//! This supplies identities and default properties, not block behavior.
mod facts;
mod json;
pub(crate) mod snapshot;
use crate::{DATA_VERSION, chunk::BlockState, compression::invalid, nbt::Tag};
use horizon_core::fast_hash::FxHashMap;
pub use json::BlockStateJsonCodec;
pub(crate) use json::java_integer;
use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    hash::{DefaultHasher, Hash, Hasher},
    io,
    sync::{
        RwLock,
        atomic::{AtomicUsize, Ordering},
    },
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct BlockStateId(pub u32);

pub struct BlockRegistry {
    states: Vec<BlockState>,
    /// Ids by a hash of their state, checked against `states`: 12 bytes per
    /// state instead of a second copy of every name and property map (about
    /// 125 MB for a registry of 190,000 states).
    ids: HashMap<u64, u32>,
    /// States whose hash an earlier state already took (none in vanilla).
    collided: BTreeMap<BlockState, BlockStateId>,
    defaults: BTreeMap<String, BlockStateId>,
    /// Saved palette entries by the NBT they came from ([`Self::saved_state_id`]).
    /// A saved area repeats the same dozen states in every section of every
    /// column; this outlives one column, unlike the cache it replaces.
    saved_states: RwLock<FxHashMap<Tag, Option<BlockStateId>>>,
}
// Bound retained cache entries across all registries, even registries leaked by
// asset loading. Slots are not reclaimed when a registry is dropped.
static SAVED_STATE_SLOTS: AtomicUsize = AtomicUsize::new(0);

fn reserve_saved_state_slot() -> bool {
    let mut count = SAVED_STATE_SLOTS.load(Ordering::Relaxed);
    while count < 8192 {
        match SAVED_STATE_SLOTS.compare_exchange_weak(
            count,
            count + 1,
            Ordering::Relaxed,
            Ordering::Relaxed,
        ) {
            Ok(_) => return true,
            Err(actual) => count = actual,
        }
    }
    false
}

// Bound both the count and size of cached keys. Unusual or oversized NBT is
// resolved normally without retaining it for the lifetime of the registry.
fn cacheable_saved_state(tag: &Tag) -> bool {
    fn short_string(tag: &Tag) -> bool {
        matches!(tag, Tag::String(s) if s.0.len() <= 128)
    }
    match tag {
        Tag::String(_) => short_string(tag),
        Tag::Compound(map) if map.len() <= 2 => map.iter().all(|(key, value)| {
            if key.0.iter().copied().eq("id".encode_utf16()) {
                short_string(value)
            } else if key.0.iter().copied().eq("properties".encode_utf16()) {
                matches!(value, Tag::Compound(props) if props.len() <= 16 && props.iter().all(|(k, v)| k.0.len() <= 128 && short_string(v)))
            } else {
                false
            }
        }),
        _ => false,
    }
}
/// The hash [`BlockRegistry`] indexes states by (fixed keys: the same in
/// every process).
fn state_key(state: &BlockState) -> u64 {
    let mut hasher = DefaultHasher::new();
    state.name.hash(&mut hasher);
    for (key, value) in &state.properties {
        key.hash(&mut hasher);
        value.hash(&mut hasher);
    }
    hasher.finish()
}
impl BlockRegistry {
    pub fn from_nbt(tag: &Tag) -> io::Result<Self> {
        if tag.get("DataVersion") != Some(&Tag::Int(DATA_VERSION)) {
            return Err(invalid("block registry DataVersion mismatch"));
        }
        let Some(Tag::List(entries)) = tag.get("states") else {
            return Err(invalid("missing registry states"));
        };
        let Some(Tag::Compound(defaults)) = tag.get("defaults") else {
            return Err(invalid("missing registry defaults"));
        };
        let states = entries
            .iter()
            .map(BlockState::from_nbt)
            .collect::<io::Result<_>>()?;
        let defaults = defaults
            .iter()
            .map(|(name, id)| {
                let name = name.to_string().map_err(|e| invalid(e.to_string()))?;
                let Tag::Int(id) = id else {
                    return Err(invalid("default state ID is not an int"));
                };
                Ok((name, i64::from(*id)))
            })
            .collect::<io::Result<_>>()?;
        Self::from_states(states, defaults)
    }

    /// The registry of a resources `facts.json` (`{"states": [[..., ..., ...,
    /// ..., state], ...], "defaults": {name: id}}`), read as it streams: no
    /// JSON or NBT tree of the whole file is built. For a pack of 190,000
    /// states the two trees were several hundred megabytes at startup.
    pub fn from_facts_json(reader: impl io::Read) -> io::Result<Self> {
        Ok(Self::from_facts_json_with_flags(reader)?.0)
    }

    /// [`Self::from_facts_json`], with each state's `(air, fluid)` columns.
    pub fn from_facts_json_with_flags(
        reader: impl io::Read,
    ) -> io::Result<(Self, Vec<(bool, bool)>)> {
        let facts: facts::Facts =
            facts::with_shared_strings(|| serde_json::from_reader(io::BufReader::new(reader)))
                .map_err(|e| invalid(e.to_string()))?;
        Self::from_facts(facts)
    }

    /// [`Self::from_facts_json_with_flags`] over bytes already in memory.
    ///
    /// `serde_json` cannot borrow out of a reader, so `from_reader` copies
    /// every string it parses; over one slice it borrows instead. The facts
    /// are about 10 MB and sit at the head of the start, before anything
    /// else in the core bundle can begin, so the difference is on the
    /// critical path.
    pub fn from_facts_json_bytes(bytes: &[u8]) -> io::Result<(Self, Vec<(bool, bool)>)> {
        let facts: facts::Facts = facts::with_shared_strings(|| serde_json::from_slice(bytes))
            .map_err(|e| invalid(e.to_string()))?;
        Self::from_facts(facts)
    }

    fn from_facts(facts: facts::Facts) -> io::Result<(Self, Vec<(bool, bool)>)> {
        let flags = facts.states.iter().map(|r| (r.air, r.fluid)).collect();
        let states = facts.states.into_iter().map(|r| r.state).collect();
        Ok((Self::from_states(states, facts.defaults)?, flags))
    }

    fn from_states(states: Vec<BlockState>, defaults: BTreeMap<String, i64>) -> io::Result<Self> {
        if states.is_empty() || states.len() > i32::MAX as usize {
            return Err(invalid("invalid registry size"));
        }
        let mut result = Self {
            states: Vec::with_capacity(states.len()),
            ids: HashMap::with_capacity(states.len()),
            collided: BTreeMap::new(),
            defaults: BTreeMap::new(),
            saved_states: RwLock::new(FxHashMap::default()),
        };
        // The states' property names and values: a few hundred strings, shared.
        let mut strings = crate::block_state_properties::SharedStrings::default();
        for (i, mut state) in states.into_iter().enumerate() {
            state.properties.share(&mut strings);
            if result.id(&state).is_some() {
                return Err(invalid("duplicate block registry state"));
            }
            match result.ids.entry(state_key(&state)) {
                std::collections::hash_map::Entry::Vacant(slot) => {
                    slot.insert(i as u32);
                }
                std::collections::hash_map::Entry::Occupied(_) => {
                    result
                        .collided
                        .insert(state.clone(), BlockStateId(i as u32));
                }
            }
            result.states.push(state);
        }
        for (name, id) in defaults {
            if id < 0
                || id > i64::from(i32::MAX)
                || result
                    .states
                    .get(id as usize)
                    .is_none_or(|s| s.name != name)
            {
                return Err(invalid("invalid default block state ID"));
            }
            result.defaults.insert(name, BlockStateId(id as u32));
        }
        let names: BTreeSet<_> = result.states.iter().map(|s| &s.name).collect();
        if names.len() != result.defaults.len()
            || names
                .iter()
                .any(|name| !result.defaults.contains_key(*name))
        {
            return Err(invalid("registry is missing block defaults"));
        }
        Ok(result)
    }
    pub fn len(&self) -> usize {
        self.states.len()
    }
    pub fn is_empty(&self) -> bool {
        self.states.is_empty()
    }
    pub fn state(&self, id: BlockStateId) -> Option<&BlockState> {
        self.states.get(id.0 as usize)
    }
    pub fn id(&self, state: &BlockState) -> Option<BlockStateId> {
        match self.ids.get(&state_key(state)) {
            Some(&id) if self.states[id as usize] == *state => Some(BlockStateId(id)),
            _ => self.collided.get(state).copied(),
        }
    }

    /// The id a saved palette entry resolves to, with `decode_column`'s rules:
    /// a missing or unknown property takes the block's default, an entry that
    /// does not parse at all is air (`None`). Memoized by the entry's own NBT,
    /// so a repeated state costs one hash of its name and properties and one
    /// comparison, with no `String`, no `Debug` and no UTF-16 decoding.
    ///
    /// Equal entries always resolve alike; entries the old parsed-state cache
    /// would have merged but that differ in NBT (an ignored extra key, or two
    /// spellings of one string) simply get their own entries with the same id.
    pub fn saved_state_id(&self, tag: &Tag) -> Option<BlockStateId> {
        let cacheable = cacheable_saved_state(tag);
        if cacheable
            && let Some(&hit) = self
                .saved_states
                .read()
                .unwrap_or_else(|e| e.into_inner())
                .get(tag)
        {
            return hit;
        }
        let id = BlockState::from_nbt(tag).ok().and_then(|state| {
            self.id(&state).or_else(|| {
                // Missing properties take the block's defaults.
                let default = self.default_state(&state.name)?;
                let mut full = self.state(default)?.clone();
                for (key, value) in &state.properties {
                    if full.properties.contains_key(key) {
                        full.properties.insert(key.clone(), value.clone());
                    }
                }
                self.id(&full).or(Some(default))
            })
        });
        if cacheable {
            let mut cache = self.saved_states.write().unwrap_or_else(|e| e.into_inner());
            if !cache.contains_key(tag) && reserve_saved_state_slot() {
                cache.insert(tag.clone(), id);
            }
        }
        id
    }
    pub fn default_state(&self, name: &str) -> Option<BlockStateId> {
        self.defaults.get(name).copied()
    }

    /// The values `property` takes on `block`, in `Property.getPossibleValues`
    /// order. The state list is in `StateDefinition` order (properties sorted
    /// by name, values in their own order), so a value's first appearance is
    /// that order.
    pub fn property_values(&self, block: &str, property: &str) -> Vec<&str> {
        let mut values: Vec<&str> = Vec::new();
        for state in self
            .states
            .iter()
            .filter(|state| state.name.as_str() == block)
        {
            if let Some(value) = state.properties.get(property) {
                let value = value.as_str();
                if !values.contains(&value) {
                    values.push(value);
                }
            }
        }
        values
    }

    /// Resolve a saved descriptor, supplying omitted default properties. Unlike
    /// vanilla's lenient logging paths, invalid properties return a typed error.
    pub fn resolve(&self, descriptor: &BlockState) -> io::Result<BlockStateId> {
        let default = self
            .default_state(&descriptor.name)
            .ok_or_else(|| invalid("unknown block name"))?;
        let mut state = self.state(default).unwrap().clone();
        for (key, value) in &descriptor.properties {
            if !state.properties.contains_key(key) {
                return Err(invalid("unknown block property"));
            }
            state.properties.insert(key.clone(), value.clone());
        }
        self.id(&state)
            .ok_or_else(|| invalid("invalid block property value"))
    }
}

/// A per-state table (`occlusion.nbt`, `cached-collision.nbt`, ...) whose
/// `states` list, the identity of every state it describes in id order, has
/// been checked against `registry`.
#[derive(Clone, Copy)]
pub struct BlockTable<'t, 'r> {
    tag: &'t Tag,
    registry: &'r BlockRegistry,
    /// How many levels, from this one down, [`BlockTableFile::read`] checked
    /// while reading: their `states` lists are kept empty in the tree.
    read_checked: u8,
}
impl<'t, 'r> BlockTable<'t, 'r> {
    /// Checks that `tag`'s `states` names the registry's states in id order.
    pub fn check(tag: &'t Tag, registry: &'r BlockRegistry, what: &str) -> io::Result<Self> {
        Self {
            tag,
            registry,
            read_checked: 0,
        }
        .checked(what)
    }
    fn checked(self, what: &str) -> io::Result<Self> {
        let Some(Tag::List(states)) = self.tag.get("states") else {
            return Err(invalid(format!("missing {what} state identities")));
        };
        if self.read_checked == 0 {
            if states.len() != self.registry.len() {
                return Err(invalid(format!("{what} registry size mismatch")));
            }
            for (id, state) in states.iter().enumerate() {
                if self.registry.state(BlockStateId(id as u32))
                    != Some(&BlockState::from_nbt(state)?)
                {
                    return Err(invalid(format!("{what} state identities differ")));
                }
            }
        }
        Ok(self)
    }
    /// The table nested under `key` (`block-properties.nbt`'s `occlusion`),
    /// with its own `states`, checked the same way.
    pub fn nested(&self, key: &str, what: &str) -> io::Result<Self> {
        Self {
            tag: self
                .tag
                .get(key)
                .ok_or_else(|| invalid(format!("missing {what}")))?,
            registry: self.registry,
            read_checked: self.read_checked.saturating_sub(1),
        }
        .checked(what)
    }
    pub fn tag(&self) -> &'t Tag {
        self.tag
    }
    pub fn get(&self, key: &str) -> Option<&'t Tag> {
        self.tag.get(key)
    }
    pub fn registry(&self) -> &'r BlockRegistry {
        self.registry
    }
}

/// A per-state table read from its file with its state identities checked
/// while reading, so they never exist as a tree: for 190,000 states (a big
/// modpack) that tree took 300-600 MB per table.
pub struct BlockTableFile<'r> {
    tag: Tag,
    registry: &'r BlockRegistry,
}
impl<'r> BlockTableFile<'r> {
    /// [`Self::read`] of the file at `path`.
    pub fn open(path: &std::path::Path, registry: &'r BlockRegistry) -> io::Result<Self> {
        let bytes = std::fs::read(path)?;
        Self::read(&bytes, registry).map_err(|e| invalid(format!("{}: {e}", path.display())))
    }
    /// Reads `bytes`, checking every `states` list of the root compound and
    /// of the compounds directly under it against `registry`.
    pub fn read(bytes: &[u8], registry: &'r BlockRegistry) -> io::Result<Self> {
        let mut visit = |states: &mut crate::nbt::ListElements<'_, '_, '_>| {
            if states.len() != registry.len() {
                return Err(states.error("table registry size mismatch"));
            }
            let mut id = 0u32;
            while let Some(tag) = states.next_element()? {
                let state =
                    BlockState::from_nbt(&tag).map_err(|_| states.error("invalid table state"))?;
                if registry.state(BlockStateId(id)) != Some(&state) {
                    return Err(states.error("table state identities differ"));
                }
                id += 1;
            }
            Ok(())
        };
        let (named, _) = crate::nbt::read_named_streaming(
            bytes,
            crate::nbt::Limits::tables(),
            "states",
            &mut visit,
        )
        .map_err(|e| invalid(e.to_string()))?;
        Ok(Self {
            tag: named.tag,
            registry,
        })
    }
    /// The table, checked (`what` names it in errors).
    pub fn table(&self, what: &str) -> io::Result<BlockTable<'_, 'r>> {
        BlockTable {
            tag: &self.tag,
            registry: self.registry,
            read_checked: 2,
        }
        .checked(what)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::nbt::{Compound, JavaString};

    fn state(name: &str, properties: &[(&str, &str)]) -> Tag {
        let mut map = Compound::new();
        map.insert(JavaString::from("id"), Tag::String(name.into()));
        if !properties.is_empty() {
            let mut props = Compound::new();
            for (key, value) in properties {
                props.insert(JavaString::from(*key), Tag::String((*value).into()));
            }
            map.insert(JavaString::from("properties"), Tag::Compound(props));
        }
        Tag::Compound(map)
    }

    /// A registry with a defaulted block, a block with one property and one
    /// with two.
    fn registry() -> BlockRegistry {
        let states = [
            state("minecraft:air", &[]),
            state("minecraft:stone", &[]),
            state("test:log", &[("axis", "x")]),
            state("test:log", &[("axis", "y")]),
            state("test:log", &[("axis", "z")]),
            state("test:door", &[("facing", "north"), ("open", "false")]),
            state("test:door", &[("facing", "east"), ("open", "false")]),
            state("test:door", &[("facing", "north"), ("open", "true")]),
        ];
        let defaults = [
            ("minecraft:air", 0),
            ("minecraft:stone", 1),
            ("test:log", 3),
            ("test:door", 5),
        ];
        BlockRegistry::from_nbt(&Tag::compound([
            ("DataVersion", Tag::Int(crate::DATA_VERSION)),
            ("states", Tag::List(states.into())),
            (
                "defaults",
                Tag::compound(defaults.into_iter().map(|(name, id)| (name, Tag::Int(id)))),
            ),
        ]))
        .unwrap()
    }

    fn materialized(name: &str, properties: &[(&str, Tag)]) -> Tag {
        let mut map = Compound::new();
        map.insert(JavaString::from("id"), Tag::String(name.into()));
        if !properties.is_empty() {
            let mut props = Compound::new();
            for (key, value) in properties {
                props.insert(JavaString::from(*key), value.clone());
            }
            map.insert(JavaString::from("properties"), Tag::Compound(props));
        }
        Tag::Compound(map)
    }

    /// `decode_column`'s per-column cache and resolution, unchanged, as the
    /// reference the new path must agree with.
    fn old_state_id(
        registry: &BlockRegistry,
        tag: &Tag,
        cache: &mut HashMap<String, u32>,
        air: u32,
    ) -> u32 {
        let state = BlockState::from_nbt(tag).ok();
        let key = state
            .as_ref()
            .map(|s| format!("{}{:?}", s.name, s.properties))
            .unwrap_or_default();
        *cache.entry(key).or_insert_with(|| {
            state
                .and_then(|s| {
                    registry.id(&s).or_else(|| {
                        let default = registry.default_state(&s.name)?;
                        let mut full = registry.state(default)?.clone();
                        for (k, v) in &s.properties {
                            if full.properties.contains_key(k) {
                                full.properties.insert(k.clone(), v.clone());
                            }
                        }
                        registry.id(&full).or(Some(default))
                    })
                })
                .map_or(air, |id| id.0)
        })
    }

    #[test]
    fn saved_palette_entries_resolve_as_the_per_column_cache_did() {
        let registry = registry();
        let air = registry.default_state("minecraft:air").unwrap().0;
        let tags = [
            // Known states, with and without the properties they carry.
            state("minecraft:stone", &[]),
            Tag::String("minecraft:stone".into()),
            state("test:log", &[("axis", "z")]),
            state("test:log", &[]),
            state("test:door", &[("facing", "north"), ("open", "false")]),
            state("test:door", &[("open", "true"), ("facing", "north")]),
            state("test:door", &[("open", "true")]),
            state("test:door", &[("facing", "east")]),
            // Missing properties take the defaults, unknown names are ignored,
            // an unknown value falls back to the default state, and an entry
            // with an ignored extra key still has its own cache slot.
            state("test:log", &[("bogus", "x")]),
            state("test:log", &[("axis", "bogus")]),
            state("test:log", &[("axis", "")]),
            state("minecraft:stone", &[("bogus", "x")]),
            materialized("test:log", &[]),
            Tag::compound([
                ("id", Tag::String("minecraft:stone".into())),
                ("extra", Tag::Int(7)),
            ]),
            // Entries `from_nbt` rejects, which resolve to air.
            Tag::compound([("Name", Tag::String("minecraft:stone".into()))]),
            Tag::compound([("id", Tag::Int(1))]),
            Tag::compound([
                ("id", Tag::String("test:log".into())),
                ("properties", Tag::String("axis=y".into())),
            ]),
            materialized("test:log", &[("axis", Tag::Int(5))]),
            Tag::String(JavaString(vec![0xd800])),
            Tag::Compound(Compound::new()),
            // An unknown block name.
            state("minecraft:nope", &[]),
        ];
        // The old cache was per column; the new one lives in the registry, so
        // repeat the whole palette (a column's worth of entries) three times.
        for round in 0..3 {
            let mut cache = HashMap::new();
            for (i, tag) in tags.iter().enumerate() {
                let new = registry.saved_state_id(tag).map_or(air, |id| id.0);
                let old = old_state_id(&registry, tag, &mut cache, air);
                assert_eq!(new, old, "round {round} entry {i}: {tag:?}");
            }
        }
        // A few ids stated independently of both implementations.
        let id = |tag: &Tag| registry.saved_state_id(tag).map_or(air, |id| id.0);
        assert_eq!(id(&state("minecraft:stone", &[])), 1);
        assert_eq!(id(&Tag::String("minecraft:stone".into())), 1);
        assert_eq!(id(&state("test:log", &[("axis", "z")])), 4);
        assert_eq!(id(&state("test:log", &[])), 3);
        assert_eq!(id(&state("test:log", &[("axis", "bogus")])), 3);
        assert_eq!(id(&state("test:door", &[("open", "true")])), 7);
        assert_eq!(id(&state("test:door", &[("facing", "east")])), 6);
        assert_eq!(id(&Tag::String(JavaString(vec![0xd800]))), air);
    }
    #[test]
    fn saved_cache_is_bounded_and_registry_local_under_concurrency() {
        let registry = registry();
        std::thread::scope(|scope| {
            for _ in 0..4 {
                scope.spawn(|| {
                    for i in 0..8300 {
                        let name = format!("unknown:{i}");
                        assert_eq!(registry.saved_state_id(&state(&name, &[])), None);
                    }
                    assert_eq!(
                        registry.saved_state_id(&state("test:log", &[])),
                        Some(BlockStateId(3))
                    );
                });
            }
        });
        assert!(registry.saved_states.read().unwrap().len() <= 8192);
        assert!(SAVED_STATE_SLOTS.load(Ordering::Relaxed) <= 8192);
        let oversized = state(&"x".repeat(129), &[]);
        assert!(!cacheable_saved_state(&oversized));
        let other = BlockRegistry::from_nbt(&Tag::compound([
            ("DataVersion", Tag::Int(crate::DATA_VERSION)),
            ("states", Tag::List(vec![state("test:log", &[])])),
            ("defaults", Tag::compound([("test:log", Tag::Int(0))])),
        ]))
        .unwrap();
        assert_eq!(
            other.saved_state_id(&state("test:log", &[])),
            Some(BlockStateId(0))
        );
    }
}
