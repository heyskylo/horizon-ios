//! What the browser installed into the game folder, and installing, updating and removing it.
//!
//! `mod-browser.json` beside `options.txt` lists each file the browser put in `resourcepacks/`,
//! and each Java mod it ported into `horizon-mods/`: where it came from, its version and its
//! checksum. Files the player put there themselves are never touched: the browser only replaces
//! or removes what it lists.
use super::{pick, safe_name, Kind, Project, Source, SourceId, Support, Version};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Component, Path, PathBuf};

pub(crate) const MANIFEST: &str = "mod-browser.json";
/// The folders the browser installs into: an entry's `file` is always inside one of them.
const FOLDERS: [&str; 5] = [
    "mods",
    "resourcepacks",
    "shaderpacks",
    super::java::MODS_DIR,
    crate::native_mods::INSTALLED,
];
/// How deep required dependencies are followed.
const MAX_DEPTH: usize = 4;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Entry {
    pub source: SourceId,
    pub project: String,
    pub slug: String,
    pub title: String,
    pub kind: Kind,
    pub version: String,
    pub number: String,
    /// Relative to the game folder: `resourcepacks/<name>`.
    pub file: String,
    /// The checksum the download was checked against: `SHA-512:<hex>`.
    pub checksum: String,
    pub page: String,
    /// Installed because another pack needs it.
    #[serde(default)]
    pub dependency: bool,
    /// A Java mod's verdict when it was installed: `Works` or `Partly`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verdict: Option<String>,
    /// What of a Java mod didn't port.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub skipped: Vec<String>,
    /// A Java mod's namespaces: two installed mods never share one.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub namespaces: Vec<String>,
}

/// Whether `file` names something inside one of the folders the browser installs into, made of
/// plain names only (no root, drive or `..`), so `home.join(file)` stays in the game folder. The
/// list is a file in that folder, which a modpack could have put there: an entry naming any other
/// place must never make the browser remove or rename it.
pub fn in_folders(file: &str) -> bool {
    let mut parts = Path::new(file).components();
    let top = parts.next();
    matches!(top, Some(Component::Normal(t)) if FOLDERS.iter().any(|f| t == *f))
        && parts.clone().next().is_some()
        && parts.all(|c| matches!(c, Component::Normal(_)))
}

impl Entry {
    /// Where the entry's file is in the game folder `home`, when it is inside one the browser
    /// installs into ([`in_folders`]).
    pub fn path(&self, home: &Path) -> Option<PathBuf> {
        in_folders(&self.file).then(|| home.join(&self.file))
    }
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Installed {
    /// An entry this launcher can't read (a shader pack, which older launchers installed) is
    /// left out, and its file left where it is; so is one whose file is outside the folders the
    /// browser installs into ([`in_folders`]).
    #[serde(deserialize_with = "readable_entries")]
    pub entries: Vec<Entry>,
}

fn readable_entries<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Vec<Entry>, D::Error> {
    let entries = Vec::<serde_json::Value>::deserialize(d)?;
    Ok(entries
        .into_iter()
        .filter_map(|e| serde_json::from_value::<Entry>(e).ok())
        .filter(|e| {
            let kept = in_folders(&e.file);
            if !kept {
                eprintln!(
                    "{MANIFEST}: left out {:?}, whose file {:?} isn't in a pack or mod folder",
                    e.title, e.file
                );
            }
            kept
        })
        .collect())
}

impl Installed {
    pub fn load(home: &Path) -> Self {
        fs::read_to_string(home.join(MANIFEST))
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, home: &Path) -> Result<(), String> {
        let _lock = super::lock_home(home);
        static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let id = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let path = home.join(MANIFEST);
        let part = home.join(format!(".{MANIFEST}.{}-{id}.part", std::process::id()));
        let text = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        fs::create_dir_all(home).map_err(|e| format!("{}: {e}", home.display()))?;
        let res = fs::write(&part, text).and_then(|()| fs::rename(&part, &path));
        if let Err(e) = res {
            let _ = fs::remove_file(&part);
            return Err(format!("{}: {e}", path.display()));
        }
        Ok(())
    }

    pub fn find(&self, source: SourceId, project: &str) -> Option<&Entry> {
        self.entries
            .iter()
            .find(|e| e.source == source && e.project == project)
    }

    /// By project id or slug, as the CLI takes them.
    pub fn lookup(&self, name: &str) -> Option<&Entry> {
        self.entries
            .iter()
            .find(|e| e.project == name || e.slug == name)
    }
}

/// One file to install: the project, the version chosen and why it was pulled in.
#[derive(Clone, Debug)]
pub struct Step {
    pub project: Project,
    pub version: Version,
    pub dependency: bool,
}

/// What installing `project` takes, dependencies first, checked before any byte is downloaded:
/// every file runs in Horizon and has a version to install.
pub fn plan(source: &dyn Source, project: &Project, everything: bool) -> Result<Vec<Step>, String> {
    if project.kind == Kind::Modpack {
        return Err(format!(
            "{} is a modpack: it is imported, not installed.",
            project.title
        ));
    }
    if project.kind.folder().is_none() {
        return Err(format!(
            "{} is ported, not installed as a file.",
            project.title
        ));
    }
    let mut steps = Vec::new();
    add(source, project.clone(), everything, false, 0, &mut steps)?;
    Ok(steps)
}

fn add(
    source: &dyn Source,
    project: Project,
    everything: bool,
    dependency: bool,
    depth: usize,
    steps: &mut Vec<Step>,
) -> Result<(), String> {
    if steps.iter().any(|s| s.project.id == project.id) {
        return Ok(());
    }
    if depth > MAX_DEPTH {
        return Err(format!("{}'s dependencies go too deep.", project.title));
    }
    if let Support::No(why) = project.support() {
        return Err(format!("{} doesn't run in Horizon: {why}", project.title));
    }
    let versions = source.versions(&project)?;
    let Some(version) = pick(project.kind, &versions, everything).cloned() else {
        return Err(match versions.iter().find_map(|v| v.blocked.clone()) {
            Some(why) => format!("{} can't be installed from here: {why}", project.title),
            None => format!("{} has no version Horizon runs.", project.title),
        });
    };
    for id in &version.required {
        let needed = source.project(id).map_err(|_| {
            format!(
                "{} needs {id} on {}, which isn't a pack Horizon loads (a Java mod?).",
                project.title,
                source.id().label()
            )
        })?;
        if needed.kind.folder().is_none() {
            return Err(format!("{} needs {}.", project.title, needed.title));
        }
        add(source, needed, everything, true, depth + 1, steps)?;
    }
    steps.push(Step {
        project,
        version,
        dependency,
    });
    Ok(())
}

/// Installs `project` (and what it needs) into the game folder `home`. A project already
/// installed is updated to the version chosen. `progress` gets a line per file and the fraction.
pub fn install(
    source: &dyn Source,
    project: &Project,
    everything: bool,
    home: &Path,
    progress: &dyn Fn(&str, f32),
) -> Result<Vec<Entry>, String> {
    let _lock = super::lock_home(home);
    if project.source == SourceId::Horizon {
        return super::horizon::install(project, home, progress);
    }
    if project.kind == Kind::Mod {
        return super::java::install(source, project, everything, home, progress);
    }
    let steps = plan(source, project, everything)?;
    put(
        source,
        steps,
        home,
        &|kind| kind.folder().expect("plan keeps folder kinds"),
        progress,
    )
}

/// Downloads each step's file into `home`, in the folder `folder` names for its kind, and lists
/// it: what was already there at that version is kept, an older version replaced.
pub(super) fn put(
    source: &dyn Source,
    steps: Vec<Step>,
    home: &Path,
    folder: &dyn Fn(Kind) -> &'static str,
    progress: &dyn Fn(&str, f32),
) -> Result<Vec<Entry>, String> {
    let _lock = super::lock_home(home);
    let mut installed = Installed::load(home);
    let mut done = Vec::new();
    for step in steps {
        let p = &step.project;
        let old = installed.find(p.source, &p.id).cloned();
        if old.as_ref().is_some_and(|o| o.version == step.version.id) {
            done.push(old.expect("checked"));
            continue;
        }
        let file = step
            .version
            .file()
            .expect("pick keeps versions with a file");
        let folder_name = folder(p.kind);
        let name = safe_name(&file.name)
            .ok_or_else(|| format!("{:?} isn't a plain file name.", file.name))?;
        let relative = format!("{folder_name}/{name}");
        let owner = installed.entries.iter().find(|e| e.file == relative);
        let ours = owner.is_some_and(|e| e.source == p.source && e.project == p.id);
        if home.join(&relative).exists() && !ours {
            return Err(match owner {
                Some(e) => format!(
                    "{relative} belongs to {}, installed from the browser.",
                    e.title
                ),
                None => format!(
                    "{relative} is already there and wasn't installed by the browser: move it \
                     away first, or keep it."
                ),
            });
        }
        let line = format!("{} {}", p.title, step.version.number);
        let path = source.fetch(file, &home.join(folder_name), &|f| progress(&line, f))?;
        let (algo, hash) = file.checksum().expect("fetch checks for one");
        if let Some(old) = &old {
            if let Some(file) = old.path(home).filter(|_| old.file != relative) {
                let _ = fs::remove_file(file);
            }
        }
        let entry = Entry {
            source: p.source,
            project: p.id.clone(),
            slug: p.slug.clone(),
            title: p.title.clone(),
            kind: p.kind,
            version: step.version.id.clone(),
            number: step.version.number.clone(),
            file: relative,
            checksum: format!("{algo}:{hash}"),
            page: p.page.clone(),
            // Asked for by the player once: it stays theirs.
            dependency: step.dependency && old.as_ref().is_none_or(|o| o.dependency),
            verdict: None,
            skipped: Vec::new(),
            namespaces: Vec::new(),
        };
        debug_assert!(path.ends_with(name));
        installed
            .entries
            .retain(|e| !(e.source == p.source && e.project == p.id));
        installed.entries.push(entry.clone());
        installed.save(home)?;
        done.push(entry);
    }
    Ok(done)
}

/// Removes what the browser installed for `name` (a project id or slug): its file and its entry.
pub fn remove(home: &Path, name: &str) -> Result<Entry, String> {
    let _lock = super::lock_home(home);
    let mut installed = Installed::load(home);
    let entry = installed
        .lookup(name)
        .cloned()
        .ok_or_else(|| format!("{name} wasn't installed by the browser."))?;
    if entry.kind == Kind::Mod && entry.source != SourceId::Horizon {
        super::java::remove(home, &entry)?;
    } else {
        match entry.path(home).map_or(Ok(()), fs::remove_file) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(format!("{}: {e}", entry.file)),
        }
    }
    if let Some(id) = super::horizon::mod_id(&entry).filter(|_| entry.source == SourceId::Horizon) {
        // The `.hmod` a mod that turned native left behind (horizon::install_version).
        if entry.file.ends_with(".hnmod") {
            let _ = fs::remove_file(home.join(super::java::MODS_DIR).join(format!("{id}.hmod")));
        }
        super::java::set_enabled(home, id, false)?;
    }
    installed
        .entries
        .retain(|e| !(e.source == entry.source && e.project == entry.project));
    installed.save(home)?;
    Ok(entry)
}

/// The newer version `entry`, installed in game folder `home`, can be updated to, if any. A
/// registry native mod's is the newest that builds on the Horizon `home` plays.
pub fn update_for(
    source: &dyn Source,
    entry: &Entry,
    home: &Path,
) -> Result<Option<Version>, String> {
    if entry.source == SourceId::Horizon {
        if let Some(update) = super::horizon::fitting_update(&entry.project, &entry.version, home) {
            return Ok(update);
        }
    }
    let project = source.project(&entry.project)?;
    let versions = source.versions(&project)?;
    Ok(pick(project.kind, &versions, true)
        .filter(|v| v.id != entry.version)
        .cloned())
}

/// Downloads a modpack's file, checked, into `folder` (kept there: the import reads it).
pub fn fetch_modpack(
    source: &dyn Source,
    project: &Project,
    folder: &Path,
    progress: &dyn Fn(f32),
) -> Result<PathBuf, String> {
    if project.kind != Kind::Modpack {
        return Err(format!("{} isn't a modpack.", project.title));
    }
    let versions = source.versions(project)?;
    let version = pick(Kind::Modpack, &versions, true)
        .ok_or_else(|| format!("{} has no file to download.", project.title))?;
    let file = version.file().expect("pick keeps versions with a file");
    source.fetch(file, folder, progress)
}

/// A data pack installed into a world.
pub struct DataPackInstall {
    pub pack: crate::datapacks::DataPack,
    /// Its version's number.
    pub number: String,
    /// What of it runs in Horizon.
    pub coverage: crate::datapacks::Coverage,
}

/// Downloads a project's data pack (checksum checked) into the world folder `world`, under a
/// free name. Refused when nothing in it runs in Horizon yet. It isn't recorded with the
/// browser's installs: the world's data pack list (Home, `worlds datapack-list`) has it.
pub fn install_datapack(
    source: &dyn Source,
    project: &Project,
    everything: bool,
    world: &Path,
    progress: &dyn Fn(f32),
) -> Result<DataPackInstall, String> {
    if project.kind != Kind::DataPack && !project.loaders.iter().any(|l| l == "datapack") {
        return Err(format!("{} isn't a data pack.", project.title));
    }
    let folder = std::env::temp_dir().join(format!(
        "horizon-launcher-datapack-{}-{}",
        std::process::id(),
        project.id
    ));
    // The registry's are checked against its index (signature, `pack.mcmeta` and `data/`).
    let fetched = match project.source {
        SourceId::Horizon => super::horizon::fetch_datapack(project, &folder, progress),
        _ => {
            let versions = source.versions(project)?;
            let version = pick(Kind::DataPack, &versions, everything).ok_or_else(|| {
                format!(
                    "{} has no data pack for {}{}.",
                    project.title,
                    super::GAME,
                    if everything {
                        " or any other version"
                    } else {
                        ""
                    }
                )
            })?;
            let file = version.file().expect("pick keeps versions with a file");
            source
                .fetch(file, &folder, progress)
                .map(|zip| (zip, version.number.clone()))
        }
    };
    let result = fetched.and_then(|(zip, number)| {
        let coverage = crate::datapacks::coverage(&zip)?;
        if coverage.read.is_empty() {
            return Err(crate::i18n::trf(
                "{title} would change nothing in Horizon yet: Horizon doesn't read its {files}.",
                &[("title", &project.title), ("files", &coverage.not_read())],
            ));
        }
        let pack = crate::datapacks::add(world, &zip)?;
        Ok(DataPackInstall {
            pack,
            number,
            coverage,
        })
    });
    let _ = fs::remove_dir_all(&folder);
    result
}

#[cfg(test)]
mod tests;
