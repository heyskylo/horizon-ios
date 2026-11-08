use super::*;
use crate::browser::{download, FileRef, Query, GAME};
use sha2::Digest;
use std::collections::HashMap;
use std::sync::Mutex;

/// A source in memory: projects, their versions, and the bytes each file URL serves.
#[derive(Default)]
struct Fake {
    projects: HashMap<String, Project>,
    versions: Mutex<HashMap<String, Vec<Version>>>,
    bytes: Mutex<HashMap<String, Vec<u8>>>,
}

impl Source for Fake {
    fn id(&self) -> SourceId {
        SourceId::Modrinth
    }
    fn search(&self, _: &Query) -> Result<Vec<Project>, String> {
        Ok(self.projects.values().cloned().collect())
    }
    fn project(&self, id: &str) -> Result<Project, String> {
        self.projects
            .get(id)
            .cloned()
            .ok_or_else(|| "not a pack".into())
    }
    fn versions(&self, p: &Project) -> Result<Vec<Version>, String> {
        Ok(self
            .versions
            .lock()
            .unwrap()
            .get(&p.id)
            .cloned()
            .unwrap_or_default())
    }
    fn fetch(
        &self,
        file: &FileRef,
        folder: &Path,
        progress: &dyn Fn(f32),
    ) -> Result<PathBuf, String> {
        let body = self
            .bytes
            .lock()
            .unwrap()
            .get(&file.url)
            .cloned()
            .unwrap_or_default();
        download::save(file, &body[..], folder, progress)
    }
}

fn project(id: &str, kind: Kind) -> Project {
    Project {
        source: SourceId::Modrinth,
        id: id.into(),
        slug: format!("{id}-slug"),
        title: format!("Pack {id}"),
        summary: String::new(),
        author: String::new(),
        downloads: 0,
        page: format!("https://modrinth.com/resourcepack/{id}-slug"),
        icon: None,
        license: "MIT".into(),
        kind,
        game_versions: vec![GAME.into()],
        loaders: vec!["minecraft".into()],
    }
}

impl Fake {
    fn add(&mut self, p: Project) {
        self.projects.insert(p.id.clone(), p);
    }
    /// A new newest version of `id`, its file serving `body`; `tamper` serves other bytes.
    fn release(&self, id: &str, number: &str, needs: &[&str], body: &[u8], tamper: bool) {
        let url = format!("https://cdn.modrinth.com/data/{id}/versions/{number}/{id}-{number}.zip");
        let file = FileRef {
            url: url.clone(),
            name: format!("{id}-{number}.zip"),
            size: body.len() as u64,
            sha512: Some(
                sha2::Sha512::digest(body)
                    .iter()
                    .map(|b| format!("{b:02x}"))
                    .collect(),
            ),
            sha256: None,
            sha1: None,
            primary: true,
        };
        let version = Version {
            id: format!("{id}@{number}"),
            number: number.into(),
            published: number.into(),
            game_versions: vec![GAME.into()],
            loaders: self.projects[id].loaders.clone(),
            files: vec![file],
            required: needs.iter().map(|s| s.to_string()).collect(),
            blocked: None,
        };
        let mut served = body.to_vec();
        if tamper {
            served[0] ^= 1;
        }
        self.bytes.lock().unwrap().insert(url, served);
        let mut versions = self.versions.lock().unwrap();
        versions.entry(id.into()).or_default().insert(0, version);
    }
}

fn home(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("horizon-installed-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

const NONE: &dyn Fn(&str, f32) = &|_, _| {};

#[test]
fn installs_with_dependencies_then_updates_and_removes() {
    let home = home("cycle");
    let mut fake = Fake::default();
    fake.add(project("base", Kind::ResourcePack));
    fake.add(project("addon", Kind::ResourcePack));
    fake.release("base", "1", &[], b"base pack", false);
    fake.release("addon", "1", &["base"], b"addon pack", false);
    let addon = fake.project("addon").unwrap();

    let done = install(&fake, &addon, false, &home, NONE).unwrap();
    let files: Vec<&str> = done.iter().map(|e| e.file.as_str()).collect();
    assert_eq!(
        files,
        ["resourcepacks/base-1.zip", "resourcepacks/addon-1.zip"]
    );
    assert!(done[0].dependency && !done[1].dependency);
    assert!(done[1].checksum.starts_with("SHA-512:"));
    assert_eq!(
        fs::read(home.join("resourcepacks/addon-1.zip")).unwrap(),
        b"addon pack"
    );
    assert_eq!(Installed::load(&home).entries.len(), 2);

    // Installing again with nothing new downloads nothing.
    fs::remove_file(home.join("resourcepacks/base-1.zip")).unwrap();
    install(&fake, &addon, false, &home, NONE).unwrap();
    assert!(!home.join("resourcepacks/base-1.zip").exists());

    // An update replaces the old file.
    fake.release("addon", "2", &["base"], b"addon pack v2", false);
    let entry = Installed::load(&home).lookup("addon").cloned().unwrap();
    assert_eq!(update_for(&fake, &entry).unwrap().unwrap().number, "2");
    install(&fake, &addon, false, &home, NONE).unwrap();
    assert!(!home.join("resourcepacks/addon-1.zip").exists());
    assert_eq!(
        fs::read(home.join("resourcepacks/addon-2.zip")).unwrap(),
        b"addon pack v2"
    );
    let entry = Installed::load(&home)
        .lookup("addon-slug")
        .cloned()
        .unwrap();
    assert_eq!(entry.number, "2");
    assert!(update_for(&fake, &entry).unwrap().is_none());

    let removed = remove(&home, "addon").unwrap();
    assert_eq!(removed.project, "addon");
    assert!(!home.join("resourcepacks/addon-2.zip").exists());
    assert!(remove(&home, "addon").is_err());
    assert_eq!(Installed::load(&home).entries.len(), 1);
    fs::remove_dir_all(&home).unwrap();
}

#[test]
fn nothing_is_installed_when_a_check_fails() {
    let home = home("refuse");
    let mut fake = Fake::default();
    fake.add(project("good", Kind::ResourcePack));
    fake.add(project("bad", Kind::ResourcePack));
    fake.add(project("needs-mod", Kind::ResourcePack));
    fake.release("good", "1", &[], b"good", false);
    fake.release("bad", "1", &[], b"tampered", true);
    fake.release("needs-mod", "1", &["sodium"], b"x", false);

    let e = install(&fake, &fake.project("bad").unwrap(), false, &home, NONE).unwrap_err();
    assert!(e.contains("checksum doesn't match"), "{e}");
    let e = install(
        &fake,
        &fake.project("needs-mod").unwrap(),
        false,
        &home,
        NONE,
    )
    .unwrap_err();
    assert!(e.contains("needs sodium"), "{e}");
    assert!(
        !home.join("resourcepacks").exists()
            || fs::read_dir(home.join("resourcepacks")).unwrap().count() == 0
    );
    assert!(Installed::load(&home).entries.is_empty());

    // The player's own file of the same name is never replaced.
    fs::create_dir_all(home.join("resourcepacks")).unwrap();
    fs::write(home.join("resourcepacks/good-1.zip"), b"mine").unwrap();
    let e = install(&fake, &fake.project("good").unwrap(), false, &home, NONE).unwrap_err();
    assert!(e.contains("wasn't installed by the browser"), "{e}");
    assert_eq!(
        fs::read(home.join("resourcepacks/good-1.zip")).unwrap(),
        b"mine"
    );
    assert!(remove(&home, "good").is_err());
    assert!(home.join("resourcepacks/good-1.zip").exists());

    let pack = project("pack", Kind::Modpack);
    assert!(plan(&fake, &pack, false)
        .unwrap_err()
        .contains("imported, not installed"));
    fs::remove_dir_all(&home).unwrap();
}

#[test]
fn modpacks_download_checked() {
    let home = home("kinds");
    let mut fake = Fake::default();
    fake.add(project("atm", Kind::Modpack));
    fake.release("atm", "1", &[], b"mrpack", false);
    let downloads = home.join("downloads");
    let path = fetch_modpack(&fake, &fake.project("atm").unwrap(), &downloads, &|_| {}).unwrap();
    assert_eq!(fs::read(path).unwrap(), b"mrpack");
    fake.release("atm", "2", &[], b"mrpack 2", true);
    assert!(fetch_modpack(&fake, &fake.project("atm").unwrap(), &downloads, &|_| {}).is_err());
    assert!(!downloads.join("atm-2.zip").exists());
    fs::remove_dir_all(&home).unwrap();
}

/// A NeoForge jar with `files` beside its `neoforge.mods.toml` for mod `id`.
fn mod_jar(id: &str, files: &[(&str, &str)]) -> Vec<u8> {
    use std::io::Write;
    let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let toml = format!(
        "modLoader = \"javafml\"\nlicense = \"MIT\"\n[[mods]]\nmodId = \"{id}\"\nversion = \"1.0\"\ndisplayName = \"{id}\"\n"
    );
    for (name, text) in [("META-INF/neoforge.mods.toml", toml.as_str())]
        .into_iter()
        .chain(files.iter().copied())
    {
        zip.start_file(name, zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(text.as_bytes()).unwrap();
    }
    zip.finish().unwrap().into_inner()
}

#[test]
fn java_mods_are_ported_turned_on_refused_on_clashes_and_removed() {
    let home = home("java");
    let mut fake = Fake::default();
    for id in ["ruby", "lib", "gem"] {
        let mut p = project(id, Kind::Mod);
        p.loaders = vec!["neoforge".into()];
        fake.add(p);
    }
    let block = r#"{"variants": {"": {"model": "ruby:block/ruby_block"}}}"#;
    let recipe = r#"{"type": "minecraft:crafting_shapeless", "ingredients": ["minecraft:red_dye"], "result": {"id": "ruby:ruby_block"}}"#;
    let ruby = mod_jar(
        "ruby",
        &[
            ("assets/ruby/blockstates/ruby_block.json", block),
            ("data/ruby/recipe/ruby_block.json", recipe),
        ],
    );
    // A library: nothing to port.
    let lib = mod_jar("lib", &[]);
    // Another project with the same mod id.
    let gem = mod_jar("ruby", &[("assets/ruby/blockstates/gem.json", block)]);
    fake.release("ruby", "1", &["lib"], &ruby, false);
    fake.release("lib", "1", &[], &lib, false);
    fake.release("gem", "1", &[], &gem, false);

    let done = install(&fake, &fake.project("ruby").unwrap(), false, &home, NONE).unwrap();
    assert_eq!(done.len(), 1, "the library isn't installed");
    let entry = &done[0];
    assert_eq!(entry.file, "horizon-mods/ruby");
    assert_eq!(entry.verdict.as_deref(), Some("Works"));
    assert!(
        entry
            .skipped
            .iter()
            .any(|s| s.starts_with("needs Pack lib")),
        "{:?}",
        entry.skipped
    );
    assert!(entry.namespaces.contains(&"ruby".to_owned()));
    assert!(home.join("horizon-mods/ruby/horizon-mod.toml").is_file());
    assert_eq!(
        fs::read_to_string(home.join("horizon-mods.txt")).unwrap(),
        "mod.ruby:true\n"
    );
    // The jar isn't kept, its check is.
    assert!(!home
        .join("mod-browser/checks-1/.downloads/ruby-1.zip")
        .exists());

    let clash = install(&fake, &fake.project("gem").unwrap(), false, &home, NONE).unwrap_err();
    assert!(clash.contains("shares its id or namespace"), "{clash}");
    assert!(!home.join("horizon-mods/gem").exists());

    let removed = remove(&home, "ruby").unwrap();
    assert_eq!(removed.kind, Kind::Mod);
    assert!(!home.join("horizon-mods/ruby").exists());
    assert_eq!(
        fs::read_to_string(home.join("horizon-mods.txt")).unwrap(),
        ""
    );
    fs::remove_dir_all(&home).unwrap();
}

#[test]
fn shader_packs_older_launchers_installed_are_left_alone() {
    let home = home("old-shader-pack");
    fs::create_dir_all(home.join("shaderpacks")).unwrap();
    fs::write(home.join("shaderpacks/old-pack.zip"), b"shaders").unwrap();
    let entry = |kind: &str, file: &str| {
        serde_json::json!({
            "source": "modrinth", "project": file, "slug": file, "title": file, "kind": kind,
            "version": "1", "number": "1", "file": file, "checksum": "SHA-512:00", "page": ""
        })
    };
    let manifest = serde_json::json!({ "entries": [
        entry("shader-pack", "shaderpacks/old-pack.zip"),
        entry("resource-pack", "resourcepacks/faithful-1.zip"),
    ]});
    fs::write(home.join("mod-browser.json"), manifest.to_string()).unwrap();
    let installed = Installed::load(&home);
    assert_eq!(installed.entries.len(), 1);
    assert_eq!(installed.entries[0].kind, Kind::ResourcePack);
    installed.save(&home).unwrap();
    assert!(home.join("shaderpacks/old-pack.zip").is_file());
    fs::remove_dir_all(&home).unwrap();
}

/// A data pack zip with these files.
fn datapack_zip(files: &[&str]) -> Vec<u8> {
    use std::io::Write;
    let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default();
    zip.start_file("pack.mcmeta", options).unwrap();
    zip.write_all(br#"{"pack":{"pack_format":94,"description":"Test"}}"#)
        .unwrap();
    for name in files {
        zip.start_file(*name, options).unwrap();
        zip.write_all(b"{}").unwrap();
    }
    zip.finish().unwrap().into_inner()
}

#[test]
fn data_packs_go_into_a_world_when_something_in_them_runs() {
    let world = home("datapack").join("saves/New World");
    fs::create_dir_all(&world).unwrap();
    let mut fake = Fake::default();
    let mut tools = project("tools", Kind::DataPack);
    tools.loaders = vec!["datapack".into()];
    fake.add(tools);
    let mut terrain = project("terrain", Kind::Mod);
    terrain.loaders = vec!["datapack".into(), "fabric".into()];
    fake.add(terrain);
    let mut jar = project("jar", Kind::Mod);
    jar.loaders = vec!["fabric".into()];
    fake.add(jar);
    let runs = datapack_zip(&[
        "data/t/function/a.mcfunction",
        "data/t/function/b.mcfunction",
        "data/minecraft/tags/function/load.json",
        "data/t/recipe/r.json",
    ]);
    fake.release("tools", "2.0", &[], &runs, false);
    fake.release(
        "terrain",
        "1.0",
        &[],
        &datapack_zip(&["data/t/worldgen/world_preset/w.json"]),
        false,
    );
    let tools = fake.project("tools").unwrap();
    let off = &|_: f32| {};

    let done = install_datapack(&fake, &tools, false, &world, off).unwrap();
    assert_eq!(done.pack.file, "tools-2.0.zip");
    assert!(done.pack.enabled);
    assert_eq!(done.number, "2.0");
    assert_eq!(
        done.coverage.summary(),
        "Runs its 2 functions, 1 function tag, 1 recipe."
    );
    assert!(world.join("datapacks/tools-2.0.zip").is_file());
    // Nothing of it in the browser's own list.
    assert!(Installed::load(world.parent().unwrap().parent().unwrap())
        .entries
        .is_empty());
    let again = install_datapack(&fake, &tools, false, &world, off).unwrap();
    assert_eq!(again.pack.file, "tools-2.0 (2).zip");

    let terrain = fake.project("terrain").unwrap();
    let refused = install_datapack(&fake, &terrain, false, &world, off)
        .err()
        .unwrap();
    assert_eq!(
        refused,
        "Pack terrain would change nothing in Horizon yet: Horizon doesn't read its 1 worldgen \
         world preset."
    );
    // A pack of only recipes, or only worldgen, now does something.
    for (slug, file) in [
        ("recipes", "data/t/recipe/r.json"),
        ("biomes", "data/t/worldgen/biome/b.json"),
    ] {
        let mut only = project(slug, Kind::DataPack);
        only.loaders = vec!["datapack".into()];
        fake.add(only);
        fake.release(slug, "1", &[], &datapack_zip(&[file]), false);
        let only = fake.project(slug).unwrap();
        assert!(
            install_datapack(&fake, &only, false, &world, off).is_ok(),
            "{slug}"
        );
    }
    let jar = fake.project("jar").unwrap();
    assert!(install_datapack(&fake, &jar, false, &world, off).is_err());

    let mut bad = project("bad", Kind::DataPack);
    bad.loaders = vec!["datapack".into()];
    fake.add(bad);
    fake.release("bad", "1", &[], &runs, true);
    let bad = fake.project("bad").unwrap();
    assert!(install_datapack(&fake, &bad, false, &world, off).is_err());
    let names: Vec<String> = fs::read_dir(world.join("datapacks"))
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(names.len(), 4, "{names:?}");
    fs::remove_dir_all(world.parent().unwrap().parent().unwrap()).unwrap();
}
