//! `cargo horizon-mod`: make Horizon mods in Rust (`docs/mods.md`).
//!
//! - `new <path> [--template code|content] [--name "Name"] [--sdk-path <dir>]`:
//!   a mod from a template: a Rust crate, or a content mod (no code).
//! - `build [--debug]`: the `.hmod` in `target/horizon-mod/`, with the mod's
//!   `assets/` and `data/`; a folder without `Cargo.toml` is content only.
//! - `run [--game <horizon-client>] [-- <game args>]`: build, then play with
//!   the mod; run `build` again and the game reloads it.
//! - `test [--debug] [--hmod <file>] [<name>]`: build, then run the mod's
//!   `tests/*.toml` scripts in the sandbox, with no game.
//! - `check <file> [--json]`: what the registry checks of a submission
//!   (`.hmod`, `.hpack`, resource pack `.zip`), with a trial run for a mod.
//! - `registry build|review|check|list`: the registry repository's CI.
use horizon_mods::{
    Capability, Content, Manifest, Package,
    manifest::{MANIFEST, component_imports, componentize},
    testing::{Script, TestGame},
};
mod check;
mod registry;

use std::{
    path::{Path, PathBuf},
    process::{Command, ExitCode, Stdio},
};

const TARGET: &str = "wasm32-unknown-unknown";
const SDK_GIT: &str = "{ git = \"https://github.com/horizon-rust/horizon\" }";

fn main() -> ExitCode {
    // Cargo runs `cargo-horizon-mod horizon-mod <args>`.
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().is_some_and(|a| a == "horizon-mod") {
        args.remove(0);
    }
    let result = match args.first().map(String::as_str) {
        Some("new") => new(&args[1..]),
        Some("build") => build(!args.iter().any(|a| a == "--debug")).map(|p| {
            println!("Built {}", p.display());
        }),
        Some("run") => run(&args[1..]),
        Some("test") => test(&args[1..]),
        Some("check") => check(&args[1..]),
        Some("registry") => registry::main(&args[1..]),
        _ => Err("usage: cargo horizon-mod new <path> [--template code|content] | build [--debug] | run [--game <horizon-client>] [-- <game args>] | test [--debug] [--hmod <file>] [<name>] | check <file> [--kind <kind>] [--key <public key> --signature <file.minisig>] [--json] | registry build|review|check|list".into()),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

/// The value after `flag` in `args`.
fn flag<'a>(args: &'a [String], flag: &str) -> Option<&'a str> {
    let i = args.iter().position(|a| a == flag)?;
    args.get(i + 1).map(String::as_str)
}

fn new(args: &[String]) -> Result<(), String> {
    let path = args
        .first()
        .filter(|a| !a.starts_with("--"))
        .ok_or("cargo horizon-mod new <path>")?;
    let dir = Path::new(path);
    if dir.exists() {
        return Err(format!("{} already exists", dir.display()));
    }
    let base = dir
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or("the path needs a folder name")?;
    let id: String = base
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '_'
            }
        })
        .collect();
    let name = flag(args, "--name").map_or_else(|| base.replace(['-', '_'], " "), str::to_string);
    let sdk = match flag(args, "--sdk-path") {
        Some(p) => {
            let p = std::fs::canonicalize(p).map_err(|e| format!("--sdk-path {p}: {e}"))?;
            format!("{{ path = {:?} }}", p.display().to_string())
        }
        None => SDK_GIT.to_string(),
    };
    let fill = |t: &str| {
        t.replace("{crate}", &id.replace('_', "-"))
            .replace("{id}", &id)
            .replace("{name}", &name)
            .replace("{sdk}", &sdk)
    };
    let code = [
        ("Cargo.toml", include_str!("../template/Cargo.toml.in")),
        (MANIFEST, include_str!("../template/horizon-mod.toml")),
        ("src/lib.rs", include_str!("../template/src/lib.rs")),
        (".gitignore", include_str!("../template/gitignore")),
        (
            "tests/hello.toml",
            include_str!("../template/tests/hello.toml"),
        ),
    ];
    // A block copying a vanilla one, with its model, name, loot, recipe and tag.
    let content = [
        (
            MANIFEST,
            include_str!("../template/content/horizon-mod.toml"),
        ),
        (
            "data/{id}/horizon/block/glowing_amethyst.json",
            include_str!("../template/content/block.json"),
        ),
        (
            "assets/{id}/blockstates/glowing_amethyst.json",
            include_str!("../template/content/blockstate.json"),
        ),
        (
            "assets/{id}/models/block/glowing_amethyst.json",
            include_str!("../template/content/model.json"),
        ),
        (
            "assets/{id}/items/glowing_amethyst.json",
            include_str!("../template/content/item.json"),
        ),
        (
            "assets/{id}/lang/en_us.json",
            include_str!("../template/content/lang.json"),
        ),
        (
            "data/{id}/loot_table/blocks/glowing_amethyst.json",
            include_str!("../template/content/loot.json"),
        ),
        (
            "data/{id}/recipe/glowing_amethyst.json",
            include_str!("../template/content/recipe.json"),
        ),
        (
            "data/minecraft/tags/block/mineable/pickaxe.json",
            include_str!("../template/content/pickaxe.json"),
        ),
        (".gitignore", include_str!("../template/gitignore")),
    ];
    let files: &[(&str, &str)] = match flag(args, "--template").unwrap_or("code") {
        "code" => &code,
        "content" => &content,
        other => return Err(format!("no template {other}: code or content")),
    };
    for &(file, text) in files {
        let file = fill(file);
        let file = file.as_str();
        let to = dir.join(file);
        std::fs::create_dir_all(to.parent().expect("a file in the mod"))
            .map_err(|e| e.to_string())?;
        std::fs::write(&to, fill(text)).map_err(|e| format!("{}: {e}", to.display()))?;
    }
    let manifest =
        Manifest::parse(&std::fs::read_to_string(dir.join(MANIFEST)).map_err(|e| e.to_string())?)?;
    Content::read(dir, &manifest)?;
    println!(
        "Created {} (mod id {id}). Next: cd {path} && cargo horizon-mod run",
        dir.display()
    );
    Ok(())
}

/// Builds the mod in the current folder and packs `target/horizon-mod/<id>.hmod`:
/// the manifest, the crate's component (none without a `Cargo.toml`: a
/// content mod) and the files in `assets/` and `data/`.
fn build(release: bool) -> Result<PathBuf, String> {
    build_in(Path::new("."), release)
}

fn build_in(root: &Path, release: bool) -> Result<PathBuf, String> {
    let manifest_text = std::fs::read_to_string(root.join(MANIFEST))
        .map_err(|e| format!("{MANIFEST}: {e} (run this in a mod's folder)"))?;
    let manifest = Manifest::parse(&manifest_text)?;
    let (wasm, target_dir) = if root.join("Cargo.toml").exists() {
        let (wasm, target) = compile(root, release)?;
        // The game refuses a component importing capabilities the manifest lacks.
        manifest.check_imports(component_imports(&wasm)?.iter().map(String::as_str))?;
        (Some(wasm), target)
    } else if manifest.capabilities.contains(&Capability::Registries) {
        (None, root.join("target"))
    } else {
        return Err(format!(
            "no Cargo.toml: only a content mod (capability registries in {MANIFEST}) has no code"
        ));
    };
    // The same checks as the game's, and the files in the order it lists them.
    let content = Content::read(root, &manifest)?;
    let mut reader = content.reader()?;
    let mut files = Vec::with_capacity(content.files.len());
    for name in &content.files {
        files.push((name.as_str(), reader.read(name)?));
    }
    let files: Vec<(&str, &[u8])> = files.iter().map(|(n, b)| (*n, b.as_slice())).collect();
    let dir = target_dir.join("horizon-mod");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let path = dir.join(format!("{}.hmod", manifest.id));
    // Write then rename, so a running game never reads half a file.
    let partial = path.with_extension("hmod.partial");
    let bytes = Package::write_files(&manifest_text, wasm.as_deref(), &files)?;
    std::fs::write(&partial, bytes).map_err(|e| e.to_string())?;
    std::fs::rename(&partial, &path).map_err(|e| e.to_string())?;
    Ok(path)
}

/// Builds the crate for WebAssembly: its component, and Cargo's target folder.
fn compile(root: &Path, release: bool) -> Result<(Vec<u8>, PathBuf), String> {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    let mut cmd = Command::new(cargo);
    cmd.current_dir(root).args([
        "build",
        "--lib",
        "--target",
        TARGET,
        "--message-format=json-render-diagnostics",
    ]);
    if release {
        cmd.arg("--release");
    }
    let out = cmd
        .stderr(Stdio::inherit())
        .output()
        .map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err(format!(
            "cargo build failed (is the target installed? rustup target add {TARGET})"
        ));
    }
    let core = String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
        .filter(|m| m["reason"] == "compiler-artifact")
        .flat_map(|m| m["filenames"].as_array().cloned().unwrap_or_default())
        .filter_map(|f| f.as_str().map(PathBuf::from))
        .rfind(|f| f.extension().is_some_and(|e| e == "wasm"))
        .ok_or("the build made no .wasm: is the crate a cdylib?")?;
    let wasm = componentize(&std::fs::read(&core).map_err(|e| e.to_string())?)?;
    let target = core.ancestors().nth(3).ok_or("unexpected target folder")?;
    Ok((wasm, target.to_path_buf()))
}

/// `check <file>`: the registry's checks of a submission, printed; fails
/// when one does.
fn check(args: &[String]) -> Result<(), String> {
    let file = args
        .first()
        .filter(|a| !a.starts_with("--"))
        .ok_or("cargo horizon-mod check <file>")?;
    let path = Path::new(file);
    let kind = match flag(args, "--kind") {
        Some(k) => horizon_mods::index::Kind::from(k.to_string()),
        None => check::kind_of(path)
            .ok_or("pass --kind mod|modpack|resourcepack: the extension doesn't say")?,
    };
    let signature = flag(args, "--signature")
        .map(|p| std::fs::read_to_string(p).map_err(|e| format!("{p}: {e}")))
        .transpose()?;
    let report = check::check(path, kind, flag(args, "--key"), signature.as_deref());
    if args.iter().any(|a| a == "--json") {
        let json = match &report {
            Ok(r) => serde_json::json!({"ok": true, "report": r.json()}),
            Err(e) => serde_json::json!({"ok": false, "error": e}),
        };
        println!("{json}");
    }
    let report = report?;
    if !args.iter().any(|a| a == "--json") {
        print!("{}", report.text());
    }
    Ok(())
}

/// `test`: every `tests/*.toml` script (those whose name holds `<name>`)
/// against the built mod, or the `.hmod` given with `--hmod`.
fn test(args: &[String]) -> Result<(), String> {
    let hmod = match flag(args, "--hmod") {
        Some(p) => PathBuf::from(p),
        None => build(!args.iter().any(|a| a == "--debug"))?,
    };
    let filter = args
        .iter()
        .enumerate()
        .find(|(i, a)| !a.starts_with("--") && (*i == 0 || args[i - 1] != "--hmod"))
        .map(|(_, a)| a.as_str());
    let mut scripts: Vec<PathBuf> = std::fs::read_dir("tests")
        .map_err(|e| {
            format!(
                "tests/: {e} (a mod's tests are tests/<name>.toml: docs/mods.md, \"Testing a mod\")"
            )
        })?
        .filter_map(|e| Some(e.ok()?.path()))
        .filter(|p| p.extension().is_some_and(|e| e == "toml"))
        .filter(|p| filter.is_none_or(|f| p.to_string_lossy().contains(f)))
        .collect();
    scripts.sort();
    if scripts.is_empty() {
        return Err("no tests/*.toml to run".into());
    }
    println!(
        "running {} scripts against {}",
        scripts.len(),
        hmod.display()
    );
    let mut failed = 0;
    for path in &scripts {
        let result = std::fs::read_to_string(path)
            .map_err(|e| e.to_string())
            .and_then(|text| Script::parse(&text))
            .and_then(|script| {
                let mut game = TestGame::load(&hmod)?;
                script.run(&mut game)?;
                Ok(script.steps.len())
            });
        match result {
            Ok(steps) => println!("test {} ... ok ({steps} steps)", path.display()),
            Err(e) => {
                failed += 1;
                println!("test {} ... FAILED\n  {e}", path.display());
            }
        }
    }
    let passed = scripts.len() - failed;
    if failed > 0 {
        return Err(format!("{failed} failed, {passed} passed"));
    }
    println!("test result: ok. {passed} passed");
    Ok(())
}

fn run(args: &[String]) -> Result<(), String> {
    let path = build(true)?;
    let game = flag(args, "--game")
        .map(str::to_string)
        .or_else(|| std::env::var("HORIZON_CLIENT").ok())
        .unwrap_or_else(|| "horizon-client".into());
    let extra = args.iter().skip_while(|a| *a != "--").skip(1);
    println!(
        "Starting {game} with {} (rebuild with `cargo horizon-mod build` to reload it)",
        path.display()
    );
    let status = Command::new(&game)
        .arg("--dev-mod")
        .arg(&path)
        .args(extra)
        .status()
        .map_err(|e| {
            format!("{game}: {e} (pass --game <path to horizon-client> or set HORIZON_CLIENT)")
        })?;
    status
        .success()
        .then_some(())
        .ok_or_else(|| format!("the game exited with {status}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_template_is_a_valid_mod() {
        let dir = std::env::temp_dir().join(format!("horizon-mod-new-{}", std::process::id()));
        let path = dir.join("My-Mod");
        new(&[path.display().to_string(), "--name".into(), "My Mod".into()]).unwrap();
        let manifest =
            Manifest::parse(&std::fs::read_to_string(path.join(MANIFEST)).unwrap()).unwrap();
        assert_eq!(
            (manifest.id.as_str(), manifest.name.as_str()),
            ("my_mod", "My Mod")
        );
        let cargo = std::fs::read_to_string(path.join("Cargo.toml")).unwrap();
        assert!(cargo.contains("name = \"my-mod\"") && cargo.contains(SDK_GIT));
        let script = std::fs::read_to_string(path.join("tests/hello.toml")).unwrap();
        assert_eq!(Script::parse(&script).unwrap().steps.len(), 3);
        assert!(
            new(&[path.display().to_string()]).is_err(),
            "never overwrites"
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn the_content_template_builds_and_checks_with_no_code() {
        let dir = std::env::temp_dir().join(format!("horizon-mod-content-{}", std::process::id()));
        let path = dir.join("gems");
        let new_args = [
            path.display().to_string(),
            "--template".into(),
            "content".into(),
        ];
        new(&new_args).unwrap();
        assert!(!path.join("Cargo.toml").exists());
        let hmod = build_in(&path, true).unwrap();
        assert_eq!(hmod, path.join("target/horizon-mod/gems.hmod"));
        let package = Package::read(&hmod).unwrap();
        assert!(package.wasm.is_none());
        let content = Content::read(&hmod, &package.manifest).unwrap();
        assert!(
            content
                .files
                .contains(&"data/gems/horizon/block/glowing_amethyst.json".into())
        );
        assert_eq!(content.files.len(), 8, "{:?}", content.files);
        let report = check::check(&hmod, check::kind_of(&hmod).unwrap(), None, None).unwrap();
        assert!(!report.code);
        assert!(
            report.text().contains("no code: content only"),
            "{}",
            report.text()
        );
        // Without registries, a mod needs code.
        let manifest = std::fs::read_to_string(path.join(MANIFEST)).unwrap();
        let manifest = manifest.replace("[\"registries\"]", "[]");
        std::fs::write(path.join(MANIFEST), manifest).unwrap();
        assert!(build_in(&path, true).unwrap_err().contains("no Cargo.toml"));
        assert!(
            new(&[
                dir.join("x").display().to_string(),
                "--template".into(),
                "lua".into()
            ])
            .is_err()
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
