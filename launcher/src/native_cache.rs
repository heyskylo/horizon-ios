//! Disposable native compilation outputs only. Installed games and player data are never targets.
//! Keep Cargo's profile directories and lock files in place: unlinking a locked inode would let
//! another Cargo acquire a replacement lock while cleanup was still deleting its outputs.
use std::fs::{self, File, Metadata};
use std::io;
use std::path::{Path, PathBuf};

const PROFILES: [&str; 2] = ["fast", "release"];

#[derive(Debug)]
pub(crate) enum Error {
    Busy,
    Unsafe(PathBuf),
    Io(io::Error),
}

impl From<io::Error> for Error {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

pub(crate) struct Cache {
    root: PathBuf,
    _lease: File,
}

fn linked(metadata: &Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        // Includes junctions and other reparse points, not just symbolic links.
        metadata.file_attributes() & 0x400 != 0
    }
    #[cfg(not(windows))]
    {
        metadata.file_type().is_symlink()
    }
}

fn checked(path: &Path, directory: bool) -> Result<Option<Metadata>, Error> {
    match fs::symlink_metadata(path) {
        Ok(meta)
            if linked(&meta)
                || (if directory {
                    !meta.is_dir()
                } else {
                    !meta.is_file()
                }) =>
        {
            Err(Error::Unsafe(path.to_owned()))
        }
        Ok(meta) => Ok(Some(meta)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.into()),
    }
}

fn lock(path: &Path) -> Result<File, Error> {
    checked(path, false)?;
    let file = File::options()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path)?;
    file.try_lock().map_err(|error| match error {
        fs::TryLockError::WouldBlock => Error::Busy,
        fs::TryLockError::Error(error) => Error::Io(error),
    })?;
    Ok(file)
}

fn size(path: &Path) -> Result<u64, Error> {
    let meta = fs::symlink_metadata(path)?;
    if linked(&meta) {
        return Err(Error::Unsafe(path.to_owned()));
    }
    if meta.is_file() {
        return Ok(meta.len());
    }
    if !meta.is_dir() {
        return Err(Error::Unsafe(path.to_owned()));
    }
    let mut bytes = 0u64;
    for entry in fs::read_dir(path)? {
        bytes = bytes.saturating_add(size(&entry?.path())?);
    }
    Ok(bytes)
}

impl Cache {
    /// One cross-process lease covers staging, Cargo, installation/export and cleanup.
    /// OS locks release on exit, including a crash; a leftover lock file is not a busy marker.
    pub(crate) fn acquire(build_dir: &Path) -> Result<Self, Error> {
        checked(build_dir, true)?;
        fs::create_dir_all(build_dir)?;
        let root = build_dir.canonicalize()?.join("native");
        checked(&root, true)?;
        fs::create_dir_all(&root)?;
        let root = root.canonicalize()?;
        let lease = lock(&root.join(".build.lock"))?;
        Ok(Self {
            root,
            _lease: lease,
        })
    }

    pub(crate) fn bytes(&self) -> Result<u64, Error> {
        self.process(None, false)
    }
    pub(crate) fn clear(&self) -> Result<u64, Error> {
        self.process(None, true)
    }
    pub(crate) fn trim(&self, limit: u64) -> Result<u64, Error> {
        if limit == 0 {
            return Ok(0);
        }
        self.process(Some(limit), true)
    }

    // Detect a managed directory redirected through an ancestor link after taking the lease.
    fn unchanged_path(&self, path: &Path) -> Result<(), Error> {
        if path.canonicalize()? != path {
            return Err(Error::Unsafe(path.to_owned()));
        }
        Ok(())
    }

    fn process(&self, limit: Option<u64>, clear: bool) -> Result<u64, Error> {
        checked(&self.root, true)?;
        self.unchanged_path(&self.root)?;
        let target = self.root.join("target");
        if checked(&target, true)?.is_none() {
            return Ok(0);
        }
        self.unchanged_path(&target)?;
        let mut locks = Vec::new();
        let mut folders = Vec::new();
        for profile in PROFILES {
            let folder = target.join(profile);
            if checked(&folder, true)?.is_some() {
                // Also protects an older launcher's Cargo or one started outside our lease.
                self.unchanged_path(&folder)?;
                locks.push(lock(&folder.join(".cargo-lock"))?);
                folders.push(folder);
            }
        }
        // Validate ALL profiles and descendants before deleting ANY output. Never traverse links.
        let mut entries = Vec::new();
        let mut bytes = 0u64;
        for folder in folders {
            for entry in fs::read_dir(folder)? {
                let entry = entry?;
                if entry.file_name() == ".cargo-lock" {
                    continue;
                }
                bytes = bytes.saturating_add(size(&entry.path())?);
                entries.push(entry.path());
            }
        }
        if !clear {
            return Ok(bytes);
        }
        if limit.is_some_and(|limit| bytes <= limit) {
            return Ok(0);
        }
        let records = self.root.join("built.json");
        let has_records = checked(&records, false)?.is_some();
        for path in entries {
            self.unchanged_path(&path)?;
            // The launcher lease and all Cargo locks remain held through removal. These are
            // fixed children of the two validated cache profiles, never caller-supplied paths.
            if fs::symlink_metadata(&path)?.is_dir() {
                fs::remove_dir_all(path)?;
            } else {
                fs::remove_file(path)?;
            }
        }
        if has_records {
            fs::remove_file(records)?;
        }
        drop(locks);
        Ok(bytes)
    }
}

#[cfg(test)]
#[path = "native_cache/tests.rs"]
mod tests;
