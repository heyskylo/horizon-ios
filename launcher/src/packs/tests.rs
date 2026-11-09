use super::*;
use std::io::Write;

fn scratch(name: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "horizon-launcher-packs-{name}-{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    root
}

fn zip_file(path: &Path, files: &[(&str, &[u8])]) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let mut zip = zip::ZipWriter::new(fs::File::create(path).unwrap());
    for (name, bytes) in files {
        zip.start_file(*name, zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(bytes).unwrap();
    }
    zip.finish().unwrap();
}

const META: &[u8] = "{\"pack\":{\"pack_format\":69,\"description\":[\"\",{\"text\":\"Smooth \u{a7}aglass\"},\"\\nand more\"]}}"
    .as_bytes();

const SHADER_META: &str = "schema = 1\nid = \"warm\"\nname = \"Warm Light\"\nversion = \"1.0\"\napi = \"0.1\"\n[[post]]\nfile = \"post.wgsl\"\n";

/// A game folder with three resource packs (one on), and shader packs of both kinds.
fn home(name: &str) -> PathBuf {
    let home = scratch(name);
    let rp = home.join("resourcepacks");
    zip_file(
        &rp.join("Glass.zip"),
        &[("pack.mcmeta", META), ("pack.png", b"png")],
    );
    zip_file(&rp.join("Broken.zip"), &[("readme.txt", b"no meta")]);
    fs::create_dir_all(rp.join("Folder Pack")).unwrap();
    fs::write(
        rp.join("Folder Pack/pack.mcmeta"),
        br#"{"pack":{"description":"Mine"}}"#,
    )
    .unwrap();
    zip_file(&rp.join("Zed.zip"), &[("pack.mcmeta", META)]);
    let sp = home.join("shaderpacks");
    fs::create_dir_all(sp.join("warm")).unwrap();
    fs::write(sp.join("warm/horizon-shaders.toml"), SHADER_META).unwrap();
    zip_file(&sp.join("Classic.zip"), &[("shaders/final.fsh", b"")]);
    fs::write(sp.join("notes.txt"), b"").unwrap();
    fs::write(
        home.join("options.txt"),
        "version:4786\nresourcePacks:[\"vanilla\",\"file/Zed.zip\"]\nincompatibleResourcePacks:[\"file/Zed.zip\"]\nlang:fr_ch\n",
    )
    .unwrap();
    fs::write(
        home.join("horizon-graphics.txt"),
        "preset:custom\npickedShaderPack:\nfxaa:true\n",
    )
    .unwrap();
    home
}

#[test]
fn lists_what_the_game_reads_and_which_are_on() {
    let home = home("list");
    let rp = list(&home, Kind::Resource);
    let names: Vec<(&str, bool)> = rp.iter().map(|p| (p.file.as_str(), p.enabled)).collect();
    assert_eq!(
        names,
        [
            ("Folder Pack", false),
            ("Glass.zip", false),
            ("Zed.zip", true)
        ],
        "by name; the pack without pack.mcmeta is left out, as the game does"
    );
    let glass = &rp[1];
    assert_eq!(glass.description, "Smooth glass and more");
    assert_eq!(glass.icon.as_deref(), Some(&b"png"[..]));
    let sp = list(&home, Kind::Shader);
    let names: Vec<&str> = sp.iter().map(|p| p.title.as_str()).collect();
    assert_eq!(names, ["Classic.zip", "Warm Light"]);
    assert!(list(&home.join("none"), Kind::Shader).is_empty());
    fs::remove_dir_all(&home).unwrap();
}

#[test]
fn turning_packs_on_and_off_writes_the_games_own_lines() {
    let home = home("toggle");
    let glass = find(&home, Kind::Resource, "Glass.zip").unwrap();
    set_enabled(&home, &glass, true).unwrap();
    let folder = find(&home, Kind::Resource, "folder pack").unwrap();
    set_enabled(&home, &folder, true).unwrap();
    assert!(
        fs::read_to_string(home.join("options.txt")).unwrap().contains(
            "resourcePacks:[\"vanilla\",\"file/Zed.zip\",\"file/Glass.zip\",\"file/Folder Pack\"]\n"
        ),
        "the newest on top"
    );
    let zed = find(&home, Kind::Resource, "Zed.zip").unwrap();
    set_enabled(&home, &zed, false).unwrap();
    assert_eq!(
        fs::read_to_string(home.join("options.txt")).unwrap(),
        "version:4786\nresourcePacks:[\"vanilla\",\"file/Glass.zip\",\"file/Folder Pack\"]\nincompatibleResourcePacks:[]\nlang:fr_ch\n"
    );

    let warm = find(&home, Kind::Shader, "Warm Light").unwrap();
    set_enabled(&home, &warm, true).unwrap();
    let classic = find(&home, Kind::Shader, "Classic.zip").unwrap();
    set_enabled(&home, &classic, true).unwrap();
    assert_eq!(
        fs::read_to_string(home.join("horizon-graphics.txt")).unwrap(),
        "preset:custom\npickedShaderPack:Classic.zip\nfxaa:true\n",
        "one shader pack at a time"
    );
    set_enabled(
        &home,
        &find(&home, Kind::Shader, "Classic.zip").unwrap(),
        false,
    )
    .unwrap();
    assert!(list(&home, Kind::Shader).iter().all(|p| !p.enabled));

    // A fresh instance has neither file yet.
    let fresh = scratch("fresh");
    fs::create_dir_all(fresh.join("resourcepacks")).unwrap();
    fs::copy(
        home.join("resourcepacks/Zed.zip"),
        fresh.join("resourcepacks/Zed.zip"),
    )
    .unwrap();
    set_enabled(
        &fresh,
        &find(&fresh, Kind::Resource, "Zed.zip").unwrap(),
        true,
    )
    .unwrap();
    assert_eq!(
        fs::read_to_string(fresh.join("options.txt")).unwrap(),
        "resourcePacks:[\"vanilla\",\"file/Zed.zip\"]\n"
    );
    fs::remove_dir_all(&home).unwrap();
    fs::remove_dir_all(&fresh).unwrap();
}

#[test]
fn a_dropped_file_goes_to_its_folder_under_a_free_name() {
    let home = home("add");
    let drops = scratch("drops");
    zip_file(&drops.join("Zed.zip"), &[("pack.mcmeta", META)]);
    zip_file(&drops.join("Shiny.zip"), &[("shaders/composite.fsh", b"")]);
    fs::write(drops.join("glow.hshader"), b"PK").unwrap();
    zip_file(&drops.join("random.zip"), &[("a.txt", b"")]);
    fs::write(drops.join("picture.png"), b"").unwrap();

    let added = add(&home, &drops.join("Zed.zip")).unwrap();
    assert_eq!(
        (added.kind, added.file.as_str(), added.enabled),
        (Kind::Resource, "Zed (2).zip", false)
    );
    assert_eq!(
        add(&home, &drops.join("Shiny.zip")).unwrap().kind,
        Kind::Shader
    );
    assert!(home.join("shaderpacks/Shiny.zip").is_file());
    assert_eq!(kind_of(&drops.join("glow.hshader")).unwrap(), Kind::Shader);
    assert!(add(&home, &drops.join("random.zip")).is_err());
    assert!(add(&home, &drops.join("picture.png")).is_err());
    fs::remove_dir_all(&home).unwrap();
    fs::remove_dir_all(&drops).unwrap();
}

#[test]
fn native_shader_choice_follows_the_manifest_after_rename() {
    let home = home("native-identity");
    let pack = find(&home, Kind::Shader, "warm").unwrap();
    set_enabled(&home, &pack, true).unwrap();
    assert_eq!(
        line_value(
            &fs::read_to_string(home.join(GRAPHICS)).unwrap(),
            "pickedShaderPack"
        ),
        Some("horizon:warm".into())
    );
    fs::rename(
        home.join("shaderpacks/warm"),
        home.join("shaderpacks/renamed"),
    )
    .unwrap();
    assert!(find(&home, Kind::Shader, "renamed").unwrap().enabled);
    set_line(&home.join(GRAPHICS), "pickedShaderPack", "warm-1.0.hshader").unwrap();
    assert!(find(&home, Kind::Shader, "renamed").unwrap().enabled);
    fs::create_dir_all(home.join("shaderpacks/a-copy")).unwrap();
    fs::copy(
        home.join("shaderpacks/renamed/horizon-shaders.toml"),
        home.join("shaderpacks/a-copy/horizon-shaders.toml"),
    )
    .unwrap();
    let listed = list(&home, Kind::Shader);
    assert_eq!(
        listed
            .iter()
            .filter(|p| p.description == "Horizon shader pack")
            .count(),
        1
    );
    assert!(listed.iter().any(|p| p.file == "a-copy" && p.enabled));
    fs::remove_dir_all(home).unwrap();
}

#[test]
fn toggling_native_shader_preserves_legacy_options_before_renaming() {
    for old in ["warm", "warm-1.0.hshader"] {
        let home = home(&format!("native-options-{old}"));
        let dir = home.join("shaderpacks");
        let legacy = dir.join(format!("{old}.txt"));
        fs::write(&legacy, "strength=1.5\n").unwrap();
        set_line(&home.join(GRAPHICS), "pickedShaderPack", old).unwrap();
        let pack = find(&home, Kind::Shader, "warm").unwrap();
        set_enabled(&home, &pack, false).unwrap();
        let canonical = dir.join(".horizon-options/warm.txt");
        assert_eq!(fs::read_to_string(&canonical).unwrap(), "strength=1.5\n");
        fs::rename(dir.join("warm"), dir.join("renamed")).unwrap();
        let renamed = find(&home, Kind::Shader, "renamed").unwrap();
        set_enabled(&home, &renamed, true).unwrap();
        fs::write(&legacy, "strength=2\n").unwrap();
        set_enabled(&home, &renamed, true).unwrap();
        assert_eq!(fs::read_to_string(&canonical).unwrap(), "strength=1.5\n");
        assert_eq!(fs::read_to_string(legacy).unwrap(), "strength=2\n");
        fs::remove_dir_all(home).unwrap();
    }
}

#[test]
fn switching_to_classic_preserves_the_outgoing_native_options_after_update() {
    let home = home("native-switch");
    let dir = home.join("shaderpacks");
    fs::rename(dir.join("warm"), dir.join(".old-fixture")).unwrap();
    zip_file(
        &dir.join("warm-2.0.hshader"),
        &[(SHADER_MANIFEST, SHADER_META.as_bytes())],
    );
    let legacy = dir.join("warm-1.0.hshader.txt");
    fs::write(&legacy, "strength=1.5\n").unwrap();
    set_line(&home.join(GRAPHICS), "pickedShaderPack", "warm-1.0.hshader").unwrap();
    let classic = find(&home, Kind::Shader, "Classic.zip").unwrap();
    set_enabled(&home, &classic, true).unwrap();
    let native = find(&home, Kind::Shader, "warm-2.0.hshader").unwrap();
    set_enabled(&home, &native, true).unwrap();
    assert_eq!(
        fs::read_to_string(dir.join(".horizon-options/warm.txt")).unwrap(),
        "strength=1.5\n"
    );
    assert_eq!(fs::read_to_string(legacy).unwrap(), "strength=1.5\n");
    fs::remove_dir_all(home).unwrap();
}

#[test]
fn invalid_native_manifest_does_not_hide_a_valid_duplicate() {
    let home = home("native-invalid-duplicate");
    let dir = home.join("shaderpacks");
    fs::rename(dir.join("warm"), dir.join(".old-fixture")).unwrap();
    let invalid = SHADER_META.replace("schema = 1", "schema = 99");
    zip_file(
        &dir.join("a.hshader"),
        &[(SHADER_MANIFEST, invalid.as_bytes())],
    );
    zip_file(
        &dir.join("z.hshader"),
        &[(SHADER_MANIFEST, SHADER_META.as_bytes())],
    );
    set_line(&home.join(GRAPHICS), "pickedShaderPack", "horizon:warm").unwrap();
    let native = find(&home, Kind::Shader, "z.hshader").unwrap();
    assert!(native.enabled);
    let invalid = find(&home, Kind::Shader, "a.hshader").unwrap();
    assert!(!invalid.enabled);
    set_enabled(&home, &invalid, true).unwrap();
    assert_eq!(
        line_value(
            &fs::read_to_string(home.join(GRAPHICS)).unwrap(),
            "pickedShaderPack"
        ),
        Some("a.hshader".into())
    );
    fs::remove_dir_all(home).unwrap();
}
