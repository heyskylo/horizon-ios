//! Installing from a local test registry: a real content `.hmod`, an index
//! naming it, and the game folder the launcher shares.
use super::*;

fn scratch(name: &str) -> PathBuf {
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
