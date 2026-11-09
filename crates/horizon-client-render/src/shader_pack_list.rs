//! The shader packs in the `shaderpacks` folder beside `options.txt`, as Options > Horizon
//! Settings > Shader Packs lists them: Horizon's own (docs/shaders.md) and those in the
//! `shaders/` format, side by side.

use std::path::{Path, PathBuf};

use horizon_shader_api::{OptionKind, OptionPage, PackOption};

pub const DIR: &str = "shaderpacks";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// A folder with `horizon-shaders.toml`, or a `.hshader`.
    Horizon,
    /// A folder with a `shaders` folder, or a `.zip`.
    Classic,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    /// The folder or file name in `shaderpacks`.
    pub file: String,
    /// Native manifest identity (`horizon:<id>`), or a classic filename.
    pub key: String,
    /// The pack's own name for Horizon's packs, else `file`.
    pub title: String,
    pub kind: Kind,
    /// A Horizon pack's `[[options]]`, and the value of each the player set (`values_path`).
    pub options: Vec<PackOption>,
    pub values: Vec<f64>,
    /// Its options page's `[[pages]]`; empty for one page of every option.
    pub pages: Vec<OptionPage>,
}

impl Entry {
    /// Saves the player's option values, for the pack in the folder `dir`.
    pub fn save_values(&self, dir: &Path) {
        let path = values_pack(&dir.join(&self.file), &self.key);
        if let Err(e) = save_values(&path, &self.options, &self.values) {
            log::warn!("shader pack options of {}: {e}", self.file);
        }
    }

    /// The option `i` one value on (`back`: one before), or at its default (`reset`). Whether
    /// the pack has that option.
    pub fn step_option(&mut self, i: usize, back: bool, reset: bool) -> bool {
        let (Some(o), Some(v)) = (self.options.get(i), self.values.get_mut(i)) else {
            return false;
        };
        *v = if reset {
            o.kind.default_constant()
        } else {
            step_value(&o.kind, *v, back)
        };
        true
    }

    /// Where option `i` sits on its slider, 0 to 1: float and int options are sliders.
    pub fn slider_position(&self, i: usize) -> Option<f32> {
        slider_position(&self.options.get(i)?.kind, *self.values.get(i)?)
    }

    /// Option `i` at the value nearest the slider position `at`. Whether its value changed.
    pub fn set_slider_position(&mut self, i: usize, at: f32) -> bool {
        let (Some(o), Some(v)) = (self.options.get(i), self.values.get_mut(i)) else {
            return false;
        };
        let Some(new) = slider_value(&o.kind, at) else {
            return false;
        };
        let changed = new != *v;
        *v = new;
        changed
    }

    /// Every option back to its default.
    pub fn reset_options(&mut self) {
        let defaults = self.options.iter().map(|o| o.kind.default_constant());
        self.values = defaults.collect();
    }
}

/// The packs in `dir`, Horizon's first, each kind by title. A missing folder has none.
pub fn scan(dir: &Path) -> Vec<Entry> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut paths: Vec<_> = entries.flatten().map(|e| e.path()).collect();
    paths.sort();
    let mut keys = std::collections::HashSet::new();
    let mut packs: Vec<Entry> = paths
        .iter()
        .filter_map(|p| entry(p))
        .filter(|e| keys.insert(e.key.clone()))
        .collect();
    for pack in &mut packs {
        if let Some(id) = pack.key.strip_prefix("horizon:") {
            pack.values = load_values(
                &native_values_pack(&dir.join(&pack.file), id),
                &pack.options,
            );
        }
    }
    packs.sort_by(|a, b| {
        (a.kind == Kind::Classic, a.title.to_lowercase())
            .cmp(&(b.kind == Kind::Classic, b.title.to_lowercase()))
    });
    packs
}

fn entry(path: &Path) -> Option<Entry> {
    let file = path.file_name()?.to_str()?.to_owned();
    if file.starts_with('.') {
        return None;
    }
    if horizon_shader_api::package::is_pack(path) {
        // Its manifest's name; a pack that can't be read is still listed, under its file
        // name: picking it says why in the chat.
        let manifest = horizon_shader_api::Pack::read_manifest(path).ok();
        let (key, title, options, pages) = manifest.map_or_else(
            || (file.clone(), file.clone(), Vec::new(), Vec::new()),
            |m| (format!("horizon:{}", m.id), m.name, m.options, m.pages),
        );
        return Some(Entry {
            key,
            file,
            title,
            kind: Kind::Horizon,
            options,
            values: Vec::new(),
            pages,
        });
    }
    let zip = path.is_file()
        && path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("zip"));
    (zip || path.join("shaders").is_dir()).then(|| Entry {
        title: file.clone(),
        key: file.clone(),
        file,
        kind: Kind::Classic,
        options: Vec::new(),
        values: Vec::new(),
        pages: Vec::new(),
    })
}

/// Resolves a saved identity, migrating a legacy native filename when its manifest or the
/// launcher's `<id>-<version>.hshader` name identifies it. Duplicate IDs use the first filename
/// in lexical order, matching `scan`; versions are free-form strings, not ordered versions.
/// An absent arbitrary old filename cannot be attributed to a manifest safely.
pub fn resolve(dir: &Path, picked: &str) -> (String, PathBuf) {
    if picked.is_empty() {
        return (String::new(), dir.join(picked));
    }
    let direct = dir.join(picked);
    if direct.exists() && !horizon_shader_api::package::is_pack(&direct) {
        return (picked.to_owned(), direct);
    }
    let mut native: Vec<_> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| {
            let path = e.path();
            let file = path.file_name()?.to_str()?.to_owned();
            if file.starts_with('.') || !horizon_shader_api::package::is_pack(&path) {
                return None;
            }
            Some((file, horizon_shader_api::Pack::read_manifest(&path).ok()?))
        })
        .collect();
    native.sort_by(|a, b| a.0.cmp(&b.0));
    let id = native
        .iter()
        .find(|(file, _)| file == picked)
        .map(|(_, m)| m.id.as_str())
        .or_else(|| picked.strip_prefix("horizon:"))
        .or_else(|| {
            // Never reinterpret an installed classic pack as a native identity.
            if dir.join(picked).exists() {
                return None;
            }
            let stem = picked.strip_suffix(".hshader")?;
            native
                .iter()
                .find(|(_, m)| {
                    stem.strip_prefix(&m.id)
                        .is_some_and(|v| v.starts_with('-') && v.len() > 1)
                })
                .map(|(_, m)| m.id.as_str())
        });
    if let Some((file, m)) = native.iter().find(|(_, m)| Some(m.id.as_str()) == id) {
        let key = format!("horizon:{}", m.id);
        if !picked.starts_with("horizon:") {
            migrate_values(&dir.join(picked), &key);
        }
        migrate_values(&dir.join(file), &key);
        return (key, dir.join(file));
    }
    (picked.to_owned(), dir.join(picked))
}

fn values_pack(pack: &Path, key: &str) -> PathBuf {
    key.strip_prefix("horizon:").map_or_else(
        || pack.to_path_buf(),
        |id| {
            pack.parent()
                .unwrap_or_else(|| Path::new("."))
                .join(".horizon-options")
                .join(id)
        },
    )
}

/// The native options path; imports a filename sidecar once, keeping the original intact.
pub fn native_values_pack(pack: &Path, id: &str) -> PathBuf {
    let key = format!("horizon:{id}");
    migrate_values(pack, &key);
    values_pack(pack, &key)
}

fn migrate_values(pack: &Path, key: &str) {
    let target = values_path(&values_pack(pack, key));
    if target.exists() {
        return;
    }
    if let Ok(text) = std::fs::read(values_path(pack)) {
        let result = std::fs::create_dir_all(target.parent().unwrap()).and_then(|()| {
            use std::io::Write;
            // Another reader may already have imported the values.
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&target)?;
            file.write_all(&text)
        });
        if let Err(e) = result
            && e.kind() != std::io::ErrorKind::AlreadyExists
        {
            log::warn!("shader pack options migration: {e}");
        }
    }
}

/// Where the option values of the Horizon pack at `pack` are kept: `<pack>.txt` beside it,
/// one `id=value` a line.
pub fn values_path(pack: &Path) -> PathBuf {
    let mut path = pack.as_os_str().to_owned();
    path.push(".txt");
    PathBuf::from(path)
}

/// The value of each of `options` saved for the pack at `pack`, else its default; a value
/// out of the option's range is brought into it.
pub fn load_values(pack: &Path, options: &[PackOption]) -> Vec<f64> {
    let text = std::fs::read_to_string(values_path(pack)).unwrap_or_default();
    let saved: std::collections::HashMap<&str, f64> = text
        .lines()
        .filter_map(|l| l.split_once('='))
        .filter_map(|(k, v)| Some((k.trim(), v.trim().parse().ok()?)))
        .collect();
    options
        .iter()
        .map(|o| {
            saved
                .get(o.id.as_str())
                .map_or_else(|| o.kind.default_constant(), |&v| clamp(&o.kind, v))
        })
        .collect()
}

/// Saves non-default values. An empty file records a reset, so old sidecars stay retired.
fn save_values(pack: &Path, options: &[PackOption], values: &[f64]) -> std::io::Result<()> {
    let text: String = options
        .iter()
        .zip(values)
        .filter(|(o, v)| o.kind.default_constant() != **v)
        .map(|(o, v)| format!("{}={v}\n", o.id))
        .collect();
    let path = values_path(pack);
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(path, text)
}

fn clamp(kind: &OptionKind, v: f64) -> f64 {
    match kind {
        OptionKind::Bool { .. } => f64::from(u8::from(v != 0.0)),
        OptionKind::Float { min, max, .. } => v.clamp(f64::from(*min), f64::from(*max)),
        OptionKind::Int { min, max, .. } => v.round().clamp(f64::from(*min), f64::from(*max)),
        OptionKind::Choice { values, .. } => v.round().clamp(0.0, values.len() as f64 - 1.0),
    }
}

/// The option's next value (`back`: the one before), wrapping around at the ends.
fn step_value(kind: &OptionKind, v: f64, back: bool) -> f64 {
    let dir = if back { -1.0 } else { 1.0 };
    match kind {
        OptionKind::Bool { .. } => f64::from(u8::from(v == 0.0)),
        OptionKind::Float { min, max, step, .. } => {
            let (min, max, step) = (f64::from(*min), f64::from(*max), f64::from(*step));
            if !back && v >= max - step * 0.01 {
                min
            } else if back && v <= min + step * 0.01 {
                max
            } else {
                // On the grid of steps from the minimum, so float error never piles up.
                let n = ((v - min) / step).round() + dir;
                (min + n * step).clamp(min, max)
            }
        }
        OptionKind::Int { min, max, .. } => {
            let (min, max) = (f64::from(*min), f64::from(*max));
            let n = v.round() + dir;
            if n > max {
                min
            } else if n < min {
                max
            } else {
                n
            }
        }
        OptionKind::Choice { values, .. } => {
            let n = values.len() as f64;
            (v.round() + dir).rem_euclid(n)
        }
    }
}

/// A float option's stops past its minimum: its steps, the last one cut short at the maximum
/// when the range isn't a whole number of them.
pub fn float_steps(min: f32, max: f32, step: f32) -> f64 {
    let (min, max, step) = (f64::from(min), f64::from(max), f64::from(step));
    ((max - min) / step - STEP_SLACK).ceil()
}
/// What a range a hair over a whole number of steps loses to rounding, short of another stop.
const STEP_SLACK: f64 = 1e-6;

/// Where `v` sits between the option's minimum and maximum, 0 to 1, as `slider_value` reads
/// it back: a float at its stop (the maximum the last one), an int along its range. `None` for
/// the options that are buttons.
fn slider_position(kind: &OptionKind, v: f64) -> Option<f32> {
    let position = match kind {
        OptionKind::Float { min, max, step, .. } => {
            let steps = float_steps(*min, *max, *step);
            let stop = ((v - f64::from(*min)) / f64::from(*step)).round();
            if v >= f64::from(*max) || steps <= 0.0 {
                1.0
            } else {
                stop / steps
            }
        }
        OptionKind::Int { min, max, .. } => {
            let (min, max) = (f64::from(*min), f64::from(*max));
            (v - min) / (max - min)
        }
        _ => return None,
    };
    Some(position.clamp(0.0, 1.0) as f32)
}

/// The option's value nearest the slider position `at`: a float on its steps from the
/// minimum (the last one the maximum, when the range isn't a whole number of steps), an int
/// whole.
fn slider_value(kind: &OptionKind, at: f32) -> Option<f64> {
    let at = f64::from(at.clamp(0.0, 1.0));
    match kind {
        OptionKind::Float { min, max, step, .. } => {
            let steps = float_steps(*min, *max, *step);
            let (min, max, step) = (f64::from(*min), f64::from(*max), f64::from(*step));
            Some(on_f32((min + (at * steps).round() * step).min(max)))
        }
        OptionKind::Int { min, max, .. } => {
            let (min, max) = (f64::from(*min), f64::from(*max));
            Some((min + at * (max - min)).round())
        }
        _ => None,
    }
}

/// `v` as the `f32` the pack's `override` gets: a step of 0.05 is no f32, and 20 of them would
/// otherwise miss a default of 1 by a hair.
fn on_f32(v: f64) -> f64 {
    f64::from(v as f32)
}

/// The value as the settings page shows it.
pub fn show_value(kind: &OptionKind, v: f64) -> String {
    match kind {
        OptionKind::Bool { .. } => crate::lang::get(if v != 0.0 {
            "options.on"
        } else {
            "options.off"
        }),
        OptionKind::Float { step, .. } => {
            let digits = if *step >= 1.0 {
                0
            } else if *step >= 0.1 {
                1
            } else {
                2
            };
            format!("{v:.digits$}")
        }
        OptionKind::Int { .. } => format!("{v:.0}"),
        OptionKind::Choice { values, .. } => {
            values.get(v.round() as usize).cloned().unwrap_or_default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn install(dir: &Path, file: &str, version: &str) {
        let manifest = format!(
            "schema = 1\nid = \"warm\"\nname = \"Warm\"\nversion = \"{version}\"\napi = \"0.1\"\n\
            [[post]]\nfile = \"p.wgsl\"\n[[options]]\nid = \"strength\"\nname = \"Strength\"\ntype = \"float\"\ndefault = 1.0\nmin = 0.0\nmax = 2.0\nstep = 0.5\n"
        );
        let files = [
            (horizon_shader_api::MANIFEST.into(), manifest.into_bytes()),
            ("p.wgsl".into(), b"original test fixture".to_vec()),
        ]
        .into();
        let pack = horizon_shader_api::Pack::from_files(files).unwrap();
        std::fs::write(dir.join(file), pack.package().unwrap()).unwrap();
    }

    #[test]
    fn native_selection_and_values_survive_install_update_rename_and_reset() {
        let dir =
            std::env::temp_dir().join(format!("horizon-identity-update-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        install(&dir, "warm-1.hshader", "1");
        std::fs::write(dir.join("warm-1.hshader.txt"), "strength=1.5\n").unwrap();
        let options = dir.join("options.txt");
        std::fs::write(&options, "").unwrap();
        std::fs::write(
            dir.join(crate::graphics::FILE),
            "pickedShaderPack:warm-1.hshader\n",
        )
        .unwrap();
        // Use the real settings loader, including persistence of the migrated selection.
        // Its shaderpacks folder sits beside options.txt.
        let packs = dir.join(DIR);
        std::fs::create_dir_all(&packs).unwrap();
        std::fs::rename(dir.join("warm-1.hshader"), packs.join("warm-1.hshader")).unwrap();
        std::fs::rename(
            dir.join("warm-1.hshader.txt"),
            packs.join("warm-1.hshader.txt"),
        )
        .unwrap();
        let settings = crate::settings::Settings::load(&options);
        assert_eq!(settings.horizon.shader_pack, "horizon:warm");
        assert_eq!(
            crate::graphics::Graphics::load(&dir).shader_pack,
            "horizon:warm"
        );
        assert_eq!(scan(&packs)[0].values, [1.5]);
        install(&packs, "warm-2.hshader", "2");
        std::fs::remove_file(packs.join("warm-1.hshader")).unwrap();
        let (_, path) = resolve(&packs, &settings.horizon.shader_pack);
        assert_eq!(path, packs.join("warm-2.hshader"));
        std::fs::rename(path, packs.join("renamed.hshader")).unwrap();
        let (_, path) = resolve(&packs, "horizon:warm");
        assert_eq!(path, packs.join("renamed.hshader"));
        let manifest = horizon_shader_api::Pack::read_manifest(&path).unwrap();
        assert_eq!(
            load_values(&native_values_pack(&path, &manifest.id), &manifest.options),
            [1.5]
        );
        let mut entry = scan(&packs).remove(0);
        entry.values[0] = 0.5;
        entry.save_values(&packs);
        assert_eq!(
            load_values(&native_values_pack(&path, &manifest.id), &manifest.options),
            [0.5]
        );
        entry.reset_options();
        entry.save_values(&packs);
        assert_eq!(scan(&packs)[0].values, [1.0]);
        assert_eq!(
            std::fs::read_to_string(packs.join("warm-1.hshader.txt")).unwrap(),
            "strength=1.5\n"
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn migrates_a_removed_registry_filename_and_resolves_duplicates_deterministically() {
        let dir = std::env::temp_dir().join(format!(
            "horizon-identity-duplicates-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        install(&dir, "z.hshader", "newest");
        install(&dir, "a.hshader", "oldest");
        std::fs::write(dir.join("warm-previous.hshader.txt"), "strength=2\n").unwrap();
        let (key, path) = resolve(&dir, "warm-previous.hshader");
        assert_eq!(key, "horizon:warm");
        assert_eq!(path, dir.join("a.hshader"));
        assert_eq!(resolve(&dir, "z.hshader"), (key.clone(), path));
        assert_eq!(scan(&dir).len(), 1);
        assert_eq!(scan(&dir)[0].values, [2.0]);
        // A classic folder named like a native ID keeps its filename identity.
        std::fs::create_dir_all(dir.join("warm/shaders")).unwrap();
        assert_eq!(resolve(&dir, "warm"), ("warm".into(), dir.join("warm")));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn lists_both_kinds_horizon_first_and_skips_the_rest() {
        let dir = std::env::temp_dir().join(format!("horizon-shader-list-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let d = dir.as_path();
        let manifest = "schema = 1\nid = \"warm\"\nname = \"Warm Light\"\nversion = \"1\"\n\
            api = \"0.1\"\n[[post]]\nfile = \"p.wgsl\"\n";
        std::fs::create_dir(d.join("warm")).unwrap();
        std::fs::write(d.join("warm").join(horizon_shader_api::MANIFEST), manifest).unwrap();
        std::fs::create_dir_all(d.join("Classic Folder/shaders")).unwrap();
        std::fs::write(d.join("archive.zip"), b"").unwrap();
        std::fs::write(d.join("notes.txt"), b"").unwrap();
        std::fs::create_dir(d.join("empty")).unwrap();
        std::fs::create_dir_all(d.join(".hidden/shaders")).unwrap();
        let packs = scan(d);
        let listed: Vec<(&str, &str, Kind)> = packs
            .iter()
            .map(|e| (e.file.as_str(), e.title.as_str(), e.kind))
            .collect();
        assert_eq!(
            listed,
            [
                ("warm", "Warm Light", Kind::Horizon),
                ("archive.zip", "archive.zip", Kind::Classic),
                ("Classic Folder", "Classic Folder", Kind::Classic),
            ]
        );
        assert!(scan(&d.join("missing")).is_empty());
        std::fs::remove_dir_all(d).unwrap();
    }

    fn option(id: &str, kind: OptionKind) -> PackOption {
        PackOption {
            id: id.into(),
            name: id.into(),
            description: String::new(),
            kind,
        }
    }

    #[test]
    fn values_step_wrap_and_round_trip() {
        let float = OptionKind::Float {
            default: 1.0,
            min: 0.0,
            max: 2.0,
            step: 0.5,
        };
        assert_eq!(step_value(&float, 1.0, false), 1.5);
        assert_eq!(step_value(&float, 2.0, false), 0.0);
        assert_eq!(step_value(&float, 0.0, true), 2.0);
        assert_eq!(show_value(&float, 1.5), "1.5");
        let choice = OptionKind::Choice {
            values: vec!["a".into(), "b".into(), "c".into()],
            default: 0,
        };
        assert_eq!(step_value(&choice, 2.0, false), 0.0);
        assert_eq!(step_value(&choice, 0.0, true), 2.0);
        assert_eq!(show_value(&choice, 1.0), "b");
        let int = OptionKind::Int {
            default: 0,
            min: -1,
            max: 1,
        };
        assert_eq!(step_value(&int, 1.0, false), -1.0);
        let on = OptionKind::Bool { default: true };
        assert_eq!(step_value(&on, 1.0, false), 0.0);

        let dir =
            std::env::temp_dir().join(format!("horizon-shader-values-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let pack = dir.join("p.hshader");
        let options = [option("a", float), option("b", choice)];
        // Only what differs from the defaults is written; out of range comes back clamped.
        save_values(&pack, &options, &[1.0, 2.0]).unwrap();
        let text = std::fs::read_to_string(values_path(&pack)).unwrap();
        assert_eq!(text, "b=2\n");
        std::fs::write(values_path(&pack), "a=9\nb=1\nzzz=4\n").unwrap();
        assert_eq!(load_values(&pack, &options), [2.0, 1.0]);
        save_values(&pack, &options, &[1.0, 0.0]).unwrap();
        assert_eq!(std::fs::read_to_string(values_path(&pack)).unwrap(), "");
        assert_eq!(load_values(&pack, &options), [1.0, 0.0]);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn sliders_take_the_nearest_step_and_stop_at_the_ends() {
        let float = |default, max, step| OptionKind::Float {
            default,
            min: 0.0,
            max,
            step,
        };
        let int = OptionKind::Int {
            default: 2,
            min: 0,
            max: 10,
        };
        let mut entry = Entry {
            file: "p".into(),
            key: "horizon:p".into(),
            title: "P".into(),
            kind: Kind::Horizon,
            options: vec![
                option("a", float(1.0, 2.0, 0.05)),
                option("b", int),
                option("c", OptionKind::Bool { default: true }),
                option("d", float(0.0, 1.0, 0.3)),
            ],
            values: vec![1.0, 2.0, 1.0, 0.0],
            pages: Vec::new(),
        };
        assert_eq!(entry.slider_position(0), Some(0.5));
        assert_eq!(entry.slider_position(1), Some(0.2));
        // A bool (and a choice) stays a button.
        assert_eq!(entry.slider_position(2), None);
        assert!(!entry.set_slider_position(2, 0.0));
        assert_eq!(entry.values[2], 1.0);
        // 0.26 of 0..2 is 0.52: the step below.
        assert!(entry.set_slider_position(0, 0.26));
        assert_eq!(show_value(&entry.options[0].kind, entry.values[0]), "0.50");
        // Twenty steps of 0.05 are the default of 1 exactly, so it isn't saved.
        assert!(entry.set_slider_position(0, 0.5));
        assert_eq!(entry.values[0], 1.0);
        assert!(!entry.set_slider_position(0, 0.505));
        // 5.7 of 0..10 is 6.
        assert!(entry.set_slider_position(1, 0.57));
        assert_eq!(entry.values[1], 6.0);
        // A range that isn't a whole number of steps still reaches its maximum.
        entry.set_slider_position(3, 1.0);
        assert_eq!(entry.values[3], 1.0);
        entry.set_slider_position(3, 0.6);
        assert_eq!(entry.values[3], f64::from(0.6_f32));
    }

    /// The arrow keys step a slider through its stops (the shell's `slider_step`, by the stops
    /// `float_steps` counts): a float whose range isn't a whole number of steps walks every one
    /// both ways, its maximum included, and stops at the ends.
    #[test]
    fn the_arrow_keys_walk_a_floats_stops_both_ways() {
        let kind = OptionKind::Float {
            default: 0.0,
            min: 0.0,
            max: 1.0,
            step: 0.3,
        };
        let mut entry = Entry {
            file: "p".into(),
            key: "horizon:p".into(),
            title: "P".into(),
            kind: Kind::Horizon,
            options: vec![horizon_shader_api::PackOption {
                id: "d".into(),
                name: "D".into(),
                description: String::new(),
                kind,
            }],
            values: vec![0.0],
            pages: Vec::new(),
        };
        let steps = float_steps(0.0, 1.0, 0.3) as u32;
        let mut walk = |forward: bool| {
            let mut seen = Vec::new();
            for _ in 0..steps + 2 {
                let at = entry.slider_position(0).unwrap();
                let next = crate::settings_shell::slider_step(at, Some(steps), 100.0, forward);
                entry.set_slider_position(0, next);
                seen.push(entry.values[0]);
            }
            seen
        };
        // The `k`th stop, as the pack's f32 `override` gets it.
        let stop = |k: u8| f64::from((f64::from(k) * f64::from(0.3_f32)) as f32);
        assert_eq!(walk(true), [stop(1), stop(2), stop(3), 1.0, 1.0, 1.0]);
        assert_eq!(walk(false), [stop(3), stop(2), stop(1), 0.0, 0.0, 0.0]);
    }
}
