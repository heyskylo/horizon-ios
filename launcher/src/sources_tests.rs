//! Each launcher's layout, rebuilt with small fake files (never Mojang's).
use super::*;
use std::cell::RefCell;
use std::sync::Mutex;

const CLIENT: &[u8] = b"fake 26.3 client jar";
const MANIFEST: &[u8] = br#"{"id": "26.3", "fake": true}"#;

fn pins() -> Pins {
    Pins::read(&serde_json::json!({ "minecraft": {
        "version": "26.3",
        "client_sha1": sha1_hex(CLIENT),
        "manifest_sha1": sha1_hex(MANIFEST),
        "accepted_manifest_sha1": [sha1_hex(b"an earlier one")],
    }}))
    .unwrap()
}

/// A scratch folder, removed at the end.
struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Scratch {
        let dir =
            std::env::temp_dir().join(format!("horizon-sources-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        Scratch(dir)
    }

    fn write(&self, path: &str, data: &[u8]) -> PathBuf {
        let path = self.0.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, data).unwrap();
        path
    }

    /// A launcher folder: the client jar at `jar`, a library and a sound under `base`.
    fn launcher(&self, root: &str, base: &str, jar: &str) -> PathBuf {
        let base = if base.is_empty() {
            root.to_string()
        } else {
            format!("{root}/{base}")
        };
        self.write(&format!("{base}/{jar}"), CLIENT);
        self.write(&format!("{base}/libraries/org/lib/1/lib-1.jar"), b"lib");
        self.write(&format!("{base}/assets/indexes/30.json"), b"{}");
        self.0.join(root)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// Mojang's piston-meta, serving `body` and counting the requests.
struct Mojang {
    body: Vec<u8>,
    urls: Mutex<Vec<String>>,
}

impl Mojang {
    fn new(body: &[u8]) -> Mojang {
        Mojang {
            body: body.to_vec(),
            urls: Mutex::new(Vec::new()),
        }
    }

    fn fetch(&self, url: &str) -> Result<Vec<u8>, String> {
        self.urls.lock().unwrap().push(url.to_string());
        Ok(self.body.clone())
    }
}

fn resolve_with(
    chosen: &Path,
    known: &[(&'static str, PathBuf)],
    linked: &Path,
    mojang: &Mojang,
) -> Result<PathBuf, String> {
    match resolve(
        chosen,
        &pins(),
        known,
        linked,
        &|url| mojang.fetch(url),
        &|_| {},
    )? {
        Resolved::Folder(dir) => Ok(dir),
        Resolved::NotFound(checked) => Err(not_found("26.3", &checked, chosen)),
    }
}

/// Whether `a` and `b` are one file, as a symlink (Unix) or a hard link (Windows) makes them:
/// a byte written through `a` shows in `b`. Both are left as they were.
fn same_file(a: &Path, b: &Path) -> bool {
    use std::io::Write;
    let (before_a, before_b) = (fs::read(a).unwrap(), fs::read(b).unwrap());
    let mut file = fs::OpenOptions::new().write(true).open(a).unwrap();
    file.write_all(&[!before_a[0]]).unwrap();
    drop(file);
    let same = fs::read(b).unwrap() != before_b;
    fs::write(a, &before_a).unwrap();
    same
}

/// The linked folder is the official layout, pointing at the launcher's files.
fn assert_linked(linked: &Path, base: &Path, jar: &str) {
    let v = linked.join("versions/26.3");
    assert_eq!(fs::read(v.join("26.3.jar")).unwrap(), CLIENT);
    assert_eq!(fs::read(v.join("26.3.json")).unwrap(), MANIFEST);
    assert!(same_file(&v.join("26.3.jar"), &base.join(jar)));
    assert_eq!(
        fs::read(linked.join("libraries/org/lib/1/lib-1.jar")).unwrap(),
        b"lib"
    );
    assert!(linked.join("assets/indexes/30.json").is_file());
}

/// Folders whose names cmd would read as its own syntax unquoted are still linked (Windows makes
/// a junction through `mklink /J`; a name without spaces isn't quoted by itself).
#[test]
fn folders_with_ampersands_are_linked() {
    let s = Scratch::new("a&b(c)");
    let jar = "libraries/com/mojang/minecraft/26.3/minecraft-26.3-client.jar";
    let dir = s.launcher("Prism&Co", "", jar);
    let linked = s.0.join("linked&here");
    let got = resolve_with(&dir, &[], &linked, &Mojang::new(MANIFEST)).unwrap();
    assert_eq!(got, linked);
    assert_linked(&linked, &dir, jar);
}

#[test]
fn maven_names_become_repository_paths() {
    assert_eq!(
        maven_path("com.mojang:minecraft:26.3:client").as_deref(),
        Some("com/mojang/minecraft/26.3/minecraft-26.3-client.jar")
    );
    assert_eq!(
        maven_path("org.lwjgl:lwjgl:3.3.3").as_deref(),
        Some("org/lwjgl/lwjgl/3.3.3/lwjgl-3.3.3.jar")
    );
    assert_eq!(
        maven_path("net.example:thing:1.0@zip").as_deref(),
        Some("net/example/thing/1.0/thing-1.0.zip")
    );
    for bad in ["minecraft", "a:b", "a:b:c:d:e", "a:..:1", "a:b/c:1", "a::1"] {
        assert_eq!(maven_path(bad), None, "{bad}");
    }
}

#[test]
fn the_official_layout_is_used_as_it_is() {
    let s = Scratch::new("official");
    let dir = s.launcher(".minecraft", "", "versions/26.3/26.3.jar");
    s.write(".minecraft/versions/26.3/26.3.json", MANIFEST);
    let mojang = Mojang::new(b"");
    let got = resolve_with(&dir, &[], &s.0.join("linked"), &mojang).unwrap();
    assert_eq!(got, dir);
    assert!(mojang.urls.lock().unwrap().is_empty());
    assert!(!s.0.join("linked").exists());
}

#[test]
fn an_official_folder_with_an_unknown_version_file_gets_the_pinned_one() {
    let s = Scratch::new("republished");
    let dir = s.launcher(".minecraft", "", "versions/26.3/26.3.jar");
    s.write(".minecraft/versions/26.3/26.3.json", b"{\"id\": \"26.3\"}");
    let linked = s.0.join("linked");
    let mojang = Mojang::new(MANIFEST);
    assert_eq!(resolve_with(&dir, &[], &linked, &mojang).unwrap(), linked);
    assert_linked(&linked, &dir, "versions/26.3/26.3.jar");
}

#[test]
fn prism_launcher_and_multimc_are_read_through_their_metadata() {
    let s = Scratch::new("prism");
    let jar = "libraries/com/mojang/minecraft/26.3/minecraft-26.3-client.jar";
    let dir = s.launcher("PrismLauncher", "", jar);
    // Prism's own version format: no `downloads.server`, the client a Maven name.
    s.write(
        "PrismLauncher/meta/net.minecraft/26.3.json",
        br#"{"formatVersion": 1, "uid": "net.minecraft", "version": "26.3",
            "mainJar": {"name": "com.mojang:minecraft:26.3:client",
                        "downloads": {"artifact": {"sha1": "x", "size": 20}}}}"#,
    );
    let linked = s.0.join("linked");
    let mojang = Mojang::new(MANIFEST);
    assert_eq!(resolve_with(&dir, &[], &linked, &mojang).unwrap(), linked);
    assert_linked(&linked, &dir, jar);
    assert_eq!(
        *mojang.urls.lock().unwrap(),
        [format!(
            "https://piston-meta.mojang.com/v1/packages/{}/26.3.json",
            sha1_hex(MANIFEST)
        )]
    );
    // A second preparation keeps the version file it has.
    resolve_with(&dir, &[], &linked, &mojang).unwrap();
    assert_eq!(mojang.urls.lock().unwrap().len(), 1);
}

/// famescare's Prism Launcher 26.3 install on Windows (#849): Prism keeps a native jar under
/// the name its metadata gives it, not at the version JSON's path, and preparing found it.
#[test]
fn prism_native_jars_are_found_through_the_linked_folder() {
    let s = Scratch::new("prism-natives");
    let jar = "libraries/com/mojang/minecraft/26.3/minecraft-26.3-client.jar";
    let dir = s.launcher("PrismLauncher", "", jar);
    s.write(
        "PrismLauncher/meta/net.minecraft/26.3.json",
        br#"{"mainJar": {"name": "com.mojang:minecraft:26.3:client"}}"#,
    );
    let prism =
        "libraries/com/mojang/jtracy-natives-windows/1.14.38/jtracy-natives-windows-1.14.38.jar";
    s.write(&format!("PrismLauncher/{prism}"), b"jtracy natives");
    let linked = s.0.join("linked");
    assert_eq!(
        resolve_with(&dir, &[], &linked, &Mojang::new(MANIFEST)).unwrap(),
        linked
    );
    let official = Path::new("com/mojang/jtracy/1.14.38/jtracy-1.14.38-natives-windows.jar");
    assert!(!linked.join("libraries").join(official).exists());
    let found = horizon_prepare::artifacts::launcher_library(
        &linked,
        "com.mojang:jtracy:1.14.38:natives-windows",
        official,
    );
    assert!(same_file(&found, &dir.join(prism)));
}

#[test]
fn a_main_jar_elsewhere_is_found_from_the_metadata() {
    let s = Scratch::new("meta-jar");
    let dir = s.launcher(
        "multimc",
        "",
        "libraries/com/mojang/minecraft/26.3/minecraft-26.3-x.jar",
    );
    s.write(
        "multimc/meta/net.minecraft/26.3.json",
        br#"{"mainJar": {"name": "com.mojang:minecraft:26.3:x"}}"#,
    );
    let found = find(&dir, &pins()).unwrap();
    assert_eq!(
        found.client,
        dir.join("libraries/com/mojang/minecraft/26.3/minecraft-26.3-x.jar")
    );
}

#[test]
fn the_modrinth_app_atlauncher_and_gdlauncher_are_found() {
    for (root, base, jar) in [
        ("ModrinthApp", "meta", "versions/26.3/26.3.jar"),
        (
            "atlauncher",
            "",
            "libraries/net/minecraft/client/26.3/client-26.3.jar",
        ),
        (
            "gdlauncher_carbon",
            "data",
            "libraries/net/minecraft/client/26.3/client-26.3.jar",
        ),
        (
            "gdlauncher_next",
            "datastore",
            "libraries/net/minecraft/client/26.3/client-26.3.jar",
        ),
    ] {
        let s = Scratch::new(root);
        let dir = s.launcher(root, base, jar);
        // The Modrinth App rewrites the version file: not the pinned one, so Mojang's is used.
        s.write(
            &format!("{root}/meta/versions/26.3/26.3.json"),
            b"{\"id\":\"26.3\"}",
        );
        let linked = s.0.join("linked");
        let mojang = Mojang::new(MANIFEST);
        assert_eq!(
            resolve_with(&dir, &[], &linked, &mojang).unwrap(),
            linked,
            "{root}"
        );
        assert_linked(&linked, &dir.join(base), jar);
    }
}

#[test]
fn dawn_client_is_found_in_cache() {
    let s = Scratch::new("dawn");
    let dir = s.launcher(".dawn", "cache/minecraft", "versions/26.3/26.3.jar");
    s.write(".dawn/cache/minecraft/versions/26.3/26.3.json", MANIFEST);
    let linked = s.0.join("linked");
    let mojang = Mojang::new(b"");
    let got = resolve_with(&dir, &[], &linked, &mojang).unwrap();
    assert_eq!(got, dir.join("cache/minecraft"));
    assert!(mojang.urls.lock().unwrap().is_empty());
}

#[test]
fn a_dawn_sync_folder_finds_the_launcher_root() {
    let s = Scratch::new("dawn-tree");
    let dir = s.launcher(".dawn", "cache/minecraft", "versions/26.3/26.3.jar");
    s.write(".dawn/cache/minecraft/versions/26.3/26.3.json", MANIFEST);
    let chosen = dir.join("auto-sync/groups/default");
    fs::create_dir_all(&chosen).unwrap();
    let linked = s.0.join("linked");
    let got = resolve_with(&chosen, &[], &linked, &Mojang::new(b"")).unwrap();
    assert_eq!(got, dir.join("cache/minecraft"));
}

/// A Modrinth App that only ever played a Fabric profile of 26.3 keeps the vanilla client jar
/// under the loader's name (#1783); a profile of another version beside it is not taken.
#[test]
fn a_modrinth_loader_profile_has_the_client_jar() {
    let s = Scratch::new("modrinth-fabric");
    s.write(
        "ModrinthApp/meta/versions/26.3.1-0.18.1/26.3.1-0.18.1.jar",
        b"other",
    );
    let jar = "versions/26.3-0.18.1/26.3-0.18.1.jar";
    let dir = s.launcher("ModrinthApp", "meta", jar);
    s.write(
        "ModrinthApp/meta/versions/26.3-0.18.1/26.3-0.18.1.json",
        br#"{"id":"26.3-0.18.1","inheritsFrom":"26.3"}"#,
    );
    let profile = dir.join("profiles/Fabric 26.3");
    fs::create_dir_all(&profile).unwrap();
    let linked = s.0.join("linked");
    let got = resolve_with(&profile, &[], &linked, &Mojang::new(MANIFEST)).unwrap();
    assert_eq!(got, linked);
    assert_linked(&linked, &dir.join("meta"), jar);
}

#[test]
fn an_instance_folder_may_be_chosen() {
    let s = Scratch::new("instance");
    let jar = "libraries/com/mojang/minecraft/26.3/minecraft-26.3-client.jar";
    let dir = s.launcher("PrismLauncher", "", jar);
    let instance = dir.join("instances/26.3/minecraft");
    fs::create_dir_all(&instance).unwrap();
    let linked = s.0.join("linked");
    let got = resolve_with(&instance, &[], &linked, &Mojang::new(MANIFEST)).unwrap();
    assert_eq!(got, linked);
    assert_linked(&linked, &dir, jar);
}

/// Every folder a Prism or MultiMC player naturally picks: the instance list, one instance, an
/// instance's game folder and a folder below it. The shared `libraries` are in the launcher
/// root, which the chosen folder's ancestors do not always reach (#693).
#[test]
fn a_prism_instances_folder_finds_the_launcher_root() {
    for (name, chosen) in [
        ("list", "instances"),
        ("instance", "instances/26.3"),
        ("game", "instances/26.3/minecraft"),
        // Deeper than the four folders above it: only the `instances` root helps here.
        ("deeper", "instances/26.3/minecraft/saves"),
    ] {
        let s = Scratch::new(&format!("prism-tree-{name}"));
        let jar = "libraries/com/mojang/minecraft/26.3/minecraft-26.3-client.jar";
        let dir = s.launcher("PrismLauncher", "", jar);
        s.write(
            "PrismLauncher/meta/net.minecraft/26.3.json",
            br#"{"mainJar": {"name": "com.mojang:minecraft:26.3:client"}}"#,
        );
        fs::create_dir_all(dir.join(chosen)).unwrap();
        let linked = s.0.join("linked");
        let got = resolve_with(&dir.join(chosen), &[], &linked, &Mojang::new(MANIFEST)).unwrap();
        assert_eq!(got, linked, "{chosen}");
        assert_linked(&linked, &dir, jar);
    }
}

#[test]
fn a_multimc_instances_folder_finds_the_launcher_root() {
    let s = Scratch::new("multimc-tree");
    let jar = "libraries/com/mojang/minecraft/26.3/minecraft-26.3-client.jar";
    let dir = s.launcher("multimc", "", jar);
    s.write(
        "multimc/meta/net.minecraft/26.3.json",
        br#"{"mainJar": {"name": "com.mojang:minecraft:26.3:client"}}"#,
    );
    let chosen = dir.join("instances/26.3/.minecraft");
    fs::create_dir_all(&chosen).unwrap();
    let linked = s.0.join("linked");
    let got = resolve_with(&chosen, &[], &linked, &Mojang::new(MANIFEST)).unwrap();
    assert_eq!(got, linked);
    assert_linked(&linked, &dir, jar);
}

/// When the chosen folder is inside a launcher tree, the error names it and says to launch the
/// instance once, instead of listing every launcher (#693).
#[test]
fn the_not_found_message_names_the_launcher_that_was_chosen() {
    let s = Scratch::new("prism-hint");
    let dir = s.0.join("PrismLauncher");
    let chosen = dir.join("instances/26.3/minecraft");
    fs::create_dir_all(&chosen).unwrap();
    let error =
        resolve_with(&chosen, &[], &s.0.join("linked"), &Mojang::new(MANIFEST)).unwrap_err();
    assert!(
        error.contains("Launch the 26.3 instance once in Prism Launcher"),
        "{error}"
    );
    assert!(!error.contains("or in Prism Launcher, MultiMC"), "{error}");
    // An unknown folder keeps the advice that names every launcher.
    let other = s.0.join("elsewhere");
    fs::create_dir_all(&other).unwrap();
    let error = resolve_with(&other, &[], &s.0.join("linked"), &Mojang::new(MANIFEST)).unwrap_err();
    assert!(
        error.contains(
            "Prism Launcher, MultiMC, the Modrinth App, ATLauncher, GDLauncher or Dawn Client"
        ),
        "{error}"
    );
}

#[test]
fn the_known_folders_are_searched_after_the_chosen_one() {
    let s = Scratch::new("known");
    let empty = s.0.join(".minecraft");
    fs::create_dir_all(empty.join("versions")).unwrap();
    let jar = "libraries/com/mojang/minecraft/26.3/minecraft-26.3-client.jar";
    let prism = s.launcher("PrismLauncher", "", jar);
    let known = [
        (OFFICIAL, empty.clone()),
        ("Modrinth App", s.0.join("ModrinthApp")),
        ("Prism Launcher", prism.clone()),
    ];
    let linked = s.0.join("linked");
    let logged = RefCell::new(Vec::new());
    let mojang = Mojang::new(MANIFEST);
    let got = resolve(
        &empty,
        &pins(),
        &known,
        &linked,
        &|url| mojang.fetch(url),
        &|line| logged.borrow_mut().push(line),
    );
    assert_eq!(got.unwrap(), Resolved::Folder(linked.clone()));
    assert_linked(&linked, &prism, jar);
    assert_eq!(
        logged.borrow()[0],
        format!(
            "Using Minecraft 26.3 from Prism Launcher ({})",
            prism.display()
        )
    );
}

#[test]
fn relinking_follows_the_launcher_the_files_now_come_from() {
    let s = Scratch::new("relink");
    let prism_jar = "libraries/com/mojang/minecraft/26.3/minecraft-26.3-client.jar";
    let prism = s.launcher("PrismLauncher", "", prism_jar);
    let linked = s.0.join("linked");
    let mojang = Mojang::new(MANIFEST);
    resolve_with(&prism, &[], &linked, &mojang).unwrap();
    let modrinth = s.0.join("ModrinthApp");
    s.write("ModrinthApp/meta/versions/26.3/26.3.jar", CLIENT);
    resolve_with(&modrinth, &[], &linked, &mojang).unwrap();
    let jar = linked.join("versions/26.3/26.3.jar");
    assert!(same_file(
        &jar,
        &modrinth.join("meta/versions/26.3/26.3.jar")
    ));
    assert!(!same_file(&jar, &prism.join(prism_jar)));
    // The Modrinth App folder has no libraries or assets: the old links are gone, and the
    // folders they pointed at are untouched.
    assert!(fs::symlink_metadata(linked.join("libraries")).is_err());
    assert!(prism.join("libraries/org/lib/1/lib-1.jar").is_file());
}

#[test]
fn nothing_found_lists_every_folder_and_what_it_lacks() {
    let s = Scratch::new("nothing");
    let chosen = s.0.join("chosen");
    let wrong = s.0.join("PrismLauncher");
    s.write(
        "PrismLauncher/libraries/com/mojang/minecraft/26.3/minecraft-26.3-client.jar",
        b"another release",
    );
    let empty = s.0.join("ModrinthApp");
    fs::create_dir_all(&empty).unwrap();
    let known = [
        ("Prism Launcher", wrong.clone()),
        ("Modrinth App", empty.clone()),
        ("ATLauncher", s.0.join("atlauncher")),
    ];
    let mojang = Mojang::new(MANIFEST);
    let error = resolve_with(&chosen, &known, &s.0.join("linked"), &mojang).unwrap_err();
    let expected = format!(
        "Minecraft 26.3 was not found. Horizon looked in:\n  \
         {} (the folder doesn't exist)\n  \
         {} (Prism Launcher: libraries/com/mojang/minecraft/26.3/minecraft-26.3-client.jar is \
         not the 26.3 release Horizon was made for)\n  \
         {} (Modrinth App: no Minecraft 26.3 client jar)\nTo fix it:\n",
        chosen.display(),
        wrong.display(),
        empty.display()
    );
    assert!(error.starts_with(&expected), "{error}");
    assert!(error.contains(
        "Prism Launcher, MultiMC, the Modrinth App, ATLauncher, GDLauncher or Dawn Client"
    ));
    // A launcher that isn't installed isn't listed.
    assert!(!error.contains("atlauncher"));
    assert!(mojang.urls.lock().unwrap().is_empty());
}

#[test]
fn a_version_file_that_is_not_the_pinned_one_is_refused() {
    let s = Scratch::new("bad-download");
    let jar = "libraries/com/mojang/minecraft/26.3/minecraft-26.3-client.jar";
    let dir = s.launcher("PrismLauncher", "", jar);
    let linked = s.0.join("linked");
    let error = resolve_with(&dir, &[], &linked, &Mojang::new(b"{}")).unwrap_err();
    assert!(error.contains("not the one Horizon pins"), "{error}");
    assert!(!linked.join("versions/26.3/26.3.json").exists());
}

#[test]
fn each_system_has_its_launcher_folders() {
    let linux = Folders {
        home: "/home/r".into(),
        data: "/home/r/.local/share".into(),
        config: "/home/r/.config".into(),
    };
    let folders = |os, f: &Folders| -> Vec<String> {
        known_folders(os, f)
            .into_iter()
            .map(|(name, dir)| format!("{name}: {}", dir.display()).replace('\\', "/"))
            .collect()
    };
    let got = folders(Os::Linux, &linux);
    for expected in [
        "the official launcher: /home/r/.minecraft",
        "Prism Launcher: /home/r/.local/share/PrismLauncher",
        "Prism Launcher: /home/r/.var/app/org.prismlauncher.PrismLauncher/data/PrismLauncher",
        "MultiMC: /home/r/.local/share/multimc",
        "Modrinth App: /home/r/.local/share/ModrinthApp",
        "Modrinth App: /home/r/.var/app/com.modrinth.ModrinthApp/data/ModrinthApp",
        "ATLauncher: /home/r/.local/share/atlauncher",
        "GDLauncher: /home/r/.local/share/gdlauncher_carbon",
        "GDLauncher: /home/r/.config/gdlauncher_next",
        "Dawn Client: /home/r/.dawn",
    ] {
        assert!(got.iter().any(|g| g == expected), "{expected} in {got:#?}");
    }
    assert!(got[0].starts_with("the official launcher"));
    let mac = Folders {
        home: "/Users/r".into(),
        data: "/Users/r/Library/Application Support".into(),
        config: "/Users/r/Library/Application Support".into(),
    };
    let got = folders(Os::MacOs, &mac);
    assert_eq!(
        got[0],
        "the official launcher: /Users/r/Library/Application Support/minecraft"
    );
    assert!(
        got.contains(&"Prism Launcher: /Users/r/Library/Application Support/PrismLauncher".into())
    );
    assert!(got.contains(&"Dawn Client: /Users/r/Library/Application Support/.dawn".into()));
    let windows = Folders {
        home: "C:/Users/r".into(),
        data: "C:/Users/r/AppData/Roaming".into(),
        config: "C:/Users/r/AppData/Roaming".into(),
    };
    let got = folders(Os::Windows, &windows);
    assert_eq!(
        got[0],
        "the official launcher: C:/Users/r/AppData/Roaming/.minecraft"
    );
    assert!(got.contains(&"ATLauncher: C:/Users/r/AppData/Roaming/ATLauncher".into()));
    assert!(got.contains(&"Dawn Client: C:/Users/r/AppData/Roaming/.dawn".into()));
}
