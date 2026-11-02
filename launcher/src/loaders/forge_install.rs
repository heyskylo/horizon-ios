//! Forge and NeoForge installers.
//!
//! An installer (`<artifact>-<version>-installer.jar`, checked against the `.sha1` its repository
//! publishes) holds `install_profile.json` and the launcher profile it names (`json`). The profile
//! lists the libraries the game runs with, the install profile those its processors need, each
//! with a sha1, a URL, or neither when a processor makes it or the installer carries it under
//! `maven/`. The processors (`jar`, `classpath`, `args`, `outputs`) then patch the game's own jar:
//! each runs as `java -cp <jar and classpath> <the jar's Main-Class> <args>`, where `{KEY}` is the
//! install profile's `data` for the client (`[group:artifact:version]` a library's file, `'text'`
//! a literal, `/path` a file the installer carries) or one of `SIDE`, `MINECRAFT_JAR`,
//! `MINECRAFT_VERSION`, `ROOT`, `INSTALLER`, `LIBRARY_DIR`, and `[coordinates]` a library's file.
//! A processor whose `outputs` already have their sha1 is skipped; one that leaves an output with
//! another is an error.

use super::forge::artifact;
use super::{maven_path, Kind};
use crate::sources::{fetch_checked, sha1_file, write_file, Fetch};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Command;

/// The repository's name of `version` (as players name it: `47.3.0`) for `minecraft`:
/// `1.20.1-47.3.0` where versions carry the game's in front.
pub(super) fn maven_version(kind: Kind, minecraft: &str, version: &str) -> String {
    match artifact(kind, minecraft).2 {
        "neoforge" => version.to_string(),
        _ => format!("{minecraft}-{version}"),
    }
}

/// An installer, opened.
struct Installer {
    path: PathBuf,
    zip: zip::ZipArchive<fs::File>,
}

impl Installer {
    fn read(&mut self, name: &str) -> Option<Vec<u8>> {
        let mut entry = self.zip.by_name(name.trim_start_matches('/')).ok()?;
        let mut body = Vec::new();
        entry.read_to_end(&mut body).ok()?;
        Some(body)
    }

    fn json(&mut self, name: &str) -> Result<Value, String> {
        let body = self
            .read(name)
            .ok_or_else(|| format!("{}: no {name}", self.path.display()))?;
        serde_json::from_slice(&body).map_err(|e| format!("{}: {name}: {e}", self.path.display()))
    }
}

/// A library's file, its sha1 (none: unknown) and where it comes from (none: a processor makes
/// it or the installer carries it).
fn library_file(library: &Value) -> Result<(String, Option<String>, Option<String>), String> {
    let artifact = &library["downloads"]["artifact"];
    let path = match artifact["path"].as_str() {
        Some(path) => path.to_string(),
        None => maven_path(library["name"].as_str().ok_or("A library has no name")?)?,
    };
    let sha1 = artifact["sha1"]
        .as_str()
        .filter(|s| !s.is_empty())
        .map(String::from);
    let url = artifact["url"]
        .as_str()
        .filter(|u| !u.is_empty())
        .map(String::from);
    Ok((path, sha1, url))
}

/// Puts each library in `libraries/`: kept when its sha1 already matches, else from the
/// installer's `maven/`, else downloaded and checked. Returns those left for the processors.
fn fetch_libraries(
    libraries: &[Value],
    dir: &Path,
    installer: &mut Installer,
    fetch: Fetch,
) -> Result<Vec<(PathBuf, Option<String>)>, String> {
    let mut later = Vec::new();
    let mut downloads = Vec::new();
    for library in libraries {
        let (path, sha1, url) = library_file(library)?;
        let file = dir.join("libraries").join(crate::mojang::relative(&path)?);
        let have = sha1_file(&file);
        if have.is_some() && (sha1.is_none() || have == sha1) {
            continue;
        }
        if let Some(body) = installer.read(&format!("maven/{path}")) {
            if let Some(sha1) = sha1
                .as_ref()
                .filter(|s| **s != crate::sources::sha1_hex(&body))
            {
                return Err(format!(
                    "The installer's {path} is not the one listed ({sha1})"
                ));
            }
            write_file(&file, &body)?;
        } else if let Some(url) = url {
            downloads.push((file, url, sha1));
        } else {
            later.push((file, sha1));
        }
    }
    // A few at a time: Forge's own repository is slow file by file.
    let next = std::sync::atomic::AtomicUsize::new(0);
    let failed = std::sync::Mutex::new(None);
    std::thread::scope(|scope| {
        for _ in 0..downloads.len().min(8) {
            scope.spawn(|| loop {
                let i = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                let Some((file, url, sha1)) = downloads.get(i) else {
                    break;
                };
                let got = match sha1 {
                    Some(sha1) => fetch_checked(fetch, url, sha1),
                    None => fetch(url),
                }
                .map_err(|e| format!("{url} {e}"))
                .and_then(|body| write_file(file, &body));
                if let Err(e) = got {
                    failed.lock().unwrap().get_or_insert(e);
                }
            });
        }
    });
    match failed.into_inner().unwrap() {
        Some(e) => Err(e),
        None => Ok(later),
    }
}

/// A processor's value: `[coordinates]` a library's file, `'text'` the text, anything else
/// with each `{KEY}` replaced by `data`'s.
fn value(raw: &str, data: &HashMap<String, String>, libraries: &Path) -> Result<String, String> {
    if let Some(name) = raw.strip_prefix('[').and_then(|r| r.strip_suffix(']')) {
        let path = libraries.join(crate::mojang::relative(&maven_path(name)?)?);
        return Ok(path.to_string_lossy().into_owned());
    }
    if let Some(text) = raw.strip_prefix('\'').and_then(|r| r.strip_suffix('\'')) {
        return Ok(text.to_string());
    }
    let mut out = String::new();
    let mut rest = raw;
    while let Some(start) = rest.find('{') {
        let len = rest[start..]
            .find('}')
            .ok_or_else(|| format!("Bad processor argument {raw:?}"))?;
        let key = &rest[start + 1..start + len];
        let found = data
            .get(key)
            .ok_or_else(|| format!("A processor needs {{{key}}}, which the installer lacks"))?;
        out.push_str(&rest[..start]);
        out.push_str(found);
        rest = &rest[start + len + 1..];
    }
    out.push_str(rest);
    Ok(out)
}

/// The install profile's `data` for the client, and the built-in keys. A `/path` value is a
/// file the installer carries, unpacked into `scratch`.
fn data(
    profile: &Value,
    installer: &mut Installer,
    minecraft: &str,
    dir: &Path,
    scratch: &Path,
) -> Result<HashMap<String, String>, String> {
    let at = |p: &Path| p.to_string_lossy().into_owned();
    let jar = dir
        .join("versions")
        .join(minecraft)
        .join(format!("{minecraft}.jar"));
    let mut data = HashMap::from([
        ("SIDE".to_string(), "client".to_string()),
        ("MINECRAFT_JAR".into(), at(&jar)),
        ("MINECRAFT_VERSION".into(), minecraft.to_string()),
        ("ROOT".into(), at(dir)),
        ("INSTALLER".into(), at(&installer.path)),
        ("LIBRARY_DIR".into(), at(&dir.join("libraries"))),
    ]);
    let libraries = dir.join("libraries");
    for (key, sides) in profile["data"].as_object().into_iter().flatten() {
        let Some(raw) = sides["client"].as_str() else {
            continue;
        };
        let resolved = if raw.starts_with('/') {
            let body = installer
                .read(raw)
                .ok_or_else(|| format!("The installer has no {raw}"))?;
            let out = scratch.join(crate::mojang::relative(raw.trim_start_matches('/'))?);
            write_file(&out, &body)?;
            at(&out)
        } else {
            value(raw, &HashMap::new(), &libraries)?
        };
        data.insert(key.clone(), resolved);
    }
    Ok(data)
}

/// The `Main-Class` a jar's manifest names.
fn main_class(jar: &Path) -> Result<String, String> {
    let file = fs::File::open(jar).map_err(|e| format!("{}: {e}", jar.display()))?;
    let mut zip = zip::ZipArchive::new(file).map_err(|e| format!("{}: {e}", jar.display()))?;
    let mut manifest = String::new();
    zip.by_name("META-INF/MANIFEST.MF")
        .map_err(|e| e.to_string())
        .and_then(|mut m| m.read_to_string(&mut manifest).map_err(|e| e.to_string()))
        .map_err(|e| format!("{}: {e}", jar.display()))?;
    manifest
        .lines()
        .find_map(|l| l.strip_prefix("Main-Class:"))
        .map(|c| c.trim().to_string())
        .ok_or_else(|| format!("{} names no Main-Class", jar.display()))
}

/// Runs the client's processors on `java`.
fn run_processors(
    profile: &Value,
    data: &HashMap<String, String>,
    dir: &Path,
    java: &Path,
    log: &dyn Fn(String),
) -> Result<(), String> {
    let libraries = dir.join("libraries");
    let library = |name: &str| -> Result<PathBuf, String> {
        Ok(libraries.join(crate::mojang::relative(&maven_path(name)?)?))
    };
    let processors: Vec<&Value> = profile["processors"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|p| {
            p["sides"]
                .as_array()
                .is_none_or(|s| s.iter().any(|s| s == "client"))
        })
        .collect();
    for (n, processor) in processors.iter().enumerate() {
        let jar_name = processor["jar"]
            .as_str()
            .ok_or("A processor names no jar")?;
        let mut outputs = Vec::new();
        for (file, sha1) in processor["outputs"].as_object().into_iter().flatten() {
            let sha1 = sha1.as_str().ok_or("A processor's output has no sha1")?;
            outputs.push((
                PathBuf::from(value(file, data, &libraries)?),
                value(sha1, data, &libraries)?,
            ));
        }
        if !outputs.is_empty()
            && outputs
                .iter()
                .all(|(file, sha1)| sha1_file(file).as_ref() == Some(sha1))
        {
            continue;
        }
        let tool = jar_name.split(':').nth(1).unwrap_or(jar_name);
        log(format!(
            "Patching Minecraft ({}/{}: {tool})",
            n + 1,
            processors.len()
        ));
        let jar = library(jar_name)?;
        let mut classpath = vec![jar.clone()];
        for name in processor["classpath"].as_array().into_iter().flatten() {
            classpath.push(library(name.as_str().ok_or("Bad processor classpath")?)?);
        }
        let mut args = Vec::new();
        for arg in processor["args"].as_array().into_iter().flatten() {
            args.push(value(
                arg.as_str().ok_or("Bad processor argument")?,
                data,
                &libraries,
            )?);
        }
        let separator = if cfg!(windows) { ";" } else { ":" };
        let classpath: Vec<String> = classpath
            .iter()
            .map(|p| p.to_string_lossy().into_owned())
            .collect();
        let out = Command::new(java)
            .arg("-cp")
            .arg(classpath.join(separator))
            .arg(main_class(&jar)?)
            .args(&args)
            .current_dir(dir)
            .output()
            .map_err(|e| format!("{}: {e}", java.display()))?;
        if !out.status.success() {
            let said = String::from_utf8_lossy(&out.stderr).into_owned()
                + &String::from_utf8_lossy(&out.stdout);
            let tail: Vec<&str> = said.lines().rev().take(12).collect();
            let tail: Vec<&str> = tail.into_iter().rev().collect();
            return Err(format!(
                "{tool} failed ({}):\n{}",
                out.status,
                tail.join("\n")
            ));
        }
        for (file, sha1) in &outputs {
            let have = sha1_file(file);
            if have.as_ref() != Some(sha1) {
                let _ = fs::remove_file(file);
                return Err(format!(
                    "{tool} made {} with sha1 {}, not {sha1}",
                    file.display(),
                    have.unwrap_or_else(|| "(missing)".into())
                ));
            }
        }
    }
    Ok(())
}

/// Installs Forge or NeoForge `version` for `minecraft` into the game folder `dir`, whose
/// `versions/<minecraft>/` already holds the game, running its processors on `java`. Keeps its
/// profile in `versions/<id>/` as `id`, with a copy of the game's jar named after it (Forge's
/// `ignoreList` names it `${version_name}.jar`, as the official launcher lays it out).
pub(super) fn install(
    kind: Kind,
    version: &str,
    minecraft: &str,
    dir: &Path,
    java: &Path,
    fetch: Fetch,
    log: &dyn Fn(String),
) -> Result<String, String> {
    let name = kind.name();
    let (repository, group, artifact_name) = artifact(kind, minecraft);
    let full = maven_version(kind, minecraft, version);
    let path = format!("{group}/{artifact_name}/{full}/{artifact_name}-{full}-installer.jar");
    let url = format!("{repository}/{path}");
    let file = dir.join("libraries").join(crate::mojang::relative(&path)?);
    let sha1 = String::from_utf8_lossy(&fetch(&format!("{url}.sha1"))?)
        .split_whitespace()
        .next()
        .unwrap_or_default()
        .to_string();
    if sha1_file(&file).as_ref() != Some(&sha1) {
        log(format!("Downloading the {name} {version} installer"));
        let body = fetch_checked(fetch, &url, &sha1).map_err(|e| format!("{url} {e}"))?;
        write_file(&file, &body)?;
    }
    let zip = fs::File::open(&file)
        .map_err(|e| e.to_string())
        .and_then(|f| zip::ZipArchive::new(f).map_err(|e| e.to_string()))
        .map_err(|e| format!("{}: {e}", file.display()))?;
    let mut installer = Installer { path: file, zip };
    let profile = installer.json("install_profile.json")?;
    if profile.get("versionInfo").is_some() {
        return Err(format!(
            "{name} {version} has an installer from before Minecraft 1.12.2, which Horizon can't \
             run yet."
        ));
    }
    if profile["minecraft"] != minecraft {
        return Err(format!(
            "{name} {version} is for Minecraft {}, not {minecraft}",
            profile["minecraft"]
        ));
    }
    let mut game = installer.json(profile["json"].as_str().unwrap_or("/version.json"))?;
    let mut libraries: Vec<Value> = profile["libraries"].as_array().cloned().unwrap_or_default();
    libraries.extend(game["libraries"].as_array().cloned().unwrap_or_default());
    log(format!(
        "Downloading {name} {version} ({} libraries)",
        libraries.len()
    ));
    let made = fetch_libraries(&libraries, dir, &mut installer, fetch)?;
    let scratch = dir
        .join(".horizon-install")
        .join(format!("{}-{version}", kind.id()));
    let _ = fs::remove_dir_all(&scratch);
    let ran = data(&profile, &mut installer, minecraft, dir, &scratch)
        .and_then(|data| run_processors(&profile, &data, dir, java, log));
    let _ = fs::remove_dir_all(&scratch);
    ran?;
    for (file, sha1) in made {
        let have = sha1_file(&file);
        if have.is_none() || (sha1.is_some() && have != sha1) {
            return Err(format!(
                "{name}'s installer left {} missing or changed",
                file.display()
            ));
        }
    }
    // Each library named the way Mojang's are, so the launch finds it.
    for library in game["libraries"].as_array_mut().into_iter().flatten() {
        let (path, sha1, url) = library_file(library)?;
        library["downloads"] = json!({"artifact": {
            "path": path, "url": url.unwrap_or_default(), "sha1": sha1.unwrap_or_default()
        }});
    }
    let id = kind.profile_id(version, minecraft);
    game["id"] = id.clone().into();
    game["jar"] = id.clone().into();
    let versions = dir.join("versions").join(crate::mojang::relative(&id)?);
    let jar = versions.join(format!("{id}.jar"));
    let vanilla = dir
        .join("versions")
        .join(minecraft)
        .join(format!("{minecraft}.jar"));
    fs::create_dir_all(&versions)
        .and_then(|_| fs::copy(&vanilla, &jar))
        .map_err(|e| format!("{}: {e}", vanilla.display()))?;
    write_file(
        &versions.join(format!("{id}.json")),
        &serde_json::to_vec_pretty(&game).map_err(|e| e.to_string())?,
    )?;
    Ok(id)
}

#[cfg(test)]
#[path = "forge_install_tests.rs"]
mod tests;
