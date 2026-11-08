//! The registry source against the sample index of `horizon_mods::index`
//! (`crates/horizon-mods/tests/index/`), its files served from disk.
use super::*;

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../crates/horizon-mods/tests/index")
}

/// The sample index with its files on disk, as a local test registry lists them.
pub(super) fn sample() -> Index {
    let text = fs::read_to_string(fixtures().join("index-v1.json")).unwrap();
    let mut index = Index::parse(&text).unwrap();
    for p in &mut index.projects {
        for v in &mut p.versions {
            let path = fixtures().join(&v.file.name);
            v.file.url = format!("file://{}", path.canonicalize().unwrap_or(path).display());
        }
    }
    index
}

pub(super) fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "horizon-launcher-registry-{name}-{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn hud(index: &Index) -> Project {
    project_of(index.project("coords_hud").unwrap()).unwrap()
}

/// Installs the sample's WebAssembly mod as a modpack does: the browser won't, it needs a native
/// port.
fn put(index: &Index, local: bool, home: &Path) -> Result<Vec<installed::Entry>, String> {
    let p = index.project("coords_hud").unwrap();
    install_version(index, local, p, &p.versions[0], home, &|_, _| {}).map(|e| vec![e])
}

#[test]
fn native_registry_entries_are_mods_that_say_they_run_native_code() {
    assert_eq!(kind_of(&index::Kind::NativeMod), Some(Kind::Mod));
    let mut index = sample();
    index.projects[0].kind = index::Kind::NativeMod;
    index.projects[0].versions[0].file.name = "coords_hud-0.1.0.hnmod".into();
    let found = search(&index, &Query::new("", Kind::Mod));
    assert!(found.iter().any(|p| p.id == index.projects[0].id));
    let entry = |file: &str| {
        serde_json::from_value::<installed::Entry>(serde_json::json!({
            "source": "horizon", "project": "p", "slug": "p", "title": "P", "kind": "mod",
            "version": "1", "number": "1", "file": file, "checksum": "", "page": ""
        }))
        .unwrap()
    };
    assert_eq!(
        mod_id(&entry("horizon-native-mods/veinminer.hnmod")),
        Some("veinminer")
    );
    assert_eq!(mod_id(&entry("horizon-mods/zoom.hmod")), Some("zoom"));
}

#[test]
fn only_horizons_own_projects_go_without_the_not_endorsed_line() {
    let mut p = sample().projects[0].clone();
    p.authors = vec!["Horizon".into()];
    p.source = "https://github.com/horizon-rust/horizon-mod-zoom".into();
    assert!(official(&p));
    p.source = "https://github.com/someone/horizon-mod-zoom".into();
    assert!(!official(&p), "the name alone doesn't make it official");
    p.authors = vec!["levzzz5154".into()];
    assert!(!official(&p));
}

#[test]
fn only_horizons_own_projects_say_they_come_from_horizon() {
    let mut index = sample();
    let id = index.projects[0].id.clone();
    index.projects[0].authors = vec!["levzzz5154".into()];
    index.projects[0].source = "https://github.com/levzzz5154/horizon-mod-zoom".into();
    assert!(!official_in(Some(&index), &id), "a contributor's project");
    index.projects[0].authors = vec!["Horizon".into()];
    index.projects[0].source = "https://github.com/horizon-rust/horizon-mod-zoom".into();
    assert!(official_in(Some(&index), &id));
    assert!(!official_in(Some(&index), "not_listed"));
    assert!(!official_in(None, &id), "no index read yet");
}

#[test]
fn searches_by_kind_and_words() {
    let index = sample();
    let found = |text: &str, kind| -> Vec<String> {
        search(&index, &Query::new(text, kind))
            .into_iter()
            .map(|p| p.id)
            .collect()
    };
    // The sample's only mod is a WebAssembly one: it stays in the index, not in the browser.
    assert!(found("", Kind::Mod).is_empty());
    assert!(found("coordinates HUD", Kind::Mod).is_empty());
    assert!(found("zoom", Kind::Mod).is_empty());
    // Modpacks under Modpacks; a kind this launcher can't install yet (a world) never.
    assert_eq!(found("", Kind::Modpack), ["cozy"]);
    assert!(found("skyblock", Kind::ResourcePack).is_empty());
    assert_eq!(found("", Kind::Shader), ["glow"]);
    // The index's download counts, most downloaded first (of native mods: the browser lists them).
    let mut counted = index.clone();
    counted.projects[0].kind = index::Kind::NativeMod;
    counted.projects[0].versions[0].file.name = "coords_hud-0.1.0.hnmod".into();
    counted.projects.push(index::Project {
        id: "zoom".into(),
        name: "Zoom".into(),
        downloads: 12_345,
        ..counted.projects[0].clone()
    });
    counted.projects[0].downloads = 80;
    let first = search(&counted, &Query::new("", Kind::Mod));
    let order: Vec<(&str, u64)> = first.iter().map(|p| (p.id.as_str(), p.downloads)).collect();
    assert_eq!(order, [("zoom", 12_345), ("coords_hud", 80)]);
    // Newest orders by the day a project's first version came out, Updated by its latest.
    let mut dated = counted.clone();
    let base = dated.projects[0].versions[0].clone();
    let version_on = |day: &str| index::Version {
        published: day.into(),
        ..base.clone()
    };
    dated.projects[0].versions = vec![version_on("2026-09-01"), version_on("2026-01-01")];
    let zoom = dated.projects.iter().position(|p| p.id == "zoom").unwrap();
    dated.projects[zoom].versions = vec![version_on("2026-05-01")];
    let sorted = |sort| -> Vec<String> {
        let mut q = Query::new("", Kind::Mod);
        q.filters.sort = Some(sort);
        search(&dated, &q).into_iter().map(|p| p.id).collect()
    };
    assert_eq!(
        sorted(super::super::details::Sort::Newest),
        ["zoom", "coords_hud"]
    );
    assert_eq!(
        sorted(super::super::details::Sort::Updated),
        ["coords_hud", "zoom"]
    );
    assert_eq!(
        sorted(super::super::details::Sort::Downloads),
        ["zoom", "coords_hud"]
    );
    let p = hud(&index);
    assert_eq!(p.support(), super::super::Support::Works);
    assert_eq!(p.page, "https://playhorizon.dev/mods/coords_hud");
}

#[test]
fn searches_by_category_and_tag() {
    let mut index = sample();
    let glow = index.projects.iter().position(|p| p.id == "glow").unwrap();
    index.projects[glow].categories = vec!["fantasy".into(), "bloom".into()];
    index.projects[glow].tags = vec!["sunset".into()];
    let found = |text: &str, categories: &[&str]| -> Vec<String> {
        let mut q = Query::new(text, Kind::Shader);
        q.filters.categories = categories.iter().map(|c| c.to_string()).collect();
        search(&index, &q).into_iter().map(|p| p.id).collect()
    };
    assert_eq!(found("", &["bloom"]), ["glow"]);
    assert_eq!(found("", &["bloom", "fantasy"]), ["glow"]);
    assert!(
        found("", &["bloom", "pbr"]).is_empty(),
        "every category asked"
    );
    assert_eq!(found("sunset", &[]), ["glow"], "tags are searched");
    assert!(found("dawn", &[]).is_empty());
    let tagged = |tags: &[&str]| -> Vec<String> {
        let mut q = Query::new("", Kind::Shader);
        q.filters.tags = tags.iter().map(|t| t.to_string()).collect();
        search(&index, &q).into_iter().map(|p| p.id).collect()
    };
    assert_eq!(tagged(&["sunset"]), ["glow"]);
    assert!(tagged(&["sunset", "dawn"]).is_empty(), "every tag asked");
    // The filter offers the kind's own list: a resolution for resource packs, not for shaders.
    let shader = Registry.tags(Kind::Shader).unwrap().categories;
    assert!(shader.iter().any(|c| c.name == "path-tracing"));
    let packs = Registry.tags(Kind::ResourcePack).unwrap().categories;
    assert!(packs.iter().any(|c| c.name == "16x"));
    assert!(!shader.iter().any(|c| c.name == "16x"));
}

#[test]
fn the_tag_filter_offers_the_kinds_tags_most_used_first() {
    let mut index = sample();
    let at = |index: &Index, id: &str| index.projects.iter().position(|p| p.id == id).unwrap();
    let glow = at(&index, "glow");
    index.projects[glow].tags = vec![
        "sunset".into(),
        "cozy".into(),
        "sunset".into(),
        "Not Plain".into(),
    ];
    assert_eq!(
        tags_of(&index, Kind::Shader),
        ["cozy", "sunset"],
        "ties by name"
    );
    let mut dusk = index.projects[glow].clone();
    dusk.id = "dusk".into();
    dusk.tags = vec!["sunset".into()];
    index.projects.push(dusk);
    let hud = at(&index, "coords_hud");
    index.projects[hud].tags = vec!["pvp".into()];
    assert_eq!(tags_of(&index, Kind::Shader), ["sunset", "cozy"]);
    assert_eq!(tags_of(&index, Kind::Mod), ["pvp"], "each kind its own");
    assert!(tags_of(&index, Kind::DataPack).is_empty());
}

/// A data pack zip with one function, and the registry project that lists it.
fn data_pack(index: &mut Index, dir: &Path) -> Project {
    use std::io::Write;
    let path = dir.join("lanterns-1.0.0.zip");
    let mut zip = zip::ZipWriter::new(fs::File::create(&path).unwrap());
    for (name, text) in [
        (
            "pack.mcmeta",
            r#"{"pack": {"description": "Lanterns", "pack_format": 94}}"#,
        ),
        ("data/lanterns/function/light.mcfunction", "say lit"),
    ] {
        zip.start_file(name, zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(text.as_bytes()).unwrap();
    }
    zip.finish().unwrap();
    let base = index.project("glow").unwrap().clone();
    let mut version = base.versions[0].clone();
    version.version = "1.0.0".into();
    version.api = "94".into();
    version.side = None;
    version.capabilities.clear();
    version.api_level = None;
    version.requires.clear();
    version.file = index::File {
        name: "lanterns-1.0.0.zip".into(),
        url: format!("file://{}", path.display()),
        size: fs::metadata(&path).unwrap().len(),
        sha256: index::sha256_file(&path).unwrap(),
        signature: None,
    };
    index.projects.push(index::Project {
        id: "lanterns".into(),
        kind: index::Kind::DataPack,
        name: "Lanterns".into(),
        key: None,
        versions: vec![version],
        ..base
    });
    project_of(index.project("lanterns").unwrap()).unwrap()
}

#[test]
fn a_registry_data_pack_is_checked_then_goes_into_a_world() {
    let dir = scratch("datapack");
    let mut index = sample();
    let p = data_pack(&mut index, &dir);
    assert_eq!(p.kind, Kind::DataPack);
    assert_eq!(p.support(), super::super::Support::Works);
    let found: Vec<String> = search(&index, &Query::new("", Kind::DataPack))
        .into_iter()
        .map(|p| p.id)
        .collect();
    assert_eq!(found, ["lanterns"]);
    // Not into the game folder: into a world.
    assert!(install_from(&index, true, None, &p, &dir.join("home"), &|_, _| {}).is_err());
    let (zip, number) = fetch_datapack_from(&index, true, &p, &dir.join("dl"), &|_| {}).unwrap();
    assert_eq!(number, "1.0.0");
    let world = dir.join("world");
    let pack = crate::datapacks::add(&world, &zip).unwrap();
    assert!(world.join("datapacks").join(&pack.file).is_file());

    // A file other than the one the index lists never gets there.
    let mut changed = index.clone();
    let at = changed.projects.len() - 1;
    changed.projects[at].versions[0].file.sha256 = "0".repeat(64);
    let e = fetch_datapack_from(&changed, true, &p, &dir.join("dl2"), &|_| {}).unwrap_err();
    assert!(e.contains("checksum") || e.contains("SHA-256"), "{e}");
    // Nor one taken down.
    index.revoked.push(index::Revoked {
        id: "lanterns".into(),
        sha256: None,
        reason: "Test.".into(),
        date: String::new(),
    });
    let e = fetch_datapack_from(&index, true, &p, &dir.join("dl3"), &|_| {}).unwrap_err();
    assert_eq!(e, "Lanterns was taken down: Test.");
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn screenshots_are_the_gallery_from_the_registry_only() {
    let mut p = sample().projects[0].clone();
    assert!(gallery_of(&p).is_empty());
    let pages = "https://horizon-rust.github.io/horizon-mods/screenshots/coords_hud";
    p.screenshots = vec![
        format!("{pages}/b.png"),
        "https://example.com/tracker.png".into(),
        format!("{pages}/a.jpg"),
    ];
    let urls: Vec<String> = gallery_of(&p).into_iter().map(|i| i.url).collect();
    assert_eq!(urls, [format!("{pages}/b.png"), format!("{pages}/a.jpg")]);
}

#[test]
fn installs_checked_turns_on_and_removes() {
    let index = sample();
    let home = scratch("install");
    let p = hud(&index);
    let e = install_from(&index, true, None, &p, &home, &|_, _| {}).unwrap_err();
    assert_eq!(e, format!("Coordinates HUD {NEEDS_PORT}."));
    let done = put(&index, true, &home).unwrap();
    assert_eq!(done[0].file, "horizon-mods/coords_hud.hmod");
    assert_eq!(done[0].skipped, [format!("its code ({NEEDS_PORT})")]);
    assert_eq!(done[0].verdict.as_deref(), Some("Verified"));
    assert!(done[0].checksum.starts_with("SHA-256:e1135345"));
    assert!(home.join("horizon-mods/coords_hud.hmod").is_file());
    let list = fs::read_to_string(home.join("horizon-mods.txt")).unwrap();
    assert_eq!(list, "mod.coords_hud:true\n");
    assert_eq!(
        fs::read_dir(home.join("mod-browser/downloads"))
            .unwrap()
            .count(),
        0
    );
    // Installed already: nothing to do.
    let again = put(&index, true, &home).unwrap();
    assert_eq!(again, done);

    let mut revoked = index.clone();
    revoked.revoked.push(index::Revoked {
        id: "coords_hud".into(),
        sha256: None,
        reason: "Test.".into(),
        date: String::new(),
    });
    let said = turn_off_revoked(&revoked, &home);
    assert_eq!(said, ["Coordinates HUD was taken down and is off: Test."]);
    assert_eq!(
        fs::read_to_string(home.join("horizon-mods.txt")).unwrap(),
        ""
    );

    installed::remove(&home, "coords_hud").unwrap();
    assert!(!home.join("horizon-mods/coords_hud.hmod").exists());
    assert!(installed::Installed::load(&home).entries.is_empty());
    fs::remove_dir_all(&home).unwrap();
}

#[test]
fn a_mod_that_turned_native_keeps_its_hmod_until_removed() {
    assert!(stays(true, "horizon-mods/coords_hud.hmod"));
    assert!(!stays(false, "horizon-mods/coords_hud.hmod"));
    assert!(!stays(true, "horizon-native-mods/coords_hud.hnmod"));
    let index = sample();
    let home = scratch("turned-native");
    put(&index, true, &home).unwrap();
    // As install_version leaves it after the native update.
    let mut list = installed::Installed::load(&home);
    list.entries[0].file = "horizon-native-mods/coords_hud.hnmod".into();
    list.save(&home).unwrap();
    fs::create_dir_all(home.join("horizon-native-mods")).unwrap();
    fs::write(home.join("horizon-native-mods/coords_hud.hnmod"), "").unwrap();
    installed::remove(&home, "coords_hud").unwrap();
    assert!(!home.join("horizon-native-mods/coords_hud.hnmod").exists());
    assert!(!home.join("horizon-mods/coords_hud.hmod").exists());
    fs::remove_dir_all(&home).unwrap();
}

#[test]
fn installs_a_shader_pack_into_shaderpacks() {
    let index = sample();
    let home = scratch("shader");
    let p = project_of(index.project("glow").unwrap()).unwrap();
    assert_eq!(p.support(), super::super::Support::Works);
    let done = install_from(&index, true, None, &p, &home, &|_, _| {}).unwrap();
    assert_eq!(done[0].file, "shaderpacks/glow-1.0.0.hshader");
    assert_eq!(done[0].kind, Kind::Shader);
    assert!(home.join("shaderpacks/glow-1.0.0.hshader").is_file());
    // Not a mod: nothing turns on in horizon-mods.txt; the game's Shader Packs page picks it.
    assert!(!home.join("horizon-mods.txt").exists());

    let mut revoked = index.clone();
    revoked.revoked.push(index::Revoked {
        id: "glow".into(),
        sha256: None,
        reason: "Test.".into(),
        date: String::new(),
    });
    let said = turn_off_revoked(&revoked, &home);
    assert_eq!(said, ["Soft Glow was taken down: Test. Remove it."]);

    installed::remove(&home, "glow").unwrap();
    assert!(!home.join("shaderpacks/glow-1.0.0.hshader").exists());
    // A file of the same name the registry didn't put there is left alone.
    fs::write(home.join("shaderpacks/glow-1.0.0.hshader"), b"mine").unwrap();
    let e = install_from(&index, true, None, &p, &home, &|_, _| {}).unwrap_err();
    assert_eq!(
        e,
        "shaderpacks/glow-1.0.0.hshader is already there: move it away first."
    );
    fs::remove_dir_all(&home).unwrap();
}

#[test]
fn refuses_a_changed_file_a_taken_id_and_local_files_from_the_real_registry() {
    let index = sample();

    let home = scratch("tampered");
    let mut tampered = index.clone();
    let cozy = tampered.projects[1].versions[0].file.url.clone();
    tampered.projects[0].versions[0].file.url = cozy;
    let e = put(&tampered, true, &home).unwrap_err();
    assert!(e.contains("larger than its source says"), "{e}");
    assert!(!home.join("horizon-mods").exists());

    let e = put(&index, false, &home).unwrap_err();
    assert!(e.contains("file servers"), "{e}");

    fs::create_dir_all(home.join("horizon-mods/coords_hud")).unwrap();
    let e = put(&index, true, &home).unwrap_err();
    assert!(e.contains("already in horizon-mods"), "{e}");

    let mut taken_down = index.clone();
    taken_down.revoked.push(index::Revoked {
        id: "coords_hud".into(),
        sha256: Some(index.projects[0].versions[0].file.sha256.clone()),
        reason: "Test.".into(),
        date: String::new(),
    });
    let e = put(&taken_down, true, &home).unwrap_err();
    assert_eq!(e, "Coordinates HUD was taken down: Test.");
    fs::remove_dir_all(&home).unwrap();
}

#[test]
fn registry_versions_run_when_made_for_this_game() {
    use super::super::{pick, support, Support};
    let v = |game: &str| vec![game.to_owned()];
    let horizon = vec![LOADER.to_owned()];
    assert_eq!(support(Kind::Mod, &v(GAME), &horizon), Support::Works);
    assert!(!support(Kind::Mod, &v("26.2"), &horizon).installable());
    let index = sample();
    let versions = |id: &str| -> Vec<Version> {
        let p = index.project(id).unwrap();
        p.versions
            .iter()
            .map(|x| version_of(&index, p, x, None))
            .collect()
    };
    assert_eq!(
        pick(Kind::Modpack, &versions("cozy"), false)
            .unwrap()
            .number,
        "1.0.0"
    );
    // A WebAssembly mod is never picked: it needs a native port.
    let hud = versions("coords_hud");
    assert_eq!(hud[0].blocked.as_deref(), Some(NEEDS_PORT));
    assert!(pick(Kind::Mod, &hud, false).is_none());
}

#[test]
fn owner_links_only_take_github_logins() {
    for ok in ["Stink-O", "ludovic111", "a"] {
        assert!(github_login(ok), "{ok}");
    }
    for bad in [
        "",
        "-x",
        "x-",
        "a--b",
        "bad login",
        "a/b",
        "x?y=1",
        &"a".repeat(40),
    ] {
        assert!(!github_login(bad), "{bad}");
    }
}

/// A version that calls mod API functions the game doesn't have is marked and skipped.
#[test]
fn versions_for_a_newer_horizon_are_not_installed() {
    let mut index = sample();
    index.projects[0].versions[0].api_level = Some(1);
    let p = index.project("coords_hud").unwrap();
    let e = newest(&index, p, Some(0)).unwrap_err();
    assert_eq!(
        e,
        "Coordinates HUD needs a newer Horizon than the game you play: update it first."
    );
    assert!(newest(&index, p, Some(1)).is_ok());
    assert!(newest(&index, p, None).is_ok());
    // In the browser, a WebAssembly mod needs a native port first.
    let v = version_of(&index, p, &p.versions[0], Some(0));
    assert_eq!(v.blocked.as_deref(), Some(NEEDS_PORT));
}

#[test]
fn ratings_keep_only_what_is_well_formed() {
    let text = r#"{"projects": {
        "zoom": {"average": 4.25, "count": 12},
        "minimap": {"average": 9, "count": 3},
        "keystrokes": {"average": 3, "count": 0},
        "Bad Id": {"average": 4, "count": 1},
        "fps_overlay": {"average": "5", "count": 1}
    }}"#;
    let ratings = parse_ratings(text);
    assert_eq!(
        ratings.into_iter().collect::<Vec<_>>(),
        [(
            "zoom".to_owned(),
            Rating {
                average: 4.25,
                count: 12
            }
        )]
    );
    assert!(parse_ratings("not json").is_empty());
    assert!(parse_ratings(r#"{"projects": []}"#).is_empty());
    assert_eq!(
        rate_url("zoom"),
        "https://playhorizon.dev/api/mods/auth/login?back=/mods/zoom"
    );
}

#[test]
fn a_project_is_starred_on_the_repository_its_source_is_in() {
    let repo = |s: &str| github_repo(s);
    let daybreak = Some("https://github.com/hildlabs/horizon-shaders".to_owned());
    // A folder of it, a trailing slash, `.git`, a query or an anchor: the same repository.
    for source in [
        "https://github.com/hildlabs/horizon-shaders",
        "https://github.com/hildlabs/horizon-shaders/tree/main/pack",
        "https://github.com/hildlabs/horizon-shaders/",
        "https://github.com/hildlabs/horizon-shaders.git",
        "https://github.com/hildlabs/horizon-shaders?tab=readme",
        "https://github.com/hildlabs/horizon-shaders#readme",
    ] {
        assert_eq!(repo(source), daybreak, "{source:?}");
    }
    assert_eq!(
        repo("https://github.com/horizon-rust/horizon.v2_mods"),
        Some("https://github.com/horizon-rust/horizon.v2_mods".to_owned())
    );
    // Not GitHub, no repository, or a name GitHub doesn't allow: no star button.
    for source in [
        "https://gitlab.com/hildlabs/horizon-shaders",
        "http://github.com/hildlabs/horizon-shaders",
        "https://github.com.evil.example/hildlabs/horizon-shaders",
        "https://github.com/hildlabs",
        "https://github.com/hildlabs/",
        "https://github.com/-hildlabs/horizon-shaders",
        "https://github.com/hildlabs/..",
        "https://github.com/hildlabs/horizon shaders",
        "",
    ] {
        assert_eq!(repo(source), None, "{source:?}");
    }
}

#[test]
fn a_picked_version_is_kept_until_it_is_cleared() {
    let home = scratch("chosen-versions");
    assert!(chosen(&home).is_empty());
    set_chosen(&home, "skins_3d", Some("1.0.1")).unwrap();
    set_chosen(&home, "flair", Some("1.0.0")).unwrap();
    assert_eq!(
        chosen(&home).get("skins_3d").map(String::as_str),
        Some("1.0.1")
    );
    set_chosen(&home, "skins_3d", None).unwrap();
    assert_eq!(chosen(&home).keys().collect::<Vec<_>>(), ["flair"]);
    fs::write(home.join(CHOSEN), b"not json").unwrap();
    assert!(chosen(&home).is_empty(), "an unreadable file picks nothing");
}
