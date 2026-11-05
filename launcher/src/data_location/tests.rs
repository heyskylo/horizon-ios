use super::*;
use std::{
    collections::BTreeMap,
    sync::atomic::{AtomicUsize, Ordering},
};

struct Temp(PathBuf);
impl Temp {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        loop {
            let path = std::env::temp_dir().join(format!(
                "horizon-location-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            match fs::create_dir(&path) {
                Ok(()) => return Self(checked_path(&fs::canonicalize(path).unwrap()).unwrap()),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(e) => panic!("create fixture: {e}"),
            }
        }
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        // std removes links themselves rather than traversing their targets.
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn snapshot(root: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    fn visit(root: &Path, path: &Path, files: &mut BTreeMap<PathBuf, Vec<u8>>) {
        for entry in fs::read_dir(path).unwrap() {
            let path = entry.unwrap().path();
            let meta = fs::symlink_metadata(&path).unwrap();
            reject_link(&path, &meta).unwrap(); // Never follow links while checking fixtures.
            if meta.is_dir() {
                visit(root, &path, files);
            } else {
                files.insert(
                    path.strip_prefix(root).unwrap().into(),
                    fs::read(path).unwrap(),
                );
            }
        }
    }
    let mut files = BTreeMap::new();
    visit(root, root, &mut files);
    files
}

struct Fixture {
    temp: Temp,
    source: Roots,
    bootstrap: PathBuf,
    pointer: Vec<u8>,
    original: BTreeMap<PathBuf, Vec<u8>>,
}
impl Fixture {
    fn new(build: &str) -> Self {
        let temp = Temp::new();
        let source = Roots {
            launcher: temp.0.join("source/launcher"),
            game: temp.0.join("source/game"),
            build: temp.0.join("source").join(build),
        };
        for (root, name, bytes) in [
            (&source.launcher, "accounts.json", &b"{\"accounts\":[]}"[..]),
            (
                &source.game,
                "saves/dummy/level.dat",
                &b"\0\xff\x80world\0"[..],
            ),
            (
                &source.build,
                "cache/compiled.bin",
                &b"\0\x01\xfe\xffbuild"[..],
            ),
        ] {
            let path = root.join(name);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, bytes).unwrap();
            fs::create_dir_all(root.join("empty")).unwrap();
        }
        let bootstrap = temp.0.join("bootstrap/location.json");
        fs::create_dir(bootstrap.parent().unwrap()).unwrap();
        let pointer = serde_json::to_vec(&Location {
            version: 1,
            roots: source.clone(),
        })
        .unwrap();
        fs::write(&bootstrap, &pointer).unwrap();
        let original = snapshot(&temp.0.join("source"));
        Self {
            temp,
            source,
            bootstrap,
            pointer,
            original,
        }
    }
    fn target(&self) -> PathBuf {
        self.temp.0.join("destination")
    }
    fn unchanged(&self) {
        assert_eq!(snapshot(&self.temp.0.join("source")), self.original);
        for root in [&self.source.launcher, &self.source.game, &self.source.build] {
            assert_eq!(fs::read_dir(root.join("empty")).unwrap().count(), 0);
        }
    }
    fn old_pointer(&self) {
        assert_eq!(fs::read(&self.bootstrap).unwrap(), self.pointer);
        assert_eq!(
            resolve_at(&self.bootstrap).unwrap(),
            Some(self.source.clone())
        );
        self.unchanged();
    }
    fn migrated(&self, roots: &Roots) {
        assert_eq!(resolve_at(&self.bootstrap).unwrap(), Some(roots.clone()));
        for (source, dest) in [
            (&self.source.launcher, &roots.launcher),
            (&self.source.game, &roots.game),
            (&self.source.build, &roots.build),
        ] {
            assert_eq!(snapshot(source), snapshot(dest));
            assert_eq!(fs::read_dir(dest.join("empty")).unwrap().count(), 0);
        }
        self.unchanged();
    }
    fn refuse(&self, target: &Path) -> String {
        let error = migrate(&self.source, target, &self.bootstrap, |_| {}).unwrap_err();
        self.old_pointer();
        error
    }
}

#[test]
fn full_move_preserves_source_and_commits_separate_build() {
    let f = Fixture::new("build");
    let target = f.target();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&f.source.launcher, fs::Permissions::from_mode(0o750)).unwrap();
        fs::set_permissions(
            f.source.launcher.join("accounts.json"),
            fs::Permissions::from_mode(0o640),
        )
        .unwrap();
    }
    let roots = migrate(&f.source, &target, &f.bootstrap, |_| {}).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        for root in [&f.source.launcher, &roots.launcher] {
            assert_eq!(
                fs::metadata(root).unwrap().permissions().mode() & 0o777,
                0o750
            );
            assert_eq!(
                fs::metadata(root.join("accounts.json"))
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o640
            );
        }
    }
    assert_eq!(
        roots,
        Roots {
            launcher: target.join("launcher"),
            game: target.join("game"),
            build: target.join("build"),
        }
    );
    assert_ne!(fs::read(&f.bootstrap).unwrap(), f.pointer);
    f.migrated(&roots);
}

#[test]
fn full_move_preserves_aliased_and_nested_build_layouts() {
    for build in ["launcher", "launcher/native-build"] {
        let f = Fixture::new(build);
        let roots = migrate(&f.source, &f.target(), &f.bootstrap, |_| {}).unwrap();
        assert_eq!(roots.build, f.target().join(build));
        assert!(!f.target().join("build").exists());
        f.migrated(&roots);
    }
}

#[test]
fn nonempty_destination_preserves_pointer_and_contents() {
    let f = Fixture::new("build");
    fs::create_dir(f.target()).unwrap();
    fs::write(f.target().join("sentinel"), b"keep").unwrap();
    assert!(f.refuse(&f.target()).contains("empty folder"));
    assert_eq!(fs::read(f.target().join("sentinel")).unwrap(), b"keep");
}

#[test]
fn nested_same_and_ancestor_destinations_preserve_pointer() {
    let f = Fixture::new("build");
    for target in [
        f.source.game.join("nested"),
        f.source.launcher.clone(),
        f.temp.0.join("source"),
    ] {
        assert!(f.refuse(&target).contains("overlaps"));
    }
}

#[cfg(any(unix, windows))]
#[test]
fn source_entry_link_is_rejected_without_touching_its_target() {
    let f = Fixture::new("build");
    let outside = Temp::new();
    fs::write(outside.0.join("sentinel"), b"untouched").unwrap();
    let link = f.source.game.join("linked");
    #[cfg(unix)]
    std::os::unix::fs::symlink(&outside.0, &link).unwrap();
    #[cfg(windows)]
    {
        let output = std::process::Command::new("cmd.exe")
            .args(["/d", "/c", "mklink", "/J"])
            .arg(link.to_string_lossy().replace('/', "\\"))
            .arg(outside.0.to_string_lossy().replace('/', "\\"))
            .output()
            .unwrap();
        assert!(output.status.success(), "junction setup: {output:?}");
    }
    struct Link(PathBuf);
    impl Drop for Link {
        fn drop(&mut self) {
            #[cfg(unix)]
            let _ = fs::remove_file(&self.0);
            #[cfg(windows)]
            let _ = fs::remove_dir(&self.0);
        }
    }
    let link = Link(link);
    let error = migrate(&f.source, &f.target(), &f.bootstrap, |_| {}).unwrap_err();
    drop(link);
    assert!(
        error.contains("Symbolic links and reparse points"),
        "{error}"
    );
    f.old_pointer();
    assert_eq!(fs::read(outside.0.join("sentinel")).unwrap(), b"untouched");
}

#[cfg(unix)]
struct PermissionsGuard(PathBuf, fs::Permissions);
#[cfg(unix)]
impl Drop for PermissionsGuard {
    fn drop(&mut self) {
        fs::set_permissions(&self.0, self.1.clone()).unwrap();
    }
}

#[cfg(any(unix, windows))]
#[test]
fn unreadable_source_keeps_old_pointer_and_source_bytes() {
    let f = Fixture::new("build");
    let path = f.source.game.join("saves/dummy/level.dat");
    #[cfg(windows)]
    let guard = {
        use std::os::windows::fs::OpenOptionsExt;
        OpenOptions::new()
            .read(true)
            .share_mode(0)
            .open(&path)
            .unwrap()
    };
    #[cfg(unix)]
    let guard = {
        use std::os::unix::fs::PermissionsExt;
        unsafe extern "C" {
            fn geteuid() -> u32;
        }
        if unsafe { geteuid() } == 0 {
            eprintln!("SKIP unreadable_source_keeps_old_pointer_and_source_bytes: root bypasses chmod(000); run as an unprivileged user");
            return;
        }
        let guard = PermissionsGuard(path.clone(), fs::metadata(&path).unwrap().permissions());
        fs::set_permissions(&path, fs::Permissions::from_mode(0o0)).unwrap();
        guard
    };
    assert!(
        File::open(&path).is_err(),
        "fixture must deny an actual read"
    );
    let mut committing = false;
    let error = migrate(&f.source, &f.target(), &f.bootstrap, |s| {
        committing |= s == "Committing data location"
    })
    .unwrap_err();
    drop(guard);
    assert!(!committing, "copy must fail before pointer commit");
    assert!(
        error.contains(&checked_path(&path).unwrap().display().to_string()),
        "{error}"
    );
    f.old_pointer();
}

#[test]
fn stale_worker_cannot_commit_after_successful_migration() {
    let f = Fixture::new("build");
    let roots = migrate(&f.source, &f.target(), &f.bootstrap, |_| {}).unwrap();
    let pointer = fs::read(&f.bootstrap).unwrap();
    let stale_target = f.temp.0.join("stale-worker");
    let error = migrate(&f.source, &stale_target, &f.bootstrap, |_| {}).unwrap_err();
    assert!(error.contains("Data location changed"), "{error}");
    assert_eq!(fs::read(&f.bootstrap).unwrap(), pointer);
    assert!(!stale_target.exists());
    f.migrated(&roots);
}

#[test]
fn shared_lock_blocks_migration_until_all_readers_release() {
    let f = Fixture::new("build");
    let first = acquire_lock_at(&f.bootstrap).unwrap();
    let second = acquire_lock_at(&f.bootstrap).unwrap();
    assert!(f.refuse(&f.target()).contains("Cannot lock"));
    drop(first);
    assert!(f.refuse(&f.target()).contains("Cannot lock"));
    assert!(!f.target().exists());
    drop(second);
    f.old_pointer();
    let roots = migrate(&f.source, &f.target(), &f.bootstrap, |_| {}).unwrap();
    f.migrated(&roots);
}

#[cfg(windows)]
#[test]
fn pointer_commit_failure_keeps_old_pointer_and_retries() {
    use std::os::windows::fs::OpenOptionsExt;
    let f = Fixture::new("build");
    // Allow reads/writes, but deny replacement of the existing pointer.
    let handle = OpenOptions::new()
        .read(true)
        .share_mode(1 | 2)
        .open(&f.bootstrap)
        .unwrap();
    let mut committing = false;
    let error = migrate(&f.source, &f.target(), &f.bootstrap, |s| {
        committing |= s == "Committing data location"
    })
    .unwrap_err();
    drop(handle);
    assert!(committing, "must fail during pointer commit: {error}");
    assert!(
        error.contains(&f.bootstrap.display().to_string()),
        "{error}"
    );
    f.old_pointer();
    let roots = migrate(&f.source, &f.temp.0.join("retry"), &f.bootstrap, |_| {}).unwrap();
    f.migrated(&roots);
}

#[cfg(windows)]
#[test]
fn canonical_windows_paths_are_validated_from_the_complete_root() {
    let path = std::fs::canonicalize(std::env::temp_dir()).unwrap();
    let plain = checked_path(&path).unwrap();
    assert_eq!(std::fs::canonicalize(&plain).unwrap(), path);
    assert_eq!(checked_path(&plain).unwrap(), plain);
}

#[test]
fn a_missing_or_unreadable_location_is_reported_before_the_window_opens() {
    let temp = Temp::new();
    let bootstrap = temp.0.join("location.json");
    let roots = Roots {
        launcher: temp.0.join("unplugged/launcher"),
        game: temp.0.join("unplugged/game"),
        build: temp.0.join("unplugged/build"),
    };
    let pointer = serde_json::to_vec(&Location {
        version: 1,
        roots: roots.clone(),
    })
    .unwrap();
    fs::write(&bootstrap, &pointer).unwrap();
    let error = startup_error_at(&bootstrap).expect("a missing folder must reach the window");
    assert!(error.contains("is missing"), "{error}");

    // A damaged pointer is the other way the launcher finds no data folder.
    fs::write(&bootstrap, b"not json").unwrap();
    let damaged = startup_error_at(&bootstrap).expect("a damaged pointer must reach the window");
    assert!(damaged.contains("Invalid"), "{damaged}");
}

#[test]
fn using_the_default_folder_keeps_the_pointer_beside_it() {
    let temp = Temp::new();
    let bootstrap = temp.0.join("location.json");
    let source = Roots {
        launcher: temp.0.join("source/launcher"),
        game: temp.0.join("source/game"),
        build: temp.0.join("source/build"),
    };
    for root in [&source.launcher, &source.game, &source.build] {
        fs::create_dir_all(root).unwrap();
    }
    let pointer = serde_json::to_vec(&Location {
        version: 1,
        roots: source.clone(),
    })
    .unwrap();
    fs::write(&bootstrap, &pointer).unwrap();

    use_default_at(&bootstrap).unwrap();

    assert!(!bootstrap.exists());
    assert_eq!(
        fs::read(temp.0.join("location.json.old")).unwrap(),
        pointer,
        "the old pointer stays recoverable"
    );
    for root in [&source.launcher, &source.game, &source.build] {
        assert!(root.is_dir(), "{} is not deleted", root.display());
    }
}

#[cfg(unix)]
#[test]
fn a_config_folder_behind_a_link_still_opens() {
    // A dotfiles-managed ~/.config is a link: players who never moved their data must start
    // without an error, and a pointer kept there must still resolve and lock.
    let temp = Temp::new();
    let real = temp.0.join("dotfiles/config");
    fs::create_dir_all(&real).unwrap();
    let config = temp.0.join("config");
    std::os::unix::fs::symlink(&real, &config).unwrap();
    let bootstrap = config.join("horizon-launcher-location/location.json");

    assert_eq!(resolve_at(&bootstrap), Ok(None));
    assert_eq!(startup_error_at(&bootstrap), None);
    drop(acquire_lock_at(&bootstrap).unwrap());

    let roots = Roots {
        launcher: temp.0.join("data/launcher"),
        game: temp.0.join("data/game"),
        build: temp.0.join("data/build"),
    };
    for root in [&roots.launcher, &roots.game, &roots.build] {
        fs::create_dir_all(root).unwrap();
    }
    let pointer = serde_json::to_vec(&Location {
        version: 1,
        roots: roots.clone(),
    })
    .unwrap();
    fs::write(&bootstrap, pointer).unwrap();
    assert_eq!(resolve_at(&bootstrap), Ok(Some(roots)));

    // The lock behind the link is the one a worker takes through the real path.
    let reader = acquire_lock_at(&bootstrap).unwrap();
    assert!(lock_at(&real.join("horizon-launcher-location/location.json"), true).is_err());
    drop(reader);
}
