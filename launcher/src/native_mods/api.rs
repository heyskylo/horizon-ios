//! The launcher's generated executable for trait mods.
use super::{Bundle, MANIFEST};
use std::fs;
use std::path::{Path, PathBuf};

/// Match horizon-mixins crate IDs before using them as folder names.
pub(super) fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 100
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

pub(super) fn read(path: &Path) -> Result<toml::Table, String> {
    let text = fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    text.parse().map_err(|e| format!("{}: {e}", path.display()))
}

fn write(path: &Path, value: &toml::Table) -> Result<(), String> {
    let text = toml::to_string(value).map_err(|e| format!("{}: {e}", path.display()))?;
    fs::write(path, text).map_err(|e| format!("{}: {e}", path.display()))
}

/// Remove launcher-only tables from the incoming copy before horizon-mixins reads it.
pub(super) fn strip(bundle: &Bundle) -> Result<(), String> {
    let path = bundle.dir.join(MANIFEST);
    let mut manifest = read(&path)?;
    let api = manifest.remove("api");
    let dependencies = manifest.remove("dependencies");
    if api.is_some() || dependencies.is_some() {
        write(&path, &manifest)?;
    }
    Ok(())
}

/// Read and validate the trait package folder at the manifest boundary.
pub(super) fn folder(dir: &Path, table: &toml::Value) -> Result<String, String> {
    let manifest = dir.join(MANIFEST);
    if !table
        .as_table()
        .is_some_and(|t| t.len() == 1 && t.contains_key("path"))
    {
        return Err(format!("{}: [api] accepts only path", manifest.display()));
    }
    let folder = table
        .get("path")
        .and_then(toml::Value::as_str)
        .ok_or_else(|| format!("{}: [api] needs a path", manifest.display()))?;
    let relative = Path::new(folder);
    // An absolute path would bypass the incoming copy when joined later.
    if relative.is_absolute() {
        return Err(format!(
            "{}: invalid [api] path {folder:?}",
            manifest.display()
        ));
    }
    let source = dir.join(relative);
    let root = dir
        .canonicalize()
        .map_err(|e| format!("{}: {e}", dir.display()))?;
    let resolved = source
        .canonicalize()
        .map_err(|e| format!("{}: {e}", source.display()))?;
    if !resolved.starts_with(&root) || !resolved.join("Cargo.toml").is_file() {
        return Err(format!(
            "{}: [api] path must be a folder in the bundle with Cargo.toml",
            manifest.display()
        ));
    }
    Ok(folder.to_owned())
}

pub(super) const PACKAGE: &str = "horizon-native-game";

/// The official game's package and executable (docs/layers.md, "How a Horizon crate plugs in"):
/// horizon-client's game with Horizon's own parts, the one the release ships as `horizon-client`.
pub(super) const OFFICIAL: &str = "horizon-official";

/// Whether the staged revision has the official game. One from before it ships horizon-client's
/// executable, as the release did then.
pub(super) fn has_official(stage: &Path) -> bool {
    stage.join("crates/horizon-official/Cargo.toml").is_file()
}

/// The package the game build compiles: the generated executable when trait mods need one, else
/// the game the release ships for `target` (the server build is horizon-client's, without
/// Horizon's parts, as `game-build.yml`'s).
pub(super) fn game_package(stage: &Path, target: super::Target, has_api: bool) -> &'static str {
    if has_api {
        PACKAGE
    } else if target == super::Target::Client && has_official(stage) {
        OFFICIAL
    } else {
        "horizon-client"
    }
}

/// The executable `package` builds.
pub(super) fn executable(package: &str) -> &str {
    if package == OFFICIAL {
        OFFICIAL
    } else {
        "horizon-client"
    }
}

/// The official game, once built into `out`, under the game's name, as the release ships it.
pub(super) fn as_client(
    stage: &Path,
    target: super::Target,
    has_api: bool,
    out: &Path,
) -> Result<(), String> {
    if game_package(stage, target, has_api) != OFFICIAL {
        return Ok(());
    }
    let official = format!("{OFFICIAL}{}", std::env::consts::EXE_SUFFIX);
    fs::copy(out.join(official), out.join(crate::backend::CLIENT))
        .map(drop)
        .map_err(|e| format!("{}: {e}", out.display()))
}
/// Where the generated executable's package is staged, in the workspace.
const GAME: &str = "crates/native-game";

fn path_dep(path: &str) -> toml::Value {
    toml::Value::Table(toml::Table::from_iter([("path".into(), path.into())]))
}

/// Whether the staged revision names its trait mods (`mod_api::name_mods`, which a panic in a
/// hook names its mod by); an older one takes the list alone.
fn names_mods(stage: &Path) -> bool {
    fs::read_to_string(stage.join("crates/horizon-client/src/mod_api.rs"))
        .is_ok_and(|text| text.contains("pub fn name_mods("))
}

/// The generated executable's `main`: the trait mods of bundles `ids`, in that order, named by
/// those ids when `named`; with Horizon's own parts registered when `official` (the revision has
/// `horizon-official`, and the build is the game's, not the server's: its `official` feature).
fn main_rs(ids: &[&str], named: bool, official: bool) -> String {
    let entries = (0..ids.len())
        .map(|i| format!("        mod_{i}::hooks(),\n"))
        .collect::<String>();
    let naming = if named {
        format!("    horizon_client::mod_api::name_mods(&{ids:?});\n")
    } else {
        String::new()
    };
    let official = if official {
        "    #[cfg(feature = \"official\")]\n    horizon_official::register();\n"
    } else {
        ""
    };
    // Mirrors crates/horizon-official/src/main.rs, with trait mods passed to main_with.
    format!(
        r#"//! Generated by the launcher.
#![forbid(unsafe_code)]

#[global_allocator]
static ALLOCATOR: horizon_threads::Allocator = horizon_threads::ALLOCATOR;

static MOD_API_LEVEL: [u8; horizon_mods::api_level::MARK.len()] = horizon_mods::api_level::MARK;

fn main() {{
    std::hint::black_box(&MOD_API_LEVEL);
    let mods: horizon_client::mod_api::Mods = vec![
{entries}    ].into();
{naming}{official}    horizon_client::entry::main_with(mods);
}}
"#
    )
}

/// API objects in the same order as the generated executable's mod list.
pub(super) fn with_api(mods: &[Bundle]) -> impl Iterator<Item = &Bundle> {
    mods.iter().filter(|m| m.api.is_some())
}

/// The bundle's `[[crate]]` folders, by id, as canonical paths. horizon-mixins stages each one as
/// `native-crates/<mod>/<id>`.
fn bundle_crates(dir: &Path) -> Result<Vec<(String, PathBuf)>, String> {
    let manifest = read(&dir.join(MANIFEST))?;
    let crates = manifest.get("crate").and_then(toml::Value::as_array);
    let mut out = Vec::new();
    for entry in crates
        .into_iter()
        .flatten()
        .filter_map(toml::Value::as_table)
    {
        let id = entry.get("id").and_then(toml::Value::as_str);
        let path = entry.get("path").and_then(toml::Value::as_str);
        if let (Some(id), Some(path)) = (id, path) {
            if let Ok(folder) = dir.join(path).canonicalize() {
                out.push((id.to_owned(), folder));
            }
        }
    }
    Ok(out)
}

/// Points the trait package's path dependencies at the bundle's `[[crate]]` folders, staged one
/// level up from the copied package (`../../native-crates/<mod>/<id>`). `source` is the package's
/// folder in the bundle, so `../<crate>` is a sibling. Any other path is refused: only the
/// package's own folder is copied.
fn crate_paths(
    bundle: &Bundle,
    source: &Path,
    manifest_path: &Path,
    manifest: &mut toml::Table,
) -> Result<(), String> {
    let crates = bundle_crates(&bundle.dir)?;
    let rewrite = |table: &mut toml::Table| -> Result<(), String> {
        for (key, dep) in table.iter_mut() {
            // horizon-client's own edge is rewritten to the game below, whatever its source.
            let package = dep.get("package").and_then(toml::Value::as_str);
            if key == "horizon-client" || package == Some("horizon-client") {
                continue;
            }
            let Some(path) = dep.get("path").and_then(toml::Value::as_str) else {
                continue;
            };
            let resolved = source.join(path).canonicalize().ok();
            let Some((id, _)) = crates
                .iter()
                .find(|(_, folder)| Some(folder) == resolved.as_ref())
            else {
                return Err(format!(
                    "{}: dependency {key} path {path:?} is not a [[crate]] of this bundle",
                    manifest_path.display()
                ));
            };
            let staged = format!("../../{}/{}/{id}", super::crates::DIR, bundle.id);
            if let Some(entry) = dep.as_table_mut() {
                entry.insert("path".into(), staged.into());
            }
        }
        Ok(())
    };
    for kind in ["dependencies", "dev-dependencies", "build-dependencies"] {
        if let Some(table) = manifest.get_mut(kind).and_then(toml::Value::as_table_mut) {
            rewrite(table)?;
        }
    }
    if let Some(targets) = manifest
        .get_mut("target")
        .and_then(toml::Value::as_table_mut)
    {
        for (_, target) in targets.iter_mut() {
            let Some(target) = target.as_table_mut() else {
                continue;
            };
            for kind in ["dependencies", "dev-dependencies", "build-dependencies"] {
                if let Some(table) = target.get_mut(kind).and_then(toml::Value::as_table_mut) {
                    rewrite(table)?;
                }
            }
        }
    }
    Ok(())
}

/// Add API packages and the generated executable to the fresh staged workspace.
pub(super) fn stage(
    stage: &Path,
    bundles: &[Bundle],
    declared: &[Vec<super::dependencies::Dependency>],
    resolution: &super::dependencies::Resolution,
) -> Result<bool, String> {
    let mods: Vec<_> = resolution
        .order
        .iter()
        .map(|&i| (i, &bundles[i]))
        .filter(|(_, b)| b.api.is_some())
        .collect();
    let mut packages = std::collections::HashMap::new();
    for (_, bundle) in &mods {
        let path = bundle
            .dir
            .join(bundle.api.as_ref().expect("filtered"))
            .join("Cargo.toml");
        let manifest = read(&path)?;
        let name = manifest
            .get("package")
            .and_then(|p| p.get("name"))
            .and_then(toml::Value::as_str)
            .ok_or_else(|| format!("{}: missing package name", path.display()))?;
        packages.insert(bundle.id.as_str(), name.to_owned());
    }
    if mods.is_empty() {
        return Ok(false);
    }
    let mut deps = toml::Table::new();
    for name in ["horizon-client", "horizon-threads", "horizon-mods"] {
        deps.insert(name.into(), path_dep(&format!("../{name}")));
    }
    // horizon-client's defaults (sound, controllers) are this package's, so a server build's
    // `--no-default-features` turns them off here too.
    if let Some(client) = deps
        .get_mut("horizon-client")
        .and_then(toml::Value::as_table_mut)
    {
        client.insert("default-features".into(), false.into());
    }
    // The official game's parts, for the game build only (the `official` feature, a default one).
    let official = has_official(stage);
    if official {
        let mut dep = toml::Table::new();
        dep.insert("path".into(), format!("../{}", OFFICIAL).into());
        dep.insert("optional".into(), true.into());
        deps.insert(OFFICIAL.into(), toml::Value::Table(dep));
    }
    let mut members = Vec::new();
    for (i, (index, bundle)) in mods.iter().enumerate() {
        let relative = format!("native-api/{}", bundle.id);
        let dest = stage.join(&relative);
        let source = bundle.dir.join(bundle.api.as_ref().expect("filtered"));
        super::sync(&source, &dest)?;
        let path = dest.join("Cargo.toml");
        let mut manifest = read(&path)?;
        crate_paths(bundle, &source, &path, &mut manifest)?;
        let package = manifest
            .get("package")
            .and_then(|p| p.get("name"))
            .and_then(toml::Value::as_str)
            .ok_or_else(|| format!("{}: missing package name", path.display()))?
            .to_owned();
        let dependencies = manifest
            .get_mut("dependencies")
            .and_then(toml::Value::as_table_mut)
            .ok_or_else(|| format!("{}: missing horizon-client dependency", path.display()))?;
        let dependency = dependencies
            .iter_mut()
            .find(|(key, value)| {
                *key == "horizon-client"
                    || value.get("package").and_then(toml::Value::as_str) == Some("horizon-client")
            })
            .ok_or_else(|| format!("{}: missing horizon-client dependency", path.display()))?
            .1;
        let mut rewritten = path_dep("../../crates/horizon-client");
        if let (Some(old), Some(new)) = (dependency.as_table(), rewritten.as_table_mut()) {
            // A renamed dependency also needs its package name.
            for key in ["package", "features", "default-features"] {
                if let Some(value) = old.get(key) {
                    new.insert(key.into(), value.clone());
                }
            }
        }
        // The game's features are the generated package's to choose (a server build turns its
        // defaults off), so no mod turns them back on.
        if let Some(new) = rewritten.as_table_mut() {
            new.insert("default-features".into(), false.into());
        }
        *dependency = rewritten;
        let targets: Vec<_> = declared[*index]
            .iter()
            .map(|dependency| super::dependencies::Package {
                dependency,
                package: packages.get(dependency.id.as_str()).map(String::as_str),
            })
            .collect();
        let enabled = super::dependencies::rewrite(&bundle.id, &mut manifest, &targets)
            .map_err(|e| e.to_string())?;
        write(&path, &manifest)?;
        let mut dep = toml::Table::new();
        dep.insert("path".into(), format!("../../{relative}").into());
        dep.insert("package".into(), package.into());
        if !enabled.is_empty() {
            dep.insert(
                "features".into(),
                toml::Value::Array(enabled.into_iter().map(toml::Value::String).collect()),
            );
        }
        deps.insert(format!("mod_{i}"), toml::Value::Table(dep));
        members.push(relative);
    }
    // Beside horizon-client, so its build script (the Windows stack size and icon, for the
    // `horizon-client` binary this package owns now) finds `packaging/` where it looks.
    let game = stage.join(GAME);
    fs::create_dir_all(game.join("src")).map_err(|e| e.to_string())?;
    let mut manifest = toml::Table::new();
    manifest.insert(
        "package".into(),
        toml::Value::Table(toml::Table::from_iter([
            ("name".into(), PACKAGE.into()),
            ("version".into(), "0.1.0".into()),
            ("edition".into(), "2024".into()),
            ("publish".into(), false.into()),
            ("build".into(), "../horizon-client/build.rs".into()),
        ])),
    );
    // This sibling package uses the client's build.rs, so its build dependencies apply too;
    // relative paths still point to the same crates from crates/native-game.
    let client = read(&stage.join("crates/horizon-client/Cargo.toml"))?;
    if let Some(dependencies) = client.get("build-dependencies") {
        manifest.insert("build-dependencies".into(), dependencies.clone());
    }
    let windows = toml::Table::from_iter([(
        "build-dependencies".into(),
        toml::Value::Table(toml::Table::from_iter([(
            "winresource".into(),
            "0.1".into(),
        )])),
    )]);
    manifest.insert(
        "target".into(),
        toml::Value::Table(toml::Table::from_iter([(
            "cfg(windows)".into(),
            toml::Value::Table(windows),
        )])),
    );
    manifest.insert(
        "bin".into(),
        toml::Value::Array(vec![toml::Value::Table(toml::Table::from_iter([
            ("name".into(), "horizon-client".into()),
            ("path".into(), "src/main.rs".into()),
        ]))]),
    );
    manifest.insert("dependencies".into(), toml::Value::Table(deps));
    let mut default = vec![toml::Value::from("horizon-client/default")];
    let mut features = toml::Table::new();
    if official {
        default.push("official".into());
        features.insert(
            "official".into(),
            toml::Value::Array(vec![format!("dep:{}", OFFICIAL).into()]),
        );
    }
    features.insert("default".into(), toml::Value::Array(default));
    manifest.insert("features".into(), toml::Value::Table(features));
    write(&game.join("Cargo.toml"), &manifest)?;
    let ids: Vec<_> = mods.iter().map(|(_, bundle)| bundle.id.as_str()).collect();
    fs::write(
        game.join("src/main.rs"),
        main_rs(&ids, names_mods(stage), official),
    )
    .map_err(|e| e.to_string())?;
    let root = stage.join("Cargo.toml");
    let mut workspace = read(&root)?;
    let list = workspace
        .get_mut("workspace")
        .and_then(|w| w.get_mut("members"))
        .and_then(toml::Value::as_array_mut)
        .ok_or("workspace has no members")?;
    members.push(GAME.into());
    list.extend(members.into_iter().map(toml::Value::String));
    write(&root, &workspace)?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(manifest: &str) -> std::path::PathBuf {
        let name = std::thread::current().name().unwrap().replace(':', "-");
        let dir = super::super::tests::dir(&name);
        fs::create_dir_all(dir.join("engine/src")).unwrap();
        fs::write(dir.join(MANIFEST), manifest).unwrap();
        fs::write(dir.join("engine/Cargo.toml"), "[package]\nname = 'example-mod'\nversion = '0.1.0'\nedition = '2024'\n[dependencies]\nhorizon-client = '0.1'\n").unwrap();
        fs::write(dir.join("engine/src/lib.rs"), "").unwrap();
        dir
    }

    #[test]
    fn validates_the_api_folder() {
        for (path, valid) in [
            ("engine", true),
            ("missing", false),
            ("../engine", false),
            ("/engine", false),
        ] {
            let dir = fixture(&format!(
                "[mod]\nid = 'example'\nversion = '1'\n[api]\npath = {path:?}\n"
            ));
            assert_eq!(super::super::bundle(&dir, None).is_ok(), valid, "{path}");
            fs::remove_dir_all(dir).unwrap();
        }
    }

    #[test]
    fn rejects_an_entry_override() {
        let dir = fixture(
            "[mod]\nid = 'example'\nversion = '1'\n[api]\npath = 'engine'\nentry = 'other'\n",
        );
        assert!(super::super::bundle(&dir, None)
            .unwrap_err()
            .contains("accepts only path"));
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn removes_only_the_api_table_for_mixins() {
        let dir = fixture(
            "[mod]\nid = 'example'\nversion = '1'\n[api]\npath = 'engine'\n[dependencies]\nbase = '^1'\n[[injection]]\nid = 'test'\n",
        );
        let bundle = super::super::bundle(&dir, None).unwrap();
        assert!(bundle.mixins_content);
        strip(&bundle).unwrap();
        let value = read(&dir.join(MANIFEST)).unwrap();
        assert!(!value.contains_key("api"));
        assert!(!value.contains_key("dependencies"));
        assert!(value.contains_key("injection"));
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn generated_main_uses_the_games_allocator() {
        let game = include_str!("../../../crates/horizon-client/src/main.rs");
        let allocator = game
            .lines()
            .find(|line| line.starts_with("static ALLOCATOR:"))
            .unwrap();
        let generated = main_rs(&[], false, false);
        assert!(generated.lines().any(|line| line == allocator));
    }

    #[test]
    fn generated_main_keeps_mod_order() {
        let ids = ["base", "extra"];
        for count in 0..=2 {
            let text = main_rs(&ids[..count], false, false);
            assert!(text.contains("entry::main_with(mods)"));
            assert_eq!(text.matches("::hooks()").count(), count);
            assert!(!text.contains("name_mods"));
            if count == 2 {
                assert!(
                    text.find("mod_0::hooks()").unwrap() < text.find("mod_1::hooks()").unwrap()
                );
            }
        }
        // A revision that names its mods gets the ids in the same order, before main_with.
        let named = main_rs(&ids, true, false);
        let naming = named
            .find(r#"mod_api::name_mods(&["base", "extra"]);"#)
            .unwrap();
        assert!(named.find("mod_1::hooks()").unwrap() < naming);
        assert!(naming < named.find("entry::main_with(mods)").unwrap());
    }

    #[test]
    fn stages_api_without_a_dependency_cycle() {
        let dir = fixture("[mod]\nid = 'example'\nversion = '1'\n[api]\npath = 'engine'\n");
        let bundle = super::super::bundle(&dir, None).unwrap();
        let stage_dir = dir.join("workspace");
        fs::create_dir_all(stage_dir.join("crates/horizon-client")).unwrap();
        fs::write(stage_dir.join("Cargo.toml"), "[workspace]\nmembers = []\n").unwrap();
        fs::write(
            stage_dir.join("crates/horizon-client/Cargo.toml"),
            "[package]\nname = 'horizon-client'\nversion = '0.1.0'\n",
        )
        .unwrap();
        let resolution = super::super::dependencies::Resolution { order: vec![0] };
        assert!(stage(&stage_dir, &[bundle], &[vec![]], &resolution).unwrap());
        let root = read(&stage_dir.join("Cargo.toml")).unwrap();
        let members = root["workspace"]["members"].as_array().unwrap();
        assert!(members
            .iter()
            .any(|v| v.as_str() == Some("native-api/example")));
        assert!(members.iter().any(|v| v.as_str() == Some(GAME)));
        let engine = read(&stage_dir.join("native-api/example/Cargo.toml")).unwrap();
        assert_eq!(
            engine["dependencies"]["horizon-client"]["path"].as_str(),
            Some("../../crates/horizon-client")
        );
        let game = read(&stage_dir.join(GAME).join("Cargo.toml")).unwrap();
        assert_eq!(
            game["dependencies"]["mod_0"]["package"].as_str(),
            Some("example-mod")
        );
        assert_eq!(
            game["dependencies"]["mod_0"]["path"].as_str(),
            Some("../../native-api/example")
        );
        assert_eq!(
            game["package"]["build"].as_str(),
            Some("../horizon-client/build.rs")
        );
        assert!(stage_dir
            .join(GAME)
            .join(
                game["dependencies"]["horizon-client"]["path"]
                    .as_str()
                    .unwrap()
            )
            .ends_with("crates/native-game/../horizon-client"));
        // The game's default features are the generated package's alone, so a server build's
        // `--no-default-features` reaches horizon-client.
        assert_eq!(
            engine["dependencies"]["horizon-client"]["default-features"].as_bool(),
            Some(false)
        );
        assert_eq!(
            game["dependencies"]["horizon-client"]["default-features"].as_bool(),
            Some(false)
        );
        assert_eq!(
            game["features"]["default"][0].as_str(),
            Some("horizon-client/default")
        );
        let main = || fs::read_to_string(stage_dir.join(GAME).join("src/main.rs")).unwrap();
        assert!(!main().contains("name_mods"));
        // A revision with `mod_api::name_mods` gets each bundle's id.
        fs::create_dir_all(stage_dir.join("crates/horizon-client/src")).unwrap();
        fs::write(
            stage_dir.join("crates/horizon-client/src/mod_api.rs"),
            "pub fn name_mods(ids: &'static [&'static str]) {}\n",
        )
        .unwrap();
        fs::write(stage_dir.join("crates/horizon-client/Cargo.toml"), "[package]\nname = 'horizon-client'\nversion = '0.1.0'\n[build-dependencies]\nhorizon-mod-api-gen = { path = '../horizon-mod-api-gen' }\n").unwrap();
        let bundle = super::super::bundle(&dir, None).unwrap();
        stage(&stage_dir, &[bundle], &[vec![]], &resolution).unwrap();
        assert!(main().contains(r#"name_mods(&["example"]);"#));
        let game = read(&stage_dir.join(GAME).join("Cargo.toml")).unwrap();
        assert_eq!(
            game["build-dependencies"]["horizon-mod-api-gen"]["path"].as_str(),
            Some("../horizon-mod-api-gen")
        );
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn api_only_skips_mixins() {
        let dir = fixture("[mod]\nid = 'example'\nversion = '1'\n[api]\npath = 'engine'\n");
        let bundle = super::super::bundle(&dir, None).unwrap();
        assert!(!bundle.mixins_content);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn the_official_games_parts_register_in_the_game_build_only() {
        let dir = fixture("[mod]\nid = 'example'\nversion = '1'\n[api]\npath = 'engine'\n");
        let stage_dir = workspace(&dir);
        let resolution = super::super::dependencies::Resolution { order: vec![0] };
        let main = || fs::read_to_string(stage_dir.join(GAME).join("src/main.rs")).unwrap();
        let game = || read(&stage_dir.join(GAME).join("Cargo.toml")).unwrap();
        // A revision from before horizon-official: horizon-client's game, as its release shipped.
        let bundle = super::super::bundle(&dir, None).unwrap();
        stage(&stage_dir, &[bundle], &[vec![]], &resolution).unwrap();
        assert!(game()["dependencies"].get("horizon-official").is_none());
        assert!(!main().contains("horizon_official"));
        fs::create_dir_all(stage_dir.join("crates/horizon-official")).unwrap();
        fs::write(stage_dir.join("crates/horizon-official/Cargo.toml"), "").unwrap();
        let bundle = super::super::bundle(&dir, None).unwrap();
        stage(&stage_dir, &[bundle], &[vec![]], &resolution).unwrap();
        let game = game();
        let official = &game["dependencies"]["horizon-official"];
        assert_eq!(official["path"].as_str(), Some("../horizon-official"));
        assert_eq!(official["optional"].as_bool(), Some(true));
        // A default feature: the server build's `--no-default-features` leaves the parts out.
        let features = &game["features"];
        assert!(features["default"]
            .as_array()
            .unwrap()
            .iter()
            .any(|f| f.as_str() == Some("official")));
        assert_eq!(
            features["official"][0].as_str(),
            Some("dep:horizon-official")
        );
        let main = main();
        let register = main
            .find("#[cfg(feature = \"official\")]\n    horizon_official::register();")
            .unwrap();
        assert!(register < main.find("entry::main_with(mods)").unwrap());
        fs::remove_dir_all(dir).unwrap();
    }

    /// A staged workspace with horizon-client, as `stage` expects it.
    fn workspace(dir: &Path) -> std::path::PathBuf {
        let stage_dir = dir.join("workspace");
        fs::create_dir_all(stage_dir.join("crates/horizon-client")).unwrap();
        fs::write(stage_dir.join("Cargo.toml"), "[workspace]\nmembers = []\n").unwrap();
        fs::write(
            stage_dir.join("crates/horizon-client/Cargo.toml"),
            "[package]\nname = 'horizon-client'\nversion = '0.1.0'\n",
        )
        .unwrap();
        stage_dir
    }

    #[test]
    fn stages_api_depending_on_a_bundle_crate() {
        let dir = fixture("[mod]\nid = 'example'\nversion = '1'\n[api]\npath = 'engine'\n[[crate]]\nid = 'shared'\npath = 'shared'\n");
        fs::create_dir_all(dir.join("shared")).unwrap();
        fs::write(
            dir.join("shared/Cargo.toml"),
            "[package]\nname = 'example-shared'\nversion = '0.1.0'\nedition = '2024'\n",
        )
        .unwrap();
        fs::write(dir.join("engine/Cargo.toml"), "[package]\nname = 'example-mod'\nversion = '0.1.0'\nedition = '2024'\n[dependencies]\nhorizon-client = '0.1'\nshared = { path = '../shared', package = 'example-shared' }\n").unwrap();
        let bundle = super::super::bundle(&dir, None).unwrap();
        let stage_dir = workspace(&dir);
        let resolution = super::super::dependencies::Resolution { order: vec![0] };
        assert!(stage(&stage_dir, &[bundle], &[vec![]], &resolution).unwrap());
        let engine = read(&stage_dir.join("native-api/example/Cargo.toml")).unwrap();
        assert_eq!(
            engine["dependencies"]["shared"]["path"].as_str(),
            Some("../../native-crates/example/shared")
        );
        assert_eq!(
            engine["dependencies"]["shared"]["package"].as_str(),
            Some("example-shared")
        );
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn refuses_a_sibling_folder_that_is_no_bundle_crate() {
        let dir = fixture("[mod]\nid = 'example'\nversion = '1'\n[api]\npath = 'engine'\n");
        fs::create_dir_all(dir.join("other")).unwrap();
        fs::write(
            dir.join("other/Cargo.toml"),
            "[package]\nname = 'example-other'\nversion = '0.1.0'\nedition = '2024'\n",
        )
        .unwrap();
        fs::write(dir.join("engine/Cargo.toml"), "[package]\nname = 'example-mod'\nversion = '0.1.0'\nedition = '2024'\n[dependencies]\nhorizon-client = '0.1'\nother = { path = '../other', package = 'example-other' }\n").unwrap();
        let bundle = super::super::bundle(&dir, None).unwrap();
        let stage_dir = workspace(&dir);
        let resolution = super::super::dependencies::Resolution { order: vec![0] };
        let error = stage(&stage_dir, &[bundle], &[vec![]], &resolution).unwrap_err();
        assert!(
            error.contains("is not a [[crate]] of this bundle"),
            "{error}"
        );
        fs::remove_dir_all(dir).unwrap();
    }
}
