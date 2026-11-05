//! Java mods (Fabric, NeoForge, Forge) from the browser (docs/mod-browser.md). Horizon doesn't
//! run their code; it ports what they carry (blocks, items, recipes, loot, tags, worldgen) with
//! `horizon-port-pack`. Before a mod is offered for install its jar is downloaded (checked as
//! every download is), carried over and its classes read (`horizon_port_pack::check`): the
//! verdict (Works, Partly, Doesn't run) and the lists of what ports and what doesn't come from
//! that, and are kept per file so a jar is checked once.
//!
//! Installing copies the carried-over folder mod into `horizon-mods/<mod id>/` beside
//! `options.txt` and turns it on in `horizon-mods.txt`, where the game's Options > Mods reads it.
use super::{installed::Entry, pick, FileRef, Kind, Project, Source, Support, Version};
use horizon_port_pack::check::{self, Check, Verdict};
use std::fs;
use std::path::{Path, PathBuf};

/// Where the checks are kept, under the game folder. Its name changes when the check finds
/// more, so older answers are checked again.
const CHECKS: &str = "mod-browser/checks-1";
/// The game's mod folder and on-list, beside `options.txt` (`horizon_client::mod_list`).
pub const MODS_DIR: &str = "horizon-mods";
const MODS_LIST: &str = "horizon-mods.txt";
/// How deep required mods are followed.
const MAX_DEPTH: usize = 3;

/// The loaders whose jars port-pack reads.
pub const LOADERS: [&str; 4] = ["neoforge", "fabric", "forge", "quilt"];

/// How well a checked mod runs.
pub fn support_of(check: &Check) -> Support {
    match check.verdict {
        Verdict::Works => Support::Works,
        Verdict::Partly => Support::Partial(check.summary.clone()),
        Verdict::No => Support::No(check.summary.clone()),
    }
}

fn checks(home: &Path) -> PathBuf {
    home.join(CHECKS)
}

/// The folder a file's check is kept in, by its checksum.
fn checked_dir(home: &Path, file: &FileRef) -> Option<PathBuf> {
    let (_, hash) = file.checksum()?;
    let hash: String = hash
        .chars()
        .filter(char::is_ascii_hexdigit)
        .take(40)
        .collect();
    (hash.len() >= 32).then(|| checks(home).join(hash))
}

fn read_check(dir: &Path) -> Option<Check> {
    serde_json::from_str(&fs::read_to_string(dir.join("check.json")).ok()?).ok()
}

/// The check of `file` if it was made before.
pub fn cached(home: &Path, file: &FileRef) -> Option<Check> {
    read_check(&checked_dir(home, file)?)
}

/// Checks `file`: the cached answer, else its jar downloaded, carried over and read. The jar
/// itself isn't kept; the carry is, when something of it ports, for the install to copy.
pub fn check_file(
    home: &Path,
    source: &dyn Source,
    file: &FileRef,
    progress: &dyn Fn(f32),
) -> Result<Check, String> {
    let dir = checked_dir(home, file).ok_or("Its source gives no checksum.")?;
    if let Some(c) = read_check(&dir) {
        return Ok(c);
    }
    let jar = source.fetch(file, &checks(home).join(".downloads"), progress)?;
    let part = dir.with_extension("part");
    let _ = fs::remove_dir_all(&part);
    let result = check::check(&jar, &part);
    let _ = fs::remove_file(&jar);
    let c = result.inspect_err(|_| {
        let _ = fs::remove_dir_all(&part);
    })?;
    if c.verdict == Verdict::No {
        for d in ["carry", "outside"] {
            let _ = fs::remove_dir_all(part.join(d));
        }
    }
    let _ = fs::remove_dir_all(&dir);
    fs::rename(&part, &dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    Ok(c)
}

/// The version of `project` to install and its check.
pub fn check_project(
    home: &Path,
    source: &dyn Source,
    project: &Project,
    everything: bool,
    progress: &dyn Fn(f32),
) -> Result<(Version, Check), String> {
    let versions = source.versions(project)?;
    let version = pick(Kind::Mod, &versions, everything)
        .cloned()
        .ok_or_else(|| match versions.iter().find_map(|v| v.blocked.clone()) {
            Some(why) => format!("{} can't be downloaded from here: {why}", project.title),
            None => format!(
                "{} has no Fabric, NeoForge or Forge version Horizon can port.",
                project.title
            ),
        })?;
    let file = version.file().expect("pick keeps versions with a file");
    let check = check_file(home, source, file, progress)?;
    Ok((version, check))
}

/// Whether mod `id` is on in `horizon-mods.txt`.
pub fn is_enabled(home: &Path, id: &str) -> bool {
    let key = format!("mod.{id}");
    fs::read_to_string(home.join(MODS_LIST))
        .unwrap_or_default()
        .lines()
        .filter_map(|l| l.split_once(':'))
        .any(|(k, v)| k.trim() == key && v.trim() == "true")
}

/// Turns mod `id` on (or off) in `horizon-mods.txt`, keeping the other lines.
pub fn set_enabled(home: &Path, id: &str, on: bool) -> Result<(), String> {
    let path = home.join(MODS_LIST);
    let text = fs::read_to_string(&path).unwrap_or_default();
    let key = format!("mod.{id}");
    let mut lines: Vec<String> = text
        .lines()
        .filter(|l| l.split_once(':').is_none_or(|(k, _)| k.trim() != key))
        .map(str::to_owned)
        .collect();
    if on {
        lines.push(format!("{key}:true"));
    }
    let mut out = lines.join("\n");
    if !out.is_empty() {
        out.push('\n');
    }
    fs::write(&path, out).map_err(|e| format!("{}: {e}", path.display()))
}

fn copy_dir(from: &Path, to: &Path) -> Result<(), String> {
    fs::create_dir_all(to).map_err(|e| format!("{}: {e}", to.display()))?;
    for entry in fs::read_dir(from).map_err(|e| format!("{}: {e}", from.display()))? {
        let entry = entry.map_err(|e| e.to_string())?;
        let (src, dst) = (entry.path(), to.join(entry.file_name()));
        if entry.file_type().map_err(|e| e.to_string())?.is_dir() {
            copy_dir(&src, &dst)?;
        } else {
            fs::copy(&src, &dst).map_err(|e| format!("{}: {e}", dst.display()))?;
        }
    }
    Ok(())
}

/// Installs the Java mod `project`: checked, carried over into `horizon-mods/<id>/` and turned
/// on, with the mods it requires that port too. A required mod with nothing to port (a library)
/// is listed in what was skipped. Returns the entries installed, the asked-for one last.
pub fn install(
    source: &dyn Source,
    project: &Project,
    everything: bool,
    home: &Path,
    progress: &dyn Fn(&str, f32),
) -> Result<Vec<Entry>, String> {
    let job = Job {
        source,
        everything,
        home,
        progress,
    };
    let mut done = Vec::new();
    job.add(project, false, 0, &mut done)?;
    Ok(done)
}

/// One install: where from, where to, and how it reports.
struct Job<'a> {
    source: &'a dyn Source,
    everything: bool,
    home: &'a Path,
    progress: &'a dyn Fn(&str, f32),
}

impl Job<'_> {
    fn add(
        &self,
        project: &Project,
        dependency: bool,
        depth: usize,
        done: &mut Vec<Entry>,
    ) -> Result<(), String> {
        let Job {
            source,
            everything,
            home,
            progress,
        } = *self;
        if done.iter().any(|e| e.project == project.id) {
            return Ok(());
        }
        let (version, check) = check_project(home, source, project, everything, &|f| {
            progress(&format!("Checking {}", project.title), f)
        })?;
        if check.verdict == Verdict::No {
            return Err(format!(
                "{} doesn't run in Horizon: {}",
                project.title, check.summary
            ));
        }
        let mut skipped = check.skipped.clone();
        for id in &version.required {
            let Ok(needed) = source.project(id) else {
                continue;
            };
            if needed.kind != Kind::Mod || depth >= MAX_DEPTH {
                continue;
            }
            match self.add(&needed, true, depth + 1, done) {
                Ok(()) => {}
                Err(_) if dependency => {}
                Err(why) => skipped.push(format!("needs {}: {why}", needed.title)),
            }
        }
        let file = version.file().expect("pick keeps versions with a file");
        let carry = checked_dir(home, file)
            .expect("checked above")
            .join("carry");
        let relative = format!("{MODS_DIR}/{}", check.mod_id);
        let mut installed = super::installed::Installed::load(home);
        let ours = |e: &Entry| e.source == project.source && e.project == project.id;
        if let Some(other) = installed.entries.iter().find(|e| {
            e.kind == Kind::Mod
                && !ours(e)
                && (e.file == relative || e.namespaces.iter().any(|n| check.namespaces.contains(n)))
        }) {
            return Err(format!(
                "{} shares its id or namespace with {}, installed already: remove one first.",
                project.title, other.title
            ));
        }
        let target = home.join(&relative);
        let old = installed.entries.iter().find(|e| ours(e)).cloned();
        if target.exists() && old.as_ref().is_none_or(|o| o.file != relative) {
            return Err(format!(
                "{relative} is already there and wasn't installed by the browser: move it away first."
            ));
        }
        progress(&format!("Installing {}", project.title), 1.);
        let part = home.join(MODS_DIR).join(format!(".{}.part", check.mod_id));
        let _ = fs::remove_dir_all(&part);
        copy_dir(&carry, &part).inspect_err(|_| {
            let _ = fs::remove_dir_all(&part);
        })?;
        if let Some(old) = &old {
            if let Some(dir) = old.path(home) {
                let _ = fs::remove_dir_all(dir);
            }
            if old.file != relative {
                let _ = set_enabled(home, old.file.trim_start_matches("horizon-mods/"), false);
            }
        }
        fs::rename(&part, &target).map_err(|e| format!("{}: {e}", target.display()))?;
        set_enabled(home, &check.mod_id, true)?;
        let (algo, hash) = file.checksum().expect("checked files have one");
        let entry = Entry {
            source: project.source,
            project: project.id.clone(),
            slug: project.slug.clone(),
            title: project.title.clone(),
            kind: Kind::Mod,
            version: version.id.clone(),
            number: version.number.clone(),
            file: relative,
            checksum: format!("{algo}:{hash}"),
            page: project.page.clone(),
            dependency: dependency && old.as_ref().is_none_or(|o| o.dependency),
            verdict: Some(check.verdict.label().to_owned()),
            skipped,
            namespaces: check.namespaces.clone(),
        };
        installed.entries.retain(|e| !ours(e));
        installed.entries.push(entry.clone());
        installed.save(home)?;
        done.push(entry);
        Ok(())
    }
}

/// Removes an installed mod's folder and turns it off.
pub fn remove(home: &Path, entry: &Entry) -> Result<(), String> {
    let Some(id) = entry.file.strip_prefix("horizon-mods/") else {
        return Err(format!("{} isn't in {MODS_DIR}/.", entry.file));
    };
    match entry.path(home).map_or(Ok(()), fs::remove_dir_all) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(format!("{}: {e}", entry.file)),
    }
    set_enabled(home, id, false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_on_list_keeps_other_lines() {
        let home = std::env::temp_dir().join(format!("horizon-java-mods-{}", std::process::id()));
        let _ = fs::remove_dir_all(&home);
        fs::create_dir_all(&home).unwrap();
        fs::write(
            home.join(MODS_LIST),
            "mod.coords_hud:true\nmod.ruby:false\n",
        )
        .unwrap();
        set_enabled(&home, "ruby", true).unwrap();
        set_enabled(&home, "gem", true).unwrap();
        assert_eq!(
            fs::read_to_string(home.join(MODS_LIST)).unwrap(),
            "mod.coords_hud:true\nmod.ruby:true\nmod.gem:true\n"
        );
        set_enabled(&home, "ruby", false).unwrap();
        set_enabled(&home, "gem", false).unwrap();
        assert_eq!(
            fs::read_to_string(home.join(MODS_LIST)).unwrap(),
            "mod.coords_hud:true\n"
        );
        fs::remove_dir_all(&home).unwrap();
    }

    #[test]
    fn checks_are_kept_by_checksum() {
        let home = Path::new("/h");
        let file = |sha512: Option<&str>, sha1: Option<&str>| FileRef {
            url: String::new(),
            name: "a.jar".into(),
            size: 1,
            sha512: sha512.map(str::to_owned),
            sha256: None,
            sha1: sha1.map(str::to_owned),
            primary: true,
        };
        let long = "ab".repeat(64);
        assert_eq!(
            checked_dir(home, &file(Some(&long), None)).unwrap(),
            home.join(CHECKS).join("ab".repeat(20))
        );
        assert!(checked_dir(home, &file(None, Some("../../etc"))).is_none());
        assert!(checked_dir(home, &file(None, None)).is_none());
    }
}
