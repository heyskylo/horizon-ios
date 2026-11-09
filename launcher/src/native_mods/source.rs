//! Native bundle source enumeration, content snapshots and installed staging.
use super::*;

/// The root of an unpacked combined mod, or the mixin folder for code-only mods.
fn bundle_root(bundle: &Bundle) -> &Path {
    bundle
        .dir
        .parent()
        .and_then(Path::parent)
        .filter(|root| {
            root.join(NATIVE_MANIFEST).is_file()
                && bundle.dir == root.join("mixins").join(&bundle.id)
        })
        .unwrap_or(&bundle.dir)
}

fn content_files(bundle: &Bundle) -> Result<Vec<(String, PathBuf)>, String> {
    let root = bundle_root(bundle);
    if !root.join(horizon_mods::manifest::MANIFEST).is_file() {
        return Ok(Vec::new());
    }
    let package = horizon_mods::Package::read(root)?;
    if package.manifest.id != bundle.id {
        return Err(tr("Native and content mod ids differ.").into());
    }
    let content = horizon_mods::Content::read(root, &package.manifest)?;
    Ok(std::iter::once(horizon_mods::manifest::MANIFEST.to_owned())
        .chain(content.files)
        .map(|name| {
            let path = root.join(&name);
            (name, path)
        })
        .collect())
}

pub(super) fn build_files(bundle: &Bundle) -> Result<Vec<(String, PathBuf)>, String> {
    let mut files = bundle_files(bundle)?;
    files.extend(content_files(bundle)?);
    files.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(files)
}

/// Content is owned by this exact build, never read from a later installed mod update.
pub(super) fn install_content(mods: &[Bundle], version: &Path) -> Result<(), String> {
    for bundle in mods {
        for (name, path) in content_files(bundle)? {
            let to = version.join("native-content").join(&bundle.id).join(name);
            fs::create_dir_all(to.parent().unwrap()).map_err(|e| e.to_string())?;
            fs::copy(path, to).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

/// The hash of a bundle's files.
pub(super) fn mod_hash(b: &Bundle) -> Result<String, String> {
    let mut hash = Sha256::new();
    for (name, path) in build_files(b)? {
        let bytes = fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        hash.update(name.as_bytes());
        hash.update((bytes.len() as u64).to_le_bytes());
        hash.update(&bytes);
    }
    Ok(hash.finalize()[..8]
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect())
}

pub(super) fn installed_mods_into(home: &Path, out: &Path) -> Result<PathBuf, String> {
    let _ = fs::remove_dir_all(out);
    let mut paths: Vec<PathBuf> = fs::read_dir(home.join(INSTALLED))
        .map(|dir| dir.flatten().map(|e| e.path()).collect())
        .unwrap_or_default();
    paths.retain(|p| horizon_mods::index::native::is_file(p));
    paths.sort();
    let mut count = 0;
    let mut seen = std::collections::BTreeSet::new();
    for path in paths {
        let bundle = horizon_mods::index::native::Bundle::read(&path)
            .map_err(|e| format!("{}: {e}", path.display()))?;
        if !crate::browser::java::is_enabled(home, &bundle.manifest.id) {
            continue;
        }
        if !seen.insert(bundle.manifest.id.clone()) {
            return Err(trf(
                "Duplicate native mod {id}.",
                &[("id", &bundle.manifest.id)],
            ));
        }
        let dir = out.join(&bundle.manifest.id);
        for (name, bytes) in bundle.source_files() {
            let to = dir.join(name);
            fs::create_dir_all(to.parent().unwrap_or(&dir)).map_err(|e| e.to_string())?;
            fs::write(&to, bytes).map_err(|e| format!("{}: {e}", to.display()))?;
        }
        count += 1;
    }
    if count == 0 {
        return Err(format!(
            "No native mod is turned on in {}: install one from the Mods page first.",
            home.join(INSTALLED).display()
        ));
    }
    Ok(out.to_path_buf())
}

/// Every file under `dir`, by its path relative to `dir` (with `/`), sorted. A symlink is refused:
/// horizon-mixins refuses them too.
pub(super) fn files(dir: &Path) -> Result<Vec<(String, PathBuf)>, String> {
    files_with_api(dir, None)
}

/// Bundle files omit only the API package's Cargo build output.
pub(super) fn bundle_files(bundle: &Bundle) -> Result<Vec<(String, PathBuf)>, String> {
    files_with_api(&bundle.dir, bundle.api.as_deref())
}

/// The API package's target folder in a canonical bundle root.
pub(super) fn api_target(root: &Path, api: Option<&str>) -> Result<Option<PathBuf>, String> {
    api.map(|path| root.join(path).canonicalize().map(|p| p.join("target")))
        .transpose()
        .map_err(|e| e.to_string())
}

pub(super) fn files_with_api(
    dir: &Path,
    api: Option<&str>,
) -> Result<Vec<(String, PathBuf)>, String> {
    fn walk(
        root: &Path,
        dir: &Path,
        skip: Option<&Path>,
        out: &mut Vec<(String, PathBuf)>,
    ) -> Result<(), String> {
        for entry in fs::read_dir(dir).map_err(|e| e.to_string())?.flatten() {
            let (path, kind) = (entry.path(), entry.file_type().map_err(|e| e.to_string())?);
            if kind.is_symlink() {
                return Err(format!(
                    "{} is a link: copy the file instead.",
                    path.display()
                ));
            } else if kind.is_dir() {
                if Some(path.as_path()) != skip {
                    walk(root, &path, skip, out)?;
                }
            } else {
                let relative = path.strip_prefix(root).unwrap_or(&path);
                out.push((relative.to_string_lossy().replace('\\', "/"), path));
            }
        }
        Ok(())
    }
    let root = dir
        .canonicalize()
        .map_err(|e| format!("{}: {e}", dir.display()))?;
    let skip = api_target(&root, api)?;
    let mut out = Vec::new();
    walk(&root, &root, skip.as_deref(), &mut out)?;
    out.sort();
    Ok(out)
}
/// The digest of a bundle's files as the Mods page reads it (`native_status::enabled`): an
/// unpacked `.hnmod`'s whole folder, its `horizon-native.json` included, else the bundle's own
/// folder. A build records it, so a build of the mods turned on reads as up to date.
pub(super) fn source_digest(bundle: &Bundle) -> Option<String> {
    let root = bundle
        .dir
        .parent()
        .and_then(Path::parent)
        .filter(|root| {
            root.join(NATIVE_MANIFEST).is_file()
                && bundle.dir == root.join("mixins").join(&bundle.id)
        })
        .unwrap_or(&bundle.dir);
    // An installed .hnmod keeps its archive digest for the Mods page.
    let paths = if root == bundle.dir {
        bundle_files(bundle)
    } else {
        files(root)
    };
    let read: Vec<_> = paths
        .ok()?
        .into_iter()
        .map(|(name, path)| Some((name, fs::read(path).ok()?)))
        .collect::<Option<_>>()?;
    Some(digest(read.iter().map(|(n, b)| (n.as_str(), b.as_slice()))))
}
