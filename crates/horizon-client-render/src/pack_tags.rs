//! Tags as resource packs' files name them (`#minecraft:logs`): the ids
//! `data/<namespace>/tags/<registry>/<path>.json` holds, nested tags included, read from a pack
//! overlay root (which links the vanilla data) and kept once read.
use serde_json::Value as Json;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

/// `minecraft:` for an id without a namespace.
fn full_id(id: &str) -> String {
    if id.contains(':') {
        id.to_owned()
    } else {
        format!("minecraft:{id}")
    }
}

/// The tags under one root, read on first use.
#[derive(Debug)]
pub struct Tags {
    root: PathBuf,
    cache: Mutex<HashMap<(String, String), Arc<HashSet<String>>>>,
}

impl Tags {
    pub fn new(root: &Path) -> Self {
        Self {
            root: root.to_path_buf(),
            cache: Mutex::default(),
        }
    }

    /// The ids in `tag` (`minecraft:logs`, without the `#`) of `registry` (`block`,
    /// `entity_type`, `worldgen/biome`...); empty for a tag no file defines.
    pub fn members(&self, registry: &str, tag: &str) -> Arc<HashSet<String>> {
        let key = (registry.to_owned(), full_id(tag));
        if let Some(hit) = self
            .cache
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(&key)
        {
            return hit.clone();
        }
        let mut out = HashSet::new();
        self.collect(registry, &key.1, &mut out, &mut HashSet::new());
        let out = Arc::new(out);
        self.cache
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(key, out.clone());
        out
    }

    /// Whether `id` is in `tag` of `registry`.
    pub fn contains(&self, registry: &str, tag: &str, id: &str) -> bool {
        self.members(registry, tag).contains(id)
    }

    /// The tag's ids into `out`; `seen` stops a tag that includes itself.
    fn collect(
        &self,
        registry: &str,
        tag: &str,
        out: &mut HashSet<String>,
        seen: &mut HashSet<String>,
    ) {
        if !seen.insert(tag.to_owned()) {
            return;
        }
        let (namespace, path) = tag.split_once(':').unwrap_or(("minecraft", tag));
        let file = self
            .root
            .join(format!("data/{namespace}/tags/{registry}/{path}.json"));
        let Ok(json) = horizon_world::json_file::read::<Json>(&file) else {
            return;
        };
        for value in json["values"].as_array().into_iter().flatten() {
            // An entry is an id or `{"id": ..., "required": false}`.
            let id = value
                .as_str()
                .or_else(|| value["id"].as_str())
                .unwrap_or_default();
            match id.strip_prefix('#') {
                Some(inner) => self.collect(registry, &full_id(inner), out, seen),
                None if !id.is_empty() => {
                    out.insert(full_id(id));
                }
                None => {}
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nested_tags_optional_entries_and_cycles_resolve() {
        static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let root =
            std::env::temp_dir().join(format!("horizon-pack-tags-{}-{n}", std::process::id()));
        let write = |path: &str, json: serde_json::Value| {
            let file = root.join(path);
            std::fs::create_dir_all(file.parent().unwrap()).unwrap();
            std::fs::write(file, json.to_string()).unwrap();
        };
        write(
            "data/minecraft/tags/block/logs.json",
            serde_json::json!({"values": ["oak_log", "#pack:fancy", {"id": "mod:log", "required": false}]}),
        );
        // A tag that includes the one including it.
        write(
            "data/pack/tags/block/fancy.json",
            serde_json::json!({"values": ["minecraft:cherry_log", "#minecraft:logs"]}),
        );
        let tags = Tags::new(&root);
        let mut logs: Vec<String> = tags.members("block", "logs").iter().cloned().collect();
        logs.sort();
        assert_eq!(
            logs,
            ["minecraft:cherry_log", "minecraft:oak_log", "mod:log"]
        );
        assert!(tags.contains("block", "minecraft:logs", "minecraft:oak_log"));
        // Another registry's tag of the same name, and a tag no file defines, are empty.
        assert!(tags.members("entity_type", "logs").is_empty());
        assert!(tags.members("block", "pack:none").is_empty());
        std::fs::remove_dir_all(root).unwrap();
    }
}
