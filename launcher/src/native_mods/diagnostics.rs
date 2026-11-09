//! Attribute compiler failures only when the staged plan identifies the injected helper.
use quote::ToTokens;
use serde_json::Value;
use std::{
    collections::BTreeSet,
    fs,
    io::{BufRead, Write},
    ops::Range,
    path::Path,
};
use syn::{spanned::Spanned, visit::Visit};

pub(super) struct Compiler<'a> {
    pub stage: &'a Path,
    pub source: &'a Path,
    pub mods: &'a [(String, String)],
}

#[derive(Default)]
pub(super) struct Output {
    errors: Vec<String>,
    owners: BTreeSet<String>,
}

impl Output {
    /// The mods the errors were traced to, as `id version`.
    pub(super) fn owners(&self) -> Vec<String> {
        self.owners.iter().cloned().collect()
    }

    pub(super) fn explain(self, error: String) -> String {
        let prefix = if self.owners.is_empty() {
            String::new()
        } else {
            format!(
                "{}:\n",
                self.owners.into_iter().collect::<Vec<_>>().join(", ")
            )
        };
        format!("{prefix}{}{error}", self.errors.join(""))
    }
}

/// Cargo's stdout is JSON; the log stays readable and stderr still drives build progress.
pub(super) fn capture(compiler: &Compiler<'_>, input: impl BufRead, mut log: impl Write) -> Output {
    let plan = fs::read(compiler.stage.join(".horizon-mixins-stage.json"))
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
        .filter(|p| p["schema"].as_u64() == Some(6));
    let mut output = Output::default();
    for line in input.lines().map_while(Result::ok) {
        let value = serde_json::from_str::<Value>(&line).ok();
        let Some(value) = value.filter(|v| v["reason"].is_string()) else {
            let _ = writeln!(log, "{line}");
            continue;
        };
        if value["reason"] != "compiler-message" {
            continue;
        }
        let message = &value["message"];
        let rendered = message["rendered"]
            .as_str()
            .or(message["message"].as_str())
            .unwrap_or_default();
        let _ = write!(log, "{rendered}");
        if message["level"] != "error" {
            continue;
        }
        if output.errors.len() < 4 {
            output.errors.push(rendered.to_owned());
        }
        // A mod's own package (its trait mod's `[api]` package, a mixin crate) is all the mod's:
        // an error there names it whatever the plan says, so a trait mod made for another commit
        // is left out like a helper that no longer compiles.
        if let Some(owner) = package_owner(compiler, &value["manifest_path"]) {
            output.owners.insert(owner);
            continue;
        }
        if let (Some(plan), Some(spans)) = (&plan, message["spans"].as_array()) {
            for span in spans.iter().filter(|s| s["is_primary"] == true) {
                if let Some(owner) = owner(compiler, plan, &value["manifest_path"], span) {
                    output.owners.insert(owner);
                }
            }
        }
    }
    output
}

/// The mod whose own package `manifest` is (`native-api/<id>/`, `native-crates/<id>/<crate>/` in
/// the stage), as `id version`; none for Horizon's crates.
fn package_owner(compiler: &Compiler<'_>, manifest: &Value) -> Option<String> {
    let manifest = Path::new(manifest.as_str()?).canonicalize().ok()?;
    let stage = compiler.stage.canonicalize().ok()?;
    let mut parts = manifest.strip_prefix(&stage).ok()?.components();
    let folder = parts.next()?.as_os_str();
    if folder != "native-api" && folder != "native-crates" {
        return None;
    }
    let id = parts.next()?.as_os_str().to_str()?;
    let (id, version) = compiler.mods.iter().find(|(m, _)| m == id)?;
    Some(format!("{id} {version}"))
}

/// A file's functions (where each is, its name and its tokens), inline modules (where, name),
/// and the blocks horizon-mixins wraps a helper in (where, the helper's name and tokens).
#[derive(Default)]
struct Items {
    functions: Vec<(Range<usize>, String, String)>,
    modules: Vec<(Range<usize>, String)>,
    calls: Vec<(Range<usize>, String, String)>,
}

impl<'ast> Visit<'ast> for Items {
    fn visit_item_fn(&mut self, function: &'ast syn::ItemFn) {
        self.functions.push((
            function.span().byte_range(),
            function.sig.ident.to_string(),
            function.to_token_stream().to_string(),
        ));
        syn::visit::visit_item_fn(self, function);
    }

    fn visit_item_mod(&mut self, module: &'ast syn::ItemMod) {
        if module.content.is_some() {
            self.modules
                .push((module.span().byte_range(), module.ident.to_string()));
        }
        syn::visit::visit_item_mod(self, module);
    }

    /// `{ fn helper(…) { … } helper(…) }` and nothing else: an error in the generated call (an
    /// argument the helper's signature no longer takes) is the helper's too.
    fn visit_block(&mut self, block: &'ast syn::Block) {
        if let [syn::Stmt::Item(syn::Item::Fn(function)), syn::Stmt::Expr(syn::Expr::Call(call), _)] =
            block.stmts.as_slice()
        {
            if matches!(&*call.func, syn::Expr::Path(path) if path.path.is_ident(&function.sig.ident))
            {
                self.calls.push((
                    block.span().byte_range(),
                    function.sig.ident.to_string(),
                    function.to_token_stream().to_string(),
                ));
            }
        }
        syn::visit::visit_block(self, block);
    }
}

/// A file being touched is insufficient: the error must be inside what a mod brought, and its
/// owner be unique. That is a helper (the function with its tokens, or with its name when the
/// pristine source has no function of that name: the staged file may be formatted, a trailing
/// comma added, which changes its tokens and not its name, with the call horizon-mixins generates
/// for it), or a support module (the inline module of its name, when the pristine source has none).
fn owner(compiler: &Compiler<'_>, plan: &Value, manifest: &Value, span: &Value) -> Option<String> {
    let stage = compiler.stage.canonicalize().ok()?;
    let file = Path::new(span["file_name"].as_str()?);
    let start = usize::try_from(span["byte_start"].as_u64()?).ok()?;
    let end = usize::try_from(span["byte_end"].as_u64()?).ok()?;
    if start >= end {
        return None;
    }
    let inside = |range: &Range<usize>| range.start <= start && end <= range.end;
    let mut files = BTreeSet::new();
    for base in [
        Some(stage.as_path()),
        manifest.as_str().and_then(|m| Path::new(m).parent()),
    ]
    .into_iter()
    .flatten()
    {
        if let Ok(path) = base.join(file).canonicalize() {
            if path.starts_with(&stage) {
                files.insert(path);
            }
        }
    }
    let empty = Vec::new();
    let injections = plan["injections"].as_array().unwrap_or(&empty);
    let modules = plan["modules"].as_array().unwrap_or(&empty);
    let mut owners = BTreeSet::new();
    for file in files {
        let source = fs::read_to_string(&file).ok()?;
        let syntax = syn::parse_file(&source).ok()?;
        let mut staged = Items::default();
        staged.visit_file(&syntax);
        let pristine =
            fs::read_to_string(compiler.source.join(file.strip_prefix(&stage).ok()?)).ok()?;
        let mut original = Items::default();
        original.visit_file(&syn::parse_file(&pristine).ok()?);
        // The plan's entries of the mods built, in this file.
        let ours = |entry: &&Value| {
            let (Some(id), Some(version), Some(path)) = (
                entry["mod_id"].as_str(),
                entry["mod_version"].as_str(),
                entry["file"].as_str(),
            ) else {
                return false;
            };
            compiler.mods.iter().any(|(m, v)| m == id && v == version)
                && stage.join(path).canonicalize().ok().as_ref() == Some(&file)
        };
        let owner = |entry: &Value| {
            format!(
                "{} {}",
                entry["mod_id"].as_str().unwrap_or_default(),
                entry["mod_version"].as_str().unwrap_or_default()
            )
        };
        for entry in injections.iter().filter(ours) {
            let Some(Ok(helper)) = entry["helper_source"]
                .as_str()
                .map(syn::parse_str::<syn::ItemFn>)
            else {
                continue;
            };
            let tokens = helper.to_token_stream().to_string();
            let name = helper.sig.ident.to_string();
            let is_helper = |(_, other, body): &(Range<usize>, String, String)| {
                *body == tokens || *other == name
            };
            if original.functions.iter().any(is_helper) {
                continue;
            }
            if staged
                .functions
                .iter()
                .chain(&staged.calls)
                .any(|f| inside(&f.0) && is_helper(f))
            {
                owners.insert(owner(entry));
            }
        }
        for entry in modules.iter().filter(ours) {
            let Some(name) = entry["name"].as_str() else {
                continue;
            };
            if original.modules.iter().any(|(_, other)| other == name) {
                continue;
            }
            if staged
                .modules
                .iter()
                .any(|(range, other)| other == name && inside(range))
            {
                owners.insert(owner(entry));
            }
        }
    }
    (owners.len() == 1)
        .then(|| owners.into_iter().next())
        .flatten()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::{io::Cursor, process::Command};

    const HELPER: &str = "fn replace() -> MeshTables { MeshTables { base: 1 } }";
    const SOURCE: &str = "// comentário: byte offsets include UTF-8\nstruct MeshTables { base: u8, animated_layers: u8 }\nfn main() {\nfn replace() -> MeshTables { MeshTables { base: 1 } }\nlet _ = replace();\n}\n";

    fn fixture(name: &str) -> std::path::PathBuf {
        let dir = super::super::tests::dir(name);
        fs::create_dir_all(dir.join("src")).unwrap();
        fs::write(dir.join("Cargo.toml"), "[workspace]\n[package]\nname = 'culprit-fixture'\nversion = '0.1.0'\nedition = '2021'\n").unwrap();
        fs::write(dir.join("src/main.rs"), SOURCE).unwrap();
        fs::create_dir_all(dir.join("original/src")).unwrap();
        fs::write(dir.join("original/src/main.rs"), "fn main() {}\n").unwrap();
        plan(&dir, json!([entry("fast_mesh", "0.1.0", HELPER)]));
        dir
    }

    fn entry(id: &str, version: &str, helper: &str) -> serde_json::Value {
        json!({"mod_id":id,"mod_version":version,"file":"src/main.rs","helper_source":helper})
    }

    fn plan(dir: &Path, entries: serde_json::Value) {
        fs::write(
            dir.join(".horizon-mixins-stage.json"),
            json!({"schema":6,"injections":entries}).to_string(),
        )
        .unwrap();
    }

    #[test]
    fn a_real_compiler_error_in_a_helper_names_its_mod_and_preserves_the_error() {
        let dir = fixture("compiler-culprit");
        let output = Command::new(env!("CARGO"))
            .args(["check", "--offline", "--message-format=json"])
            .arg("--manifest-path")
            .arg(dir.join("Cargo.toml"))
            .env("CARGO_TARGET_DIR", dir.join("target"))
            .output()
            .unwrap();
        assert!(!output.status.success());
        let stdout = String::from_utf8(output.stdout).unwrap();
        assert!(
            stdout.contains("E0063"),
            "{stdout}\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let mods = vec![("fast_mesh".to_owned(), "0.1.0".to_owned())];
        let compiler = Compiler {
            stage: &dir,
            source: &dir.join("original"),
            mods: &mods,
        };
        let original = String::from_utf8(output.stderr).unwrap();
        let mut log = Vec::new();
        let error = capture(&compiler, Cursor::new(&stdout), &mut log).explain(original.clone());
        assert!(error.contains("fast_mesh 0.1.0"), "{error}");
        assert!(error.contains("E0063"), "{error}");
        assert!(error.ends_with(&original));
        assert!(String::from_utf8(log).unwrap().contains("E0063"));

        // The actual process reader and attempt logger keep both the owner and rustc's message.
        {
            let _guard = super::super::tests::LOG_TEST.lock().unwrap();
            let mut command = Command::new(env!("CARGO"));
            command
                .args(["check", "--offline", "--message-format=json"])
                .arg("--manifest-path")
                .arg(dir.join("Cargo.toml"))
                .env("CARGO_TARGET_DIR", dir.join("target"));
            let error = super::super::logged(|| {
                super::super::run_checked(
                    command,
                    "Compiling the fixture",
                    |_| {},
                    Some(Compiler {
                        stage: &dir,
                        source: &dir.join("original"),
                        mods: &mods,
                    }),
                )
                .map_err(|ran| ran.error)
            })
            .unwrap_err();
            assert!(
                error.contains("fast_mesh 0.1.0") && error.contains("E0063"),
                "{error}"
            );
            let log = fs::read_to_string(super::super::log_path()).unwrap();
            assert!(
                log.contains("fast_mesh 0.1.0") && log.contains("E0063"),
                "{log}"
            );
            assert!(!log.contains("\"reason\":\"compiler-message\""), "{log}");
        }

        // Code already present in the pristine source is not proven to be injected.
        fs::write(dir.join("original/src/main.rs"), SOURCE).unwrap();
        assert!(!capture(&compiler, Cursor::new(&stdout), Vec::new())
            .explain(original.clone())
            .contains("fast_mesh"));
        fs::write(dir.join("original/src/main.rs"), "fn main() {}\n").unwrap();

        // Identical helpers owned by two mods cannot be attributed by guessing their name.
        let both = vec![mods[0].clone(), ("other_mesh".into(), "2.0.0".into())];
        plan(
            &dir,
            json!([
                entry("fast_mesh", "0.1.0", HELPER),
                entry("other_mesh", "2.0.0", HELPER)
            ]),
        );
        let error = capture(
            &Compiler {
                stage: &dir,
                source: &dir.join("original"),
                mods: &both,
            },
            Cursor::new(&stdout),
            Vec::new(),
        )
        .explain(original.clone());
        assert!(
            !error.contains("fast_mesh") && !error.contains("other_mesh"),
            "{error}"
        );
        assert!(error.contains("E0063") && error.ends_with(&original));

        // The staged file was formatted after the helper was planned (rustfmt added a trailing
        // comma): the helper is still found, by its name.
        plan(
            &dir,
            json!([entry(
                "fast_mesh",
                "0.1.0",
                "fn replace() -> MeshTables { MeshTables { base: 1, } }"
            )]),
        );
        let error = capture(&compiler, Cursor::new(&stdout), Vec::new()).explain(original.clone());
        assert!(error.contains("fast_mesh 0.1.0"), "{error}");

        // An error in a support module names the mod that declared it.
        fs::write(
            dir.join("src/main.rs"),
            "struct MeshTables { base: u8, animated_layers: u8 }\nfn main() {}\nmod fast_mesh_tables {\n    pub fn replace() -> super::MeshTables { super::MeshTables { base: 1 } }\n}\n",
        )
        .unwrap();
        fs::write(
            dir.join(".horizon-mixins-stage.json"),
            json!({"schema":6,"injections":[],"modules":[{"mod_id":"fast_mesh","mod_version":"0.1.0","file":"src/main.rs","name":"fast_mesh_tables"}]}).to_string(),
        )
        .unwrap();
        let in_module = Command::new(env!("CARGO"))
            .args(["check", "--offline", "--message-format=json"])
            .arg("--manifest-path")
            .arg(dir.join("Cargo.toml"))
            .env("CARGO_TARGET_DIR", dir.join("target"))
            .output()
            .unwrap();
        let output = capture(&compiler, Cursor::new(&in_module.stdout), Vec::new());
        assert_eq!(output.owners(), ["fast_mesh 0.1.0"]);
        fs::write(dir.join("src/main.rs"), SOURCE).unwrap();

        for metadata in ["not json", "{}", r#"{"schema":99,"injections":[]}"#] {
            fs::write(dir.join(".horizon-mixins-stage.json"), metadata).unwrap();
            let error =
                capture(&compiler, Cursor::new(&stdout), Vec::new()).explain(original.clone());
            assert!(!error.contains("fast_mesh"), "{error}");
            assert!(error.contains("E0063") && error.ends_with(&original));
        }
        plan(&dir, json!([entry("fast_mesh", "0.0.1", HELPER)]));
        assert!(!capture(&compiler, Cursor::new(&stdout), Vec::new())
            .explain(original.clone())
            .contains("fast_mesh"));

        // The same compile with its missing field restored is a successful control.
        fs::write(
            dir.join("src/main.rs"),
            SOURCE.replace("base: 1 }", "base: 1, animated_layers: 0 }"),
        )
        .unwrap();
        let output = Command::new(env!("CARGO"))
            .args(["check", "--offline", "--message-format=json"])
            .arg("--manifest-path")
            .arg(dir.join("Cargo.toml"))
            .env("CARGO_TARGET_DIR", dir.join("target"))
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let error =
            capture(&compiler, Cursor::new(output.stdout), Vec::new()).explain("unchanged".into());
        assert_eq!(error, "unchanged");

        // The same E0063 in ordinary game code, in a file the mod touched, names no mod.
        plan(&dir, json!([entry("fast_mesh", "0.1.0", HELPER)]));
        fs::write(dir.join("src/main.rs"), "struct MeshTables { base: u8, animated_layers: u8 }\nfn main() { let _ = MeshTables { base: 1 }; }\n").unwrap();
        let output = Command::new(env!("CARGO"))
            .args(["check", "--offline", "--message-format=json"])
            .arg("--manifest-path")
            .arg(dir.join("Cargo.toml"))
            .env("CARGO_TARGET_DIR", dir.join("target"))
            .output()
            .unwrap();
        assert!(!output.status.success());
        let error = capture(&compiler, Cursor::new(output.stdout), Vec::new())
            .explain("original failure".into());
        assert!(
            error.contains("E0063") && error.ends_with("original failure"),
            "{error}"
        );
        assert!(!error.contains("fast_mesh"), "{error}");
        fs::remove_dir_all(dir).unwrap();
    }

    /// A trait mod pinned to a fork's commit (its `[api]` package uses hooks the base Nightly
    /// lacks): the error is in the mod's own package, so the mod is named and left out, and the
    /// build can move to the commit it pins. Horizon's own crates name no one.
    #[test]
    fn an_error_in_a_mods_own_package_names_the_mod() {
        let dir = super::super::tests::dir("compiler-package-owner");
        for package in [
            "native-api/quests",
            "native-crates/quests/shared",
            "crates/horizon-client",
        ] {
            fs::create_dir_all(dir.join(package)).unwrap();
            fs::write(dir.join(package).join("Cargo.toml"), "").unwrap();
        }
        let mods = vec![("quests".to_owned(), "0.1.0".to_owned())];
        let compiler = Compiler {
            stage: &dir,
            source: &dir.join("original"),
            mods: &mods,
        };
        let line = |package: &str| {
            json!({
                "reason": "compiler-message",
                "manifest_path": dir.join(package).join("Cargo.toml"),
                "message": {"level": "error", "rendered": "error[E0432]: unresolved import\n", "spans": []},
            })
            .to_string()
        };
        for package in ["native-api/quests", "native-crates/quests/shared"] {
            let output = capture(&compiler, Cursor::new(line(package)), Vec::new());
            assert_eq!(output.owners(), ["quests 0.1.0"], "{package}");
        }
        let output = capture(
            &compiler,
            Cursor::new(line("crates/horizon-client")),
            Vec::new(),
        );
        assert!(output.owners().is_empty(), "{:?}", output.owners());
        fs::remove_dir_all(dir).unwrap();
    }

    /// flair 1.0.0 on Horizon 66f3981: Horizon widened `extra` to five-element tuples, so the
    /// call horizon-mixins generates for the mod's helper, outside the helper itself, fails.
    #[test]
    fn an_error_in_the_generated_call_to_a_helper_names_its_mod() {
        const FLAIR: &str = "fn flair_armor(extra: &[(u8, u8)]) -> usize { extra.len() }";
        let check = |dir: &Path, main: &str| {
            fs::write(dir.join("src/main.rs"), main).unwrap();
            let output = Command::new(env!("CARGO"))
                .args(["check", "--offline", "--message-format=json"])
                .arg("--manifest-path")
                .arg(dir.join("Cargo.toml"))
                .env("CARGO_TARGET_DIR", dir.join("target"))
                .output()
                .unwrap();
            let stdout = String::from_utf8(output.stdout).unwrap();
            assert!(stdout.contains("E0308"), "{stdout}");
            stdout
        };
        let dir = fixture("compiler-generated-call");
        fs::write(
            dir.join("original/src/main.rs"),
            "fn render(_extra: &[(u8, u8, bool)]) {}\nfn main() {}\n",
        )
        .unwrap();
        plan(&dir, json!([entry("flair", "1.0.0", FLAIR)]));
        let mods = vec![("flair".to_owned(), "1.0.0".to_owned())];
        let compiler = Compiler {
            stage: &dir,
            source: &dir.join("original"),
            mods: &mods,
        };
        let stdout = check(
            &dir,
            &format!("fn render(extra: &[(u8, u8, bool)]) {{\n    {{\n        {FLAIR}\n        flair_armor(extra)\n    }};\n}}\nfn main() {{ render(&[]); }}\n"),
        );
        let output = capture(&compiler, Cursor::new(&stdout), Vec::new());
        assert_eq!(output.owners(), ["flair 1.0.0"]);

        // The same call in a block that also holds the game's own code is not proven the mod's.
        let stdout = check(
            &dir,
            &format!("fn render(extra: &[(u8, u8, bool)]) {{\n    {{\n        {FLAIR}\n        let _ = 1;\n        flair_armor(extra)\n    }};\n}}\nfn main() {{ render(&[]); }}\n"),
        );
        let output = capture(&compiler, Cursor::new(&stdout), Vec::new());
        assert!(output.owners().is_empty(), "{:?}", output.owners());
        fs::remove_dir_all(dir).unwrap();
    }
}
