//! Native mods' config files, loaded and reloaded (NeoForge's `ModConfig`
//! and `ModConfigEvent`, docs/native-mods.md). A mod lists its configs
//! (`native_register_configs`), each a file of a kind: `Client` on the
//! window, `Common` once per process (the window, or a dedicated server),
//! `Server` on the server for each world it opens, which a world may
//! override in its `serverconfig/` folder. The game writes a missing file's
//! default text, reads it and hands the text to the mod
//! (`native_config_loading`); the mod parses it. Once a second it looks at
//! each file's modification time and hands changed text over again
//! (`native_config_reloading`). A server's configs are dropped when it stops
//! (`native_config_unloading`).
use super::Game;
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

/// How often the files' modification times are read.
const CHECK_EVERY: Duration = Duration::from_secs(1);

/// Which side reads a config and when (NeoForge's `ModConfig.Type`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum NativeConfigKind {
    /// The window only, when the game starts.
    Client,
    /// Once per process: the window when the game starts, or a dedicated
    /// server when it starts. A single player's server thread shares the
    /// window's.
    Common,
    /// The server, before `native_server_about_to_start`, for each world it
    /// opens: the world's `serverconfig/<file>` when there is one, else
    /// `config/<file>`.
    Server,
}

impl NativeConfigKind {
    /// The default file name's suffix, as NeoForge's.
    fn suffix(self) -> &'static str {
        match self {
            Self::Client => "client",
            Self::Common => "common",
            Self::Server => "server",
        }
    }
}

/// A config a mod registers: its kind, its file in `config/` and the text
/// written there when the file is missing.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct NativeConfig {
    pub mod_id: String,
    pub kind: NativeConfigKind,
    /// A path relative to `config/`: `<mod id>-<kind>.toml` unless named.
    pub file: String,
    pub default: String,
}

impl NativeConfig {
    /// `config/<mod_id>-<kind>.toml`, `default` when missing.
    pub fn new(mod_id: &str, kind: NativeConfigKind, default: &str) -> Self {
        Self {
            mod_id: mod_id.to_owned(),
            kind,
            file: format!("{mod_id}-{}.toml", kind.suffix()),
            default: default.to_owned(),
        }
    }
    /// The same in another file of `config/`, such as `mymod/machines.toml`.
    pub fn file(mut self, file: &str) -> Self {
        file.clone_into(&mut self.file);
        self
    }
}

/// A config's file as the game read it.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct NativeConfigFile {
    pub mod_id: String,
    pub kind: NativeConfigKind,
    /// The registered name, relative to `config/`.
    pub file: String,
    /// Where it was read: in `config/`, or a world's `serverconfig/`.
    pub path: PathBuf,
    pub text: String,
}

/// A loaded config and what its file looked like when read.
struct Tracked {
    config: NativeConfigFile,
    modified: Option<SystemTime>,
}

/// One game's configs: the window's, a server thread's or a dedicated
/// server's.
#[derive(Default)]
pub(super) struct NativeConfigs {
    started: bool,
    files: Vec<Tracked>,
    next_check: Option<Instant>,
}

/// Whether `file` stays inside the folder it is joined to.
fn inside(file: &str) -> bool {
    let path = Path::new(file);
    path.components().next().is_some()
        && path.components().all(|c| matches!(c, Component::Normal(_)))
}

/// The file `config` is read from: the world's `serverconfig/` copy of a
/// server config when there is one, else `config_dir`'s, written from the
/// default when missing.
fn locate(
    config: &NativeConfig,
    config_dir: &Path,
    world: Option<&Path>,
) -> std::io::Result<PathBuf> {
    if config.kind == NativeConfigKind::Server
        && let Some(world) = world
    {
        let own = world.join("serverconfig").join(&config.file);
        if own.is_file() {
            return Ok(own);
        }
    }
    let path = config_dir.join(&config.file);
    if !path.exists() {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&path, &config.default)?;
    }
    Ok(path)
}

fn modified(path: &Path) -> Option<SystemTime> {
    fs::metadata(path).and_then(|m| m.modified()).ok()
}

impl NativeConfigs {
    /// Reads `configs` of `kinds` (a duplicate path or a file outside
    /// `config/` is skipped) and answers each loaded file.
    fn load(
        &mut self,
        configs: Vec<NativeConfig>,
        kinds: &[NativeConfigKind],
        config_dir: &Path,
        world: Option<&Path>,
    ) -> Vec<NativeConfigFile> {
        self.started = true;
        let mut loaded = Vec::new();
        for config in configs.into_iter().filter(|c| kinds.contains(&c.kind)) {
            if !inside(&config.file) {
                log::warn!(
                    "Horizon: mod {} config {:?} is not a file in config/",
                    config.mod_id,
                    config.file
                );
                continue;
            }
            let path = match locate(&config, config_dir, world) {
                Ok(path) => path,
                Err(e) => {
                    log::warn!("Horizon: mod {} config {}: {e}", config.mod_id, config.file);
                    continue;
                }
            };
            if self.files.iter().any(|t| t.config.path == path) {
                log::warn!(
                    "Horizon: mod {} config {} is registered twice",
                    config.mod_id,
                    path.display()
                );
                continue;
            }
            let modified = modified(&path);
            let text = match fs::read_to_string(&path) {
                Ok(text) => text,
                Err(e) => {
                    log::warn!(
                        "Horizon: mod {} config {}: {e}",
                        config.mod_id,
                        path.display()
                    );
                    continue;
                }
            };
            let file = NativeConfigFile {
                mod_id: config.mod_id,
                kind: config.kind,
                file: config.file,
                path,
                text,
            };
            loaded.push(file.clone());
            self.files.push(Tracked {
                config: file,
                modified,
            });
        }
        loaded
    }

    /// The files whose text changed since they were read, read again; a
    /// file that is gone or unreadable keeps its last text.
    fn changed(&mut self) -> Vec<NativeConfigFile> {
        let mut changed = Vec::new();
        for tracked in &mut self.files {
            let now = modified(&tracked.config.path);
            if now.is_none() || now == tracked.modified {
                continue;
            }
            let Ok(text) = fs::read_to_string(&tracked.config.path) else {
                continue;
            };
            tracked.modified = now;
            if text != tracked.config.text {
                tracked.config.text = text;
                changed.push(tracked.config.clone());
            }
        }
        changed
    }

    /// Whether a second passed since the last look at the files.
    fn due(&mut self, now: Instant) -> bool {
        if self.files.is_empty() || self.next_check.is_some_and(|t| now < t) {
            return false;
        }
        self.next_check = Some(now + CHECK_EVERY);
        true
    }

    /// Drops the server's configs and answers them.
    fn drop_server(&mut self) -> Vec<NativeConfigFile> {
        let (server, rest) = std::mem::take(&mut self.files)
            .into_iter()
            .partition(|t| t.config.kind == NativeConfigKind::Server);
        self.files = rest;
        server.into_iter().map(|t| t.config).collect()
    }
}

impl Game<'_> {
    /// Asks the mods for their configs and loads those of `kinds`.
    fn native_configs_load(&mut self, kinds: &[NativeConfigKind], world: Option<&Path>) {
        let config_dir = match Self::native_config_dir() {
            Ok(dir) => dir,
            Err(e) => {
                log::warn!("Horizon: mod configs: {e}");
                return;
            }
        };
        let mut configs = Vec::new();
        self.native_register_configs(&mut configs);
        if configs.is_empty() {
            self.native_configs.started = true;
            return;
        }
        for file in self.native_configs.load(configs, kinds, &config_dir, world) {
            self.native_config_loading(&file);
        }
    }

    /// The window's `Client` and `Common` configs, once: on its first frame
    /// step, or before it starts a world's server thread if that comes first.
    pub(super) fn native_configs_client_start(&mut self) {
        if !self.native_configs.started {
            use NativeConfigKind::{Client, Common};
            self.native_configs_load(&[Client, Common], None);
        }
    }

    /// The window's frame step: its configs load on the first, and changed
    /// files reload once a second.
    pub(super) fn native_configs_tick_client(&mut self) {
        self.native_configs_client_start();
        self.native_configs_poll();
    }

    /// A server about to open `world`: a dedicated server loads its
    /// `Common` configs too, a single player's server thread only `Server`.
    pub(super) fn native_configs_server_start(&mut self, world: &Path, dedicated: bool) {
        use NativeConfigKind::{Common, Server};
        let kinds: &[NativeConfigKind] = if dedicated && !self.native_configs.started {
            &[Common, Server]
        } else {
            &[Server]
        };
        self.native_configs_load(kinds, Some(world));
    }

    /// The server stops: its `Server` configs unload.
    pub(super) fn native_configs_server_stop(&mut self) {
        for file in self.native_configs.drop_server() {
            self.native_config_unloading(&file);
        }
    }

    /// Hands changed files over again, at most once a second.
    pub(super) fn native_configs_poll(&mut self) {
        if !self.native_configs.due(Instant::now()) {
            return;
        }
        for file in self.native_configs.changed() {
            self.native_config_reloading(&file);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use NativeConfigKind::{Client, Common, Server};

    struct Folder(PathBuf);

    impl Folder {
        fn new(name: &str) -> Self {
            let dir = std::env::temp_dir().join(format!(
                "horizon-native-configs-{name}-{}",
                std::process::id()
            ));
            let _ = fs::remove_dir_all(&dir);
            fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }
    }

    impl Drop for Folder {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    /// A time after `path`'s, so the next look sees a change even on a file
    /// system that keeps whole seconds.
    fn touch_later(path: &Path, text: &str) {
        let before = modified(path).unwrap();
        fs::write(path, text).unwrap();
        let file = fs::File::options().write(true).open(path).unwrap();
        file.set_modified(before + Duration::from_secs(2)).unwrap();
    }

    #[test]
    fn a_missing_file_gets_its_default_and_an_edited_one_is_read() {
        let root = Folder::new("default");
        let config = root.0.join("config");
        fs::create_dir_all(&config).unwrap();
        fs::write(config.join("gt-common.toml"), "voltage = 512\n").unwrap();
        let mut configs = NativeConfigs::default();
        let loaded = configs.load(
            vec![
                NativeConfig::new("gt", Client, "hud = true\n"),
                NativeConfig::new("gt", Common, "voltage = 32\n"),
                NativeConfig::new("gt", Server, "ores = 1\n"),
            ],
            &[Client, Common],
            &config,
            None,
        );
        let got: Vec<_> = loaded
            .iter()
            .map(|f| (f.file.as_str(), f.text.as_str()))
            .collect();
        assert_eq!(
            got,
            [
                ("gt-client.toml", "hud = true\n"),
                ("gt-common.toml", "voltage = 512\n")
            ]
        );
        assert_eq!(
            fs::read_to_string(config.join("gt-client.toml")).unwrap(),
            "hud = true\n"
        );
        // Another side's kind is neither read nor written.
        assert!(!config.join("gt-server.toml").exists());
    }

    #[test]
    fn a_world_overrides_a_server_config_and_the_folder_serves_the_rest() {
        let root = Folder::new("world");
        let (config, world) = (root.0.join("config"), root.0.join("saves/W"));
        fs::create_dir_all(world.join("serverconfig/gt")).unwrap();
        fs::write(world.join("serverconfig/gt/ores.toml"), "veins = 9\n").unwrap();
        let mut configs = NativeConfigs::default();
        let loaded = configs.load(
            vec![
                NativeConfig::new("gt", Server, "veins = 3\n").file("gt/ores.toml"),
                NativeConfig::new("gt", Server, "tiers = 4\n"),
                // A Common config of the world's folder is not looked for.
                NativeConfig::new("gt", Common, "a = 1\n"),
            ],
            &[Server],
            &config,
            Some(&world),
        );
        let got: Vec<_> = loaded
            .iter()
            .map(|f| (f.path.clone(), f.text.as_str()))
            .collect();
        assert_eq!(
            got,
            [
                (world.join("serverconfig/gt/ores.toml"), "veins = 9\n"),
                (config.join("gt-server.toml"), "tiers = 4\n"),
            ]
        );
        // The world's copy stands for the default: nothing is written beside it.
        assert!(!config.join("gt/ores.toml").exists());
        assert_eq!(configs.drop_server().len(), 2);
        assert!(configs.files.is_empty());
    }

    #[test]
    fn a_file_outside_config_or_registered_twice_is_skipped() {
        let root = Folder::new("skip");
        let config = root.0.join("config");
        let mut configs = NativeConfigs::default();
        let loaded = configs.load(
            vec![
                NativeConfig::new("a", Common, "x").file("../options.txt"),
                NativeConfig::new("a", Common, "x").file(""),
                NativeConfig::new("a", Common, "first"),
                NativeConfig::new("b", Common, "second").file("a-common.toml"),
            ],
            &[Common],
            &config,
            None,
        );
        assert_eq!(
            loaded.iter().map(|f| f.mod_id.as_str()).collect::<Vec<_>>(),
            ["a"]
        );
        assert!(!root.0.join("options.txt").exists());
    }

    #[test]
    fn a_change_reloads_once_and_a_touch_without_one_does_not() {
        let root = Folder::new("reload");
        let config = root.0.join("config");
        let mut configs = NativeConfigs::default();
        configs.load(
            vec![NativeConfig::new("gt", Common, "a = 1\n")],
            &[Common],
            &config,
            None,
        );
        let path = config.join("gt-common.toml");
        assert!(configs.changed().is_empty());
        touch_later(&path, "a = 2\n");
        let changed = configs.changed();
        assert_eq!(
            changed.iter().map(|f| f.text.as_str()).collect::<Vec<_>>(),
            ["a = 2\n"]
        );
        assert!(configs.changed().is_empty());
        // Saved again with the same text: nothing to reload.
        touch_later(&path, "a = 2\n");
        assert!(configs.changed().is_empty());
        // Gone: the last text stays, and nothing fires.
        fs::remove_file(&path).unwrap();
        assert!(configs.changed().is_empty());
        assert_eq!(configs.files[0].config.text, "a = 2\n");
    }

    #[test]
    fn files_are_looked_at_once_a_second_and_never_without_configs() {
        let mut configs = NativeConfigs::default();
        let start = Instant::now();
        assert!(!configs.due(start));
        configs.files.push(Tracked {
            config: NativeConfigFile {
                mod_id: "gt".into(),
                kind: Common,
                file: "gt-common.toml".into(),
                path: PathBuf::new(),
                text: String::new(),
            },
            modified: None,
        });
        assert!(configs.due(start));
        assert!(!configs.due(start + Duration::from_millis(999)));
        assert!(configs.due(start + CHECK_EVERY));
    }
}
