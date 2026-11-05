//! Filesystem/JSON half of particle resource reloads. Preparing this value on
//! the pack worker lets the frame owner install it without reading pack files.

use super::{ModBehaviour, ParticleType};
use std::path::Path;

#[derive(Clone, Default)]
pub struct PreparedParticles {
    pub(super) vanilla: Vec<(ParticleType, Vec<String>)>,
    pub(super) mods: Vec<PreparedModParticle>,
}
#[derive(Clone)]
pub(super) struct PreparedModParticle {
    pub(super) key: String,
    pub(super) textures: Vec<String>,
    pub(super) behaviour: Option<ModBehaviour>,
}

impl PreparedParticles {
    /// Parse the resource generation without touching a live particle engine.
    pub fn load(root: &Path) -> Self {
        let mut prepared = Self::default();
        let assets = root.join("assets");
        if let Ok(entries) = std::fs::read_dir(&assets) {
            let mut namespaces: Vec<_> = entries.flatten().collect();
            namespaces.sort_by_key(|entry| entry.file_name());
            for namespace in namespaces {
                let name = namespace.file_name();
                let Some(ns) = name.to_str() else { continue };
                if ns == "minecraft" || !namespace.path().is_dir() {
                    continue;
                }
                prepared.load_mod_sprites(ns, &namespace.path().join("particles"));
            }
        }
        let dir = assets.join("minecraft/particles");
        for &ty in ParticleType::ALL {
            let Ok(bytes) =
                horizon_core::prepared_files::read(&dir.join(format!("{}.json", ty.name())))
            else {
                continue;
            };
            let Ok(json) = serde_json::from_slice::<serde_json::Value>(&bytes) else {
                continue;
            };
            let textures = texture_names(&json);
            if !textures.is_empty() {
                prepared.vanilla.push((ty, textures));
            }
        }
        prepared
    }

    /// A resource path includes its folders: `particles/trails/spark.json`
    /// belongs to `<ns>:trails/spark`, not `<ns>:spark`.
    fn load_mod_sprites(&mut self, ns: &str, root: &Path) {
        let mut pending = vec![(root.to_path_buf(), String::new())];
        while let Some((dir, prefix)) = pending.pop() {
            let Ok(entries) = std::fs::read_dir(dir) else {
                continue;
            };
            let mut entries: Vec<_> = entries.flatten().collect();
            entries.sort_by_key(|entry| entry.file_name());
            for entry in entries {
                let path = entry.path();
                let Ok(kind) = entry.file_type() else {
                    continue;
                };
                let name = entry.file_name();
                let Some(name) = name.to_str() else { continue };
                if kind.is_dir() {
                    pending.push((path, format!("{prefix}{name}/")));
                    continue;
                }
                // Do not recurse through directory symlinks (including cycles),
                // while ordinary file symlinks retain the existing loader behavior.
                if !(kind.is_file() || kind.is_symlink() && path.is_file()) {
                    continue;
                }
                let Some(stem) = name.strip_suffix(".json") else {
                    continue;
                };
                let raw_key = format!("{ns}:{prefix}{stem}");
                let Some(key) = horizon_client_core::identifier::parse_identifier(&raw_key) else {
                    continue;
                };
                if ParticleType::from_name(&key).is_some() {
                    continue;
                }
                let Ok(bytes) = std::fs::read(&path) else {
                    continue;
                };
                let Ok(json) = serde_json::from_slice::<serde_json::Value>(&bytes) else {
                    continue;
                };
                let behaviour = json.get("horizon:behaviour").and_then(|value| {
                    let behaviour = ModBehaviour::parse(value);
                    if behaviour.is_none() {
                        log::warn!("Invalid particle behaviour in {}", path.display());
                    }
                    behaviour
                });
                let textures = texture_names(&json);
                if !textures.is_empty() {
                    self.mods.push(PreparedModParticle {
                        key,
                        textures,
                        behaviour,
                    });
                }
            }
        }
    }
}

fn texture_names(json: &serde_json::Value) -> Vec<String> {
    json.get("textures")
        .and_then(serde_json::Value::as_array)
        .map(|textures| {
            textures
                .iter()
                .filter_map(serde_json::Value::as_str)
                .filter_map(texture_name)
                .collect()
        })
        .unwrap_or_default()
}

/// ParticleDescription's identifiers remain namespaced when `particle/` is
/// inserted into their path. The atlas omits only the `minecraft` namespace.
fn texture_name(raw: &str) -> Option<String> {
    let id = horizon_client_core::identifier::parse_identifier(raw)?;
    let (namespace, path) = id.split_once(':')?;
    Some(if namespace == "minecraft" {
        format!("particle/{path}")
    } else {
        format!("{namespace}:particle/{path}")
    })
}
