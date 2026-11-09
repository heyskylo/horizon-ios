//! The native mods horizon-mixins compiled into a workspace, read from the record it leaves there.
use std::{collections::BTreeSet, path::Path};

/// What horizon-mixins writes at the root of a workspace it changed: each injection and module it
/// applied, with its mod's `[mod] id`. The launcher's native builds have one, and so has a bundle
/// applied by hand or by a mod's own script, which has no `native-mods.json` beside its game.
pub const MIXINS_STAGE: &str = ".horizon-mixins-stage.json";

/// The ids of the mods whose injections or modules horizon-mixins applied to the workspace at
/// `root`, sorted: none for a workspace it never changed, a record that names no mod, or one that
/// does not parse.
pub fn staged_mods(root: &Path) -> Vec<String> {
    let Ok(text) = std::fs::read_to_string(root.join(MIXINS_STAGE)) else {
        return Vec::new();
    };
    let Ok(record) = serde_json::from_str::<serde_json::Value>(&text) else {
        return Vec::new();
    };
    let ids: BTreeSet<&str> = ["injections", "modules"]
        .iter()
        .filter_map(|list| record[*list].as_array())
        .flatten()
        .filter_map(|entry| entry["mod_id"].as_str())
        .filter(|id| !id.is_empty())
        .collect();
    ids.into_iter().map(str::to_owned).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn workspace(name: &str, record: Option<&str>) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "horizon-mixins-stage-{name}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        if let Some(record) = record {
            std::fs::write(dir.join(MIXINS_STAGE), record).unwrap();
        }
        dir
    }

    #[test]
    fn the_mods_of_every_injection_and_module_once() {
        // The shape horizon-mixins 0.6.0 writes (schema 6), trimmed to the fields read here.
        let dir = workspace(
            "mods",
            Some(
                r#"{"schema": 6, "transformer": "0.6.0", "inputs": {"Cargo.toml": "ab"},
                "injections": [
                    {"mod_id": "ironfall", "injection_id": "hud", "kind": "HEAD"},
                    {"mod_id": "veinminer", "injection_id": "break", "kind": "HEAD"},
                    {"mod_id": "ironfall", "injection_id": "tick", "kind": "HEAD"}
                ],
                "modules": [
                    {"mod_id": "ironfall", "module_id": "glue"},
                    {"mod_id": "atlas", "module_id": "maps"}
                ]}"#,
            ),
        );
        assert_eq!(staged_mods(&dir), ["atlas", "ironfall", "veinminer"]);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn no_mods_without_a_record_or_with_an_empty_or_broken_one() {
        for (name, record) in [
            ("none", None),
            (
                "empty",
                Some(r#"{"schema": 6, "injections": [], "modules": []}"#),
            ),
            ("broken", Some("{\"injections\": [")),
        ] {
            let dir = workspace(name, record);
            assert!(staged_mods(&dir).is_empty(), "{name}");
            std::fs::remove_dir_all(&dir).unwrap();
        }
    }
}
