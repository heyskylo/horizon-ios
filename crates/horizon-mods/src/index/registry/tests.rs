//! A registry checkout holding the sample index's mod, modpack and shader pack
//! builds the sample index (`tests/index/index-v1.json`) exactly.
use super::*;

const HMOD: &str = "e1135345692f6dd8fd4c51920151944b82333e17049c9bb7d756f27b8deefc53";
const HPACK: &str = "241ac840f9c5ea3f6252d849da10c90d36cb1da4d6cd8d1d268c2760a353cd65";
const HSHADER: &str = "331cfe29a666a0de804e9a9ff3d19d246eebcfbfa70b4f49bba8482d30ad9f02";

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/index")
}

fn sample() -> Index {
    Index::parse(&std::fs::read_to_string(fixtures().join("index-v1.json")).unwrap()).unwrap()
}

fn hud_entry() -> String {
    let p = sample();
    let sig = p.project("coords_hud").unwrap().versions[0]
        .file
        .signature
        .clone()
        .unwrap();
    format!(
        r#"id = "coords_hud"
kind = "mod"
name = "Coordinates HUD"
summary = "Your coordinates, facing and the time of day in a corner of the screen."
description = "`/where` puts them in chat, `/coords` hides or shows the panel."
authors = ["Horizon"]
owners = ["ludovic111"]
license = "MIT"
source = "https://github.com/horizon-rust/horizon/tree/main/crates/horizon-mod-sdk/examples/coords-hud"
icon = "icons/coords_hud.png"
categories = ["utility"]
key = "RWTVtuGb/GRXcBf3Re5hzM07EzoStr2sLTJjghJia0xX15ULoZRFFZDb"

[[versions]]
version = "0.1.0"
published = "2026-10-24"
game = "26.3"
file = "coords_hud-0.1.0.hmod"
sha256 = "{HMOD}"
signature = """{sig}"""
"#
    )
}

const COZY: &str = r#"id = "cozy"
kind = "modpack"
name = "Cozy"
summary = "Coordinates and a calm render distance."
description = "Coordinates HUD with a calm render distance, played together as one profile."
categories = ["lightweight"]
icon = "icons/cozy.png"
authors = ["Horizon"]
owners = ["ludovic111"]
license = "MIT"
source = "https://github.com/horizon-rust/horizon-mods"

[[versions]]
version = "1.0.0"
published = "2026-10-24"
game = "26.3"
file = "cozy-1.0.0.hpack"
sha256 = "241ac840f9c5ea3f6252d849da10c90d36cb1da4d6cd8d1d268c2760a353cd65"
"#;

const GLOW: &str = r#"id = "glow"
kind = "shader"
name = "Soft Glow"
summary = "A post-processing pass over the world."
description = "A soft bloom drawn over the world after it renders, in Horizon's own shader format."
categories = ["bloom"]
icon = "icons/glow.png"
authors = ["Horizon"]
owners = ["ludovic111"]
license = "MIT"
source = "https://github.com/horizon-rust/horizon-mods"

[[versions]]
version = "1.0.0"
published = "2026-10-26"
game = "26.3"
file = "glow-1.0.0.hshader"
sha256 = "331cfe29a666a0de804e9a9ff3d19d246eebcfbfa70b4f49bba8482d30ad9f02"
"#;

const REVOKED_TOML: &str = r#"[[revoked]]
id = "bad_mod"
sha256 = "1111111111111111111111111111111111111111111111111111111111111111"
reason = "Sent chat messages it didn't declare."
date = "2026-10-24"
"#;

/// The header of a PNG of `w`×`h` pixels: what the registry reads of an image.
fn png(w: u32, h: u32) -> Vec<u8> {
    let mut b = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR".to_vec();
    b.extend(w.to_be_bytes());
    b.extend(h.to_be_bytes());
    b.extend([8, 6, 0, 0, 0]);
    b
}

/// A registry checkout and its downloaded files, in a scratch folder.
struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Scratch {
        let dir =
            std::env::temp_dir().join(format!("horizon-registry-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        for sub in ["repo/mods", "repo/icons", "files"] {
            std::fs::create_dir_all(dir.join(sub)).unwrap();
        }
        let s = Scratch(dir);
        s.write("mods/coords_hud.toml", &hud_entry());
        s.write("mods/cozy.toml", COZY);
        s.write("mods/glow.toml", GLOW);
        s.write("revoked.toml", REVOKED_TOML);
        for icon in ["coords_hud", "cozy", "glow"] {
            s.write_bytes(&format!("icons/{icon}.png"), &png(128, 128));
        }
        for (name, hash) in [
            ("coords_hud-0.1.0.hmod", HMOD),
            ("cozy-1.0.0.hpack", HPACK),
            ("glow-1.0.0.hshader", HSHADER),
        ] {
            std::fs::copy(fixtures().join(name), s.0.join("files").join(hash)).unwrap();
        }
        s
    }

    fn write(&self, path: &str, text: &str) {
        std::fs::write(self.0.join("repo").join(path), text).unwrap();
    }

    fn write_bytes(&self, path: &str, bytes: &[u8]) {
        let path = self.0.join("repo").join(path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, bytes).unwrap();
    }

    fn build(&self) -> Result<Index, Vec<String>> {
        let (repo, files) = (self.0.join("repo"), self.0.join("files"));
        build(&Sources {
            registry: &repo,
            files: &files,
            release: super::super::FILE_HOST,
            site: "https://horizon-rust.github.io/horizon-mods/",
            generated: "2026-10-24T18:00:00Z".into(),
        })
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn builds_the_sample_index() {
    let s = Scratch::new("sample");
    let built = s.build().unwrap();
    let sample = sample();
    assert_eq!(built.projects, sample.projects[..3]);
    assert_eq!(built.revoked, sample.revoked);
    assert_eq!(built.generated, sample.generated);
    // What it writes reads back.
    Index::parse(&serde_json::to_string_pretty(&built).unwrap()).unwrap();
}

#[test]
fn lists_every_problem_of_a_registry() {
    let s = Scratch::new("bad");
    // A file that isn't the one its hash names, an unsigned version of a
    // signed project, a shader pack, and a mod whose file is another mod.
    std::fs::write(s.0.join("files").join(HPACK), b"other bytes").unwrap();
    s.write(
        "mods/coords_hud.toml",
        &hud_entry().replace("signature = \"\"\"", "changelog = \"\"\""),
    );
    s.write(
        "mods/glow.toml",
        &COZY
            .replace("\"cozy\"", "\"glow\"")
            .replace("modpack", "shader"),
    );
    s.write(
        "mods/other.toml",
        &hud_entry()
            .replace("\"coords_hud\"", "\"other\"")
            .replace("key = ", "# key = ")
            .replace("signature = \"\"\"", "changelog = \"\"\""),
    );
    let errors = s.build().unwrap_err();
    let has = |part: &str| errors.iter().any(|e| e.contains(part));
    assert!(
        has("cozy 1.0.0: the file isn't the one its sha256 names"),
        "{errors:?}"
    );
    assert!(has("coords_hud 0.1.0: the project has a key"), "{errors:?}");
    // A shader entry is taken now, and checked like the others.
    assert!(
        has("glow 1.0.0: the file isn't the one its sha256 names"),
        "{errors:?}"
    );
    assert!(
        has("other 0.1.0: the file's id is \"coords_hud\""),
        "{errors:?}"
    );
    // The modpack's category, on what is now a shader.
    assert!(
        has("\"lightweight\" is not a category of a shader"),
        "{errors:?}"
    );
    assert!(has("glow: its icon \"icons/cozy.png\""), "{errors:?}");
    assert!(
        has("other: its icon \"icons/coords_hud.png\""),
        "{errors:?}"
    );
    assert_eq!(errors.len(), 7, "{errors:?}");
}

#[test]
fn an_entry_is_named_by_its_id_and_has_only_known_fields() {
    let s = Scratch::new("names");
    s.write("mods/hud.toml", &hud_entry());
    s.write("mods/notes.txt", "");
    s.write("mods/cozy.toml", &format!("{COZY}homepage = \"x\"\n"));
    let errors = load(&s.0.join("repo")).unwrap_err();
    assert_eq!(errors.len(), 3, "{errors:?}");
    assert!(errors[0].contains("cozy.toml") && errors[0].contains("homepage"));
    assert!(errors[1].contains("must be mods/coords_hud.toml"));
    assert!(errors[2].contains("only <id>.toml"));
}

#[test]
fn an_entry_written_reads_back() {
    let entry: Entry = toml::from_str(&hud_entry()).unwrap();
    let text = entry.to_toml().unwrap();
    assert_eq!(toml::from_str::<Entry>(&text).unwrap(), entry);
    assert!(text.contains("[[versions]]"), "{text}");
}

#[test]
fn owners_and_owner_ids_must_name_the_same_accounts() {
    let entry = |extra: &str| -> Entry {
        toml::from_str(&hud_entry().replacen("owners = [\"ludovic111\"]", extra, 1)).unwrap()
    };
    let mismatch = |e: &Entry| e.check_fields().iter().any(|m| m.contains("owner_ids"));
    // No IDs yet (older entries), or one ID per login: fine.
    assert!(!mismatch(&entry(r#"owners = ["ludovic111"]"#)));
    assert!(!mismatch(&entry(
        "owner_ids = [136255825, 583231]\nowners = [\"ludovic111\", \"someone\"]"
    )));
    // A co-owner added to `owners` alone would silently not be an owner: refused when it's made.
    assert!(mismatch(&entry(
        "owner_ids = [136255825]\nowners = [\"ludovic111\", \"someone\"]"
    )));
    assert!(mismatch(&entry(
        "owner_ids = [136255825, 583231]\nowners = [\"ludovic111\"]"
    )));
}

#[test]
fn owner_ids_are_optional_and_kept() {
    // Entries from before IDs were recorded still read, and write no `owner_ids`.
    let old: Entry = toml::from_str(&hud_entry()).unwrap();
    assert!(old.owner_ids.is_empty());
    assert!(!old.to_toml().unwrap().contains("owner_ids"));
    let text = hud_entry().replacen("owners = ", "owner_ids = [583231]\nowners = ", 1);
    let new: Entry = toml::from_str(&text).unwrap();
    assert_eq!(new.owner_ids, [583231]);
    assert_eq!(
        toml::from_str::<Entry>(&new.to_toml().unwrap()).unwrap(),
        new
    );
    // A change of IDs is pointed out to reviewers.
    let r = review(
        std::slice::from_ref(&old),
        std::slice::from_ref(&new),
        "ludovic111",
    );
    assert!(
        r.notes
            .iter()
            .any(|n| n.contains("owner IDs (none) -> 583231")),
        "{r:?}"
    );
}

#[test]
fn a_review_protects_published_files() {
    let base: Entry = toml::from_str(&hud_entry()).unwrap();
    let cozy: Entry = toml::from_str(COZY).unwrap();
    let one = std::slice::from_ref(&base);
    let same = review(one, one, "someone");
    assert_eq!(same, Review::default());

    let mut update = base.clone();
    let mut next = update.versions[0].clone();
    next.version = "0.2.0".into();
    update.versions.insert(0, next);
    let r = review(one, std::slice::from_ref(&update), "ludovic111");
    assert!(r.errors.is_empty(), "{r:?}");
    assert_eq!(r.notes, ["coords_hud: 1 new version(s)"]);
    let r = review(one, &[update], "stranger");
    assert!(r.notes[0].contains("isn't one of its owners"), "{r:?}");

    let mut swapped = base.clone();
    swapped.versions[0].sha256 = HPACK.into();
    swapped.kind = Kind::ResourcePack;
    swapped.key = None;
    let r = review(&[base.clone(), cozy.clone()], &[swapped], "ludovic111");
    assert_eq!(r.errors.len(), 3, "{r:?}");
    assert!(r.errors.iter().any(|e| e.contains("cozy: removed")));
    assert!(r.errors.iter().any(|e| e.contains("never changes")));
    assert!(r.errors.iter().any(|e| e.contains("kind can't change")));
    assert!(r.notes.iter().any(|n| n.contains("signing key")));

    let r = review(&[], &[cozy], "newcomer");
    assert_eq!(r.notes, ["cozy: a new modpack by ludovic111"]);
}

#[test]
fn a_version_taken_down_by_hash_may_leave_its_entry() {
    // A lost signing key: the next version comes under a new key, and the old one is dropped.
    let base: Entry = toml::from_str(&hud_entry()).unwrap();
    let old = base.versions[0].clone();
    let mut rotated = base.clone();
    rotated.versions[0].version = "0.2.0".into();
    rotated.versions[0].sha256 = HPACK.into();
    let down = |id: &str, sha256: &str| Revoked {
        id: id.into(),
        sha256: Some(sha256.into()),
        reason: "signed with a lost key".into(),
        date: "2026-11-09".into(),
    };
    let one = std::slice::from_ref(&base);
    let next = std::slice::from_ref(&rotated);

    let r = review_with_revoked(one, next, &[down("coords_hud", &old.sha256)], "ludovic111");
    assert!(r.errors.is_empty(), "{r:?}");
    assert!(
        r.notes
            .iter()
            .any(|n| n == "coords_hud 0.1.0: leaves the entry, taken down in revoked.toml"),
        "{r:?}"
    );
    // Not taken down, another project's take-down, or a take-down of every version
    // without a hash: still refused.
    for revoked in [
        vec![],
        vec![down("other_mod", &old.sha256)],
        vec![Revoked {
            sha256: None,
            ..down("coords_hud", "")
        }],
    ] {
        let r = review_with_revoked(one, next, &revoked, "ludovic111");
        assert!(
            r.errors
                .iter()
                .any(|e| e.contains("coords_hud 0.1.0: removed")),
            "{revoked:?}: {r:?}"
        );
    }
}

#[test]
fn a_review_refuses_following_a_repository() {
    let pinned: Entry = toml::from_str(COZY).unwrap();
    let follows: Entry = toml::from_str(COZY_REPO).unwrap();
    // A new entry that follows a repository, and a pinned entry turned into one.
    for base in [vec![], vec![pinned.clone()]] {
        let r = review(&base, std::slice::from_ref(&follows), "acme");
        assert!(
            r.errors
                .iter()
                .any(|e| e.contains("doesn't follow repositories")),
            "{r:?}"
        );
    }
    // An entry from before keeps its repository until it is pinned, and may drop it.
    let r = review(
        std::slice::from_ref(&follows),
        std::slice::from_ref(&follows),
        "acme",
    );
    assert!(r.errors.is_empty(), "{r:?}");
    let r = review(&[follows], &[pinned], "acme");
    assert!(
        !r.errors.iter().any(|e| e.contains("repositories")),
        "{r:?}"
    );
}

#[test]
fn a_mod_may_turn_native_once() {
    let base: Entry = toml::from_str(&hud_entry()).unwrap();
    let mut native = base.clone();
    native.kind = Kind::NativeMod;
    let mut next = native.versions[0].clone();
    next.version = "2.0.0".into();
    next.file = "coords_hud-2.0.0.hnmod".into();
    native.versions.insert(0, next);
    let r = review(
        std::slice::from_ref(&base),
        std::slice::from_ref(&native),
        "ludovic111",
    );
    assert!(r.errors.is_empty(), "{r:?}");
    assert!(r.notes.iter().any(|n| n.contains("turns native")), "{r:?}");
    // Its content-only .hmod versions are from before; the new ones native, and a .hmod
    // holding native code is native too.
    let none = Path::new("no-such-file");
    assert_eq!(file_kind(&native, &native.versions[1], none), Kind::Mod);
    assert_eq!(
        file_kind(&native, &native.versions[0], none),
        Kind::NativeMod
    );
    let unified = std::env::temp_dir().join(format!("unified-{}.hmod", std::process::id()));
    let mut zip = zip::ZipWriter::new(std::fs::File::create(&unified).unwrap());
    zip.start_file(
        super::super::native::MANIFEST,
        zip::write::SimpleFileOptions::default(),
    )
    .unwrap();
    zip.finish().unwrap();
    assert_eq!(
        file_kind(&native, &native.versions[1], &unified),
        Kind::NativeMod
    );
    std::fs::remove_file(unified).unwrap();
    // And never back.
    let r = review(&[native], &[base], "ludovic111");
    assert!(
        r.errors.iter().any(|e| e.contains("kind can't change")),
        "{r:?}"
    );
}

#[test]
fn a_new_icon_is_a_new_address() {
    let s = Scratch::new("icon-address");
    let icon = |s: &Scratch| {
        s.build()
            .unwrap()
            .project("coords_hud")
            .unwrap()
            .icon
            .clone()
            .unwrap()
    };
    let before = icon(&s);
    let (path, version) = before.split_once("?v=").unwrap();
    assert_eq!(
        path,
        "https://horizon-rust.github.io/horizon-mods/icons/coords_hud.png"
    );
    assert_eq!(version.len(), 12, "{before}");
    assert_eq!(icon(&s), before, "the same icon keeps its address");
    s.write_bytes("icons/coords_hud.png", &png(256, 256));
    let after = icon(&s);
    assert!(after.starts_with(path) && after != before, "{after}");
}

#[test]
fn page_fields_and_screenshots_reach_the_index() {
    let s = Scratch::new("page");
    let page = "tags = [\"hud\", \"coordinates\"]\n\
                screenshots = [\"screenshots/coords_hud/1.png\"]\n";
    s.write("mods/coords_hud.toml", &format!("{page}{}", hud_entry()));
    s.write_bytes("screenshots/coords_hud/1.png", &png(1280, 720));
    let built = s.build().unwrap();
    let p = built.project("coords_hud").unwrap();
    assert_eq!(p.categories, ["utility"]);
    assert_eq!(p.tags, ["hud", "coordinates"]);
    assert_eq!(
        p.screenshots,
        ["https://horizon-rust.github.io/horizon-mods/screenshots/coords_hud/1.png"]
    );
    Index::parse(&serde_json::to_string(&built).unwrap()).unwrap();

    // A category of another kind, a screenshot too small, one in another project's folder, one on
    // another host, and an icon that isn't square.
    let bad = "screenshots = [\"screenshots/coords_hud/1.png\", \"screenshots/other/2.png\", \
               \"https://x.test/3.jpg\"]\n";
    let entry = hud_entry().replace("[\"utility\"]", "[\"bloom\"]");
    s.write("mods/coords_hud.toml", &format!("{bad}{entry}"));
    s.write_bytes("screenshots/coords_hud/1.png", &png(100, 100));
    s.write_bytes("icons/coords_hud.png", &png(128, 64));
    let errors = s.build().unwrap_err();
    let has = |part: &str| errors.iter().any(|e| e.contains(part));
    assert!(has("\"bloom\" is not a category of a mod"), "{errors:?}");
    assert!(has("1.png: 100×100 pixels"), "{errors:?}");
    assert!(
        has("\"screenshots/other/2.png\" is neither screenshots/coords_hud/"),
        "{errors:?}"
    );
    assert!(has("the icon is 128×64"), "{errors:?}");
    assert!(
        has("\"https://x.test/3.jpg\" must be kept in the registry"),
        "{errors:?}"
    );
    assert_eq!(errors.len(), 5, "{errors:?}");
}

#[test]
fn only_open_kinds_are_taken() {
    assert!(open(&Kind::Mod).is_ok() && open(&Kind::ResourcePack).is_ok());
    assert!(open(&Kind::Shader).is_ok() && open(&Kind::ShaderPack).is_ok());
    assert!(open(&Kind::DataPack).is_ok());
    assert!(open(&Kind::Other("world".into())).is_err());
    let mut submit: Entry = toml::from_str(COZY).unwrap();
    submit.id = "submit".into();
    let dir = std::env::temp_dir();
    let src = Sources {
        registry: &dir,
        files: &dir,
        release: super::super::FILE_HOST,
        site: "https://x.test/",
        generated: String::new(),
    };
    assert!(project(&submit, &src).unwrap_err()[0].contains("reserved"));
    assert!(plain_version("1.0.0-beta.2") && !plain_version("../1") && !plain_version("1 0"));
    assert!(plain_date("2026-10-24") && !plain_date("24/10/2026"));
}

const COZY_REPO: &str = r#"id = "cozy"
kind = "modpack"
repo = "acme/cozy-pack"
owners = ["acme"]
description = "Coordinates HUD with a calm render distance, played together as one profile."
categories = ["lightweight"]
icon = "icons/cozy.png"
"#;

#[test]
fn a_project_listed_by_its_repository_takes_its_versions_from_releases() {
    let s = Scratch::new("repo");
    s.write("mods/coords_hud.toml", &hud_entry());
    s.write("mods/cozy.toml", COZY_REPO);
    let hpack = fixtures().join("cozy-1.0.0.hpack");
    let entry: Entry = toml::from_str(COZY_REPO).unwrap();
    // Listed, nothing released yet: not in the index, and not an error.
    let built = s.build().unwrap();
    assert!(built.project("cozy").is_none());
    // The page file fills what the entry leaves out; the entry's own words win.
    let page = page(
        &entry,
        "name = \"Cozy\"\nsummary = \"Calm.\"\nlicense = \"MIT\"\nauthors = [\"Acme\"]\n",
    )
    .unwrap();
    assert!(
        self::page(&entry, "name = \"Cozy\"")
            .unwrap_err()
            .contains("summary")
    );
    assert!(self::page(&entry, "owners = [\"me\"]").is_err());
    let release = |tag, asset, notes| Release {
        tag,
        published: "2026-10-30",
        notes,
        asset,
        game: "26.3",
        signature: None,
    };
    let v = release_version(
        &entry,
        &release("v1.0.0", "cozy-1.0.0.hpack", "Notes."),
        &hpack,
    )
    .unwrap();
    assert_eq!(v.version, "1.0.0");
    assert_eq!(v.file.sha256, HPACK);
    assert_eq!(
        v.file.url,
        "https://github.com/acme/cozy-pack/releases/download/v1.0.0/cozy-1.0.0.hpack"
    );
    assert_eq!(v.changelog, "Notes.");
    // A name the launcher can't download, a wrong extension, a project with a key but no signature.
    assert!(release_version(&entry, &release("v1/x", "cozy-1.0.0.hpack", ""), &hpack).is_err());
    assert!(release_version(&entry, &release("v1", "cozy.zip", ""), &hpack).is_err());
    let mut keyed = entry.clone();
    keyed.key = Some("RWTVtuGb/GRXcBf3Re5hzM07EzoStr2sLTJjghJia0xX15ULoZRFFZDb".into());
    assert!(
        release_version(&keyed, &release("v1", "cozy-1.0.0.hpack", ""), &hpack)
            .unwrap_err()
            .contains("sign")
    );
    let cache = Cache {
        page,
        versions: vec![v.clone()],
    };
    std::fs::create_dir_all(s.0.join("repo").join(CACHE)).unwrap();
    s.write(
        "versions/cozy.json",
        &serde_json::to_string(&cache).unwrap(),
    );
    let built = s.build().unwrap();
    let p = built.project("cozy").unwrap();
    assert_eq!(
        (p.name.as_str(), p.authors.clone()),
        ("Cozy", vec!["Acme".to_string()])
    );
    assert_eq!(p.source, "https://github.com/acme/cozy-pack");
    assert_eq!(p.versions, vec![v]);
    // The registry's own words win over the repository's.
    s.write(
        "mods/cozy.toml",
        &format!("{COZY_REPO}name = \"Cozy Pack\"\n"),
    );
    assert_eq!(
        s.build().unwrap().project("cozy").unwrap().name,
        "Cozy Pack"
    );
    // A repository that is not owner/name is refused.
    s.write(
        "mods/cozy.toml",
        &COZY_REPO.replace("acme/cozy-pack", "acme"),
    );
    assert!(
        s.build()
            .unwrap_err()
            .iter()
            .any(|e| e.contains("owner/name"))
    );
}

#[cfg(unix)]
#[test]
fn registry_images_cannot_read_outside_the_checkout() {
    let s = Scratch::new("outside-image");
    let repo = s.0.join("repo");
    let files = s.0.join("files");
    let src = Sources {
        registry: &repo,
        files: &files,
        release: super::super::FILE_HOST,
        site: "https://registry.test/",
        generated: String::new(),
    };
    let outside = s.0.join("outside.png");
    std::fs::write(&outside, png(128, 128)).unwrap();
    let icon = repo.join("icons/coords_hud.png");
    std::fs::remove_file(&icon).unwrap();
    std::os::unix::fs::symlink(&outside, &icon).unwrap();
    let mut errors = Vec::new();
    assert!(
        image(
            &src,
            "coords_hud",
            "icons/coords_hud.png",
            "icons/",
            &mut errors
        )
        .is_none()
    );
    assert!(
        errors
            .iter()
            .any(|e| e.contains("outside the registry checkout")),
        "{errors:?}"
    );
    assert_eq!(std::fs::read(&outside).unwrap(), png(128, 128));
}

#[test]
fn local_images_keep_their_formats_and_actual_size_limits() {
    let s = Scratch::new("bounded-images");
    let repo = s.0.join("repo");
    let files = s.0.join("files");
    let src = Sources {
        registry: &repo,
        files: &files,
        release: super::super::FILE_HOST,
        site: "https://registry.test/",
        generated: String::new(),
    };
    let mut errors = Vec::new();
    assert_eq!(
        image(
            &src,
            "coords_hud",
            "icons/coords_hud.png",
            "icons/",
            &mut errors
        ),
        Some("https://registry.test/icons/coords_hud.png".into())
    );
    s.write_bytes("screenshots/coords_hud/1.png", &png(1280, 720));
    assert_eq!(
        image(
            &src,
            "coords_hud",
            "screenshots/coords_hud/1.png",
            "screenshots/coords_hud/",
            &mut errors
        ),
        Some("https://registry.test/screenshots/coords_hud/1.png".into())
    );
    assert!(errors.is_empty(), "{errors:?}");
    let exact = vec![0; MAX_ICON as usize];
    assert_eq!(
        bounded_image_bytes(&exact[..], "growing.png", MAX_ICON).unwrap(),
        exact
    );
    let too_large = vec![0; MAX_ICON as usize + 1];
    assert!(bounded_image_bytes(&too_large[..], "growing.png", MAX_ICON).is_err());
    s.write_bytes("icons/coords_hud.png", &too_large);
    assert!(
        image(
            &src,
            "coords_hud",
            "icons/coords_hud.png",
            "icons/",
            &mut errors
        )
        .is_none()
    );
    assert!(
        errors.iter().any(|e| e.contains("over 256 KiB")),
        "{errors:?}"
    );
}

#[cfg(unix)]
#[test]
fn a_fifo_image_is_rejected_without_waiting_for_a_writer() {
    let s = Scratch::new("fifo-image");
    let repo = s.0.join("repo");
    let fifo = repo.join("icons/pipe.png");
    assert!(
        std::process::Command::new("mkfifo")
            .arg(&fifo)
            .status()
            .unwrap()
            .success()
    );
    let (send, receive) = std::sync::mpsc::channel();
    let worker = std::thread::spawn(move || {
        let _ = send.send(image_bytes(&repo, "icons/pipe.png", MAX_ICON));
    });
    let error = receive
        .recv_timeout(std::time::Duration::from_secs(1))
        .expect("checking an image must not block opening a FIFO")
        .unwrap_err();
    worker.join().unwrap();
    assert!(error.contains("not a regular image file"), "{error}");
}
