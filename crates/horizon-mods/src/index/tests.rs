//! The sample index (`tests/index/index-v1.json`, the one docs/api/mod-index.md
//! shows) and its files: `coords_hud-0.1.0.hmod`, signed with the throwaway
//! key `test.pub` by minisign 0.12 (its secret key was deleted),
//! `cozy-1.0.0.hpack`, unsigned, and `glow-1.0.0.hshader`, unsigned
//! (`horizon-shader new --template post`, then `horizon-shader pack`).
use super::*;
use std::path::PathBuf;

const OTHER_KEY: &str = "RWSe8ZD+cTOAqNd/jnXTmDtoKp0c0HxkGMrBBWcWQeEkPmcHB+XkaE9S";

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/index")
}

fn sample() -> Index {
    Index::parse(&std::fs::read_to_string(fixtures().join("index-v1.json")).unwrap()).unwrap()
}

fn check(index: &Index, id: &str, file: &Path) -> Result<Trust, String> {
    let p = index.project(id).unwrap();
    check_file(index, p, &p.versions[0], file)
}

#[test]
fn reads_the_sample_and_keeps_unknown_kinds() {
    let index = sample();
    assert_eq!(index.schema, SCHEMA);
    let kinds: Vec<&str> = index.projects.iter().map(|p| p.kind.name()).collect();
    assert_eq!(kinds, ["mod", "modpack", "shader", "world"]);
    assert_eq!(index.projects[3].kind, Kind::Other("world".into()));
    let hud = index.project("coords_hud").unwrap();
    assert_eq!(
        hud.versions[0].capabilities,
        [Capability::GameRead, Capability::Hud, Capability::Chat]
    );
    // What CI writes reads back the same.
    let text = serde_json::to_string(&index).unwrap();
    let again = Index::parse(&text).unwrap();
    assert_eq!(again, index);
    // Download counts: the publish job adds them; none is left out.
    assert_eq!(hud.downloads, 0);
    assert!(!text.contains("downloads"));
    let counted = text.replacen("\"versions\"", "\"downloads\":12345,\"versions\"", 1);
    assert_eq!(Index::parse(&counted).unwrap().projects[0].downloads, 12345);
}

#[test]
fn files_pass_against_their_entries() {
    let index = sample();
    let hmod = fixtures().join("coords_hud-0.1.0.hmod");
    assert_eq!(check(&index, "coords_hud", &hmod), Ok(Trust::Verified));
    let hpack = fixtures().join("cozy-1.0.0.hpack");
    assert_eq!(check(&index, "cozy", &hpack), Ok(Trust::Unsigned));
    let hshader = fixtures().join("glow-1.0.0.hshader");
    assert_eq!(check(&index, "glow", &hshader), Ok(Trust::Unsigned));
    let world = index.project("skyblock_start").unwrap();
    let e = check_file(&index, world, &world.versions[0], &hpack).unwrap_err();
    assert!(e.contains("bytes"), "{e}");
}

#[test]
fn a_changed_file_key_or_manifest_is_refused() {
    let index = sample();
    let dir = std::env::temp_dir().join(format!("horizon-index-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let hmod = fixtures().join("coords_hud-0.1.0.hmod");
    let mut bytes = std::fs::read(&hmod).unwrap();
    let last = bytes.len() - 1;
    bytes[last] ^= 1;
    let tampered = dir.join("coords_hud-0.1.0.hmod");
    std::fs::write(&tampered, &bytes).unwrap();
    let e = check(&index, "coords_hud", &tampered).unwrap_err();
    assert!(e.contains("SHA-256"), "{e}");

    let mut other_key = index.clone();
    other_key.projects[0].key = Some(OTHER_KEY.into());
    let e = check(&other_key, "coords_hud", &hmod).unwrap_err();
    assert!(e.contains("key"), "{e}");

    let mut more_caps = index.clone();
    more_caps.projects[0].versions[0].capabilities.pop();
    let e = check(&more_caps, "coords_hud", &hmod).unwrap_err();
    assert!(e.contains("isn't what the index lists"), "{e}");

    let mut other_level = index.clone();
    other_level.projects[0].versions[0].api_level = Some(1);
    let e = check(&other_level, "coords_hud", &hmod).unwrap_err();
    assert!(e.contains("isn't what the index lists"), "{e}");
    // An index built before levels lists none: the file still passes.
    let mut no_level = index.clone();
    no_level.projects[0].versions[0].api_level = None;
    assert_eq!(check(&no_level, "coords_hud", &hmod), Ok(Trust::Verified));

    let mut revoked = index.clone();
    revoked.revoked.push(Revoked {
        id: "coords_hud".into(),
        sha256: None,
        reason: "Test.".into(),
        date: String::new(),
    });
    let e = check(&revoked, "coords_hud", &hmod).unwrap_err();
    assert!(e.contains("taken down: Test."), "{e}");
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn revocations_match_by_hash_or_by_id() {
    let index = sample();
    let ones = "1".repeat(64);
    assert!(index.revoked("bad_mod", &ones).is_some());
    assert!(index.revoked("bad_mod", &"2".repeat(64)).is_none());
    assert!(index.revoked("coords_hud", &ones).is_none());
}

#[test]
fn refuses_broken_indexes() {
    let text = std::fs::read_to_string(fixtures().join("index-v1.json")).unwrap();
    let newer = text.replacen("\"schema\": 1", "\"schema\": 2", 1);
    assert!(Index::parse(&newer).unwrap_err().contains("schema 2"));
    let broken = |edit: &dyn Fn(&mut Index)| {
        let mut index = sample();
        edit(&mut index);
        index.check().unwrap_err()
    };
    let e = broken(&|i| {
        let twin = i.projects[0].clone();
        i.projects.push(twin)
    });
    assert!(e.contains("listed twice"), "{e}");
    let e = broken(&|i| i.projects[0].versions[0].file.url = "http://example.org/x.hmod".into());
    assert!(e.contains("https"), "{e}");
    // A local test registry may list files on disk; the published one may not.
    let local = text.replace(
        "https://github.com/horizon-rust/horizon-mods/releases/download/coords_hud-0.1.0/",
        "file:///tmp/",
    );
    assert!(Index::parse(&local).unwrap_err().contains("https"));
    assert!(Index::parse_local(&local).is_ok());
    let e = broken(&|i| i.projects[0].versions[0].file.name = "../coords_hud.hmod".into());
    assert!(e.contains("plain file name"), "{e}");
    let e = broken(&|i| i.projects[0].versions[0].file.name = "coords_hud.zip".into());
    assert!(e.contains("plain file name"), "{e}");
    let e = broken(&|i| i.projects[0].versions[0].file.signature = None);
    assert!(e.contains("unsigned"), "{e}");
    let e = broken(&|i| i.projects[0].key = Some("not a key".into()));
    assert!(e.contains("key"), "{e}");
    let e = broken(&|i| i.projects[1].versions[0].requires[0].version = "9.9".into());
    assert!(e.contains("not in the index"), "{e}");
    let e = broken(&|i| i.projects[1].versions[0].requires[0].sha256 = "2".repeat(64));
    assert!(e.contains("another file"), "{e}");
    let e = broken(&|i| i.projects[0].id = "Coords".into());
    assert!(e.contains("is not"), "{e}");
}

#[test]
fn resource_packs_need_a_pack_mcmeta() {
    use std::io::Write;
    let dir = std::env::temp_dir().join(format!("horizon-index-rp-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let pack = |files: &[(&str, &str)]| {
        let path = dir.join("look.zip");
        let mut zip = zip::ZipWriter::new(std::fs::File::create(&path).unwrap());
        for (name, text) in files {
            zip.start_file(*name, zip::write::SimpleFileOptions::default())
                .unwrap();
            zip.write_all(text.as_bytes()).unwrap();
        }
        zip.finish().unwrap();
        path
    };
    let meta = r#"{"pack": {"description": "Look", "min_format": 75, "max_format": 75}}"#;
    assert_eq!(
        check_resource_pack(&pack(&[("pack.mcmeta", meta)])),
        Ok("75".into())
    );
    let e = check_resource_pack(&pack(&[("assets/x.png", "")])).unwrap_err();
    assert!(e.contains("missing"), "{e}");
    let e = check_resource_pack(&pack(&[("pack.mcmeta", "{}")])).unwrap_err();
    assert!(e.contains("\"pack\""), "{e}");
    let e = check_resource_pack(&pack(&[("pack.mcmeta", meta), ("../x", "")])).unwrap_err();
    assert!(e.contains("plain path"), "{e}");
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn the_newest_version_this_horizon_runs_is_installed() {
    let mut index = sample();
    let mut newer = index.projects[0].versions[0].clone();
    newer.version = "0.2.0".into();
    newer.file.sha256 = "2".repeat(64);
    newer.api_level = Some(crate::api_level::LEVEL + 1);
    index.projects[0].versions.insert(0, newer);
    let p = index.project("coords_hud").unwrap();
    let pick = |level| {
        index
            .installable(p, "26.3", level)
            .map(|v| v.version.as_str())
    };
    assert_eq!(pick(crate::api_level::LEVEL), Some("0.1.0"));
    assert_eq!(pick(crate::api_level::LEVEL + 1), Some("0.2.0"));
    assert_eq!(index.installable(p, "26.4", crate::api_level::LEVEL), None);
    // No level listed: runnable as far as the index knows.
    index.projects[0].versions[0].api_level = None;
    let p = index.project("coords_hud").unwrap();
    let v = index.installable(p, "26.3", 0).unwrap();
    assert_eq!(v.version, "0.2.0");
}

#[test]
fn shader_packs_pass_the_game_s_own_check() {
    let manifest = "schema = 1\nid = \"glow\"\nname = \"Glow\"\nversion = \"1.0.0\"\napi = \"0.1\"\n\
                    [[post]]\nfile = \"post.wgsl\"\n";
    let pack = |post: &str| {
        let files = [
            (horizon_shader_api::MANIFEST, manifest),
            ("post.wgsl", post),
        ]
        .map(|(n, t)| (n.to_owned(), t.as_bytes().to_vec()))
        .into();
        horizon_shader_api::Pack::from_files(files)
            .unwrap()
            .package()
            .unwrap()
    };
    let dir = std::env::temp_dir().join(format!("horizon-mods-shader-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let good = dir.join("glow-1.0.0.hshader");
    std::fs::write(
        &good,
        pack("fn post(input: PostInput) -> vec4<f32> { return input.color; }\n"),
    )
    .unwrap();
    let bad = dir.join("bad.hshader");
    std::fs::write(
        &bad,
        pack("fn post(input: PostInput) -> vec4<f32> { return input.color }\n"),
    )
    .unwrap();
    let facts = inspect(&Kind::Shader, &good);
    let refused = inspect(&Kind::Shader, &bad).unwrap_err();
    let not_a_pack = inspect(&Kind::Shader, &fixtures().join("cozy-1.0.0.hpack")).unwrap_err();
    std::fs::remove_dir_all(&dir).unwrap();
    let facts = facts.unwrap();
    assert_eq!(
        (
            facts.id.as_deref(),
            facts.version.as_deref(),
            facts.api.as_str()
        ),
        (Some("glow"), Some("1.0.0"), "0.1")
    );
    assert!(refused.contains("post.wgsl:1:"), "{refused}");
    assert!(
        not_a_pack.contains(horizon_shader_api::MANIFEST),
        "{not_a_pack}"
    );
    assert_eq!(Kind::Shader.extension(), Some("hshader"));
    assert!(open(&Kind::Shader).is_ok());
}
