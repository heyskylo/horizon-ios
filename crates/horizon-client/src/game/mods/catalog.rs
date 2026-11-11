//! Worker-owned descriptions and immutable sources for content generations.
use horizon_assets::root_lease::RootLease;
use horizon_client::mod_list::Installed;
use horizon_client::mod_registry::Cancellation;
use horizon_mods::{Capability, Content, Manifest, Package};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::Arc,
};

#[derive(Clone, Debug)]
pub(crate) struct Entry {
    pub revision: String,
    pub live: bool,
    pub source: PathBuf,
    pub manifest: Manifest,
    pub dev: bool,
    pub lease: RootLease,
}
impl PartialEq for Entry {
    fn eq(&self, other: &Self) -> bool {
        self.revision == other.revision && self.live == other.live && self.dev == other.dev
    }
}
impl Eq for Entry {}

#[derive(Clone, Debug, Default)]
pub(crate) struct Catalog {
    pub native_ids: BTreeSet<String>,
    pub rows: Vec<Installed>,
    pub entries: BTreeMap<String, Arc<Entry>>,
    pub order: Vec<String>,
    pub invalid: BTreeSet<String>,
    pub built: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Selection {
    pub enabled: BTreeSet<String>,
    pub entries: BTreeMap<String, Arc<Entry>>,
    pub order: Vec<String>,
}
impl Selection {
    pub fn wanted(enabled: &BTreeSet<String>, catalog: &Catalog) -> Self {
        Self {
            enabled: enabled.clone(),
            entries: catalog
                .entries
                .iter()
                .filter(|(id, entry)| enabled.contains(*id) || entry.dev)
                .map(|(id, entry)| (id.clone(), entry.clone()))
                .collect(),
            order: catalog
                .order
                .iter()
                .filter(|id| {
                    catalog
                        .entries
                        .get(*id)
                        .is_some_and(|entry| enabled.contains(*id) || entry.dev)
                })
                .cloned()
                .collect(),
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct PreparedContent {
    pub root: PathBuf,
    pub catalog: Arc<Catalog>,
    pub selection: Selection,
    pub lease: RootLease,
    pub data_changed: bool,
}

pub(crate) struct Switch {
    pub next: Selection,
    pub waiting: BTreeSet<String>,
}

/// Classify differences against the running generation, not just enabled IDs.
/// Both generations must be live-safe: removing old registry content also
/// changes IDs even if the replacement contains only assets.
pub(crate) fn switch_plan(
    applied: &Selection,
    wanted: &BTreeSet<String>,
    catalog: &Catalog,
    local: bool,
) -> Switch {
    let mut next = Selection::wanted(wanted, catalog);
    let ids: BTreeSet<_> = applied
        .entries
        .keys()
        .chain(next.entries.keys())
        .chain(
            catalog
                .invalid
                .iter()
                .filter(|id| wanted.contains(*id) || applied.enabled.contains(*id)),
        )
        .cloned()
        .collect();
    let mut waiting = BTreeSet::new();
    for id in ids {
        let old = applied.entries.get(&id);
        let new = next.entries.get(&id);
        if old == new && !catalog.invalid.contains(&id) {
            continue;
        }
        let can_apply = local
            && !catalog.invalid.contains(&id)
            && old.is_none_or(|entry| entry.live)
            && new.is_none_or(|entry| entry.live);
        if can_apply {
            continue;
        }
        waiting.insert(id.clone());
        match old {
            Some(entry) => {
                next.entries.insert(id.clone(), entry.clone());
            }
            None => {
                next.entries.remove(&id);
            }
        }
        if applied.enabled.contains(&id) {
            next.enabled.insert(id);
        } else {
            next.enabled.remove(&id);
        }
    }
    // Keep existing registry precedence stable through a live switch. Newly
    // enabled assets/data-only entries follow the captured catalog order.
    let mut order: Vec<_> = applied
        .order
        .iter()
        .filter(|id| next.entries.contains_key(*id))
        .cloned()
        .collect();
    for id in &catalog.order {
        if next.entries.contains_key(id) && !order.contains(id) {
            order.push(id.clone());
        }
    }
    next.order = order;
    Switch { next, waiting }
}

fn cancelled(cancel: &Cancellation) -> Result<(), String> {
    if cancel.cancelled() {
        Err("mod catalogue read cancelled".into())
    } else {
        Ok(())
    }
}

/// Hash/copy on the worker in bounded chunks. Reuse the project's SHA-256
/// machinery; no package-wide byte allocation is needed to retain an old source.
fn hash_file(path: &Path, hash: &mut Sha256, cancel: &Cancellation) -> Result<(), String> {
    let mut file = fs::File::open(path).map_err(|e| e.to_string())?;
    let mut buffer = [0; 64 * 1024];
    loop {
        cancelled(cancel)?;
        let count = file.read(&mut buffer).map_err(|e| e.to_string())?;
        if count == 0 {
            break;
        }
        hash.update(&buffer[..count]);
    }
    Ok(())
}

fn revision(path: &Path, files: &[String], cancel: &Cancellation) -> Result<String, String> {
    let mut hash = Sha256::new();
    if path.is_dir() {
        for name in files {
            hash.update((name.len() as u64).to_le_bytes());
            hash.update(name.as_bytes());
            hash.update(
                fs::metadata(path.join(name))
                    .map_err(|e| e.to_string())?
                    .len()
                    .to_le_bytes(),
            );
            hash_file(&path.join(name), &mut hash, cancel)?;
        }
    } else {
        hash_file(path, &mut hash, cancel)?;
    }
    Ok(format!("{:x}", hash.finalize()))
}

fn copy_file(from: &Path, to: &Path, cancel: &Cancellation) -> Result<(), String> {
    fs::create_dir_all(to.parent().ok_or("snapshot file has no parent")?)
        .map_err(|e| e.to_string())?;
    let mut input = fs::File::open(from).map_err(|e| e.to_string())?;
    let mut output = fs::File::create(to).map_err(|e| e.to_string())?;
    let mut buffer = [0; 64 * 1024];
    loop {
        cancelled(cancel)?;
        let count = input.read(&mut buffer).map_err(|e| e.to_string())?;
        if count == 0 {
            break;
        }
        output
            .write_all(&buffer[..count])
            .map_err(|e| e.to_string())?;
    }
    output.sync_all().map_err(|e| e.to_string())
}

/// A running registry-bearing mod must remain reproducible after an update
/// replaces its installed archive. Retain a verified immutable source and its
/// lease, rather than quietly rebuilding that old ID from the new archive.
fn pin_source(
    home: &Path,
    path: &Path,
    content: &Content,
    cancel: &Cancellation,
) -> Result<(PathBuf, String, RootLease), String> {
    let directory = path.is_dir();
    let mut files = content.files.clone();
    files.push(horizon_mods::manifest::MANIFEST.into());
    if directory && path.join(horizon_mods::manifest::WASM).is_file() {
        files.push(horizon_mods::manifest::WASM.into());
    }
    files.sort();
    files.dedup();
    let digest = revision(path, &files, cancel)?;
    let root = home.join(".horizon-cache/mod-sources").join(&digest);
    let _builder = RootLease::build_lock(&root).map_err(|e| e.to_string())?;
    let source = if directory {
        root.join("files")
    } else {
        root.join("package.hmod")
    };
    let stamp = root.join("horizon-mod-source.stamp");
    let lease = RootLease::acquire(&root).map_err(|e| e.to_string())?;
    if fs::read_to_string(&stamp).is_ok_and(|kept| kept == digest) && source.exists() {
        cancelled(cancel)?;
        fs::write(&stamp, &digest).map_err(|e| e.to_string())?;
        return Ok((source, digest, lease));
    }
    drop(lease);
    // The caller holds the home snapshot gate; no other builder can publish
    // this source while its previous interrupted attempt is being replaced.
    let writer = RootLease::try_exclusive(&root)
        .map_err(|e| e.to_string())?
        .ok_or("incomplete mod snapshot still has readers; retry after they finish")?;
    if root.exists() {
        fs::remove_dir_all(&root).map_err(|e| e.to_string())?;
    }
    fs::create_dir_all(&root).map_err(|e| e.to_string())?;
    if directory {
        for name in &files {
            copy_file(&path.join(name), &source.join(name), cancel)?;
        }
    } else {
        copy_file(path, &source, cancel)?;
    }
    if directory {
        let package = Package::read(path)?;
        let current = Content::read(path, &package.manifest)?;
        let mut current_files = current.files;
        current_files.push(horizon_mods::manifest::MANIFEST.into());
        if path.join(horizon_mods::manifest::WASM).is_file() {
            current_files.push(horizon_mods::manifest::WASM.into());
        }
        current_files.sort();
        current_files.dedup();
        if current_files != files {
            return Err("mod files changed during snapshot preparation; retry".into());
        }
    }
    if revision(&source, &files, cancel)? != digest || revision(path, &files, cancel)? != digest {
        return Err(
            "mod changed while its source snapshot was being built; retry the refresh".into(),
        );
    }
    cancelled(cancel)?;
    fs::write(&stamp, &digest).map_err(|e| e.to_string())?;
    let lease = writer
        .into_shared(RootLease::default())
        .map_err(|e| e.to_string())?;
    Ok((source, digest, lease))
}

/// The caller holds mod_registry::with_content_snapshot on a worker.
pub(crate) fn read(
    home: &Path,
    dev: &[PathBuf],
    cancel: &Cancellation,
) -> Result<Arc<Catalog>, String> {
    let version = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(Path::to_owned));
    read_at(home, dev, cancel, version.as_deref(), |_| {})
}

pub(crate) fn read_at(
    home: &Path,
    dev: &[PathBuf],
    cancel: &Cancellation,
    version: Option<&Path>,
    mut before_pin: impl FnMut(&Path),
) -> Result<Arc<Catalog>, String> {
    let native = version
        .map(|p| horizon_mods::find(&p.join("native-content")))
        .unwrap_or_default();
    let native_ids: BTreeSet<_> = native
        .iter()
        .filter_map(|p| Package::read(p).ok().map(|p| p.manifest.id))
        .collect();
    let mut paths = native.clone();
    paths.extend(
        horizon_mods::find(&home.join(horizon_client::mod_list::DIR))
            .into_iter()
            .filter(|p| {
                !Package::read(p)
                    .ok()
                    .is_some_and(|pkg| native_ids.contains(&pkg.manifest.id))
            }),
    );
    paths.retain(|path| !dev.contains(path));
    paths.extend(dev.iter().cloned());
    let mut catalog = Catalog {
        native_ids,
        ..Default::default()
    };
    for path in paths {
        cancelled(cancel)?;
        let mut row = Installed {
            id: String::new(),
            title: path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned(),
            description: String::new(),
            problem: None,
            dev: dev.contains(&path),
            native: None,
        };
        let result = (|| {
            let package = Package::read(&path)?;
            let manifest = &package.manifest;
            row.id = manifest.id.clone();
            row.title = format!("{} {}", manifest.name, manifest.version);
            row.description = manifest.description.clone();
            if package.wasm.is_some() {
                row.problem = Some(super::STOPPED.into());
            }
            if !manifest.capabilities.contains(&Capability::Registries) {
                return Ok(());
            }
            let declared: Vec<String> = super::native_declared()
                .1
                .iter()
                .filter(|item| {
                    item.id
                        .split_once(':')
                        .is_some_and(|(ns, _)| ns == manifest.id)
                })
                .map(|item| item.id.clone())
                .collect();
            let content = Content::read_declaring(&path, manifest, &declared)?;
            before_pin(&path);
            let (source, revision, lease) = pin_source(home, &path, &content, cancel)?;
            // Classification and metadata describe the pinned bytes, not a
            // mutable package read before a developer edited it.
            let pinned = Package::read(&source)?;
            let manifest = &pinned.manifest;
            let declared: Vec<String> = super::native_declared()
                .1
                .iter()
                .filter(|item| {
                    item.id
                        .split_once(':')
                        .is_some_and(|(ns, _)| ns == manifest.id)
                })
                .map(|item| item.id.clone())
                .collect();
            let content = Content::read_declaring(&source, manifest, &declared)?;
            let live =
                pinned.wasm.is_none() && horizon_client::mod_content::live_switch(&content).is_ok();
            row.id = manifest.id.clone();
            row.title = format!("{} {}", manifest.name, manifest.version);
            row.description = manifest.description.clone();
            row.problem = pinned.wasm.is_some().then(|| super::STOPPED.into());
            if !catalog.entries.contains_key(&manifest.id) {
                catalog.order.push(manifest.id.clone());
                catalog.entries.insert(
                    manifest.id.clone(),
                    Arc::new(Entry {
                        revision,
                        live,
                        source,
                        manifest: manifest.clone(),
                        dev: row.dev,
                        lease,
                    }),
                );
            }
            Ok::<(), String>(())
        })();
        if let Err(error) = result {
            cancelled(cancel)?;
            if row.id.is_empty() {
                if let Some(id) = path
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .filter(|id| horizon_mods::index::valid_id(id))
                {
                    catalog.invalid.insert(id.into());
                }
            } else {
                catalog.invalid.insert(row.id.clone());
            }
            row.problem = Some(error);
        }
        catalog.rows.push(row);
    }
    catalog.built = std::env::current_exe()
        .ok()
        .and_then(|exe| fs::read_to_string(exe.with_file_name(super::BUILD_INFO)).ok())
        .unwrap_or_default();
    let unbuilt = super::not_built(
        &home.join(horizon_client::mod_list::NATIVE_DIR),
        &catalog.built,
        &catalog.rows,
    );
    catalog.rows.extend(unbuilt);
    Ok(Arc::new(catalog))
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Scratch(PathBuf);
    impl Scratch {
        fn new() -> Self {
            static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
            let path = std::env::temp_dir().join(format!(
                "horizon-catalog-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            ));
            fs::create_dir_all(path.join(horizon_client::mod_list::DIR)).unwrap();
            Self(path)
        }
        fn package(&self, bytes: &[u8]) -> PathBuf {
            let path = self.0.join(horizon_client::mod_list::DIR).join("ruby.hmod");
            fs::write(&path, bytes).unwrap();
            path
        }
        fn read(&self) -> Arc<Catalog> {
            read(&self.0, &[], &Cancellation::default()).unwrap()
        }
    }
    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    fn package(version: &str, registry: bool) -> Vec<u8> {
        let manifest = format!(
            "id = \"ruby\"\nname = \"Ruby\"\nversion = \"{version}\"\napi = \"0.2\"\nside = \"both\"\ncapabilities = [\"registries\"]\n"
        );
        let file = if registry {
            "data/ruby/horizon/item/gem.json"
        } else {
            "assets/ruby/lang/en_us.json"
        };
        let bytes = if registry {
            br#"{"template":"minecraft:diamond"}"#.as_slice()
        } else {
            version.as_bytes()
        };
        Package::write_files(&manifest, None, &[(file, bytes)]).unwrap()
    }
    #[test]
    fn same_enabled_id_update_uses_revision_and_retains_old_real_bytes() {
        let dir = Scratch::new();
        dir.package(&package("1.0.0", false));
        let old = dir.read();
        let enabled = BTreeSet::from(["ruby".into()]);
        let running = Selection::wanted(&enabled, &old);
        dir.package(&package("2.0.0", false));
        let new = dir.read();
        let plan = switch_plan(&running, &enabled, &new, true);
        assert_ne!(plan.next, running);
        assert!(plan.waiting.is_empty());
        assert_eq!(
            Package::read(&running.entries["ruby"].source)
                .unwrap()
                .manifest
                .version,
            "1.0.0"
        );
        assert_eq!(
            Package::read(&plan.next.entries["ruby"].source)
                .unwrap()
                .manifest
                .version,
            "2.0.0"
        );
        assert!(
            RootLease::try_exclusive(running.entries["ruby"].source.parent().unwrap())
                .unwrap()
                .is_none()
        );
        let remote = switch_plan(&running, &enabled, &new, false);
        assert_eq!(remote.next, running);
        assert!(remote.waiting.contains("ruby"));
    }
    #[test]
    fn registry_to_assets_update_waits_and_disabled_broken_pack_does_not() {
        let dir = Scratch::new();
        dir.package(&package("1.0.0", true));
        let old = dir.read();
        let enabled = BTreeSet::from(["ruby".into()]);
        let running = Selection::wanted(&enabled, &old);
        dir.package(&package("2.0.0", false));
        let mut new = (*dir.read()).clone();
        new.invalid.insert("broken_disabled".into());
        let plan = switch_plan(&running, &enabled, &new, true);
        assert_eq!(plan.next, running);
        assert_eq!(plan.waiting, BTreeSet::from(["ruby".into()]));
    }
    #[test]
    fn editing_installed_archive_between_read_and_pin_classifies_pinned_generation() {
        let dir = Scratch::new();
        dir.package(&package("1.0.0", false));
        let replacement = package("2.0.0", true);
        let catalog = read_with(&dir.0, &[], &Cancellation::default(), |path| {
            fs::write(path, &replacement).unwrap()
        })
        .unwrap();
        let entry = &catalog.entries["ruby"];
        assert!(
            !entry.live,
            "the pinned registry change must not be classified by the earlier asset-only archive"
        );
        assert_eq!(entry.manifest.version, "2.0.0");
        assert_eq!(
            Package::read(&entry.source).unwrap().manifest,
            entry.manifest
        );
        assert_eq!(catalog.rows[0].title, "Ruby 2.0.0");
    }
    #[test]
    fn cancelled_refresh_does_not_publish_a_partial_catalogue() {
        let dir = Scratch::new();
        dir.package(&package("1.0.0", false));
        let cancel = Cancellation::default();
        cancel.cancel();
        assert!(read(&dir.0, &[], &cancel).is_err());
        assert!(!dir.0.join(".horizon-cache/mod-sources").exists());
    }
}

#[cfg(test)]
fn read_with(
    home: &Path,
    dev: &[PathBuf],
    cancel: &Cancellation,
    before_pin: impl FnMut(&Path),
) -> Result<Arc<Catalog>, String> {
    read_at(home, dev, cancel, None, before_pin)
}
impl Catalog {
    pub fn wanted_ids(
        &self,
        enabled: &BTreeSet<String>,
        disabled: &BTreeSet<String>,
    ) -> BTreeSet<String> {
        enabled
            .iter()
            .filter(|id| !self.native_ids.contains(*id))
            .cloned()
            .chain(
                self.native_ids
                    .iter()
                    .filter(|id| !disabled.contains(*id))
                    .cloned(),
            )
            .collect()
    }
}
