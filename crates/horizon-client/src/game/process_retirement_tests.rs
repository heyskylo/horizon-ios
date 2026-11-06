//! The actual process-exit guard, after its engine-local readers drop.
use super::ProcessResourceRetirement;
use horizon_assets::root_lease::RootLease;
use std::{
    fs,
    path::PathBuf,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

struct Scratch(PathBuf);
impl Scratch {
    fn new() -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "horizon-process-retirement-{}-{nonce}",
            std::process::id()
        ));
        fs::create_dir(&path).unwrap();
        Self(fs::canonicalize(path).unwrap())
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn outer_guard_keeps_external_readers_then_collects_after_local_owners_drop() {
    let scratch = Scratch::new();
    let home = scratch.0.clone();
    let stamp = "0123456789abcdef".repeat(4);
    let root = home.join(".horizon-cache/mod-content").join(&stamp[..16]);
    fs::create_dir_all(&root).unwrap();
    let marker = root.join("horizon-mod-content.stamp");
    fs::write(&marker, stamp).unwrap();
    fs::write(root.join("reader.txt"), b"an existing generation").unwrap();
    let old = SystemTime::now()
        .checked_sub(Duration::from_secs(2 * 24 * 3600))
        .unwrap();
    fs::File::options()
        .write(true)
        .open(&marker)
        .unwrap()
        .set_times(fs::FileTimes::new().set_modified(old))
        .unwrap();
    assert!(
        fs::metadata(&marker)
            .unwrap()
            .modified()
            .unwrap()
            .elapsed()
            .unwrap()
            > Duration::from_secs(24 * 3600)
    );

    // Another owner can outlive this engine/process guard. The real OS reader
    // lock makes its final asynchronous collection leave the generation intact.
    let external = RootLease::acquire(&root).unwrap();
    assert!(external.is_active());
    drop(ProcessResourceRetirement { home: home.clone() });
    assert!(root.join("reader.txt").is_file());
    assert!(ProcessResourceRetirement::readers_stopped());
    drop(external);

    {
        // Exactly the production declaration order: the outer guard precedes
        // engine-local roots/transactions, so those owners drop before GC waits
        // for its own HomeAccess transaction. No direct collector API is used.
        let _retirement = ProcessResourceRetirement { home: home.clone() };
        let _local_root = RootLease::acquire(&root).unwrap();
        let _local_snapshot = horizon_client::import::acquire_home_access(&home).unwrap();
        assert!(root.is_dir());
    }
    assert!(
        !root.exists(),
        "the next actual guard collects the aged, unleased root"
    );
    assert!(
        ProcessResourceRetirement::readers_stopped(),
        "collector handles retired"
    );
}
