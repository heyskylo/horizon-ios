//! The settings store (docs/layers.md, "Platform registries"): a crate above the engine keeps its
//! own settings as `key:value` lines in the game folder, next to `options.txt`, which stays
//! exactly vanilla's. Its lines go in `horizon-extras.txt` (a setting that moved out of the
//! engine's extras keeps its key there, so players keep their choice) or in a file of its own.
//!
//! The engine routes the lines when it loads and saves the player's settings, never per frame.
//! With no store registered it reads and writes nothing more than before.
use std::fs;
use std::path::Path;

use crate::TooLate;
use crate::registry::Registry;

/// One crate's settings. The values live in the crate (statics); the engine only hands it its
/// lines and asks for them back.
pub trait Store: Sync {
    /// Its file in the game folder: `horizon-extras.txt`, or its own `horizon-<name>.txt` that the
    /// engine doesn't write.
    fn file(&self) -> &'static str;
    /// Whether a line's key is this store's.
    fn owns(&self, key: &str) -> bool;
    /// One of its lines (key and value trimmed), as the game loads the settings. A value it can't
    /// read keeps the setting as it was.
    fn load(&self, key: &str, value: &str);
    /// Its lines, as the game saves the settings.
    fn save(&self, line: &mut dyn FnMut(&str, &str));
    /// Whether every setting is at its default: then a file that doesn't exist yet isn't made.
    fn is_default(&self) -> bool;
}

static STORES: Registry<dyn Store> = Registry::new();

/// Adds `store` to those the game loads and saves. Before the game starts, as
/// [`crate::register`].
pub fn register(store: &'static dyn Store) -> Result<(), TooLate> {
    STORES.register(store)
}

pub(crate) fn all() -> &'static [&'static dyn Store] {
    STORES.all()
}

/// The engine's side: where it loads and saves the stores. Only the engine calls these.
pub mod engine {
    use super::*;

    /// Loads every store. `read(file)` is the text of a file of the game folder, if it exists;
    /// the engine passes the text it already read for a file it reads itself.
    pub fn load(read: &mut dyn FnMut(&str) -> Option<String>) {
        load_into(all(), read);
    }

    /// Appends the stores' lines of `file`, a file the engine writes itself, to `text`, its own
    /// lines. True when every store of that file is at its defaults (none counts).
    pub fn append(file: &str, text: &mut String) -> bool {
        append_lines(all(), file, text)
    }

    /// Writes the stores' own files in `dir`: every file of a store but `engine_files`, the ones
    /// the engine writes with [`append`]. A line of the file that no store owns is kept.
    pub fn save_own(dir: &Path, engine_files: &[&str]) {
        save_files(all(), dir, engine_files);
    }
}

/// Each file the stores use, once, in registration order.
fn files(stores: &[&dyn Store]) -> Vec<&'static str> {
    let mut files: Vec<&'static str> = Vec::new();
    for store in stores {
        if !files.contains(&store.file()) {
            files.push(store.file());
        }
    }
    files
}

/// `key:value` lines, as the game's settings files have them.
fn lines(text: &str) -> impl Iterator<Item = (&str, &str, &str)> {
    text.lines().filter_map(|line| {
        let (key, value) = line.split_once(':')?;
        Some((line, key.trim(), value.trim()))
    })
}

fn load_into(stores: &[&dyn Store], read: &mut dyn FnMut(&str) -> Option<String>) {
    for file in files(stores) {
        let Some(text) = read(file) else {
            continue;
        };
        for (_, key, value) in lines(&text) {
            for store in stores.iter().filter(|s| s.file() == file && s.owns(key)) {
                store.load(key, value);
            }
        }
    }
}

fn append_lines(stores: &[&dyn Store], file: &str, text: &mut String) -> bool {
    let mut default = true;
    for store in stores.iter().filter(|s| s.file() == file) {
        default &= store.is_default();
        store.save(&mut |key, value| {
            text.push_str(key);
            text.push(':');
            text.push_str(value);
            text.push('\n');
        });
    }
    default
}

fn save_files(stores: &[&dyn Store], dir: &Path, engine_files: &[&str]) {
    for file in files(stores) {
        if engine_files.contains(&file) {
            continue;
        }
        let path = dir.join(file);
        let existing = fs::read_to_string(&path).ok();
        let mut text = String::new();
        let default = append_lines(stores, file, &mut text);
        let Some(existing) = existing else {
            if !default {
                let _ = fs::write(&path, text);
            }
            continue;
        };
        // Lines no store owns: another build's, or a setting whose crate isn't in this one.
        for (line, key, _) in lines(&existing) {
            if !stores.iter().any(|s| s.file() == file && s.owns(key)) {
                text.push_str(line);
                text.push('\n');
            }
        }
        let _ = fs::write(&path, text);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// A store of one switch, `<prefix>.on`, off by default.
    struct Switch {
        file: &'static str,
        key: &'static str,
        on: Mutex<bool>,
    }

    impl Switch {
        const fn new(file: &'static str, key: &'static str) -> Self {
            Self {
                file,
                key,
                on: Mutex::new(false),
            }
        }
        fn on(&self) -> bool {
            *self.on.lock().unwrap()
        }
        fn set(&self, on: bool) {
            *self.on.lock().unwrap() = on;
        }
    }

    impl Store for Switch {
        fn file(&self) -> &'static str {
            self.file
        }
        fn owns(&self, key: &str) -> bool {
            key == self.key
        }
        fn load(&self, _key: &str, value: &str) {
            match value {
                "true" => self.set(true),
                "false" => self.set(false),
                _ => {}
            }
        }
        fn save(&self, line: &mut dyn FnMut(&str, &str)) {
            line(self.key, if self.on() { "true" } else { "false" });
        }
        fn is_default(&self) -> bool {
            !self.on()
        }
    }

    fn scratch(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("horizon-hooks-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn each_store_reads_only_its_own_lines() {
        let zoom = Switch::new("horizon-extras.txt", "zoom");
        let pad = Switch::new("horizon-pad.txt", "rumble");
        let stores: [&dyn Store; 2] = [&zoom, &pad];
        let mut asked = Vec::new();
        load_into(&stores, &mut |file| {
            asked.push(file.to_owned());
            match file {
                // The engine's own lines, and a key the store's name merely starts.
                "horizon-extras.txt" => Some("fps:true\nzoom : true\nzoomLevel:4\n".into()),
                _ => Some("zoom:true\nrumble:maybe\n".into()),
            }
        });
        assert!(zoom.on());
        // `rumble:maybe` is no value it reads, and `zoom` in its file isn't its key.
        assert!(!pad.on());
        assert_eq!(
            asked,
            ["horizon-extras.txt", "horizon-pad.txt"],
            "each file read once"
        );
    }

    #[test]
    fn a_shared_file_gets_the_stores_lines_after_the_engines() {
        let zoom = Switch::new("horizon-extras.txt", "zoom");
        let pad = Switch::new("horizon-pad.txt", "rumble");
        let stores: [&dyn Store; 2] = [&zoom, &pad];
        let mut text = "fps:true\n".to_owned();
        assert!(append_lines(&stores, "horizon-extras.txt", &mut text));
        assert_eq!(text, "fps:true\nzoom:false\n");
        zoom.set(true);
        let mut text = String::new();
        assert!(!append_lines(&stores, "horizon-extras.txt", &mut text));
        assert_eq!(text, "zoom:true\n");
    }

    #[test]
    fn own_files_appear_once_changed_and_keep_lines_nobody_owns() {
        let dir = scratch("own-files");
        let pad = Switch::new("horizon-pad.txt", "rumble");
        let zoom = Switch::new("horizon-extras.txt", "zoom");
        let stores: [&dyn Store; 2] = [&pad, &zoom];
        save_files(&stores, &dir, &["horizon-extras.txt"]);
        assert!(
            !dir.join("horizon-pad.txt").exists(),
            "defaults make no file"
        );
        assert!(
            !dir.join("horizon-extras.txt").exists(),
            "the engine writes that one"
        );
        pad.set(true);
        save_files(&stores, &dir, &["horizon-extras.txt"]);
        assert_eq!(
            fs::read_to_string(dir.join("horizon-pad.txt")).unwrap(),
            "rumble:true\n"
        );
        // Another build's line stays; the store's own line is rewritten, not repeated.
        fs::write(dir.join("horizon-pad.txt"), "rumble:true\ndeadzone:0.2\n").unwrap();
        pad.set(false);
        save_files(&stores, &dir, &["horizon-extras.txt"]);
        assert_eq!(
            fs::read_to_string(dir.join("horizon-pad.txt")).unwrap(),
            "rumble:false\ndeadzone:0.2\n"
        );
        let _ = fs::remove_dir_all(dir);
    }
}
