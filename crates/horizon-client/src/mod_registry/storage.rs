//! The game's mod-browser commit gate and recoverable package/manifest transaction.
use super::{Cancellation, MANIFEST, check_cancel};
use std::{
    fs::{self, File, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    time::Duration,
};

static NEXT: AtomicU64 = AtomicU64::new(0);
const TRANSACTION: &str = ".mod-browser-transaction";

pub(super) fn lock_home(home: &Path, cancel: &Cancellation) -> Result<File, String> {
    fs::create_dir_all(home).map_err(|e| e.to_string())?;
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(home.join(".mod-browser.lock"))
        .map_err(|e| e.to_string())?;
    loop {
        check_cancel(cancel)?;
        match file.try_lock() {
            Ok(()) => return Ok(file),
            Err(fs::TryLockError::WouldBlock) => std::thread::sleep(Duration::from_millis(10)),
            Err(fs::TryLockError::Error(e)) => return Err(e.to_string()),
        }
    }
}

fn sync_dir(dir: &Path) -> Result<(), String> {
    #[cfg(unix)]
    File::open(dir)
        .and_then(|file| file.sync_all())
        .map_err(|e| e.to_string())?;
    #[cfg(not(unix))]
    let _ = dir;
    Ok(())
}

/// An exclusively created staging file. Only this operation removes it.
pub(super) struct Staged(pub PathBuf);
impl Drop for Staged {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}
pub(super) fn stage(dir: &Path, bytes: &[u8]) -> Result<Staged, String> {
    fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    loop {
        let path = dir.join(format!(
            ".horizon-part-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(mut file) => {
                let staged = Staged(path);
                file.write_all(bytes)
                    .and_then(|()| file.sync_all())
                    .map_err(|e| e.to_string())?;
                return Ok(staged);
            }
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e.to_string()),
        }
    }
}
pub(super) fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let staged = stage(path.parent().ok_or("file has no parent")?, bytes)?;
    fs::rename(&staged.0, path).map_err(|e| e.to_string())?;
    sync_dir(path.parent().unwrap())
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
enum Phase {
    #[default]
    Prepared,
    Committed,
}

#[derive(serde::Serialize, serde::Deserialize)]
struct Journal {
    id: String,
    package: bool,
    manifest: bool,
    #[serde(default)]
    phase: Phase,
}

fn remove_file(path: &Path) -> io::Result<()> {
    match fs::remove_file(path) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        result => result,
    }
}
fn backup(path: &Path, to: &Path) -> io::Result<bool> {
    if !path.exists() {
        return Ok(false);
    }
    if !fs::symlink_metadata(path)?.is_file() {
        return Err(io::Error::other("mod transaction expected a regular file"));
    }
    fs::copy(path, to)?;
    File::open(to)?.sync_all()?;
    Ok(true)
}
fn restore(backup: &Path, target: &Path, existed: bool) -> io::Result<()> {
    if !existed {
        return remove_file(target);
    }
    let part = backup.with_extension("restore");
    fs::copy(backup, &part)?;
    File::open(&part)?.sync_all()?;
    fs::rename(part, target)
}

fn read_journal(path: &Path) -> Result<Journal, String> {
    let bytes = super::read_bounded(
        File::open(path).map_err(|e| e.to_string())?,
        4096,
        &Cancellation::default(),
    )?;
    let journal: Journal = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
    if !horizon_mods::index::valid_id(&journal.id) {
        return Err("invalid mod transaction id".into());
    }
    Ok(journal)
}

/// Caller holds the home lock. A prepared journal rolls back the pair;
/// a committed journal keeps it and only retires recovery files.
pub(super) fn recover(home: &Path) -> Result<(), String> {
    let dir = home.join(TRANSACTION);
    let path = dir.join("journal.json");
    if !path.exists() {
        if dir.exists() {
            fs::remove_dir_all(dir).map_err(|e| e.to_string())?;
        }
        return Ok(());
    }
    let journal = read_journal(&path)?;
    if journal.phase == Phase::Committed {
        // An earlier marker rename may have succeeded while its directory sync
        // failed. Make that visible decision durable before deleting backups.
        sync_dir(&dir).map_err(|e| format!("mod install is committed; cleanup pending: {e}"))?;
        fs::remove_dir_all(&dir)
            .map_err(|e| format!("mod install is committed; cleanup pending: {e}"))?;
        return sync_dir(home);
    }
    let target = home
        .join(crate::mod_list::DIR)
        .join(format!("{}.hmod", journal.id));
    restore(&dir.join("previous-package"), &target, journal.package).map_err(|e| e.to_string())?;
    restore(
        &dir.join("previous-manifest"),
        &home.join(MANIFEST),
        journal.manifest,
    )
    .map_err(|e| e.to_string())?;
    sync_dir(target.parent().unwrap())?;
    sync_dir(home)?;
    remove_file(&path).map_err(|e| e.to_string())?;
    // The rollback marker must be durably gone before deleting its backups;
    // otherwise a crash could resurrect Prepared with nothing left to restore.
    sync_dir(&dir)?;
    let _ = fs::remove_dir_all(dir);
    Ok(())
}

/// Once the journal is published, finish or roll back irrespective of cancellation.
/// Cancellation is checked by the caller before entering this short commit phase.
pub(super) fn commit(home: &Path, id: &str, package: &Path, manifest: &[u8]) -> Result<(), String> {
    commit_with(home, id, package, manifest, &|_| Ok(()))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CommitPoint {
    AfterPackage,
    BeforeDecision,
    AfterDecisionRename,
    BeforeCleanup,
}

fn rollback_error(home: &Path, error: String) -> Result<(), String> {
    match recover(home) {
        Ok(()) => Err(error),
        Err(rollback) => Err(format!("{error}; rollback is pending: {rollback}")),
    }
}

fn commit_with(
    home: &Path,
    id: &str,
    package: &Path,
    manifest: &[u8],
    at: &dyn Fn(CommitPoint) -> Result<(), String>,
) -> Result<(), String> {
    let target = home.join(crate::mod_list::DIR).join(format!("{id}.hmod"));
    fs::create_dir_all(target.parent().unwrap()).map_err(|e| e.to_string())?;
    let dir = home.join(TRANSACTION);
    fs::create_dir(&dir).map_err(|e| e.to_string())?;
    let prepared = (|| {
        let journal = Journal {
            id: id.into(),
            package: backup(&target, &dir.join("previous-package")).map_err(|e| e.to_string())?,
            manifest: backup(&home.join(MANIFEST), &dir.join("previous-manifest"))
                .map_err(|e| e.to_string())?,
            phase: Phase::Prepared,
        };
        atomic_write(&dir.join("next-manifest"), manifest)?;
        atomic_write(
            &dir.join("journal.json"),
            &serde_json::to_vec(&journal).map_err(|e| e.to_string())?,
        )?;
        sync_dir(home)?;
        Ok(journal)
    })();
    let mut journal = match prepared {
        Ok(journal) => journal,
        Err(error) => {
            if dir.join("journal.json").exists() {
                return rollback_error(home, error);
            }
            let _ = fs::remove_dir_all(dir);
            return Err(error);
        }
    };
    let result = (|| {
        fs::rename(package, &target).map_err(|e| e.to_string())?;
        at(CommitPoint::AfterPackage)?;
        fs::rename(dir.join("next-manifest"), home.join(MANIFEST)).map_err(|e| e.to_string())?;
        sync_dir(target.parent().unwrap())?;
        sync_dir(home)?;
        at(CommitPoint::BeforeDecision)
    })();
    if let Err(error) = result {
        return rollback_error(home, error);
    }

    journal.phase = Phase::Committed;
    let decision = (|| {
        let marker = stage(
            &dir,
            &serde_json::to_vec(&journal).map_err(|e| e.to_string())?,
        )?;
        fs::rename(&marker.0, dir.join("journal.json")).map_err(|e| e.to_string())?;
        at(CommitPoint::AfterDecisionRename)?;
        sync_dir(&dir)
    })();
    if let Err(error) = decision {
        // rename can succeed and its following directory sync fail. Inspect the
        // visible atomic record rather than incorrectly claiming rollback.
        match read_journal(&dir.join("journal.json")) {
            Ok(visible) if visible.id == journal.id && visible.phase == Phase::Committed => {
                log::warn!(
                    "Horizon: mod {} committed; recovery files retained after marker sync failure: {error}",
                    journal.id
                );
                return Ok(());
            }
            Ok(visible) if visible.id == journal.id && visible.phase == Phase::Prepared => {
                return rollback_error(home, error);
            }
            _ => {
                return Err(format!(
                    "{error}; mod commit outcome is uncertain, recovery is pending"
                ));
            }
        }
    }
    if let Err(error) = at(CommitPoint::BeforeCleanup).and_then(|()| recover(home)) {
        // The pair and committed decision are already durable. Cleanup failure
        // is not an installation failure and must never roll the pair back.
        log::warn!(
            "Horizon: mod {} committed; cleanup pending: {error}",
            journal.id
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn commit_decision_distinguishes_rollback_from_cleanup_failures() {
        for point in [
            CommitPoint::BeforeDecision,
            CommitPoint::AfterDecisionRename,
            CommitPoint::BeforeCleanup,
        ] {
            let home = super::super::tests::scratch(&format!("decision-{point:?}"));
            fs::create_dir_all(home.join(crate::mod_list::DIR)).unwrap();
            let target = home.join(crate::mod_list::DIR).join("ruby.hmod");
            fs::write(&target, b"old package").unwrap();
            fs::write(home.join(MANIFEST), b"old manifest").unwrap();
            let next = stage(&home, b"new package").unwrap();
            let outcome = commit_with(&home, "ruby", &next.0, b"new manifest", &|at| {
                if at == point {
                    Err(format!("injected {point:?}"))
                } else {
                    Ok(())
                }
            });
            if point == CommitPoint::BeforeDecision {
                assert!(outcome.is_err());
                assert_eq!(fs::read(&target).unwrap(), b"old package");
                assert_eq!(fs::read(home.join(MANIFEST)).unwrap(), b"old manifest");
            } else {
                assert!(
                    outcome.is_ok(),
                    "a committed pair is not reported as rolled back"
                );
                let dir = home.join(TRANSACTION);
                assert_eq!(
                    read_journal(&dir.join("journal.json")).unwrap().phase,
                    Phase::Committed
                );
                assert!(
                    dir.join("previous-package").exists(),
                    "ambiguous sync/cleanup retains recovery files"
                );
                assert_eq!(fs::read(&target).unwrap(), b"new package");
                super::super::recover_home(&home).unwrap();
                assert_eq!(fs::read(&target).unwrap(), b"new package");
                assert_eq!(fs::read(home.join(MANIFEST)).unwrap(), b"new manifest");
            }
            assert!(!home.join(TRANSACTION).exists());
            fs::remove_dir_all(home).unwrap();
        }
    }

    #[test]
    fn a_content_reader_cannot_observe_the_package_prefix_of_a_rollback() {
        use std::sync::mpsc;
        let home = super::super::tests::scratch("snapshot-rollback");
        fs::create_dir_all(home.join(crate::mod_list::DIR)).unwrap();
        let target = home.join(crate::mod_list::DIR).join("ruby.hmod");
        let old = super::super::tests::generation_package("1", "old");
        let new = super::super::tests::generation_package("2", "new");
        fs::write(&target, &old).unwrap();
        fs::write(home.join(MANIFEST), br#"{"entries":[]}"#).unwrap();
        let next = stage(&home, &new).unwrap();
        let writer_home = home.clone();
        let (published, prefix) = mpsc::channel();
        let (release, waiting) = mpsc::channel();
        let waiting = std::sync::Mutex::new(waiting);
        let writer = std::thread::spawn(move || {
            let _lock = lock_home(&writer_home, &Cancellation::default()).unwrap();
            commit_with(&writer_home, "ruby", &next.0, b"new manifest", &|at| {
                if at == CommitPoint::AfterPackage {
                    published.send(()).unwrap();
                    waiting.lock().unwrap().recv().unwrap();
                    return Err("fail before manifest publication".into());
                }
                Ok(())
            })
        });
        prefix.recv_timeout(Duration::from_secs(5)).unwrap();
        let reader_home = home.clone();
        let (started, starting) = mpsc::channel();
        let (read, observed) = mpsc::channel();
        let reader = std::thread::spawn(move || {
            started.send(()).unwrap();
            let snapshot = super::super::with_content_snapshot(&reader_home, || {
                super::super::tests::read_generation(&reader_home)
            })
            .unwrap();
            read.send(snapshot).unwrap();
        });
        starting.recv().unwrap();
        let premature = observed.recv_timeout(Duration::from_millis(100)).ok();
        release.send(()).unwrap();
        assert!(writer.join().unwrap().is_err());
        reader.join().unwrap();
        assert!(
            premature.is_none(),
            "reader saw an uncommitted package during rollback"
        );
        assert_eq!(observed.recv().unwrap(), ("1".into(), b"old".to_vec()));
        assert_eq!(fs::read(&target).unwrap(), old);
        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn interrupted_commit_recovers_each_published_prefix() {
        for phase in 0..=2 {
            let home = super::super::tests::scratch(&format!("recover-{phase}"));
            fs::create_dir_all(home.join(crate::mod_list::DIR)).unwrap();
            let target = home.join(crate::mod_list::DIR).join("ruby.hmod");
            fs::write(&target, b"old package").unwrap();
            fs::write(home.join(MANIFEST), b"old manifest").unwrap();
            let dir = home.join(TRANSACTION);
            fs::create_dir(&dir).unwrap();
            backup(&target, &dir.join("previous-package")).unwrap();
            backup(&home.join(MANIFEST), &dir.join("previous-manifest")).unwrap();
            atomic_write(
                &dir.join("journal.json"),
                &serde_json::to_vec(&Journal {
                    id: "ruby".into(),
                    package: true,
                    manifest: true,
                    phase: Phase::Prepared,
                })
                .unwrap(),
            )
            .unwrap();
            if phase >= 1 {
                fs::write(&target, b"new package").unwrap();
            }
            if phase >= 2 {
                fs::write(home.join(MANIFEST), b"new manifest").unwrap();
            }
            super::super::recover_home(&home).unwrap();
            assert_eq!(fs::read(&target).unwrap(), b"old package");
            assert_eq!(fs::read(home.join(MANIFEST)).unwrap(), b"old manifest");
            super::super::recover_home(&home).unwrap();
            assert!(!dir.exists());
            fs::remove_dir_all(home).unwrap();
        }
    }

    #[test]
    fn staged_files_are_unique_and_do_not_touch_an_existing_cache_until_published() {
        let home = super::super::tests::scratch("unique-cache");
        let cache = home.join("index.json");
        fs::write(&cache, b"previous valid cache").unwrap();
        let a = stage(&home, b"candidate a").unwrap();
        let b = stage(&home, b"candidate b").unwrap();
        assert_ne!(a.0, b.0);
        drop(a);
        drop(b); // An interrupted fetch before publication.
        assert_eq!(fs::read(&cache).unwrap(), b"previous valid cache");
        atomic_write(&cache, b"new valid cache").unwrap();
        assert_eq!(fs::read(&cache).unwrap(), b"new valid cache");
        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn manifest_failure_restores_the_previous_package_and_manifest() {
        let home = super::super::tests::scratch("transaction-failure");
        fs::create_dir_all(home.join(crate::mod_list::DIR)).unwrap();
        let target = home.join(crate::mod_list::DIR).join("ruby.hmod");
        fs::write(&target, b"old package").unwrap();
        fs::write(home.join(MANIFEST), b"old manifest").unwrap();
        let next = stage(&home, b"new package").unwrap();
        let error = commit_with(&home, "ruby", &next.0, b"new manifest", &|point| {
            if point == CommitPoint::AfterPackage {
                Err("manifest write failed".into())
            } else {
                Ok(())
            }
        })
        .unwrap_err();
        assert!(error.contains("manifest write failed"));
        assert_eq!(fs::read(target).unwrap(), b"old package");
        assert_eq!(fs::read(home.join(MANIFEST)).unwrap(), b"old manifest");
        assert!(!home.join(TRANSACTION).exists());
        fs::remove_dir_all(home).unwrap();
    }
}
