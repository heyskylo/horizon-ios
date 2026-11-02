//! An installer shaped like Forge's whose processor runs on a stand-in `java`, served by a fetch
//! that never reaches the network, and the real ones.
use super::*;
use crate::loaders::Loader;
use crate::sources::sha1_hex;
use std::io::Write;

#[test]
fn a_version_has_its_repository_name() {
    assert_eq!(
        maven_version(Kind::NeoForge, "1.20.1", "47.1.106"),
        "1.20.1-47.1.106"
    );
    assert_eq!(
        maven_version(Kind::NeoForge, "26.3", "26.3.0.101"),
        "26.3.0.101"
    );
    assert_eq!(maven_version(Kind::Forge, "26.3", "66.1.3"), "26.3-66.1.3");
    assert_eq!(
        Kind::NeoForge.profile_id("47.1.106", "1.20.1"),
        "1.20.1-neoforge-47.1.106"
    );
}

fn zip(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut out = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    for (name, body) in entries {
        out.start_file(*name, zip::write::SimpleFileOptions::default())
            .unwrap();
        out.write_all(body).unwrap();
    }
    out.finish().unwrap().into_inner()
}

/// A Forge installer: a library it carries, one to download, one its processor makes from the
/// game's jar and the installer's patch, with that output's sha1.
#[cfg(unix)]
#[test]
fn an_installer_runs_its_processors_and_checks_what_they_make() {
    use std::os::unix::fs::PermissionsExt;
    let root = std::env::temp_dir().join(format!("horizon-forge-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    let dir = root.join("minecraft");
    let vanilla = b"the game's own jar".to_vec();
    let patch = b"+forge".to_vec();
    let jar = dir.join("versions/26.3/26.3.jar");
    fs::create_dir_all(jar.parent().unwrap()).unwrap();
    fs::write(&jar, &vanilla).unwrap();
    // The processor stand-in: notes its arguments, then writes `--input` and `--apply` to `--output`.
    let java = root.join("java");
    fs::write(
        &java,
        "#!/bin/sh\nprintf '%s\\n' \"$@\" > \"$(dirname \"$0\")/args.txt\"\n\
         while [ $# -gt 0 ]; do case \"$1\" in --input) i=\"$2\";; --output) o=\"$2\";; \
         --apply) a=\"$2\";; esac; shift; done\nmkdir -p \"$(dirname \"$o\")\"\ncat \"$i\" \"$a\" > \"$o\"\n",
    )
    .unwrap();
    fs::set_permissions(&java, fs::Permissions::from_mode(0o755)).unwrap();

    let patched = [vanilla.clone(), patch.clone()].concat();
    let tool = zip(&[(
        "META-INF/MANIFEST.MF",
        b"Manifest-Version: 1.0\r\nMain-Class: net.x.Patch\r\n",
    )]);
    let helper = b"helper".to_vec();
    let universal = b"universal".to_vec();
    let maven = "https://maven.minecraftforge.net";
    let lib = |name: &str, path: &str, url: &str, sha1: &str| json!({"name": name, "downloads": {"artifact": {"path": path, "url": url, "sha1": sha1}}});
    let profile = |patched_sha: &str| {
        json!({
            "spec": 1, "minecraft": "26.3", "json": "/version.json",
            "data": {
                "BINPATCH": {"client": "/data/client.lzma", "server": "/data/server.lzma"},
                "PATCHED": {"client": "[net.minecraftforge:forge:26.3-66.1.3:client]"},
                "PATCHED_SHA": {"client": format!("'{patched_sha}'")}
            },
            "processors": [
                {"sides": ["server"], "jar": "net.x:missing:1", "args": []},
                {"jar": "net.x:patcher:1", "classpath": ["net.x:helper:1"],
                 "args": ["--input", "{MINECRAFT_JAR}", "--output", "{PATCHED}", "--apply", "{BINPATCH}"],
                 "outputs": {"{PATCHED}": "{PATCHED_SHA}"}}
            ],
            "libraries": [
                lib("net.x:patcher:1", "net/x/patcher/1/patcher-1.jar", &format!("{maven}/net/x/patcher/1/patcher-1.jar"), &sha1_hex(&tool)),
                lib("net.x:helper:1", "net/x/helper/1/helper-1.jar", &format!("{maven}/net/x/helper/1/helper-1.jar"), &sha1_hex(&helper))
            ]
        })
    };
    let version = json!({
        "id": "26.3-forge-66.1.3", "inheritsFrom": "26.3",
        "mainClass": "net.minecraftforge.bootstrap.ForgeBootstrap",
        "libraries": [
            lib("net.minecraftforge:forge:26.3-66.1.3:universal",
                "net/minecraftforge/forge/26.3-66.1.3/forge-26.3-66.1.3-universal.jar",
                "https://maven.minecraftforge.net/net/minecraftforge/forge/26.3-66.1.3/forge-26.3-66.1.3-universal.jar",
                &sha1_hex(&universal)),
            lib("net.minecraftforge:forge:26.3-66.1.3:client",
                "net/minecraftforge/forge/26.3-66.1.3/forge-26.3-66.1.3-client.jar", "", "")
        ]
    });
    let installer = |profile: &Value| {
        zip(&[
            ("install_profile.json", profile.to_string().as_bytes()),
            ("version.json", version.to_string().as_bytes()),
            ("data/client.lzma", &patch),
            (
                "maven/net/minecraftforge/forge/26.3-66.1.3/forge-26.3-66.1.3-universal.jar",
                &universal,
            ),
        ])
    };
    let url =
        format!("{maven}/net/minecraftforge/forge/26.3-66.1.3/forge-26.3-66.1.3-installer.jar");
    let serve = |installer: Vec<u8>| -> HashMap<String, Vec<u8>> {
        HashMap::from([
            (format!("{url}.sha1"), sha1_hex(&installer).into_bytes()),
            (url.clone(), installer),
            (
                format!("{maven}/net/x/patcher/1/patcher-1.jar"),
                tool.clone(),
            ),
            (
                format!("{maven}/net/x/helper/1/helper-1.jar"),
                helper.clone(),
            ),
        ])
    };
    let loader = Loader {
        kind: Kind::Forge,
        version: "66.1.3".into(),
    };
    let install_with = |served: &HashMap<String, Vec<u8>>| {
        let fetch = |url: &str| served.get(url).cloned().ok_or(format!("404 {url}"));
        crate::loaders::install(&loader, "26.3", &dir, &java, &fetch, &|_| {})
    };

    let served = serve(installer(&profile(&sha1_hex(&patched))));
    let id = install_with(&served).unwrap();
    assert_eq!(id, "26.3-forge-66.1.3");
    let made =
        dir.join("libraries/net/minecraftforge/forge/26.3-66.1.3/forge-26.3-66.1.3-client.jar");
    assert_eq!(fs::read(&made).unwrap(), patched);
    assert_eq!(
        fs::read(dir.join(
            "libraries/net/minecraftforge/forge/26.3-66.1.3/forge-26.3-66.1.3-universal.jar"
        ))
        .unwrap(),
        universal
    );
    // The processor ran as its jar's Main-Class, with its classpath, never the server's.
    let args = fs::read_to_string(root.join("args.txt")).unwrap();
    let args: Vec<&str> = args.lines().collect();
    assert_eq!(args[0], "-cp");
    assert!(args[1].contains("net/x/patcher/1/patcher-1.jar"));
    assert!(args[1].contains("net/x/helper/1/helper-1.jar"));
    assert_eq!(args[2], "net.x.Patch");
    assert_eq!(args[4], jar.to_string_lossy());
    // The profile under its id, with the game's jar named after it.
    let kept: Value =
        serde_json::from_slice(&fs::read(dir.join(format!("versions/{id}/{id}.json"))).unwrap())
            .unwrap();
    assert_eq!(kept["jar"], id.as_str());
    assert_eq!(
        fs::read(dir.join(format!("versions/{id}/{id}.jar"))).unwrap(),
        vanilla
    );
    assert_eq!(
        kept["libraries"][1]["downloads"]["artifact"]["path"],
        "net/minecraftforge/forge/26.3-66.1.3/forge-26.3-66.1.3-client.jar"
    );

    // Done once: with its output in place, the processor doesn't run again.
    fs::remove_file(&java).unwrap();
    install_with(&served).unwrap();

    // An output other than the one the installer names is refused, and not kept.
    fs::remove_file(&made).unwrap();
    fs::write(&java, "#!/bin/sh\nwhile [ $# -gt 0 ]; do [ \"$1\" = --output ] && echo wrong > \"$2\"; shift; done\n").unwrap();
    fs::set_permissions(&java, fs::Permissions::from_mode(0o755)).unwrap();
    let error = install_with(&served).unwrap_err();
    assert!(
        error.contains("patcher made") && error.contains(&sha1_hex(&patched)),
        "{error}"
    );
    assert!(!made.exists());

    // A downloaded library that isn't the one listed is refused.
    let mut tampered = served.clone();
    fs::remove_file(dir.join("libraries/net/x/helper/1/helper-1.jar")).unwrap();
    tampered.insert(
        format!("{maven}/net/x/helper/1/helper-1.jar"),
        b"tampered".to_vec(),
    );
    assert!(install_with(&tampered)
        .unwrap_err()
        .contains("helper-1.jar is not the one"));

    // An installer for another game version is refused.
    let mut other = profile(&sha1_hex(&patched));
    other["minecraft"] = "26.2".into();
    let error = install_with(&serve(installer(&other))).unwrap_err();
    assert!(
        error.contains("for Minecraft \"26.2\", not 26.3"),
        "{error}"
    );
    fs::remove_dir_all(&root).unwrap();
}

/// The real repositories and installers on the Java Mojang ships (network, the game's jar
/// downloaded by the owner gate first): `cargo test forge -- --ignored` with
/// `HORIZON_FORGE_TEST_DIR` a game folder holding `versions/<mc>/`, `HORIZON_FORGE_TEST_JAVA`
/// a `java`, and `HORIZON_FORGE_TEST` `kind:minecraft` (`neoforge:26.3`).
#[test]
#[ignore]
fn a_real_installer_installs() {
    let dir = PathBuf::from(std::env::var("HORIZON_FORGE_TEST_DIR").unwrap());
    let java = PathBuf::from(std::env::var("HORIZON_FORGE_TEST_JAVA").unwrap());
    let wanted = std::env::var("HORIZON_FORGE_TEST").unwrap();
    let (kind, minecraft) = wanted.split_once(':').unwrap();
    let loader = crate::loaders::choose(kind, minecraft, None).unwrap();
    let id = crate::loaders::install(
        &loader,
        minecraft,
        &dir,
        &java,
        &crate::backend::get_bytes,
        &|l| eprintln!("{l}"),
    )
    .unwrap();
    assert_eq!(id, loader.kind.profile_id(&loader.version, minecraft));
    // Everything the loader puts on the classpath is there, its copy of the game's jar last.
    let file = dir.join(format!("versions/{id}/{id}.json"));
    let own = crate::java_launch::read(&file).unwrap();
    let mut on: Vec<PathBuf> = own["libraries"]
        .as_array()
        .unwrap()
        .iter()
        .map(|l| {
            dir.join("libraries")
                .join(l["downloads"]["artifact"]["path"].as_str().unwrap())
        })
        .collect();
    let version = crate::java_launch::resolve(own, &dir).unwrap();
    let os = crate::java_launch::os_name();
    let classpath = crate::java_launch::classpath(&version, &dir, os).unwrap();
    on.push(dir.join(format!("versions/{id}/{id}.jar")));
    assert_eq!(classpath.last(), on.last());
    for path in on {
        assert!(
            classpath.contains(&path) && path.is_file(),
            "{}",
            path.display()
        );
    }
}
