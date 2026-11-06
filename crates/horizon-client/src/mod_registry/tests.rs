//! Installing from a local test registry: a real content `.hmod`, an index
//! naming it, and the game folder the launcher shares.
use super::*;

pub(super) fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "horizon-mod-registry-{name}-{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

/// A content mod `ruby` at `version`: a manifest and one lang file.
fn ruby(version: &str) -> Vec<u8> {
    let manifest = format!(
        "id = \"ruby\"\nname = \"Ruby\"\nversion = \"{version}\"\napi = \"0.2\"\nside = \"both\"\ncapabilities = [\"registries\"]\n"
    );
    horizon_mods::Package::write_files(
        &manifest,
        None,
        &[(
            "assets/ruby/lang/en_us.json",
            br#"{"item.ruby.ruby": "Ruby"}"#,
        )],
    )
    .unwrap()
}

/// A registry listing `ruby` at `version` with these bytes (the file at
/// `url`), and a native mod and a mod for another Minecraft version that the
/// game doesn't offer.
fn registry(version: &str, bytes: &[u8], url: &str) -> Index {
    let file = |name: &str, size: usize, sha: String| serde_json::json!({ "name": name, "url": url, "size": size, "sha256": sha });
    let ruby = serde_json::json!({
        "id": "ruby", "kind": "mod", "name": "Ruby", "summary": "Rubies",
        "description": "# Ruby\nGems.", "authors": ["someone"], "license": "MIT",
        "source": "https://github.com/someone/ruby", "categories": ["decoration"],
        "tags": ["gems"], "downloads": 5,
        "versions": [{ "version": version, "published": "2026-11-05", "api": "0.2",
            "side": "both", "capabilities": ["registries"], "game": crate::mod_registry::GAME,
            "file": file(&format!("ruby-{version}.hmod"), bytes.len(), index::sha256(bytes)) }]
    });
    let old = serde_json::json!({
        "id": "old", "kind": "mod", "name": "Old", "summary": "For 1.21",
        "license": "MIT", "source": "https://github.com/someone/old",
        "versions": [{ "version": "1", "api": "0.2", "game": "1.21.1",
            "capabilities": ["registries"],
            "file": file("old-1.hmod", 1, "a".repeat(64)) }]
    });
    let native = serde_json::json!({
        "id": "zoom", "kind": "native-mod", "name": "Zoom", "summary": "Zoom",
        "license": "MIT", "source": "https://github.com/someone/zoom", "downloads": 99,
        "versions": [{ "version": "1", "api": "0.2", "game": crate::mod_registry::GAME,
            "file": file("zoom-1.hnmod", 1, "b".repeat(64)) }]
    });
    let text = serde_json::json!({ "schema": 1, "projects": [ruby, old, native] }).to_string();
    Index::parse_local(&text).unwrap()
}

fn serve(bytes: Vec<u8>) -> impl Fn(&str, u64) -> Result<Vec<u8>, String> {
    move |_, max| match bytes.len() as u64 <= max {
        true => Ok(bytes.clone()),
        false => Err("too large".into()),
    }
}

pub(super) fn generation_package(version: &str, marker: &str) -> Vec<u8> {
    let manifest = format!(
        "id=\"ruby\"\nname=\"Ruby\"\nversion=\"{version}\"\napi=\"0.2\"\nside=\"both\"\ncapabilities=[\"registries\"]\n"
    );
    horizon_mods::Package::write_files(
        &manifest,
        None,
        &[("assets/ruby/marker.txt", marker.as_bytes())],
    )
    .unwrap()
}

pub(super) fn read_generation(home: &Path) -> (String, Vec<u8>) {
    let path = home.join(crate::mod_list::DIR).join("ruby.hmod");
    let package = horizon_mods::Package::read(&path).unwrap();
    let content = horizon_mods::Content::read_declaring(&path, &package.manifest, &[]).unwrap();
    (
        package.manifest.version,
        content.file("assets/ruby/marker.txt").unwrap(),
    )
}

#[test]
fn a_content_snapshot_holds_one_generation_across_manifest_and_asset_reads() {
    use std::sync::mpsc;
    let home = scratch("snapshot-update");
    let old = generation_package("1", "old");
    let new = generation_package("2", "new");
    install(
        &registry("1", &old, "file:///ruby.hmod"),
        &home,
        "ruby",
        &serve(old),
    )
    .unwrap();
    let reader_home = home.clone();
    let (ready, parsed) = mpsc::channel();
    let (release, waiting) = mpsc::channel();
    let reader = std::thread::spawn(move || {
        with_content_snapshot(&reader_home, || {
            let path = reader_home.join(crate::mod_list::DIR).join("ruby.hmod");
            let package = horizon_mods::Package::read(&path).unwrap();
            ready.send(()).unwrap();
            waiting.recv().unwrap();
            let content =
                horizon_mods::Content::read_declaring(&path, &package.manifest, &[]).unwrap();
            (
                package.manifest.version,
                content.file("assets/ruby/marker.txt").unwrap(),
            )
        })
        .unwrap()
    });
    parsed.recv_timeout(Duration::from_secs(5)).unwrap();
    let writer_home = home.clone();
    let (probe, probed) = mpsc::channel();
    let writer = std::thread::spawn(move || {
        // A real OS-lock probe makes the interleaving deterministic: the
        // snapshot must own the gate at the midpoint of its two file reads.
        let file = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(writer_home.join(".mod-browser.lock"))
            .unwrap();
        let blocked = matches!(file.try_lock(), Err(fs::TryLockError::WouldBlock));
        drop(file);
        probe.send(blocked).unwrap();
        install(
            &registry("2", &new, "file:///ruby.hmod"),
            &writer_home,
            "ruby",
            &serve(new),
        )
    });
    let blocked = probed.recv_timeout(Duration::from_secs(5)).unwrap();
    release.send(()).unwrap();
    let snapshot = reader.join().unwrap();
    writer.join().unwrap().unwrap();
    assert!(
        blocked,
        "content snapshot released the writer gate between reads"
    );
    assert_eq!(snapshot, ("1".into(), b"old".to_vec()));
    assert_eq!(read_generation(&home), ("2".into(), b"new".to_vec()));
    fs::remove_dir_all(home).unwrap();
}

#[test]
fn the_browser_lists_only_content_mods_for_this_game() {
    let bytes = ruby("1.0");
    let index = registry("1.0", &bytes, "file:///ruby.hmod");
    let home = scratch("list");
    let listed = listings(&index, &home);
    let ids: Vec<&str> = listed.iter().map(|l| l.id.as_str()).collect();
    assert_eq!(ids, ["ruby"], "no native mod, no mod for another version");
    assert_eq!(listed[0].version, "1.0");
    assert_eq!(listed[0].page, "https://playhorizon.dev/mods/ruby");
    assert_eq!(listed[0].installed, None);
    fs::remove_dir_all(&home).unwrap();
}

#[test]
fn an_install_is_checked_then_recorded_as_the_launchers_own() {
    let bytes = ruby("1.0");
    let index = registry("1.0", &bytes, "file:///ruby.hmod");
    let home = scratch("install");
    // An entry of the launcher's this game doesn't model stays as it was.
    let theirs = serde_json::json!({ "entries": [{ "source": "modrinth", "project": "abc",
        "slug": "faithful", "title": "Faithful", "kind": "resource-pack", "version": "1",
        "number": "1", "file": "resourcepacks/faithful.zip", "checksum": "SHA-512:00",
        "page": "https://modrinth.com/resourcepack/faithful" }] });
    fs::write(home.join(MANIFEST), theirs.to_string()).unwrap();

    let done = install(&index, &home, "ruby", &serve(bytes.clone())).unwrap();
    assert_eq!(done.version, "1.0");
    assert!(!done.verified, "the sample isn't signed");
    let path = home.join("horizon-mods/ruby.hmod");
    assert_eq!(fs::read(&path).unwrap(), bytes);
    assert_eq!(
        horizon_mods::Package::read(&path).unwrap().manifest.id,
        "ruby"
    );
    let record: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(home.join(MANIFEST)).unwrap()).unwrap();
    let entries = record["entries"].as_array().unwrap();
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0]["slug"], "faithful");
    assert_eq!(entries[1]["source"], "horizon");
    assert_eq!(entries[1]["file"], "horizon-mods/ruby.hmod");
    assert_eq!(entries[1]["verdict"], "Unsigned");
    assert_eq!(listings(&index, &home)[0].installed.as_deref(), Some("1.0"));

    // An update replaces the file and the entry.
    let newer = ruby("1.1");
    let index = registry("1.1", &newer, "file:///ruby.hmod");
    assert!(listings(&index, &home)[0].has_update());
    install(&index, &home, "ruby", &serve(newer.clone())).unwrap();
    assert_eq!(fs::read(&path).unwrap(), newer);
    let record: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(home.join(MANIFEST)).unwrap()).unwrap();
    assert_eq!(record["entries"].as_array().unwrap().len(), 2);
    assert_eq!(record["entries"][1]["version"], "1.1");
    fs::remove_dir_all(&home).unwrap();
}

#[test]
fn bytes_other_than_the_index_lists_never_reach_the_mods_folder() {
    let bytes = ruby("1.0");
    let index = registry("1.0", &bytes, "file:///ruby.hmod");
    let home = scratch("tampered");
    let mut tampered = bytes.clone();
    *tampered.last_mut().unwrap() ^= 1;
    let error = install(&index, &home, "ruby", &serve(tampered)).unwrap_err();
    assert!(error.contains("SHA-256"), "{error}");
    assert!(!home.join("horizon-mods/ruby.hmod").exists());
    assert!(!home.join("mod-browser/downloads/ruby.hmod.part").exists());
    assert!(!home.join(MANIFEST).exists());
    // A taken-down file is refused before anything is downloaded.
    let mut index = index;
    index.revoked.push(index::Revoked {
        id: "ruby".into(),
        sha256: Some(index::sha256(&bytes)),
        reason: "bad".into(),
        date: "2026-11-05".into(),
    });
    assert!(install(&index, &home, "ruby", &serve(bytes)).is_err());
    assert!(!home.join("horizon-mods/ruby.hmod").exists());
    fs::remove_dir_all(&home).unwrap();
}

#[test]
fn a_mod_the_player_put_there_is_never_replaced() {
    let bytes = ruby("1.0");
    let index = registry("1.0", &bytes, "file:///ruby.hmod");
    let home = scratch("theirs");
    fs::create_dir_all(home.join("horizon-mods")).unwrap();
    fs::write(home.join("horizon-mods/ruby.hmod"), b"their own").unwrap();
    let error = install(&index, &home, "ruby", &serve(bytes)).unwrap_err();
    assert!(error.contains("already in horizon-mods"), "{error}");
    assert_eq!(
        fs::read(home.join("horizon-mods/ruby.hmod")).unwrap(),
        b"their own"
    );
    fs::remove_dir_all(&home).unwrap();
}

#[test]
fn concurrent_installs_keep_both_records_and_matching_packages() {
    use std::sync::Barrier;
    let first = ruby("1.0");
    let second = horizon_mods::Package::write_files(
        "id=\"sapphire\"\nname=\"Sapphire\"\nversion=\"1.0\"\napi=\"0.2\"\nside=\"both\"\ncapabilities=[\"registries\"]\n",
        None, &[],
    ).unwrap();
    let mut index = registry("1.0", &first, "file:///ruby.hmod");
    let mut project = index.project("ruby").unwrap().clone();
    project.id = "sapphire".into();
    project.name = "Sapphire".into();
    project.versions[0].file.name = "sapphire.hmod".into();
    project.versions[0].file.url = "file:///sapphire.hmod".into();
    project.versions[0].file.size = second.len() as u64;
    project.versions[0].file.sha256 = index::sha256(&second);
    index.projects.push(project);
    let index = Arc::new(index);
    let barrier = Arc::new(Barrier::new(2));
    let home = scratch("concurrent");
    let threads: Vec<_> = [("ruby", first.clone()), ("sapphire", second.clone())]
        .into_iter()
        .map(|(id, bytes)| {
            let index = index.clone();
            let barrier = barrier.clone();
            let home = home.clone();
            std::thread::spawn(move || {
                install(&index, &home, id, &|_, _| {
                    barrier.wait();
                    Ok(bytes.clone())
                })
            })
        })
        .collect();
    for thread in threads {
        thread.join().unwrap().unwrap();
    }
    let installed = Installed::load_checked(&home).unwrap();
    assert_eq!(installed.entries.len(), 2);
    for (id, bytes) in [("ruby", first), ("sapphire", second)] {
        assert_eq!(
            fs::read(home.join(crate::mod_list::DIR).join(format!("{id}.hmod"))).unwrap(),
            bytes
        );
        assert_eq!(
            installed.entries[installed.position(id).unwrap()]["checksum"],
            format!("SHA-256:{}", index::sha256(&bytes))
        );
    }
    fs::remove_dir_all(home).unwrap();
}

#[test]
fn local_and_cached_index_bounds_reject_a_valid_prefix_with_excess_data() {
    let home = scratch("index-bounds");
    let path = home.join("index.json");
    let mut bytes = br#"{"schema":1,"projects":[]}"#.to_vec();
    bytes.resize(MAX_INDEX as usize + 1, b' ');
    fs::write(&path, &bytes).unwrap();
    let cancel = Arc::new(Cancellation::default());
    assert!(fetch_text(path.to_str().unwrap(), &cancel).is_err());
    fs::create_dir_all(home.join("mod-browser")).unwrap();
    fs::write(home.join(CACHE), bytes).unwrap();
    assert!(load_index(&home, home.join("missing.json").to_str().unwrap()).is_err());
    assert!(read_bounded(&b"1234"[..], 3, &cancel).is_err());
    assert_eq!(read_bounded(&b"123"[..], 3, &cancel).unwrap(), b"123");
    fs::remove_dir_all(home).unwrap();
}

#[test]
fn managed_index_shutdown_closes_a_stalled_tls_handshake() {
    use std::{io::Read, net::TcpListener, sync::mpsc};
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("https://{}/index.json", listener.local_addr().unwrap());
    let (accepted, ready) = mpsc::channel();
    let peer = std::thread::spawn(move || {
        let (mut socket, _) = listener.accept().unwrap();
        socket
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let mut bytes = [0; 4096];
        assert!(socket.read(&mut bytes).unwrap() > 0);
        accepted.send(()).unwrap();
        // Deliberately never reply to ClientHello.
        loop {
            match socket.read(&mut bytes) {
                Ok(0) | Err(_) => break,
                Ok(_) => {}
            }
        }
    });
    let cancel = Arc::new(Cancellation::default());
    let worker_cancel = cancel.clone();
    let home = scratch("stalled-tls");
    let worker_home = home.clone();
    let (done, result) = mpsc::channel();
    let worker = std::thread::spawn(move || {
        done.send(load_index_managed(&worker_home, &url, &worker_cancel))
            .unwrap();
    });
    ready.recv_timeout(Duration::from_secs(5)).unwrap();
    cancel.cancel();
    assert!(
        result
            .recv_timeout(Duration::from_secs(3))
            .unwrap()
            .is_err()
    );
    worker.join().unwrap();
    peer.join().unwrap();
    assert_eq!(cancel.attached_sockets(), 0);
    assert!(!home.join(CACHE).exists());
    fs::remove_dir_all(home).unwrap();
}

#[test]
fn cancelling_after_download_preserves_the_installed_pair() {
    let home = scratch("cancel-before-commit");
    let old = ruby("1");
    install(
        &registry("1", &old, "file:///ruby.hmod"),
        &home,
        "ruby",
        &serve(old.clone()),
    )
    .unwrap();
    let manifest = fs::read(home.join(MANIFEST)).unwrap();
    let new = ruby("2");
    let index = registry("2", &new, "file:///ruby.hmod");
    let cancel = Cancellation::default();
    assert!(
        install_managed(
            &index,
            &home,
            "ruby",
            &|_, _| {
                cancel.cancel();
                Ok(new.clone())
            },
            &cancel
        )
        .is_err()
    );
    assert_eq!(
        fs::read(home.join(crate::mod_list::DIR).join("ruby.hmod")).unwrap(),
        old
    );
    assert_eq!(fs::read(home.join(MANIFEST)).unwrap(), manifest);
    fs::remove_dir_all(home).unwrap();
}

#[test]
fn waiting_for_another_home_commit_can_be_cancelled() {
    use std::sync::mpsc;
    let home = scratch("cancel-lock-wait");
    let held = storage::lock_home(&home, &Cancellation::default()).unwrap();
    let cancel = Arc::new(Cancellation::default());
    let worker_cancel = cancel.clone();
    let worker_home = home.clone();
    let (done, result) = mpsc::channel();
    let worker = std::thread::spawn(move || {
        let result = storage::lock_home(&worker_home, &worker_cancel);
        done.send(result.is_err()).unwrap();
    });
    cancel.cancel();
    assert!(result.recv_timeout(Duration::from_secs(2)).unwrap());
    worker.join().unwrap();
    drop(held);
    fs::remove_dir_all(home).unwrap();
}

#[test]
fn a_cancelled_snapshot_does_not_wait_for_another_publisher_or_read_files() {
    use std::sync::{
        atomic::{AtomicBool, Ordering},
        mpsc,
    };
    let home = scratch("cancel-snapshot-wait");
    let held = storage::lock_home(&home, &Cancellation::default()).unwrap();
    let cancel = Arc::new(Cancellation::default());
    let worker_cancel = cancel.clone();
    let read = Arc::new(AtomicBool::new(false));
    let worker_read = read.clone();
    let worker_home = home.clone();
    let (started, entered) = mpsc::channel();
    let (done, result) = mpsc::channel();
    let worker = std::thread::spawn(move || {
        started.send(()).unwrap();
        let snapshot = with_content_snapshot_managed(&worker_home, &worker_cancel, || {
            worker_read.store(true, Ordering::SeqCst);
        });
        done.send(snapshot).unwrap();
    });
    entered.recv_timeout(Duration::from_secs(2)).unwrap();
    assert!(matches!(
        result.recv_timeout(Duration::from_millis(50)),
        Err(mpsc::RecvTimeoutError::Timeout)
    ));
    cancel.cancel();
    let error = result
        .recv_timeout(Duration::from_secs(2))
        .unwrap()
        .unwrap_err();
    assert!(error.contains("cancelled"), "{error}");
    assert!(
        !read.load(Ordering::SeqCst),
        "cancelled snapshot entered file-reading callback"
    );
    // The publisher is still locked: completion cannot depend on releasing it.
    worker.join().unwrap();
    drop(held);
    fs::remove_dir_all(home).unwrap();
}
