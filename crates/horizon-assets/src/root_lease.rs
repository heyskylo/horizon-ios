//! Lifetimes of generated resource roots shared by loaders and delayed readers.
//!
//! The lock inode lives outside the directory collected by cache GC and is never
//! unlinked. Shared locks retain roots across processes; an exclusive lock permits
//! replacement or deletion. Acquisition is a worker operation. `lease_for` only
//! clones an already acquired lease and performs no filesystem work.

use std::{
    collections::HashMap,
    fs::{self, File},
    io,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, OnceLock, Weak},
};

#[derive(Debug)]
struct Held {
    _file: Option<File>,
    // An overlay can link to another generated root. Its delayed readers need both.
    _parents: Vec<RootLease>,
}

/// Cheap to clone; the last clone releases the OS lock. Ordinary prepared roots
/// which are not generated caches carry an inert lease.
#[derive(Clone, Debug, Default)]
pub struct RootLease(Option<Arc<Held>>);

type Registry = Mutex<HashMap<PathBuf, Vec<Weak<Held>>>>;
fn registry() -> &'static Registry {
    static ROOTS: OnceLock<Registry> = OnceLock::new();
    ROOTS.get_or_init(Default::default)
}

/// An existing lease for this root or a file below it, without opening any files.
pub fn lease_for(path: &Path) -> Option<RootLease> {
    let roots = registry().lock().unwrap_or_else(|e| e.into_inner());
    let live: Vec<_> = roots
        .iter()
        .filter(|(root, _)| path.starts_with(root))
        .flat_map(|(root, leases)| {
            leases.iter().filter_map(move |weak| {
                weak.upgrade().map(|held| (root.components().count(), held))
            })
        })
        .collect();
    let deepest = live.iter().map(|(depth, _)| *depth).max()?;
    // Keep dependency bundles too. Choosing only a newer bare root lock could
    // lose an older generation's leases on the source folders its links read.
    Some(RootLease::combine(
        live.into_iter()
            .filter(|(depth, _)| *depth == deepest)
            .map(|(_, held)| RootLease(Some(held))),
    ))
}

fn lock_path(root: &Path) -> io::Result<Option<PathBuf>> {
    let root = std::path::absolute(root)?;
    let Some(name) = root.file_name().and_then(|s| s.to_str()) else {
        return Ok(None);
    };
    let parent = root.parent().unwrap();
    let kind = parent.file_name().and_then(|s| s.to_str()).unwrap_or("");
    let (cache, key) = if name == "mod-content" {
        (parent, "mod-content-legacy".to_owned())
    } else if ((kind == "mod-content" && name.len() == 16)
        || (kind == "mod-sources" && name.len() == 64)
        || (kind == "resourcepack-generations" && name.len() == 64))
        && name.bytes().all(|b| b.is_ascii_hexdigit())
    {
        (parent.parent().unwrap(), format!("{kind}-{name}"))
    } else {
        return Ok(None);
    };
    Ok(Some(
        cache
            .join(".resource-root-leases")
            .join(format!("{key}.lock")),
    ))
}

fn open_lock(path: &Path) -> io::Result<File> {
    fs::create_dir_all(path.parent().unwrap())?;
    File::options()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(path)
}

fn register(root: &Path, file: File, parent: RootLease) -> RootLease {
    let held = Arc::new(Held {
        _file: Some(file),
        _parents: vec![parent],
    });
    let mut roots = registry().lock().unwrap_or_else(|e| e.into_inner());
    roots.retain(|_, leases| {
        leases.retain(|lease| lease.strong_count() != 0);
        !leases.is_empty()
    });
    roots
        .entry(root.to_owned())
        .or_default()
        .push(Arc::downgrade(&held));
    if let Ok(absolute) = std::path::absolute(root)
        && absolute != root
    {
        roots
            .entry(absolute)
            .or_default()
            .push(Arc::downgrade(&held));
    }
    if let Ok(canonical) = fs::canonicalize(root)
        && canonical != root
    {
        roots
            .entry(canonical)
            .or_default()
            .push(Arc::downgrade(&held));
    }
    RootLease(Some(held))
}

impl RootLease {
    /// Retain all inputs behind an overlay (base and immutable mod sources).
    pub fn combine(leases: impl IntoIterator<Item = Self>) -> Self {
        let parents: Vec<_> = leases.into_iter().filter(Self::is_active).collect();
        if parents.is_empty() {
            Self::default()
        } else {
            Self(Some(Arc::new(Held {
                _file: None,
                _parents: parents,
            })))
        }
    }

    /// Associate an already protected overlay path with the same dependencies.
    /// This is a memory-only operation; the caller publishes it before the root.
    pub fn register_alias(&self, root: &Path) {
        if let Some(held) = &self.0 {
            registry()
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .entry(root.to_owned())
                .or_default()
                .push(Arc::downgrade(held));
        }
    }

    pub fn is_active(&self) -> bool {
        self.0.is_some()
    }

    /// Acquire before loading or publishing a generated root, on a worker.
    pub fn acquire(root: &Path) -> io::Result<Self> {
        Self::acquire_with_parent(root, Self::default())
    }

    pub fn acquire_with_parent(root: &Path, parent: Self) -> io::Result<Self> {
        let Some(path) = lock_path(root)? else {
            return Ok(parent);
        };
        let file = open_lock(&path)?;
        file.lock_shared()?;
        Ok(register(root, file, parent))
    }

    /// A nonblocking writer/collector lease. Busy or unmanaged roots are left alone.
    pub fn try_exclusive(root: &Path) -> io::Result<Option<ExclusiveRoot>> {
        let Some(path) = lock_path(root)? else {
            return Ok(None);
        };
        let file = open_lock(&path)?;
        match file.try_lock() {
            Ok(()) => Ok(Some(ExclusiveRoot {
                file,
                root: root.to_owned(),
            })),
            Err(fs::TryLockError::WouldBlock) => Ok(None),
            Err(fs::TryLockError::Error(error)) => Err(error),
        }
    }

    /// Serialize workers preparing the same cache key without blocking live readers.
    pub fn build_lock(root: &Path) -> io::Result<File> {
        let path =
            lock_path(root)?.ok_or_else(|| io::Error::other("not a generated resource root"))?;
        let file = open_lock(&path.with_extension("build"))?;
        file.lock()?;
        Ok(file)
    }
}

/// Held while replacing/removing a generated root. Never remove its lock file.
pub struct ExclusiveRoot {
    file: File,
    root: PathBuf,
}

impl ExclusiveRoot {
    /// Publish the built root while retaining a shared reader lease.
    pub fn into_shared(self, parent: RootLease) -> io::Result<RootLease> {
        self.file.lock_shared()?;
        Ok(register(&self.root, self.file, parent))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        io::{BufRead, Write},
        process::{Command, Stdio},
    };

    fn root(label: &str) -> PathBuf {
        let path = std::env::temp_dir()
            .join(format!("horizon-lease-{label}-{}", std::process::id()))
            .join("mod-content/00000000000000ab");
        fs::create_dir_all(&path).unwrap();
        path
    }

    #[test]
    fn clones_and_delayed_readers_keep_the_same_root_locked() {
        let root = root("clone");
        fs::write(root.join("lazy.bin"), b"old snapshot").unwrap();
        let lease = RootLease::acquire(&root).unwrap();
        let reader = lease_for(&root.join("lazy.bin")).unwrap();
        drop(lease);
        assert!(RootLease::try_exclusive(&root).unwrap().is_none());
        assert_eq!(fs::read(root.join("lazy.bin")).unwrap(), b"old snapshot");
        drop(reader);
        assert!(RootLease::try_exclusive(&root).unwrap().is_some());
        fs::remove_dir_all(root.parent().unwrap().parent().unwrap()).unwrap();
    }

    #[test]
    fn an_overlay_keeps_its_managed_base_alive() {
        let base = root("parent");
        let overlay = base.with_file_name("00000000000000cd");
        fs::create_dir_all(&overlay).unwrap();
        let base_lease = RootLease::acquire(&base).unwrap();
        let overlay_lease = RootLease::acquire_with_parent(&overlay, base_lease).unwrap();
        assert!(RootLease::try_exclusive(&base).unwrap().is_none());
        drop(overlay_lease);
        assert!(RootLease::try_exclusive(&base).unwrap().is_some());
        fs::remove_dir_all(base.parent().unwrap().parent().unwrap()).unwrap();
    }

    #[test]
    fn dropping_a_newer_acquisition_does_not_hide_the_older_reader() {
        let root = root("two-readers");
        let older = RootLease::acquire(&root).unwrap();
        let newer = RootLease::acquire(&root).unwrap();
        drop(newer);
        let delayed = lease_for(&root).expect("the older reader is still registered");
        drop(older);
        assert!(RootLease::try_exclusive(&root).unwrap().is_none());
        drop(delayed);
        assert!(RootLease::try_exclusive(&root).unwrap().is_some());
        fs::remove_dir_all(root.parent().unwrap().parent().unwrap()).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn canonical_and_symlink_spellings_share_reader_protection() {
        let root = root("aliases");
        let cache = root.parent().unwrap().parent().unwrap();
        let alias = cache.with_extension("alias");
        std::os::unix::fs::symlink(cache, &alias).unwrap();
        let aliased = alias.join("mod-content/00000000000000ab");
        let older = RootLease::acquire(&aliased).unwrap();
        let newer = RootLease::acquire(&root).unwrap();
        drop(newer);
        let reader = lease_for(&fs::canonicalize(&root).unwrap()).unwrap();
        drop(older);
        assert!(RootLease::try_exclusive(&root).unwrap().is_none());
        assert!(RootLease::try_exclusive(&aliased).unwrap().is_none());
        drop(reader);
        assert!(RootLease::try_exclusive(&root).unwrap().is_some());
        fs::remove_file(alias).unwrap();
        fs::remove_dir_all(cache).unwrap();
    }

    #[test]
    fn lease_child() {
        let Some(root) = std::env::var_os("HORIZON_LEASE_CHILD_ROOT") else {
            return;
        };
        let _lease = RootLease::acquire(Path::new(&root)).unwrap();
        println!("LEASED");
        std::io::stdout().flush().unwrap();
        let mut input = String::new();
        std::io::stdin().read_line(&mut input).unwrap();
    }

    #[test]
    fn another_process_prevents_collection_until_it_releases() {
        let root = root("process");
        let mut child = Command::new(std::env::current_exe().unwrap())
            .args(["root_lease::tests::lease_child", "--exact", "--nocapture"])
            .env("HORIZON_LEASE_CHILD_ROOT", &root)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap();
        let mut output = std::io::BufReader::new(child.stdout.take().unwrap());
        let (send, ready) = std::sync::mpsc::channel();
        let reader = std::thread::spawn(move || {
            let mut announced = false;
            loop {
                let mut line = String::new();
                if output.read_line(&mut line).unwrap_or(0) == 0 {
                    return;
                }
                if !announced && line.contains("LEASED") {
                    let _ = send.send(());
                    announced = true;
                }
            }
        });
        if ready
            .recv_timeout(std::time::Duration::from_secs(5))
            .is_err()
        {
            let _ = child.kill();
            let _ = child.wait();
            let _ = reader.join();
            panic!("child did not report its lease within five seconds");
        }
        assert!(RootLease::try_exclusive(&root).unwrap().is_none());
        child.stdin.take().unwrap().write_all(b"release\n").unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            if let Some(status) = child.try_wait().unwrap() {
                assert!(status.success());
                break;
            }
            if std::time::Instant::now() >= deadline {
                let _ = child.kill();
                let _ = child.wait();
                panic!("child did not release its lease within five seconds");
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        reader.join().unwrap();
        assert!(RootLease::try_exclusive(&root).unwrap().is_some());
        fs::remove_dir_all(root.parent().unwrap().parent().unwrap()).unwrap();
    }
}
