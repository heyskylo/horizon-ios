//! `BlockStateData`: the table that maps the 1.12 block states (`blockId << 4 | data`)
//! to 1.13 block states, and the 1.12 `Name` + `Properties` tags to those ids.
//! The table is vanilla's `register` calls, one line each in call order, from the
//! prepared resources (`tables::tables().block_state_data`, exported from the
//! player's jar). A line is tab separated: the 1.12 id (`blockId << 4 | data`), the
//! 1.13 state's name and properties, then the name and properties of each 1.12 state
//! that maps to it; properties are `key=value` joined by `;`, sorted by key.
//!
//! Vanilla keeps one tag object per `register` call (and per defaulted id) and
//! `ChunkPalettedStorageFix` compares them by identity, so a state is an
//! *instance* here: `register` calls make instances, ids without a call share
//! the instance of the first call for their block.
use std::collections::HashMap;
use std::sync::OnceLock;

use super::dynamic::{as_str, key, string};
use crate::nbt::{Compound, Tag};

/// A block state: name and properties sorted by key.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct State {
    pub name: String,
    pub props: Vec<(String, String)>,
}

impl State {
    pub fn new(name: &str, props: &[(&str, &str)]) -> Self {
        let mut props: Vec<(String, String)> = props
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect();
        props.sort();
        Self {
            name: name.to_owned(),
            props,
        }
    }
    pub fn property(&self, name: &str) -> &str {
        self.props
            .iter()
            .find(|(k, _)| k == name)
            .map_or("", |(_, v)| v)
    }
    /// The NBT tag: `{Name}` or `{Name, Properties}`.
    pub fn to_tag(&self) -> Tag {
        let mut tag = Tag::compound([("Name", string(&self.name))]);
        if !self.props.is_empty() {
            let mut props = Compound::new();
            for (k, v) in &self.props {
                props.insert(key(k), string(v));
            }
            super::dynamic::set(&mut tag, "Properties", Tag::Compound(props));
        }
        tag
    }
    /// A tag read back as a state (`Name` and an optional `Properties` of strings only).
    pub fn from_tag(tag: &Tag) -> Option<Self> {
        let Tag::Compound(c) = tag else {
            return None;
        };
        let name = as_str(c.get(&key("Name")))?;
        let mut props = Vec::new();
        let mut expected = 1;
        if let Some(p) = c.get(&key("Properties")) {
            expected += 1;
            let Tag::Compound(p) = p else {
                return None;
            };
            for (k, v) in p {
                props.push((k.to_string_lossy(), as_str(Some(v))?));
            }
        }
        if c.len() != expected {
            return None;
        }
        props.sort();
        Some(Self { name, props })
    }
}

pub struct Data {
    /// One entry per `register` call.
    pub instances: Vec<State>,
    /// `MAP`: the instance of each id (after the defaulting of `finalizeMaps`).
    map: Vec<Option<u32>>,
    id_by_old: HashMap<State, i32>,
    id_by_old_name: HashMap<String, i32>,
}

fn parse_props(s: &str) -> Vec<(String, String)> {
    if s.is_empty() {
        return Vec::new();
    }
    s.split(';')
        .map(|kv| {
            let (k, v) = kv.split_once('=').unwrap_or((kv, ""));
            (k.to_owned(), v.to_owned())
        })
        .collect()
}

pub fn data() -> &'static Data {
    static DATA: OnceLock<Data> = OnceLock::new();
    DATA.get_or_init(|| {
        let mut map: Vec<Option<u32>> = vec![None; 4096];
        let mut defaults: Vec<Option<u32>> = vec![None; 256];
        let mut instances = Vec::new();
        let mut id_by_old = HashMap::new();
        let mut id_by_old_name: HashMap<String, i32> = HashMap::new();
        for line in super::tables::tables().block_state_data {
            let f: Vec<&str> = line.split('\t').collect();
            let id: usize = f[0].parse().expect("id");
            let instance = instances.len() as u32;
            instances.push(State {
                name: f[1].to_owned(),
                props: parse_props(f.get(2).copied().unwrap_or("")),
            });
            map[id] = Some(instance);
            if defaults[id >> 4].is_none() {
                defaults[id >> 4] = Some(instance);
            }
            for pair in f[3..].chunks(2) {
                let old = State {
                    name: pair[0].to_owned(),
                    props: parse_props(pair.get(1).copied().unwrap_or("")),
                };
                id_by_old_name.entry(old.name.clone()).or_insert(id as i32);
                id_by_old.insert(old, id as i32);
            }
        }
        for (i, slot) in map.iter_mut().enumerate() {
            if slot.is_none() {
                *slot = defaults[i >> 4];
            }
        }
        Data {
            instances,
            map,
            id_by_old,
            id_by_old_name,
        }
    })
}

impl Data {
    /// `getTag(id)` as an instance.
    pub fn instance(&self, id: i32) -> u32 {
        let slot = usize::try_from(id)
            .ok()
            .filter(|i| *i < 4096)
            .and_then(|i| self.map[i]);
        slot.or(self.map[0]).expect("air is registered")
    }
    pub fn state(&self, instance: u32) -> &State {
        &self.instances[instance as usize]
    }
}

/// `BlockStateData.getTag`.
pub fn get_tag(id: i32) -> Tag {
    let d = data();
    d.state(d.instance(id)).to_tag()
}

/// `BlockStateData.upgradeBlockStateTag`.
pub fn upgrade_block_state_tag(old: &Tag) -> Tag {
    let d = data();
    match State::from_tag(old).and_then(|s| d.id_by_old.get(&s).copied()) {
        Some(id) if (0..4096).contains(&id) => match d.map[id as usize] {
            Some(i) => d.state(i).to_tag(),
            None => old.clone(),
        },
        _ => old.clone(),
    }
}

/// `BlockStateData.upgradeBlock(String)`.
pub fn upgrade_block_name(old: &str) -> String {
    let d = data();
    match d.id_by_old_name.get(old) {
        Some(&id) if (0..4096).contains(&id) => match d.map[id as usize] {
            Some(i) => d.state(i).name.clone(),
            None => old.to_owned(),
        },
        _ => old.to_owned(),
    }
}

/// `BlockStateData.upgradeBlock(int)`.
pub fn upgrade_block_id(id: i32) -> String {
    let d = data();
    match usize::try_from(id).ok().filter(|i| *i < 4096) {
        Some(i) => match d.map[i] {
            Some(inst) => d.state(inst).name.clone(),
            None => "minecraft:air".to_owned(),
        },
        None => "minecraft:air".to_owned(),
    }
}
