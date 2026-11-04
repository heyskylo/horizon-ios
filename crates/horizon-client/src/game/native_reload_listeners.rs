//! Native mods' own data folders, read on each data load of a server
//! (NeoForge's `AddServerReloadListenersEvent`, docs/native-mods.md). A mod
//! names a folder under `data/<namespace>/` (`native_add_server_reload_listeners`);
//! when a world opens and on `/reload`, the folder's `.json` files of the
//! resource root (vanilla's and content mods') and of the world's enabled data
//! packs are read as vanilla's `SimpleJsonResourceReloadListener.prepare` reads
//! its folder, and handed to the mod (`native_server_reload_listener_apply`).
use super::{Game, functions};
use horizon_client::datapack::DataPack;
use std::collections::BTreeMap;
use std::path::{Component, Path};

/// A folder of JSON files a mod reads from the data packs, as vanilla's
/// `SimpleJsonResourceReloadListener` over `FileToIdConverter.json(folder)`.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct NativeReloadListener {
    /// `mymod:ore_veins`: which of the mod's listeners the files are for.
    pub id: String,
    /// The folder under `data/<namespace>/`: `ore_veins`, `mymod/ore_veins`.
    pub folder: String,
}

impl NativeReloadListener {
    /// The `.json` files of `data/<namespace>/<folder>/`, for listener `id`.
    pub fn json(id: &str, folder: &str) -> Self {
        Self {
            id: id.to_owned(),
            folder: folder.to_owned(),
        }
    }
}

/// Whether `folder` stays below `data/<namespace>/`.
fn folder_ok(folder: &str) -> bool {
    let path = Path::new(folder);
    !folder.contains('\\')
        && path.components().next().is_some()
        && path.components().all(|c| matches!(c, Component::Normal(_)))
}

/// `FileToIdConverter.fileToId`: `data/<ns>/<folder>/<path>.json` is
/// `<ns>:<path>`; None for a file of another folder or extension.
fn file_id(file: &str, folder: &str) -> Option<String> {
    let (ns, rest) = file.strip_prefix("data/")?.split_once('/')?;
    let path = rest
        .strip_prefix(folder)?
        .strip_prefix('/')?
        .strip_suffix(".json")?;
    (!path.is_empty()).then(|| format!("{ns}:{path}"))
}

/// The folder's files over `packs`, the bottom one first: each id's file is
/// the top pack's (`ResourceManager.listResources`), parsed; one that isn't
/// JSON is logged and skipped, as `prepare` does. Sorted by id.
fn read_folder(
    packs: &[&DataPack],
    listener: &str,
    folder: &str,
) -> Vec<(String, serde_json::Value)> {
    let mut top: BTreeMap<String, (&DataPack, String)> = BTreeMap::new();
    for pack in packs {
        for file in pack.list_folder(folder) {
            if let Some(id) = file_id(&file, folder) {
                top.insert(id, (pack, file));
            }
        }
    }
    let mut files = Vec::with_capacity(top.len());
    for (id, (pack, file)) in top {
        let Some(bytes) = pack.read(&file) else {
            continue;
        };
        match serde_json::from_slice(&bytes) {
            Ok(json) => files.push((id, json)),
            Err(e) => log::error!(
                "Horizon: couldn't parse data file '{id}' from '{file}' in pack {} for {listener}: {e}",
                pack.id
            ),
        }
    }
    files
}

impl Game<'_> {
    /// A server's data load (a world opening, `/reload`): every listener's
    /// files go to its mod, before `native_server_data_loaded`.
    pub(super) fn native_reload_listeners(&mut self) {
        let mut listeners = Vec::new();
        self.native_add_server_reload_listeners(&mut listeners);
        if listeners.is_empty() {
            return;
        }
        let stack = functions::pack_stack(
            &self.world_root,
            &self.options.resources,
            self.cmds.vanilla_disabled,
            &self.level.enabled_features,
        );
        let packs: Vec<&DataPack> = stack.builtin.iter().chain(stack.enabled_packs()).collect();
        for listener in &listeners {
            if !folder_ok(&listener.folder) {
                log::warn!(
                    "Horizon: reload listener {} folder {:?} is not a folder under data/<namespace>/",
                    listener.id,
                    listener.folder
                );
                continue;
            }
            let files = read_folder(&packs, &listener.id, &listener.folder);
            self.native_server_reload_listener_apply(listener, &files);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::fs;
    use std::path::PathBuf;

    struct Folder(PathBuf);

    impl Folder {
        fn new(name: &str) -> Self {
            let dir = std::env::temp_dir().join(format!(
                "horizon-reload-listeners-{name}-{}",
                std::process::id()
            ));
            let _ = fs::remove_dir_all(&dir);
            Self(dir)
        }
        /// A folder pack with these files.
        fn pack(&self, name: &str, files: &[(&str, &str)]) -> DataPack {
            let root = self.0.join(name);
            for (path, text) in files {
                let path = root.join(path);
                fs::create_dir_all(path.parent().unwrap()).unwrap();
                fs::write(path, text).unwrap();
            }
            DataPack::builtin(&root)
        }
    }

    impl Drop for Folder {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn the_top_pack_wins_each_id_and_ids_drop_the_folder_and_extension() {
        let dir = Folder::new("top");
        let root = dir.pack(
            "root",
            &[
                ("data/gt/ore_veins/copper.json", r#"{"weight": 1}"#),
                ("data/gt/ore_veins/deep/tin.json", r#"{"weight": 2}"#),
                ("data/other/ore_veins/zinc.json", r#"{"weight": 3}"#),
                // Another folder, and a name that only starts like it.
                ("data/gt/recipe/plate.json", "{}"),
                ("data/gt/ore_veins_old/lead.json", "{}"),
            ],
        );
        let world = dir.pack(
            "world",
            &[
                ("data/gt/ore_veins/copper.json", r#"{"weight": 10}"#),
                ("data/gt/ore_veins/notes.txt", "not json, not read"),
            ],
        );
        let files = read_folder(&[&root, &world], "gt:veins", "ore_veins");
        assert_eq!(
            files,
            [
                ("gt:copper".to_owned(), json!({"weight": 10})),
                ("gt:deep/tin".to_owned(), json!({"weight": 2})),
                ("other:zinc".to_owned(), json!({"weight": 3})),
            ]
        );
    }

    #[test]
    fn a_broken_top_file_is_skipped_without_falling_back() {
        let dir = Folder::new("broken");
        let root = dir.pack(
            "root",
            &[
                ("data/gt/machines/press.json", r#"{"tier": 1}"#),
                ("data/gt/machines/mixer.json", r#"{"tier": 2}"#),
            ],
        );
        let world = dir.pack("world", &[("data/gt/machines/press.json", "{ tier: ")]);
        let files = read_folder(&[&root, &world], "gt:machines", "machines");
        assert_eq!(files, [("gt:mixer".to_owned(), json!({"tier": 2}))]);
    }

    #[test]
    fn a_nested_folder_reads_only_below_it() {
        let dir = Folder::new("nested");
        let pack = dir.pack(
            "root",
            &[
                ("data/gt/gt/veins/a.json", "1"),
                ("data/gt/gt/other/b.json", "2"),
            ],
        );
        let files = read_folder(&[&pack], "gt:veins", "gt/veins");
        assert_eq!(files, [("gt:a".to_owned(), json!(1))]);
    }

    #[test]
    fn a_zipped_pack_reads_only_the_folder() {
        let dir = Folder::new("zip");
        let packs = dir.0.join("datapacks");
        fs::create_dir_all(&packs).unwrap();
        let mut zip = zip::ZipWriter::new(fs::File::create(packs.join("veins.zip")).unwrap());
        for (name, text) in [
            ("pack.mcmeta", r#"{"pack":{"pack_format":48}}"#),
            ("data/gt/ore_veins/iron.json", r#"{"y": -16}"#),
            ("data/gt/ore_veins_old/lead.json", "{}"),
            ("data/gt/recipe/plate.json", "{}"),
        ] {
            zip.start_file(name, zip::write::SimpleFileOptions::default())
                .unwrap();
            std::io::Write::write_all(&mut zip, text.as_bytes()).unwrap();
        }
        zip.finish().unwrap();
        let found = horizon_client::datapack::discover_folder(&packs);
        let refs: Vec<&DataPack> = found.iter().collect();
        let files = read_folder(&refs, "gt:veins", "ore_veins");
        assert_eq!(files, [("gt:iron".to_owned(), json!({"y": -16}))]);
    }

    #[test]
    fn a_folder_outside_the_namespace_is_refused() {
        assert!(folder_ok("ore_veins"));
        assert!(folder_ok("gt/ore_veins"));
        for bad in ["", "..", "../recipe", "gt/../../x", "/abs", "a\\b"] {
            assert!(!folder_ok(bad), "{bad:?}");
        }
    }
}
