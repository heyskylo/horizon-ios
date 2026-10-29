//! `cargo horizon-mod registry build|review|check|list`: the registry repository's CI
//! (`horizon-rust/horizon-mods`, docs/api/mod-index.md). Network-free: the
//! workflow downloads the files and publishes what this writes.
use horizon_mods::index::{
    FILE_HOST,
    registry::{self, Sources},
};
use std::path::Path;

/// Where the registry's site is published (its icons).
const SITE: &str = "https://horizon-rust.github.io/horizon-mods/";

const USAGE: &str = "usage: cargo horizon-mod registry build <checkout> --files <dir> --out <index.json> [--release <url>] [--site <url>] [--generated <time>]
       cargo horizon-mod registry review <base checkout> <head checkout> --author <login> [--json]
       cargo horizon-mod registry check <base checkout> <head checkout> --files <dir>
       cargo horizon-mod registry list <checkout>";

fn flag<'a>(args: &'a [String], name: &str) -> Option<&'a str> {
    super::flag(args, name)
}

fn positional(args: &[String]) -> Vec<&str> {
    let mut out = Vec::new();
    let mut skip = false;
    for a in args {
        if skip {
            skip = false;
        } else if a == "--json" {
        } else if a.starts_with("--") {
            skip = true;
        } else {
            out.push(a.as_str());
        }
    }
    out
}

fn lines(errors: Vec<String>) -> String {
    format!("{} problem(s):\n  {}", errors.len(), errors.join("\n  "))
}

pub fn main(args: &[String]) -> Result<(), String> {
    let rest = args.get(1..).unwrap_or_default();
    let pos = positional(rest);
    match args.first().map(String::as_str) {
        Some("build") => {
            let [checkout] = pos[..] else {
                return Err(USAGE.into());
            };
            let (files, out) = (flag(rest, "--files"), flag(rest, "--out"));
            let (Some(files), Some(out)) = (files, out) else {
                return Err(USAGE.into());
            };
            let generated = flag(rest, "--generated")
                .map(str::to_owned)
                .or_else(|| std::env::var("REGISTRY_GENERATED").ok())
                .unwrap_or_default();
            let index = registry::build(&Sources {
                registry: Path::new(checkout),
                files: Path::new(files),
                release: flag(rest, "--release").unwrap_or(FILE_HOST),
                site: flag(rest, "--site").unwrap_or(SITE),
                generated,
            })
            .map_err(lines)?;
            let text = serde_json::to_string_pretty(&index).map_err(|e| e.to_string())?;
            std::fs::write(out, text + "\n").map_err(|e| format!("{out}: {e}"))?;
            println!(
                "Wrote {out}: {} project(s), {} taken down",
                index.projects.len(),
                index.revoked.len()
            );
            Ok(())
        }
        Some("review") => {
            let [base, head] = pos[..] else {
                return Err(USAGE.into());
            };
            let author = flag(rest, "--author").ok_or(USAGE)?;
            let (base, _) = registry::load(Path::new(base)).map_err(lines)?;
            let (head, _) = registry::load(Path::new(head)).map_err(lines)?;
            let review = registry::review(&base, &head, author);
            if rest.iter().any(|a| a == "--json") {
                let json = serde_json::json!({"errors": review.errors, "notes": review.notes});
                println!("{json}");
            } else {
                for n in &review.notes {
                    println!("note: {n}");
                }
                for e in &review.errors {
                    println!("error: {e}");
                }
            }
            if review.errors.is_empty() {
                Ok(())
            } else {
                Err(format!(
                    "{} change(s) the registry refuses",
                    review.errors.len()
                ))
            }
        }
        // `check` (and its trial run) on each file `head` lists and `base`
        // doesn't: a pull request's new files.
        Some("check") => {
            let [base, head] = pos[..] else {
                return Err(USAGE.into());
            };
            let files = Path::new(flag(rest, "--files").ok_or(USAGE)?);
            let old: Vec<String> = match registry::load(Path::new(base)) {
                Ok((entries, _)) => entries
                    .iter()
                    .flat_map(|e| e.versions.iter().map(|v| v.sha256.clone()))
                    .collect(),
                // The first pull request has no entries to compare with.
                Err(_) => Vec::new(),
            };
            let (entries, _) = registry::load(Path::new(head)).map_err(lines)?;
            let mut failed = 0;
            for e in &entries {
                for v in e.versions.iter().filter(|v| !old.contains(&v.sha256)) {
                    println!("{} {} ({}):", e.id, v.version, v.file);
                    let report = crate::check::check(
                        &files.join(&v.sha256),
                        e.kind.clone(),
                        e.key.as_deref(),
                        v.signature.as_deref(),
                    );
                    match report {
                        Ok(r) => print!("{}", r.text()),
                        Err(err) => {
                            failed += 1;
                            println!("  error: {err}");
                        }
                    }
                }
            }
            if failed == 0 {
                Ok(())
            } else {
                Err(format!("{failed} file(s) failed their check"))
            }
        }
        // The files CI must download for `build`: `<sha256> <id> <version> <file>`.
        Some("list") => {
            let [checkout] = pos[..] else {
                return Err(USAGE.into());
            };
            let (entries, _) = registry::load(Path::new(checkout)).map_err(lines)?;
            for e in &entries {
                for v in &e.versions {
                    println!("{} {} {} {}", v.sha256, e.id, v.version, v.file);
                }
            }
            Ok(())
        }
        _ => Err(USAGE.into()),
    }
}
