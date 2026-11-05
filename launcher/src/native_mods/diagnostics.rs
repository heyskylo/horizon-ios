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

#[derive(Default)]
struct Functions(Vec<(Range<usize>, String)>);

impl<'ast> Visit<'ast> for Functions {
    fn visit_item_fn(&mut self, function: &'ast syn::ItemFn) {
        self.0.push((
            function.span().byte_range(),
            function.to_token_stream().to_string(),
        ));
        syn::visit::visit_item_fn(self, function);
    }
}

/// A file being touched is insufficient: the entire helper must match and its owner be unique.
fn owner(compiler: &Compiler<'_>, plan: &Value, manifest: &Value, span: &Value) -> Option<String> {
    let stage = compiler.stage.canonicalize().ok()?;
    let file = Path::new(span["file_name"].as_str()?);
    let start = usize::try_from(span["byte_start"].as_u64()?).ok()?;
    let end = usize::try_from(span["byte_end"].as_u64()?).ok()?;
    if start >= end {
        return None;
    }
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
    let entries = plan["injections"].as_array()?;
    let mut owners = BTreeSet::new();
    for file in files {
        let source = fs::read_to_string(&file).ok()?;
        let syntax = syn::parse_file(&source).ok()?;
        let mut functions = Functions::default();
        functions.visit_file(&syntax);
        let pristine =
            fs::read_to_string(compiler.source.join(file.strip_prefix(&stage).ok()?)).ok()?;
        let mut original = Functions::default();
        original.visit_file(&syn::parse_file(&pristine).ok()?);
        for entry in entries {
            let (Some(id), Some(version), Some(path), Some(helper)) = (
                entry["mod_id"].as_str(),
                entry["mod_version"].as_str(),
                entry["file"].as_str(),
                entry["helper_source"].as_str(),
            ) else {
                continue;
            };
            if !compiler.mods.iter().any(|(m, v)| m == id && v == version)
                || stage.join(path).canonicalize().ok().as_ref() != Some(&file)
            {
                continue;
            }
            let Ok(helper) = syn::parse_str::<syn::ItemFn>(helper) else {
                continue;
            };
            let tokens = helper.to_token_stream().to_string();
            if original.0.iter().any(|(_, body)| *body == tokens) {
                continue;
            }
            if functions
                .0
                .iter()
                .any(|(range, body)| range.start <= start && end <= range.end && *body == tokens)
            {
                owners.insert(format!("{id} {version}"));
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
                super::super::run_with_compiler(
                    command,
                    "Compiling the fixture",
                    |_| {},
                    Some(Compiler {
                        stage: &dir,
                        source: &dir.join("original"),
                        mods: &mods,
                    }),
                )
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
}
