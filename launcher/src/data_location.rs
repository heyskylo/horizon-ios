//! Offline, copy-first relocation. The fixed bootstrap is never resolved through Settings.
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Component, Path, PathBuf},
    sync::OnceLock,
    time::{Duration, Instant},
};

type Result<T> = std::result::Result<T, String>;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Roots {
    pub launcher: PathBuf,
    pub game: PathBuf,
    pub build: PathBuf,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Location {
    version: u32,
    roots: Roots,
}

pub fn bootstrap_path() -> Result<PathBuf> {
    // Portable installs and tests can keep their location pointer separate from the user profile.
    if let Some(path) = std::env::var_os("HORIZON_LAUNCHER_LOCATION_FILE").filter(|p| !p.is_empty())
    {
        return checked_path(Path::new(&path));
    }
    dirs::config_dir()
        .map(|p| p.join("horizon-launcher-location").join("location.json"))
        .ok_or_else(|| "Cannot locate the platform configuration folder for location.json".into())
}

fn overridden(name: &str) -> bool {
    std::env::var_os(name).is_some_and(|v| !v.is_empty())
}

fn has_override() -> bool {
    overridden("HORIZON_LAUNCHER_HOME") || overridden("HORIZON_HOME")
}

/// Stable for the lifetime of this process, including errors. Environment overrides are
/// applied per root by the caller; fully isolated callers need not read the real bootstrap.
pub fn configured_roots() -> Result<Option<&'static Roots>> {
    static LOCATION: OnceLock<Result<Option<Roots>>> = OnceLock::new();
    LOCATION
        .get_or_init(|| {
            if overridden("HORIZON_LAUNCHER_HOME") && overridden("HORIZON_HOME") {
                Ok(None)
            } else {
                resolve_at(&bootstrap_path()?)
            }
        })
        .as_ref()
        .map(Option::as_ref)
        .map_err(Clone::clone)
}

pub fn validate() -> Result<()> {
    configured_roots().map(|_| ())
}

/// Explicit paths make tests independent of the real user's bootstrap and environment.
pub fn resolve_at(bootstrap: &Path) -> Result<Option<Roots>> {
    checked_path(bootstrap)?;
    let bytes = match fs::read(bootstrap) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(problem(bootstrap, e)),
    };
    let location: Location = serde_json::from_slice(&bytes).map_err(|e| {
        format!(
            "Invalid {}: {e}; restore this file or the previous data folder",
            bootstrap.display()
        )
    })?;
    if location.version != 1 {
        return Err(format!(
            "Unsupported data location version {} in {}",
            location.version,
            bootstrap.display()
        ));
    }
    let canonical = validate_roots(&location.roots)?;
    tool_safe(&canonical.build)?;
    for root in [
        &location.roots.launcher,
        &location.roots.game,
        &location.roots.build,
    ] {
        if !root.is_dir() {
            return Err(format!(
                "Data folder {} is missing; reconnect its drive or restore location.json",
                root.display()
            ));
        }
    }
    Ok(Some(canonical))
}

/// Hold until process exit. Workers must enter before acquiring this shared lease.
/// Override-based isolated runs do not touch the user's bootstrap lock.
pub fn acquire_startup_lock() -> Result<Option<File>> {
    if overridden("HORIZON_LAUNCHER_HOME") && overridden("HORIZON_HOME") {
        Ok(None)
    } else {
        acquire_lock_at(&bootstrap_path()?).map(Some)
    }
}

pub fn acquire_lock_at(bootstrap: &Path) -> Result<File> {
    lock_at(bootstrap, false)
}

fn lock_at(bootstrap: &Path, exclusive: bool) -> Result<File> {
    checked_path(bootstrap)?;
    let parent = bootstrap
        .parent()
        .ok_or("Bootstrap needs a parent folder")?;
    fs::create_dir_all(parent).map_err(|e| problem(parent, e))?;
    let path = parent.join("migration.lock");
    checked_path(&path)?;
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&path)
        .map_err(|e| problem(&path, e))?;
    let result = if exclusive {
        file.try_lock()
    } else {
        file.try_lock_shared()
    };
    result.map_err(|e| {
        format!(
            "Cannot lock {}: {e}; close other launchers and stop games/builds before retrying",
            path.display()
        )
    })?;
    Ok(file)
}

pub fn migrate_current(source: &Roots, target: &Path, progress: impl FnMut(&str)) -> Result<Roots> {
    migrate_current_with_prepare(source, target, progress, |_, _| Ok(()))
}

/// Called only by the offline helper, after the GUI requests exit. Wait briefly for its lease.
pub fn migrate_current_with_prepare(
    source: &Roots,
    target: &Path,
    progress: impl FnMut(&str),
    prepare: impl FnOnce(&Roots, &Roots) -> Result<()>,
) -> Result<Roots> {
    if has_override() {
        return Err("Unset HORIZON_LAUNCHER_HOME and HORIZON_HOME before moving data".into());
    }
    let bootstrap = bootstrap_path()?;
    let deadline = Instant::now() + Duration::from_secs(10);
    let lease = loop {
        match lock_at(&bootstrap, true) {
            Ok(lease) => break lease,
            Err(_) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(100)),
            Err(e) => return Err(e),
        }
    };
    migrate_locked(source, target, &bootstrap, progress, prepare, &lease)
}

pub fn migrate(
    source: &Roots,
    target: &Path,
    bootstrap: &Path,
    progress: impl FnMut(&str),
) -> Result<Roots> {
    migrate_with_prepare(source, target, bootstrap, progress, |_, _| Ok(()))
}

/// prepare receives (staging roots, final roots), after byte verification and before commit.
/// It may rewrite managed absolute paths in structured files, and must sync those writes.
/// This explicit API deliberately ignores environment variables; production uses migrate_current.
pub fn migrate_with_prepare(
    source: &Roots,
    target: &Path,
    bootstrap: &Path,
    progress: impl FnMut(&str),
    prepare: impl FnOnce(&Roots, &Roots) -> Result<()>,
) -> Result<Roots> {
    let lease = lock_at(bootstrap, true)?;
    migrate_locked(source, target, bootstrap, progress, prepare, &lease)
}

fn migrate_locked(
    source: &Roots,
    target: &Path,
    bootstrap: &Path,
    mut progress: impl FnMut(&str),
    prepare: impl FnOnce(&Roots, &Roots) -> Result<()>,
    _lease: &File,
) -> Result<Roots> {
    let source = validate_roots(source)?;
    if let Some(current) = resolve_at(bootstrap)? {
        if current != source {
            return Err(
                "Data location changed while waiting; restart the launcher and retry the move"
                    .into(),
            );
        }
    }
    let target = checked_path(target)?;
    for root in [
        &source.launcher,
        &source.game,
        &source.build,
        &checked_path(bootstrap)?,
    ] {
        if overlaps(root, &target) {
            return Err(format!(
                "Destination {} overlaps existing data or its bootstrap {}",
                target.display(),
                root.display()
            ));
        }
    }
    if target.exists()
        && (!target.is_dir()
            || fs::read_dir(&target)
                .map_err(|e| problem(&target, e))?
                .next()
                .is_some())
    {
        return Err(format!(
            "Destination {} must be an empty folder",
            target.display()
        ));
    }
    let destination = layout(&source, &target);
    tool_safe(&destination.build)?;
    fs::create_dir_all(&target).map_err(|e| problem(&target, e))?;
    let stage = target.join(".relocation-staging");
    let staged = layout(&source, &stage);
    let result = (|| {
        private_dir(&stage)?;
        progress("Copying and verifying launcher data");
        copy_tree(&source.launcher, &staged.launcher, bootstrap, &mut progress)?;
        progress("Copying and verifying game data");
        copy_tree(&source.game, &staged.game, bootstrap, &mut progress)?;
        if !within(&source.build, &source.launcher) {
            progress("Copying and verifying build data");
            copy_tree(&source.build, &staged.build, bootstrap, &mut progress)?;
        } else if !staged.build.exists() {
            fs::create_dir_all(&staged.build).map_err(|e| problem(&staged.build, e))?;
        }
        prepare(&staged, &destination)?;
        for name in ["launcher", "game", "build"] {
            let from = stage.join(name);
            if from.exists() {
                fs::rename(&from, target.join(name)).map_err(|e| problem(&from, e))?;
            }
        }
        fs::remove_dir(&stage).map_err(|e| problem(&stage, e))?;
        sync_dir(&target)?;
        progress("Committing data location");
        commit(bootstrap, &destination)?;
        Ok(destination)
    })();
    result.map_err(|e: String| format!("{e}. Original data was kept. Partial copies may remain in {}; choose an empty folder to retry", target.display()))
}

fn layout(source: &Roots, target: &Path) -> Roots {
    let launcher = target.join("launcher");
    let build = if within(&source.build, &source.launcher) {
        // Windows containment is case-insensitive, while strip_prefix is not.
        launcher.join(
            source
                .build
                .components()
                .skip(source.launcher.components().count())
                .collect::<PathBuf>(),
        )
    } else {
        target.join("build")
    };
    Roots {
        launcher,
        game: target.join("game"),
        build,
    }
}

fn validate_roots(roots: &Roots) -> Result<Roots> {
    let roots = Roots {
        launcher: checked_path(&roots.launcher)?,
        game: checked_path(&roots.game)?,
        build: checked_path(&roots.build)?,
    };
    for root in [&roots.launcher, &roots.game, &roots.build] {
        match fs::metadata(root) {
            Ok(meta) if !meta.is_dir() => {
                return Err(format!("Data root must be a folder: {}", root.display()))
            }
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(problem(root, e)),
            _ => {}
        }
    }
    if overlaps(&roots.launcher, &roots.game)
        || overlaps(&roots.build, &roots.game)
        || (overlaps(&roots.launcher, &roots.build) && !within(&roots.build, &roots.launcher))
    {
        return Err(
            "Source data roots overlap; only a build folder inside/equal to launcher is supported"
                .into(),
        );
    }
    Ok(roots)
}

fn within(path: &Path, parent: &Path) -> bool {
    #[cfg(windows)]
    {
        Path::new(&path.to_string_lossy().to_lowercase())
            .starts_with(parent.to_string_lossy().to_lowercase())
    }
    #[cfg(not(windows))]
    {
        path.starts_with(parent)
    }
}
fn overlaps(a: &Path, b: &Path) -> bool {
    within(a, b) || within(b, a)
}

/// Canonicalize existing ancestors, reject links/junctions and ambiguous relative components.
fn checked_path(path: &Path) -> Result<PathBuf> {
    if !path.is_absolute() || path.components().any(|c| matches!(c, Component::ParentDir)) {
        return Err(format!(
            "Use an absolute path without '..': {}",
            path.display()
        ));
    }
    let mut current = PathBuf::new();
    for part in path.components() {
        current.push(part);
        // A Windows drive/UNC prefix alone is not a filesystem root. Inspect it
        // only after RootDir completes it, including canonical verbatim paths.
        if matches!(part, Component::Prefix(_)) {
            continue;
        }
        match fs::symlink_metadata(&current) {
            Ok(meta) => {
                reject_link(&current, &meta)?;
                current = fs::canonicalize(&current).map_err(|e| problem(&current, e))?;
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(problem(&current, e)),
        }
    }
    #[cfg(windows)]
    if let Some(rest) = current.to_str().and_then(|s| s.strip_prefix(r"\\?\")) {
        if rest.as_bytes().get(1) == Some(&b':') {
            return Ok(PathBuf::from(rest));
        }
    }
    Ok(current)
}

fn reject_link(path: &Path, meta: &fs::Metadata) -> Result<()> {
    #[cfg(windows)]
    let reparse = {
        use std::os::windows::fs::MetadataExt;
        meta.file_attributes() & 0x400 != 0
    };
    #[cfg(not(windows))]
    let reparse = false;
    if meta.file_type().is_symlink() || reparse {
        return Err(format!(
            "Symbolic links and reparse points cannot be relocated: {}",
            path.display()
        ));
    }
    Ok(())
}

fn tool_safe(path: &Path) -> Result<()> {
    if cfg!(windows)
        && !path
            .to_str()
            .is_some_and(|s| s.is_ascii() && !s.bytes().any(|b| b.is_ascii_whitespace()))
    {
        return Err(
            "Choose a destination with only ASCII characters and no spaces for Windows build tools"
                .into(),
        );
    }
    Ok(())
}

fn private_dir(path: &Path) -> Result<()> {
    let builder = fs::DirBuilder::new();
    #[cfg(unix)]
    let builder = {
        use std::os::unix::fs::DirBuilderExt;
        let mut builder = builder;
        builder.mode(0o700);
        builder
    };
    builder.create(path).map_err(|e| problem(path, e))
}

fn new_file(path: &Path) -> Result<File> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options.open(path).map_err(|e| problem(path, e))
}

fn copy_tree(
    source: &Path,
    dest: &Path,
    bootstrap: &Path,
    progress: &mut impl FnMut(&str),
) -> Result<()> {
    let meta = match fs::symlink_metadata(source) {
        Ok(meta) => meta,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return private_dir(dest),
        Err(e) => return Err(problem(source, e)),
    };
    reject_link(source, &meta)?;
    if meta.is_dir() {
        private_dir(dest)?;
        copy_acl(source, dest)?;
        for entry in fs::read_dir(source).map_err(|e| problem(source, e))? {
            let entry = entry.map_err(|e| problem(source, e))?;
            let path = entry.path();
            let controls = [
                bootstrap.to_path_buf(),
                bootstrap.with_file_name("migration.lock"),
                bootstrap.with_file_name("location.json.new"),
            ];
            if controls
                .iter()
                .any(|p| checked_path(p).is_ok_and(|p| within(&path, &p) && within(&p, &path)))
            {
                continue;
            }
            copy_tree(&path, &dest.join(entry.file_name()), bootstrap, progress)?;
        }
        fs::set_permissions(dest, meta.permissions()).map_err(|e| problem(dest, e))?;
        sync_dir(dest)?;
    } else if meta.is_file() {
        let mut input = File::open(source).map_err(|e| problem(source, e))?;
        let mut output = new_file(dest)?;
        copy_acl(source, dest)?; // Apply the source DACL before writing credentials or other bytes.
        std::io::copy(&mut input, &mut output).map_err(|e| problem(dest, e))?;
        output.sync_all().map_err(|e| problem(dest, e))?;
        if hash(source)? != hash(dest)? {
            return Err(format!("Copy verification failed for {}", source.display()));
        }
        let after = fs::symlink_metadata(source).map_err(|e| problem(source, e))?;
        reject_link(source, &after)?;
        if meta.len() != after.len() || meta.modified().ok() != after.modified().ok() {
            return Err(format!(
                "Source changed during copying: {}; stop all writers and retry",
                source.display()
            ));
        }
        fs::set_permissions(dest, meta.permissions()).map_err(|e| problem(dest, e))?;
        progress("Verified file");
    } else {
        return Err(format!("Unsupported special file: {}", source.display()));
    }
    Ok(())
}

fn hash(path: &Path) -> Result<[u8; 32]> {
    let mut file = File::open(path).map_err(|e| problem(path, e))?;
    let mut hash = Sha256::new();
    let mut buffer = [0; 64 * 1024];
    loop {
        let n = file.read(&mut buffer).map_err(|e| problem(path, e))?;
        if n == 0 {
            break;
        }
        hash.update(&buffer[..n]);
    }
    Ok(hash.finalize().into())
}

fn commit(bootstrap: &Path, roots: &Roots) -> Result<()> {
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_nanos();
    let temp = bootstrap.with_file_name(format!("location.{}.{nonce}.new", std::process::id()));
    let bytes = serde_json::to_vec_pretty(&Location {
        version: 1,
        roots: roots.clone(),
    })
    .map_err(|e| e.to_string())?;
    let mut file = new_file(&temp)?;
    let written = file
        .write_all(&bytes)
        .and_then(|_| file.sync_all())
        .map_err(|e| problem(&temp, e));
    drop(file);
    let replaced = written.and_then(|_| atomic_replace(&temp, bootstrap));
    if replaced.is_err() {
        let _ = fs::remove_file(&temp);
    }
    replaced?;
    // The pointer is now committed: failure to sync the directory must not report rollback.
    let _ = sync_dir(
        bootstrap
            .parent()
            .ok_or("Bootstrap needs a parent folder")?,
    );
    Ok(())
}

fn sync_dir(path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        File::open(path)
            .and_then(|f| f.sync_all())
            .map_err(|e| problem(path, e))?;
    }
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}

#[cfg(not(windows))]
fn atomic_replace(from: &Path, to: &Path) -> Result<()> {
    fs::rename(from, to).map_err(|e| problem(to, e))
}

#[cfg(windows)]
fn wide(path: &Path) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    path.as_os_str().encode_wide().chain([0]).collect()
}

#[cfg(windows)]
fn atomic_replace(from: &Path, to: &Path) -> Result<()> {
    #[link(name = "kernel32")]
    extern "system" {
        fn MoveFileExW(from: *const u16, to: *const u16, flags: u32) -> i32;
    }
    if unsafe { MoveFileExW(wide(from).as_ptr(), wide(to).as_ptr(), 1 | 8) } == 0 {
        return Err(problem(to, std::io::Error::last_os_error()));
    }
    Ok(())
}

#[cfg(not(windows))]
fn copy_acl(_source: &Path, _dest: &Path) -> Result<()> {
    Ok(())
}

#[cfg(windows)]
fn copy_acl(source: &Path, dest: &Path) -> Result<()> {
    use std::ffi::c_void;
    #[link(name = "advapi32")]
    extern "system" {
        fn GetFileSecurityW(
            path: *const u16,
            info: u32,
            descriptor: *mut c_void,
            length: u32,
            needed: *mut u32,
        ) -> i32;
        fn SetFileSecurityW(path: *const u16, info: u32, descriptor: *const c_void) -> i32;
    }
    let mut needed = 0;
    let source_w = wide(source);
    unsafe {
        GetFileSecurityW(source_w.as_ptr(), 4, std::ptr::null_mut(), 0, &mut needed);
    }
    if needed == 0 {
        return Err(problem(source, std::io::Error::last_os_error()));
    }
    // Aligned storage for the self-relative SECURITY_DESCRIPTOR; protect against target inheritance.
    let mut descriptor = vec![0u32; (needed as usize).div_ceil(4)];
    if unsafe {
        GetFileSecurityW(
            source_w.as_ptr(),
            4,
            descriptor.as_mut_ptr().cast(),
            needed,
            &mut needed,
        )
    } == 0
    {
        return Err(problem(source, std::io::Error::last_os_error()));
    }
    if unsafe {
        SetFileSecurityW(
            wide(dest).as_ptr(),
            4 | 0x8000_0000,
            descriptor.as_ptr().cast(),
        )
    } == 0
    {
        return Err(format!(
            "Cannot preserve access permissions on {}: {}",
            dest.display(),
            std::io::Error::last_os_error()
        ));
    }
    Ok(())
}

fn problem(path: &Path, error: impl std::fmt::Display) -> String {
    format!("{}: {error}", path.display())
}

#[cfg(test)]
#[path = "data_location/tests.rs"]
mod tests;
