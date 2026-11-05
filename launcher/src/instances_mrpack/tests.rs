//! Indexes shaped as Modrinth's format describes them, and a pack imported into an instance made
//! in a scratch folder (no network: its one file is the server's, never downloaded).
use super::*;
use serde_json::json;
use std::fs;
use std::io::Write;

#[test]
fn a_packs_index_names_its_game_and_loader() {
    let of = |deps: Value| game_of(&json!({ "dependencies": deps }));
    let loader = |kind, version: &str| {
        Some(Loader {
            kind,
            version: version.into(),
        })
    };
    assert_eq!(
        of(json!({"minecraft": "1.20.1", "forge": "47.3.0"})).unwrap(),
        ("1.20.1".into(), loader(Kind::Forge, "47.3.0"))
    );
    assert_eq!(
        of(json!({"neoforge": "21.1.77", "minecraft": "1.21.1"})).unwrap(),
        ("1.21.1".into(), loader(Kind::NeoForge, "21.1.77"))
    );
    assert_eq!(
        of(json!({"minecraft": "1.21.4", "fabric-loader": "0.16.9"}))
            .unwrap()
            .1,
        loader(Kind::Fabric, "0.16.9")
    );
    assert_eq!(
        of(json!({"minecraft": "1.21.4", "quilt-loader": "0.27.1"}))
            .unwrap()
            .1,
        loader(Kind::Quilt, "0.27.1")
    );
    assert_eq!(of(json!({"minecraft": "1.21.4"})).unwrap().1, None);
    assert!(of(json!({"forge": "47.3.0"}))
        .unwrap_err()
        .contains("Minecraft version"));
    assert!(
        of(json!({"minecraft": "1.20.1", "forge": "47.3.0", "fabric-loader": "0.16.9"}))
            .unwrap_err()
            .contains("more than one")
    );
    assert!(of(json!({"minecraft": "1.12.2", "liteloader": "1.12.2"}))
        .unwrap_err()
        .contains("liteloader"));
}

#[test]
fn a_modded_pack_fills_the_instance_it_is_imported_into() {
    let root = std::env::temp_dir().join(format!("horizon-mrpack-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    let home = root.join("instance");
    fs::create_dir_all(&root).unwrap();
    let index = json!({
        "formatVersion": 1, "game": "minecraft", "versionId": "1.0", "name": "Pack",
        "dependencies": {"minecraft": "1.20.1", "forge": "47.3.0"},
        "files": [
            {"path": "mods/server-only.jar", "hashes": {}, "env": {"client": "unsupported"},
             "downloads": ["https://cdn.modrinth.com/data/x/s.jar"], "fileSize": 1}
        ]
    });
    let file = root.join("pack.mrpack");
    let mut zip = zip::ZipWriter::new(fs::File::create(&file).unwrap());
    for (name, body) in [
        ("modrinth.index.json", index.to_string().into_bytes()),
        ("overrides/options.txt", b"fov:0.5".to_vec()),
        ("client-overrides/config/a.toml", b"x = 1".to_vec()),
        ("overrides/mods/bundled.jar", b"jar".to_vec()),
        ("overrides/kubejs/startup.js", b"//".to_vec()),
        ("overrides/../escape.txt", b"no".to_vec()),
        ("server-overrides/server.properties", b"motd=no".to_vec()),
    ] {
        zip.start_file(name, zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(&body).unwrap();
    }
    zip.finish().unwrap();
    // A Horizon instance takes no loader pack; a Java one does.
    let make = |name: &str| -> Result<Instance, String> {
        fs::create_dir_all(&home).unwrap();
        Ok(Instance {
            id: "pack".into(),
            name: name.into(),
            kind: instances::Kind::Own,
            work: home.clone(),
            home: home.clone(),
            icon: None,
            group: None,
            version: None,
            played: std::time::Duration::ZERO,
            last_played: None,
            launch: Default::default(),
            java: Some(Default::default()),
        })
    };
    assert!(import::import_into(&file, &|_, _| {}, &make).is_err());
    let made = import::import_mrpack_into(&file, &|_, _| {}, &make).unwrap();
    assert_eq!(made.name, "Pack");
    assert_eq!(fs::read(home.join("options.txt")).unwrap(), b"fov:0.5");
    assert_eq!(fs::read(home.join("config/a.toml")).unwrap(), b"x = 1");
    assert!(!home.join("server.properties").exists());
    // A Java instance keeps its mods and whatever else the pack carries, never outside itself.
    assert_eq!(fs::read(home.join("mods/bundled.jar")).unwrap(), b"jar");
    assert_eq!(fs::read(home.join("kubejs/startup.js")).unwrap(), b"//");
    assert!(!root.join("escape.txt").exists());
    assert!(!home.join("mods/server-only.jar").exists());
    fs::remove_dir_all(&root).unwrap();
}
