use super::*;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

struct Temp(PathBuf);
impl Temp {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "horizon-native-cache-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn put(&self, name: &str, bytes: &[u8]) -> PathBuf {
        let path = self.0.join(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, bytes).unwrap();
        path
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn clears_only_disposable_outputs_and_keeps_installed_games_and_player_data() {
    let dir = Temp::new();
    let protected = [
        "saves/W/level.dat",
        "instances/A/options.txt",
        "mods/player-mod.hnmod",
        "versions/native-game/horizon-client",
        "native/server-builds/engine/horizon-engine",
        "native/stage/Cargo.toml",
        "native/cache/mod-output",
        "cargo/registry/crate",
        "rust/old/bin/rustc",
        "native/target/debug/custom-output",
        "native/target/.rustc_info.json",
    ];
    for path in protected {
        dir.put(path, path.as_bytes());
    }
    dir.put("native/target/fast/deps/a", b"abc");
    dir.put("native/target/release/incremental/b", b"1234567");
    dir.put("native/target/fast/.cargo-lock", b"fast-lock");
    dir.put("native/target/release/.cargo-lock", b"release-lock");
    dir.put("native/built.json", b"old incremental records");
    let original_lock = File::options()
        .read(true)
        .write(true)
        .open(dir.0.join("native/target/fast/.cargo-lock"))
        .unwrap();
    let cache = Cache::acquire(&dir.0).unwrap();
    assert_eq!(cache.bytes().unwrap(), 10);
    assert_eq!(cache.clear().unwrap(), 10);
    assert_eq!(cache.bytes().unwrap(), 0);
    assert_eq!(cache.clear().unwrap(), 0);
    assert!(!dir.0.join("native/built.json").exists());
    for path in protected {
        assert_eq!(fs::read(dir.0.join(path)).unwrap(), path.as_bytes());
    }
    for (profile, contents) in [
        ("fast", b"fast-lock".as_slice()),
        ("release", b"release-lock".as_slice()),
    ] {
        assert!(dir.0.join("native/target").join(profile).is_dir());
        assert_eq!(
            fs::read(
                dir.0
                    .join("native/target")
                    .join(profile)
                    .join(".cargo-lock")
            )
            .unwrap(),
            contents
        );
    }
    original_lock.try_lock().unwrap();
    let reopened = File::options()
        .read(true)
        .write(true)
        .open(dir.0.join("native/target/fast/.cargo-lock"))
        .unwrap();
    assert!(
        matches!(reopened.try_lock(), Err(fs::TryLockError::WouldBlock)),
        "cleanup must retain the same Cargo lock file, not replace it"
    );
}

#[test]
fn a_released_lock_is_free_while_a_spawned_child_still_holds_its_descriptor() {
    let dir = Temp::new();
    let path = dir.put("native/.build.lock", b"");
    let held = lock(&path).unwrap();
    // What a child spawned at this moment holds until its exec: a copy of the same descriptor.
    let inherited = held.0.try_clone().unwrap();
    drop(held);
    assert!(lock(&path).is_ok());
    drop(inherited);
}

#[test]
fn limit_keeps_warm_cache_until_exceeded_and_zero_disables_automatic_cleanup() {
    let dir = Temp::new();
    let artifact = dir.put("native/target/fast/compiled", b"12345");
    let cache = Cache::acquire(&dir.0).unwrap();
    assert_eq!(cache.trim(0).unwrap(), 0);
    assert_eq!(cache.trim(6).unwrap(), 0);
    assert_eq!(cache.trim(5).unwrap(), 0);
    assert_eq!(fs::read(&artifact).unwrap(), b"12345");
    assert_eq!(cache.trim(4).unwrap(), 5);
    assert!(!artifact.exists());
}

#[test]
fn launcher_build_lease_refuses_concurrent_operations_and_releases_without_deleting_lock() {
    let dir = Temp::new();
    let artifact = dir.put("native/target/fast/a", b"keep");
    let first = Cache::acquire(&dir.0).unwrap();
    assert!(matches!(Cache::acquire(&dir.0), Err(Error::Busy)));
    assert_eq!(fs::read(artifact).unwrap(), b"keep");
    drop(first);
    assert!(dir.0.join("native/.build.lock").is_file());
    let next = Cache::acquire(&dir.0).unwrap();
    assert_eq!(next.clear().unwrap(), 4);
}

#[test]
fn an_active_release_build_prevents_any_deletion_in_the_fast_profile_too() {
    let dir = Temp::new();
    let first = dir.put("native/target/fast/keep", b"first");
    let second = dir.put("native/target/release/keep", b"second");
    let cargo_lock = dir.put("native/target/release/.cargo-lock", b"");
    let active = File::options()
        .read(true)
        .write(true)
        .open(&cargo_lock)
        .unwrap();
    active.try_lock().unwrap();
    let cache = Cache::acquire(&dir.0).unwrap();
    assert!(matches!(cache.clear(), Err(Error::Busy)));
    assert_eq!(fs::read(first).unwrap(), b"first");
    assert_eq!(fs::read(second).unwrap(), b"second");
    drop(active);
    assert_eq!(cache.clear().unwrap(), 11);
    assert!(cargo_lock.exists());
}

#[cfg(unix)]
fn link_dir(from: &Path, to: &Path) {
    std::os::unix::fs::symlink(from, to).unwrap();
}
#[cfg(windows)]
fn link_dir(from: &Path, to: &Path) {
    let output = Command::new("cmd")
        .args(["/C", "mklink", "/J"])
        .arg(to.to_string_lossy().replace('/', "\\"))
        .arg(from.to_string_lossy().replace('/', "\\"))
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn refuses_directory_links_and_junctions_before_deleting_anything() {
    for name in [
        "native",
        "native/target",
        "native/target/fast",
        "native/target/fast/nested",
    ] {
        let dir = Temp::new();
        let player = dir.put("player-data/level.dat", b"irreplaceable world");
        let outside = player.parent().unwrap();
        let link = dir.0.join(name);
        fs::create_dir_all(link.parent().unwrap()).unwrap();
        link_dir(outside, &link);
        let other = if name == "native" {
            dir.put("other-cache", b"other")
        } else {
            dir.put("native/target/release/must-survive", b"other")
        };
        match Cache::acquire(&dir.0) {
            Err(Error::Unsafe(_)) => assert_eq!(name, "native"),
            Ok(cache) => {
                assert!(matches!(cache.bytes(), Err(Error::Unsafe(_))));
                assert!(matches!(cache.clear(), Err(Error::Unsafe(_))));
            }
            Err(error) => panic!("{error:?}"),
        }
        assert_eq!(fs::read(&player).unwrap(), b"irreplaceable world");
        assert_eq!(fs::read(other).unwrap(), b"other");
        assert!(!outside.join(".build.lock").exists());
    }
}

#[test]
fn an_ancestor_redirect_cannot_remove_external_player_data() {
    let dir = Temp::new();
    let managed = dir.0.join("managed");
    let old = dir.0.join("previous-managed");
    let outside = dir.0.join("player-data");
    let original = dir.put("managed/native/target/fast/output", b"original cache");
    let player = dir.put("player-data/native/target/fast/world", b"player data");
    let cache = Cache::acquire(&managed).unwrap();
    match fs::rename(&managed, &old) {
        Ok(()) => link_dir(&outside, &managed),
        Err(error) => {
            // Windows can forbid renaming the ancestor while the lease is open. That OS
            // barrier also protects the player data; validate it instead of skipping the case.
            assert!(
                cfg!(windows) && error.kind() == io::ErrorKind::PermissionDenied,
                "{error}"
            );
            assert_eq!(fs::read(&original).unwrap(), b"original cache");
            assert_eq!(cache.clear().unwrap(), b"original cache".len() as u64);
            assert_eq!(fs::read(&player).unwrap(), b"player data");
            return;
        }
    }
    assert!(matches!(cache.clear(), Err(Error::Unsafe(_))));
    assert_eq!(fs::read(player).unwrap(), b"player data");
    assert_eq!(
        fs::read(old.join(original.strip_prefix(managed).unwrap())).unwrap(),
        b"original cache"
    );
}

#[test]
fn unexpected_file_types_and_linked_records_fail_before_removing_outputs() {
    for wrong in [
        "native/target/release",
        "native/target/release/.cargo-lock",
        "native/built.json",
    ] {
        let dir = Temp::new();
        let artifact = dir.put("native/target/fast/keep", b"preserve");
        if wrong.ends_with("release") {
            dir.put(wrong, b"not a directory");
        } else {
            fs::create_dir_all(dir.0.join(wrong)).unwrap();
        }
        let cache = Cache::acquire(&dir.0).unwrap();
        assert!(matches!(cache.clear(), Err(Error::Unsafe(_))));
        assert_eq!(fs::read(artifact).unwrap(), b"preserve");
    }
}

struct Process(Child);
impl Process {
    fn wait(&mut self) {
        let start = Instant::now();
        loop {
            if let Some(status) = self.0.try_wait().unwrap() {
                assert!(status.success());
                return;
            }
            assert!(
                start.elapsed() < Duration::from_secs(90),
                "Cargo fixture timed out"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
    }
}
impl Drop for Process {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn retry_busy<T>(mut run: impl FnMut() -> io::Result<T>) -> io::Result<T> {
    let start = Instant::now();
    loop {
        match run() {
            Ok(value) => return Ok(value),
            Err(err)
                if err.raw_os_error() == Some(26) && start.elapsed() < Duration::from_secs(2) =>
            {
                std::thread::sleep(Duration::from_millis(20));
            }
            Err(err) => return Err(err),
        }
    }
}

#[test]
fn real_cargo_build_is_protected_and_installed_copy_survives_cleanup() {
    let dir = Temp::new();
    dir.put("fixture/Cargo.toml", b"[package]\nname='native-cache-fixture'\nversion='0.1.0'\nedition='2021'\n[workspace]\n[profile.fast]\ninherits='dev'\n");
    dir.put(
        "fixture/src/main.rs",
        br#"fn main() {
            let args: Vec<_> = std::env::args_os().collect();
            if args.len() == 3 {
                let lease = std::fs::File::options().read(true).write(true).open(&args[1]).unwrap();
                lease.try_lock().unwrap();
                std::fs::write(&args[2], "locked").unwrap();
                loop { std::thread::sleep(std::time::Duration::from_millis(20)); }
            }
            println!("installed fixture");
        }"#,
    );
    dir.put(
        "fixture/build.rs",
        br#"fn main() {
        let root = std::path::PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").unwrap());
        std::fs::write(root.join("started"), "ready").unwrap();
        for _ in 0..9000 {
            if root.join("resume").exists() { return; }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        panic!("fixture timed out");
    }"#,
    );
    let sentinel = dir.put("native/target/fast/prior-output", b"still in use");
    let cache = Cache::acquire(&dir.0).unwrap();
    let log = File::create(dir.0.join("cargo.log")).unwrap();
    let mut cargo = Process(
        Command::new(std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into()))
            .args(["build", "--offline", "--profile", "fast", "--manifest-path"])
            .arg(dir.0.join("fixture/Cargo.toml"))
            .arg("--target-dir")
            .arg(dir.0.join("native/target"))
            .stdout(Stdio::from(log.try_clone().unwrap()))
            .stderr(Stdio::from(log))
            .spawn()
            .unwrap(),
    );
    let start = Instant::now();
    while !dir.0.join("fixture/started").exists() {
        assert!(
            cargo.0.try_wait().unwrap().is_none(),
            "{}",
            fs::read_to_string(dir.0.join("cargo.log")).unwrap()
        );
        assert!(
            start.elapsed() < Duration::from_secs(90),
            "Cargo never reached its build script"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    // Simulate a launcher exiting while its Cargo child continues: the Cargo lock alone
    // must still protect the build after our launcher's own lease has disappeared.
    drop(cache);
    let cache = Cache::acquire(&dir.0).unwrap();
    assert!(
        matches!(cache.clear(), Err(Error::Busy)),
        "real Cargo lock must prevent cleanup"
    );
    assert_eq!(fs::read(sentinel).unwrap(), b"still in use");
    dir.put("fixture/resume", b"continue");
    cargo.wait();
    let binary = format!("native-cache-fixture{}", std::env::consts::EXE_SUFFIX);
    let installed = dir.0.join(format!("versions/native/{binary}"));
    fs::create_dir_all(installed.parent().unwrap()).unwrap();
    let temp_installed = dir.0.join(format!("versions/native/{binary}.tmp"));
    fs::copy(
        dir.0.join("native/target/fast").join(&binary),
        &temp_installed,
    )
    .unwrap();
    fs::rename(&temp_installed, &installed).unwrap();
    assert!(cache.clear().unwrap() > 0);
    assert!(!dir.0.join("native/target/fast").join(binary).exists());
    let output = retry_busy(|| Command::new(&installed).output()).unwrap();
    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stdout).unwrap().trim(),
        "installed fixture"
    );
    drop(cache);
    // An independent process holds the launcher's lease; force-exit releases the OS lock
    // without deleting its control file, unlike a stale PID or create_new marker.
    let ready = dir.0.join("lease-ready");
    let mut holder = Process(
        retry_busy(|| {
            Command::new(&installed)
                .arg(dir.0.join("native/.build.lock"))
                .arg(&ready)
                .spawn()
        })
        .unwrap(),
    );
    let started = Instant::now();
    while !ready.exists() {
        assert!(holder.0.try_wait().unwrap().is_none());
        assert!(started.elapsed() < Duration::from_secs(30));
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(matches!(Cache::acquire(&dir.0), Err(Error::Busy)));
    holder.0.kill().unwrap();
    holder.0.wait().unwrap();
    assert!(Cache::acquire(&dir.0).is_ok());
}
