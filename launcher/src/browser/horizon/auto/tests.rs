//! Automatic updates against fixture indexes: a content mod `ruby` whose versions are real
//! `.hmod` files on disk, listed in an index the way the registry lists them.
use super::*;
use crate::browser::horizon::tests::{sample, scratch};

const MANIFEST: &str = "id = \"ruby\"\nname = \"Ruby\"\napi = \"0.2\"\nside = \"both\"\n\
                        capabilities = [\"registries\"]\n";
const BLOCK: &[u8] = br#"{"template": "minecraft:iron_block"}"#;
const ITEM: &[u8] = br#"{"template": "minecraft:diamond"}"#;

/// `ruby` at `version` with blocks `blocks` and items `items`, written into `dir`.
fn ruby(dir: &Path, version: &str, blocks: &[&str], items: &[&str]) -> PathBuf {
    let names: Vec<(String, &[u8])> = blocks
        .iter()
        .map(|b| (format!("data/ruby/horizon/block/{b}.json"), BLOCK))
        .chain(
            items
                .iter()
                .map(|i| (format!("data/ruby/horizon/item/{i}.json"), ITEM)),
        )
        .collect();
    let files: Vec<(&str, &[u8])> = names.iter().map(|(n, b)| (n.as_str(), *b)).collect();
    let manifest = format!("{MANIFEST}version = \"{version}\"\n");
    let bytes = horizon_mods::Package::write_files(&manifest, None, &files).unwrap();
    let path = dir.join(format!("ruby-{version}.hmod"));
    fs::write(&path, bytes).unwrap();
    path
}

/// An index listing `ruby`'s `versions` (newest first: version, game, file on disk).
fn index_of(versions: &[(&str, &str, &Path)]) -> Index {
    let mut index = sample();
    let base = index.project("coords_hud").unwrap().clone();
    let listed = versions
        .iter()
        .map(|(version, game, path)| index::Version {
            version: (*version).into(),
            api: "0.2".into(),
            side: Some(horizon_mods::Side::Both),
            capabilities: vec![horizon_mods::Capability::Registries],
            api_level: None,
            game: (*game).into(),
            broken_on: Vec::new(),
            requires: Vec::new(),
            file: index::File {
                name: path.file_name().unwrap().to_string_lossy().into_owned(),
                url: format!("file://{}", path.display()),
                size: fs::metadata(path).unwrap().len(),
                sha256: index::sha256_file(path).unwrap(),
                signature: None,
            },
            ..base.versions[0].clone()
        })
        .collect();
    index.projects.push(index::Project {
        id: "ruby".into(),
        name: "Ruby".into(),
        key: None,
        versions: listed,
        ..base
    });
    index
}

const PLAY: Context = Context {
    in_pack: false,
    level: 100,
    natives: true,
};

/// The browser's entry for `ruby` at `version`.
fn entry(version: &str) -> installed::Entry {
    installed::Entry {
        source: SourceId::Horizon,
        project: "ruby".into(),
        slug: "ruby".into(),
        title: "Ruby".into(),
        kind: Kind::Mod,
        version: version.into(),
        number: version.into(),
        file: "horizon-mods/ruby.hmod".into(),
        checksum: String::new(),
        page: String::new(),
        dependency: false,
        verdict: None,
        skipped: Vec::new(),
        namespaces: Vec::new(),
    }
}

fn version_of(d: Decision) -> Option<String> {
    match d {
        Decision::Update(v) => Some(v.version.clone()),
        Decision::Skip => None,
    }
}

#[test]
fn updates_forward_on_the_same_game_only() {
    let dir = scratch("auto-decide");
    let (a, b, c) = (
        ruby(&dir, "1.0.0", &["a"], &[]),
        ruby(&dir, "1.1.0", &["a"], &[]),
        ruby(&dir, "1.2.0", &["a"], &[]),
    );
    let index = index_of(&[
        ("1.2.0", GAME, &c),
        ("1.1.0", GAME, &b),
        ("1.0.0", GAME, &a),
    ]);
    let to = |e: &installed::Entry, ctx: &Context| version_of(decide(&index, e, ctx));
    assert_eq!(to(&entry("1.0.0"), &PLAY), Some("1.2.0".into()));
    assert_eq!(to(&entry("1.2.0"), &PLAY), None, "already the newest");
    assert_eq!(
        to(&entry("0.9.0"), &PLAY),
        None,
        "a version the index no longer lists"
    );
    let in_pack = Context {
        in_pack: true,
        ..PLAY
    };
    assert_eq!(to(&entry("1.0.0"), &in_pack), None, "a modpack's profile");
    let native = installed::Entry {
        file: "horizon-native-mods/ruby.hnmod".into(),
        ..entry("1.0.0")
    };
    let pinned = Context {
        natives: false,
        ..PLAY
    };
    assert_eq!(
        to(&native, &PLAY),
        Some("1.2.0".into()),
        "then built again before Play"
    );
    assert_eq!(
        to(&native, &pinned),
        None,
        "pinned to a Nightly, or before Play"
    );
    assert_eq!(
        to(&entry("1.0.0"), &pinned),
        Some("1.2.0".into()),
        "content mods still do"
    );
    let data = installed::Entry {
        kind: Kind::DataPack,
        ..entry("1.0.0")
    };
    assert_eq!(to(&data, &PLAY), None);
    let modrinth = installed::Entry {
        source: SourceId::Modrinth,
        ..entry("1.0.0")
    };
    assert_eq!(to(&modrinth, &PLAY), None);

    // The newest needs a newer Horizon: the one before it.
    let mut newer = index.clone();
    let ruby_at = newer.projects.len() - 1;
    newer.projects[ruby_at].versions[0].api_level = Some(7);
    let low = Context { level: 6, ..PLAY };
    assert_eq!(
        version_of(decide(&newer, &entry("1.0.0"), &low)),
        Some("1.1.0".into())
    );
    // Never backwards: the newest was taken down while it is installed.
    let mut down = index.clone();
    down.revoked.push(index::Revoked {
        id: "ruby".into(),
        sha256: Some(index::sha256_file(&c).unwrap()),
        reason: "Test.".into(),
        date: String::new(),
    });
    assert_eq!(version_of(decide(&down, &entry("1.2.0"), &PLAY)), None);
    // Never across Minecraft versions: what is installed was for another game.
    let other = index_of(&[("1.2.0", GAME, &c), ("1.0.0", "26.2", &a)]);
    assert_eq!(version_of(decide(&other, &entry("1.0.0"), &PLAY)), None);
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_new_version_that_drops_content_is_named() {
    let dir = scratch("auto-dropped");
    let old = ruby(&dir, "1.0.0", &["lamp", "ore"], &["gem"]);
    let more = ruby(&dir, "1.1.0", &["lamp", "ore", "brick"], &["gem"]);
    let less = ruby(&dir, "1.2.0", &["lamp"], &[]);
    assert!(dropped(&old, &more).unwrap().is_empty());
    assert_eq!(
        dropped(&old, &less).unwrap(),
        ["block ruby:ore", "item ruby:gem"]
    );
    fs::remove_dir_all(&dir).unwrap();
}

/// The installed ruby: the browser's entry and the file's bytes.
fn installed_ruby(home: &Path) -> (String, Vec<u8>) {
    let list = installed::Installed::load(home);
    let e = list.find(SourceId::Horizon, "ruby").unwrap();
    (e.version.clone(), fs::read(home.join(&e.file)).unwrap())
}

#[test]
fn updates_holds_and_keeps_the_replaced_file_until_the_next_start() {
    let dir = scratch("auto-run");
    let home = dir.join("home");
    let v1 = ruby(&dir, "1.0.0", &["lamp", "ore"], &[]);
    let v2 = ruby(&dir, "1.1.0", &["lamp", "ore", "brick"], &[]);
    let v3 = ruby(&dir, "1.2.0", &["lamp"], &[]);
    let first = index_of(&[("1.0.0", GAME, &v1)]);
    let p = first.project("ruby").unwrap();
    install_version_guarded(
        &first,
        true,
        p,
        &p.versions[0],
        &home,
        &|_| Ok(()),
        &|_, _| {},
    )
    .unwrap();

    // 1.1.0 adds a block: updated, the 1.0.0 file kept aside.
    let index = index_of(&[("1.1.0", GAME, &v2), ("1.0.0", GAME, &v1)]);
    let report = run_with(&index, true, &home, &PLAY, &|_, _| {});
    assert_eq!(report.updated, ["Ruby 1.1.0"]);
    assert!(
        report.held.is_empty() && report.failed.is_empty(),
        "{report:?}"
    );
    assert_eq!(
        installed_ruby(&home),
        ("1.1.0".into(), fs::read(&v2).unwrap())
    );
    let kept = kept_list(&home);
    assert_eq!(kept.len(), 1);
    assert_eq!(
        fs::read(home.join(&kept[0].copy)).unwrap(),
        fs::read(&v1).unwrap()
    );
    // A second run in the same launcher keeps it; the next start drops it, the update done.
    run_with(&index, true, &home, &PLAY, &|_, _| {});
    assert!(home.join(&kept[0].copy).is_file());
    save_kept(
        &home,
        &[Kept {
            pid: 0,
            ..kept[0].clone()
        }],
    )
    .unwrap();
    recover(&home);
    assert!(!home.join(&kept[0].copy).exists());
    assert!(kept_list(&home).is_empty());

    // 1.2.0 drops two blocks: it waits, and 1.1.0 stays as it was.
    let index = index_of(&[
        ("1.2.0", GAME, &v3),
        ("1.1.0", GAME, &v2),
        ("1.0.0", GAME, &v1),
    ]);
    let report = run_with(&index, true, &home, &PLAY, &|_, _| {});
    assert!(report.updated.is_empty(), "{report:?}");
    assert_eq!(report.held.len(), 1);
    assert!(
        report.held[0].contains("block ruby:brick, block ruby:ore"),
        "{}",
        report.held[0]
    );
    assert_eq!(
        installed_ruby(&home),
        ("1.1.0".into(), fs::read(&v2).unwrap())
    );
    assert!(kept_list(&home).is_empty());
    assert!(!home.join("mod-browser/downloads/ruby-1.2.0.hmod").exists());
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn an_update_cut_short_is_rolled_back_at_the_next_start() {
    let dir = scratch("auto-rollback");
    let home = dir.join("home");
    let v1 = ruby(&dir, "1.0.0", &["lamp"], &[]);
    let index = index_of(&[("1.0.0", GAME, &v1)]);
    let p = index.project("ruby").unwrap();
    install_version_guarded(
        &index,
        true,
        p,
        &p.versions[0],
        &home,
        &|_| Ok(()),
        &|_, _| {},
    )
    .unwrap();
    let before = installed::Installed::load(&home);
    let old = before.find(SourceId::Horizon, "ruby").unwrap().clone();
    // An earlier launcher kept 1.0.0, then stopped with a half-written file and the list still
    // naming 1.0.0.
    let mut k = keep(&home, &old).unwrap();
    k.pid = 0;
    save_kept(&home, &[k]).unwrap();
    fs::write(home.join(&old.file), b"half").unwrap();
    recover(&home);
    assert_eq!(
        installed_ruby(&home),
        ("1.0.0".into(), fs::read(&v1).unwrap())
    );
    assert!(kept_list(&home).is_empty());
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn the_report_says_what_changed_and_nothing_otherwise() {
    assert!(Report::default().lines().is_empty());
    let r = Report {
        updated: vec!["Ruby 1.1.0".into(), "Glow 2.0".into()],
        held: vec!["held".into()],
        ..Report::default()
    };
    assert_eq!(
        r.lines(),
        ["Updated automatically: Ruby 1.1.0, Glow 2.0.", "held"]
    );
}

/// Turning a mod off in the Mods page is the player's choice, and an automatic update replaces
/// the file without undoing it (#10071). A new install and the player's own install turn it on.
#[test]
fn an_automatic_update_keeps_a_disabled_mod_off() {
    use crate::browser::horizon::install_version;
    use crate::browser::java;
    let dir = scratch("auto-disabled");
    let home = dir.join("home");
    let v1 = ruby(&dir, "1.0.0", &["lamp"], &[]);
    let v2 = ruby(&dir, "1.1.0", &["lamp", "ore"], &[]);
    let v3 = ruby(&dir, "1.2.0", &["lamp", "ore", "brick"], &[]);
    let first = index_of(&[("1.0.0", GAME, &v1)]);
    let p = first.project("ruby").unwrap();
    install_version(&first, true, p, &p.versions[0], &home, &|_, _| {}).unwrap();
    assert!(java::is_enabled(&home, "ruby"), "a new install is on");

    // The player turns it off; the automatic update replaces its file and leaves it off.
    java::set_enabled(&home, "ruby", false).unwrap();
    let index = index_of(&[("1.1.0", GAME, &v2), ("1.0.0", GAME, &v1)]);
    let report = run_with(&index, true, &home, &PLAY, &|_, _| {});
    assert_eq!(report.updated, ["Ruby 1.1.0"], "{report:?}");
    assert!(!java::is_enabled(&home, "ruby"), "the update kept it off");
    assert_eq!(
        installed_ruby(&home),
        ("1.1.0".into(), fs::read(&v2).unwrap())
    );

    // Turned back on, the next automatic update leaves it on.
    java::set_enabled(&home, "ruby", true).unwrap();
    let index = index_of(&[("1.2.0", GAME, &v3), ("1.1.0", GAME, &v2)]);
    let report = run_with(&index, true, &home, &PLAY, &|_, _| {});
    assert_eq!(report.updated, ["Ruby 1.2.0"], "{report:?}");
    assert!(java::is_enabled(&home, "ruby"));

    // The player's own install of the installed, disabled mod turns it on again.
    java::set_enabled(&home, "ruby", false).unwrap();
    let v4 = ruby(&dir, "1.3.0", &["lamp"], &[]);
    let index = index_of(&[("1.3.0", GAME, &v4), ("1.2.0", GAME, &v3)]);
    let p = index.project("ruby").unwrap();
    install_version(&index, true, p, &p.versions[0], &home, &|_, _| {}).unwrap();
    assert!(java::is_enabled(&home, "ruby"), "an explicit install is on");
    assert_eq!(
        installed_ruby(&home),
        ("1.3.0".into(), fs::read(&v4).unwrap())
    );
}

#[test]
fn auto_update_and_manual_mutation_do_not_race_or_lose_entries() {
    use std::sync::mpsc::channel;
    use std::sync::Arc;
    use std::time::Duration;

    let dir = scratch("auto-race-install");
    let home = dir.join("home");
    let v1 = ruby(&dir, "1.0.0", &["block1"], &[]);
    let v2 = ruby(&dir, "1.1.0", &["block1"], &[]);
    let index = Arc::new(index_of(&[("1.1.0", GAME, &v2), ("1.0.0", GAME, &v1)]));

    // Start with Ruby 1.0.0 installed.
    let p_ruby = index.project("ruby").unwrap();
    install_version_guarded(
        &index,
        true,
        p_ruby,
        &p_ruby.versions[1], // 1.0.0
        &home,
        &|_| Ok(()),
        &|_, _| {},
    )
    .unwrap();

    let initial = installed::Installed::load(&home);
    assert_eq!(initial.entries.len(), 1);
    assert_eq!(initial.entries[0].version, "1.0.0");

    let (auto_started_tx, auto_started_rx) = channel();
    let (auto_resume_tx, auto_resume_rx) = channel();

    let home_auto = home.clone();
    let index_auto = index.clone();
    let auto_thread = std::thread::spawn(move || {
        let resume = std::sync::Mutex::new(auto_resume_rx);
        run_with(&index_auto, true, &home_auto, &PLAY, &|_line, _fraction| {
            let _ = auto_started_tx.send(());
            if let Ok(rx) = resume.lock() {
                let _ = rx.recv_timeout(Duration::from_secs(5));
            }
        })
    });

    auto_started_rx
        .recv_timeout(Duration::from_secs(5))
        .expect("auto update started");

    let home_manual = home.clone();
    let index_manual = index.clone();
    let manual_thread = std::thread::spawn(move || {
        let p_coords = index_manual.project("coords_hud").unwrap();
        install_version_guarded(
            &index_manual,
            true,
            p_coords,
            &p_coords.versions[0],
            &home_manual,
            &|_| Ok(()),
            &|_, _| {},
        )
    });

    let _ = auto_resume_tx.send(());

    let report = auto_thread.join().expect("auto thread finished");
    assert_eq!(report.updated, vec!["Ruby 1.1.0"]);

    let manual_result = manual_thread.join().expect("manual thread finished");
    assert!(manual_result.is_ok(), "{:?}", manual_result.err());

    let final_installed = installed::Installed::load(&home);
    assert_eq!(
        final_installed.entries.len(),
        2,
        "entries clobbered: {:?}",
        final_installed.entries
    );
    let ruby_entry = final_installed
        .find(SourceId::Horizon, "ruby")
        .expect("ruby present");
    assert_eq!(ruby_entry.version, "1.1.0");
    let coords_entry = final_installed
        .find(SourceId::Horizon, "coords_hud")
        .expect("coords_hud present");
    assert_eq!(coords_entry.version, "0.1.0");

    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn auto_update_does_not_revive_concurrently_removed_mod() {
    use std::sync::mpsc::channel;
    use std::sync::Arc;
    use std::time::Duration;

    let dir = scratch("auto-race-remove");
    let home = dir.join("home");
    let v1 = ruby(&dir, "1.0.0", &["block1"], &[]);
    let v2 = ruby(&dir, "1.1.0", &["block1"], &[]);
    let index = Arc::new(index_of(&[("1.1.0", GAME, &v2), ("1.0.0", GAME, &v1)]));

    let p_ruby = index.project("ruby").unwrap();
    install_version_guarded(
        &index,
        true,
        p_ruby,
        &p_ruby.versions[1],
        &home,
        &|_| Ok(()),
        &|_, _| {},
    )
    .unwrap();
    let p_coords = index.project("coords_hud").unwrap();
    install_version_guarded(
        &index,
        true,
        p_coords,
        &p_coords.versions[0],
        &home,
        &|_| Ok(()),
        &|_, _| {},
    )
    .unwrap();

    assert_eq!(installed::Installed::load(&home).entries.len(), 2);

    let (auto_started_tx, auto_started_rx) = channel();
    let (auto_resume_tx, auto_resume_rx) = channel();

    let home_auto = home.clone();
    let index_auto = index.clone();
    let auto_thread = std::thread::spawn(move || {
        let resume = std::sync::Mutex::new(auto_resume_rx);
        run_with(&index_auto, true, &home_auto, &PLAY, &|_line, _fraction| {
            let _ = auto_started_tx.send(());
            if let Ok(rx) = resume.lock() {
                let _ = rx.recv_timeout(Duration::from_secs(5));
            }
        })
    });

    auto_started_rx
        .recv_timeout(Duration::from_secs(5))
        .expect("auto update started");

    let home_manual = home.clone();
    let remove_thread = std::thread::spawn(move || installed::remove(&home_manual, "coords_hud"));

    let _ = auto_resume_tx.send(());

    let _report = auto_thread.join().expect("auto thread finished");
    let remove_result = remove_thread.join().expect("remove thread finished");
    assert!(remove_result.is_ok(), "{:?}", remove_result.err());

    let final_installed = installed::Installed::load(&home);
    assert_eq!(final_installed.entries.len(), 1);
    assert!(final_installed
        .find(SourceId::Horizon, "coords_hud")
        .is_none());
    let ruby_entry = final_installed
        .find(SourceId::Horizon, "ruby")
        .expect("ruby present");
    assert_eq!(ruby_entry.version, "1.1.0");

    fs::remove_dir_all(&dir).unwrap();
}
