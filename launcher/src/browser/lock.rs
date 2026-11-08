//! Mutex per game home folder (`home`): serializes mod mutations (automatic updates, manual
//! install, removal, and enable/disable) so concurrent background runs and player actions cannot
//! interleave, overwrite `mod-browser.json` with a stale snapshot, or clobber each other.
//!
//! Locks are re-entrant on the current thread for the same game home, so nested operations
//! (e.g. `auto::run` -> `update` -> `install_version_guarded` -> `Installed::save`) proceed
//! without deadlocking.
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Condvar, Mutex, OnceLock};
use std::thread::ThreadId;

struct LockState {
    holder: ThreadId,
    count: usize,
}

#[derive(Default)]
struct Registry {
    locks: Mutex<HashMap<PathBuf, LockState>>,
    condvar: Condvar,
}

static REGISTRY: OnceLock<Registry> = OnceLock::new();

fn registry() -> &'static Registry {
    REGISTRY.get_or_init(Registry::default)
}

/// Strips Windows extended-length prefixes (`\\?\` and `\\?\UNC\`) so canonicalized and
/// non-canonicalized paths match.
fn strip_verbatim(path: PathBuf) -> PathBuf {
    #[cfg(windows)]
    {
        let s = path.to_string_lossy();
        if let Some(stripped) = s.strip_prefix(r"\\?\UNC\") {
            return PathBuf::from(format!(r"\\{stripped}"));
        }
        if let Some(stripped) = s.strip_prefix(r"\\?\") {
            return PathBuf::from(stripped);
        }
    }
    path
}

/// Normalizes a game home folder to a uniform canonical representation for keyed locking.
pub fn normalize_home(path: &Path) -> PathBuf {
    let p = if let Ok(c) = path.canonicalize() {
        c
    } else if path.is_relative() {
        std::env::current_dir()
            .map(|cwd| cwd.join(path))
            .unwrap_or_else(|_| path.to_path_buf())
    } else {
        path.to_path_buf()
    };
    let clean = strip_verbatim(p);
    #[cfg(windows)]
    {
        PathBuf::from(clean.to_string_lossy().to_lowercase())
    }
    #[cfg(not(windows))]
    {
        clean
    }
}

/// An acquired lock on a game home folder. Releases the lock (or decrements recursion count)
/// when dropped.
#[must_use = "if unused the lock drops immediately"]
pub struct HomeLockGuard {
    path: PathBuf,
    _not_send: std::marker::PhantomData<*const ()>,
}

impl Drop for HomeLockGuard {
    fn drop(&mut self) {
        let reg = registry();
        let mut locks = reg.locks.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(state) = locks.get_mut(&self.path) {
            state.count -= 1;
            if state.count == 0 {
                locks.remove(&self.path);
                reg.condvar.notify_all();
            }
        }
    }
}

/// Acquires exclusive mutation access for `home`, blocking until any other thread finishes.
///
/// Re-entrant: if the current thread already holds the lock for this folder, this succeeds
/// immediately and increments the recursion depth.
pub fn lock_home(home: &Path) -> HomeLockGuard {
    let path = normalize_home(home);
    let current_tid = std::thread::current().id();
    let reg = registry();
    let mut locks = reg.locks.lock().unwrap_or_else(|e| e.into_inner());
    loop {
        match locks.get_mut(&path) {
            Some(state) => {
                if state.holder == current_tid {
                    state.count += 1;
                    return HomeLockGuard {
                        path,
                        _not_send: std::marker::PhantomData,
                    };
                }
                locks = reg.condvar.wait(locks).unwrap_or_else(|e| e.into_inner());
            }
            None => {
                locks.insert(
                    path.clone(),
                    LockState {
                        holder: current_tid,
                        count: 1,
                    },
                );
                return HomeLockGuard {
                    path,
                    _not_send: std::marker::PhantomData,
                };
            }
        }
    }
}

/// Attempts to acquire exclusive mutation access for `home` without blocking.
///
/// Returns `Some(guard)` if the lock was acquired or already held by the current thread,
/// or `None` if another thread currently holds it.
pub fn try_lock_home(home: &Path) -> Option<HomeLockGuard> {
    let path = normalize_home(home);
    let current_tid = std::thread::current().id();
    let reg = registry();
    let mut locks = reg.locks.lock().unwrap_or_else(|e| e.into_inner());
    match locks.get_mut(&path) {
        Some(state) => {
            if state.holder == current_tid {
                state.count += 1;
                Some(HomeLockGuard {
                    path,
                    _not_send: std::marker::PhantomData,
                })
            } else {
                None
            }
        }
        None => {
            locks.insert(
                path.clone(),
                LockState {
                    holder: current_tid,
                    count: 1,
                },
            );
            Some(HomeLockGuard {
                path,
                _not_send: std::marker::PhantomData,
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc::channel;
    use std::time::Duration;

    #[test]
    fn reentrancy_on_same_thread() {
        let dir = std::env::temp_dir().join(format!("test-lock-reenter-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let g1 = lock_home(&dir);
        let g2 = lock_home(&dir);
        let g3 = try_lock_home(&dir);
        assert!(g3.is_some());
        drop(g3);
        drop(g2);
        drop(g1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn mutual_exclusion_between_threads() {
        let dir = std::env::temp_dir().join(format!("test-lock-mutex-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let g1 = lock_home(&dir);

        let (tx, rx) = channel();
        let target = dir.clone();
        std::thread::spawn(move || {
            let acquired = try_lock_home(&target);
            tx.send(acquired.is_some()).unwrap();
        });
        assert_eq!(rx.recv_timeout(Duration::from_secs(2)), Ok(false));

        drop(g1);

        let (tx2, rx2) = channel();
        let target2 = dir.clone();
        std::thread::spawn(move || {
            let _g = lock_home(&target2);
            tx2.send(true).unwrap();
        });
        assert_eq!(rx2.recv_timeout(Duration::from_secs(2)), Ok(true));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn different_homes_do_not_block_each_other() {
        let d1 = std::env::temp_dir().join(format!("test-lock-diff-1-{}", std::process::id()));
        let d2 = std::env::temp_dir().join(format!("test-lock-diff-2-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&d1);
        let _ = std::fs::create_dir_all(&d2);

        let g1 = lock_home(&d1);
        let (tx, rx) = channel();
        let target = d2.clone();
        std::thread::spawn(move || {
            let g2 = try_lock_home(&target);
            tx.send(g2.is_some()).unwrap();
        });
        assert_eq!(rx.recv_timeout(Duration::from_secs(2)), Ok(true));
        drop(g1);
        let _ = std::fs::remove_dir_all(&d1);
        let _ = std::fs::remove_dir_all(&d2);
    }
}
