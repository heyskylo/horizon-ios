//! `horizon-shader`: make Horizon shader packs (docs/shaders.md).
//!
//! - `new <dir> [--template post|sky|material] [--name "Name"]`: a pack from a template.
//! - `check <pack>... [--json]`: the check the game, the launcher and the registry run.
//! - `pack <dir> [--out <file.hshader>]`: check, then write the reproducible `.hshader`.
use horizon_shader_api::{EXTENSION, MANIFEST, Pack, Problem, check, package::sha256_hex};
use serde::Serialize;
use std::{path::Path, process::ExitCode};

const USAGE: &str = "usage: horizon-shader new <dir> [--template post|sky|material] [--name \"Name\"] | check <pack>... [--json] | pack <dir> [--out <file.hshader>]";

/// (template, its files as (name, text)).
const TEMPLATES: [(&str, [(&str, &str); 2]); 3] = [
    (
        "post",
        [
            (
                MANIFEST,
                include_str!("../../templates/post/horizon-shaders.toml"),
            ),
            ("post.wgsl", include_str!("../../templates/post/post.wgsl")),
        ],
    ),
    (
        "sky",
        [
            (
                MANIFEST,
                include_str!("../../templates/sky/horizon-shaders.toml"),
            ),
            ("sky.wgsl", include_str!("../../templates/sky/sky.wgsl")),
        ],
    ),
    (
        "material",
        [
            (
                MANIFEST,
                include_str!("../../templates/material/horizon-shaders.toml"),
            ),
            (
                "material.wgsl",
                include_str!("../../templates/material/material.wgsl"),
            ),
        ],
    ),
];

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        Some("new") => new(&args[1..]),
        Some("check") => check_command(&args[1..]),
        Some("pack") => pack(&args[1..]),
        _ => Err(USAGE.into()),
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

/// The arguments that are neither flags nor their values.
fn positional(args: &[String], flags: &[&str]) -> Vec<String> {
    let mut out = Vec::new();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        if flags.contains(&a.as_str()) {
            it.next();
        } else if !a.starts_with("--") {
            out.push(a.clone());
        }
    }
    out
}

fn new(args: &[String]) -> Result<(), String> {
    let [dir] = positional(args, &["--template", "--name"])
        .try_into()
        .map_err(|_| USAGE.to_string())?;
    let dir = Path::new(&dir);
    if dir.exists() {
        return Err(format!("{} already exists", dir.display()));
    }
    let template = flag(args, "--template").unwrap_or("post");
    let (_, files) = TEMPLATES
        .iter()
        .find(|(name, _)| *name == template)
        .ok_or_else(|| format!("no template {template:?}: post, sky or material"))?;
    let base = dir
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or("the path needs a folder name")?;
    let mut id: String = base
        .chars()
        .map(|c| match c {
            'a'..='z' | '0'..='9' => c,
            'A'..='Z' => c.to_ascii_lowercase(),
            _ => '_',
        })
        .collect();
    id.truncate(64);
    let name = flag(args, "--name").map_or_else(|| base.replace(['-', '_'], " "), str::to_owned);
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    for (file, text) in files {
        let text = text
            .replace("{{id}}", &id)
            .replace("{{name}}", &name.replace('"', "'"));
        let path = dir.join(file);
        std::fs::write(&path, text).map_err(|e| format!("{}: {e}", path.display()))?;
    }
    println!(
        "Made {} from the {template} template. Edit it, then: horizon-shader check {}",
        dir.display(),
        dir.display()
    );
    Ok(())
}

#[derive(Serialize)]
struct StageJson {
    stage: &'static str,
    file: String,
    label: String,
    expressions: usize,
    texture_reads: usize,
}

#[derive(Serialize)]
struct ProblemJson {
    file: String,
    line: usize,
    column: usize,
    message: String,
}

#[derive(Serialize)]
struct CheckJson {
    path: String,
    ok: bool,
    id: Option<String>,
    version: Option<String>,
    api: Option<String>,
    /// The file's own SHA-256 (a `.hshader`), else the reproducible package's.
    sha256: Option<String>,
    stages: Vec<StageJson>,
    problems: Vec<ProblemJson>,
}

/// Reads and checks the pack at `path`.
fn check_one(path: &Path) -> CheckJson {
    let mut out = CheckJson {
        path: path.display().to_string(),
        ok: false,
        id: None,
        version: None,
        api: None,
        sha256: None,
        stages: Vec::new(),
        problems: Vec::new(),
    };
    let problem = |message: String| ProblemJson {
        file: path.display().to_string(),
        line: 0,
        column: 0,
        message,
    };
    let pack = match Pack::read(path) {
        Ok(p) => p,
        Err(e) => {
            out.problems.push(problem(e));
            return out;
        }
    };
    out.id = Some(pack.manifest.id.clone());
    out.version = Some(pack.manifest.version.clone());
    out.api = Some(pack.manifest.api.to_string());
    out.sha256 = if path.is_dir() {
        pack.sha256().ok()
    } else {
        std::fs::read(path).ok().map(|b| sha256_hex(&b))
    };
    match check(&pack) {
        Ok(report) => {
            out.ok = true;
            out.stages = report
                .stages
                .into_iter()
                .map(|s| StageJson {
                    stage: s.kind.name(),
                    file: s.file,
                    label: s.label,
                    expressions: s.expressions,
                    texture_reads: s.texture_reads,
                })
                .collect();
        }
        Err(problems) => {
            out.problems = problems
                .into_iter()
                .map(
                    |Problem {
                         file,
                         line,
                         column,
                         message,
                     }| ProblemJson {
                        file,
                        line,
                        column,
                        message,
                    },
                )
                .collect();
        }
    }
    out
}

fn check_command(args: &[String]) -> Result<(), String> {
    let paths = positional(args, &[]);
    if paths.is_empty() {
        return Err(USAGE.into());
    }
    let results: Vec<CheckJson> = paths.iter().map(|p| check_one(Path::new(p))).collect();
    let failed = results.iter().filter(|r| !r.ok).count();
    if args.iter().any(|a| a == "--json") {
        let json = serde_json::to_string_pretty(&results).map_err(|e| e.to_string())?;
        println!("{json}");
    } else {
        for r in &results {
            print_result(r);
        }
    }
    match failed {
        0 => Ok(()),
        n => Err(format!("{n} of {} packs failed the check", results.len())),
    }
}

fn print_result(r: &CheckJson) {
    let what = match (&r.id, &r.version, &r.api) {
        (Some(id), Some(v), Some(api)) => format!("{id} {v} (shader API {api})"),
        _ => r.path.clone(),
    };
    if !r.ok {
        println!("{what}: FAILED");
        for p in &r.problems {
            if p.line == 0 {
                println!("  {}: {}", p.file, p.message);
            } else {
                println!("  {}:{}:{}: {}", p.file, p.line, p.column, p.message);
            }
        }
        return;
    }
    println!("{what}: ok");
    for s in &r.stages {
        println!(
            "  {:<9} {:<28} {:>6} expressions, {} texture reads",
            s.stage, s.label, s.expressions, s.texture_reads
        );
    }
    if let Some(sha) = &r.sha256 {
        println!("  sha256 {sha}");
    }
}

fn pack(args: &[String]) -> Result<(), String> {
    let [dir] = positional(args, &["--out"])
        .try_into()
        .map_err(|_| USAGE.to_string())?;
    let dir = Path::new(&dir)
        .canonicalize()
        .map_err(|e| format!("{dir}: {e}"))?;
    if !dir.is_dir() {
        return Err(format!("{} is not a pack folder", dir.display()));
    }
    let dir = dir.as_path();
    let pack = Pack::read(dir)?;
    if let Err(problems) = check(&pack) {
        for p in &problems {
            eprintln!("{p}");
        }
        return Err(format!("{} failed the check", dir.display()));
    }
    let bytes = pack.package()?;
    let out = flag(args, "--out").map_or_else(
        || {
            let m = &pack.manifest;
            dir.with_file_name(format!("{}-{}.{EXTENSION}", m.id, m.version))
        },
        Into::into,
    );
    std::fs::write(&out, &bytes).map_err(|e| format!("{}: {e}", out.display()))?;
    println!("Wrote {} (sha256 {})", out.display(), sha256_hex(&bytes));
    Ok(())
}
