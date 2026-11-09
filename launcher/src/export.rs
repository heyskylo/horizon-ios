//! Sharing an instance (or a port): its settings, packs and Horizon mods, and the worlds the
//! player picks, as a `.mrpack` (Modrinth's modpack format, from its published spec) or a plain
//! `.zip` of the game folder's layout.
//!
//! Only what makes the instance goes in, from a fixed list ([`FILES`], [`FOLDERS`]): never logs,
//! screenshots, backups, caches, or anything tied to the player's account. A world goes without
//! its per-player files (`playerdata/`, `stats/`, `advancements/`) and its level.dat without the
//! player's UUID.
//!
//! In a `.mrpack`, a resource pack the browser installed from Modrinth, still the same file, is
//! listed by its download (`files`, with its SHA-1 and SHA-512) instead of being copied; the
//! rest goes under `overrides/`, which a launcher copies into the game folder.
use crate::browser::installed::{Entry, Installed};
use crate::browser::{FileRef, Kind, SourceId, GAME};
use horizon_platform::world::nbt::{self, Limits, Tag};
use sha2::Digest;
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

/// The game folder's own files that are the instance's settings.
pub const FILES: [&str; 6] = [
    "options.txt",
    "horizon-graphics.txt",
    "horizon-extras.txt",
    "horizon-controller.txt",
    "horizon-mods.txt",
    "servers.dat",
];
/// Its folders of packs, mods and their settings.
pub const FOLDERS: [&str; 4] = ["resourcepacks", "shaderpacks", "horizon-mods", "config"];
/// What a world keeps per player: left out.
const PER_PLAYER: [&str; 4] = ["playerdata", "stats", "advancements", "session.lock"];
const INDEX: &str = "modrinth.index.json";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    Mrpack,
    Zip,
}

impl Format {
    pub fn extension(self) -> &'static str {
        match self {
            Format::Mrpack => "mrpack",
            Format::Zip => "zip",
        }
    }
}

/// A file of the instance that its source serves, listed in a `.mrpack` rather than copied.
#[derive(Clone, Debug, PartialEq)]
pub struct Linked {
    /// Relative to the game folder: `resourcepacks/<name>`.
    pub path: String,
    pub file: FileRef,
}

/// What an export wrote.
#[derive(Debug, PartialEq)]
pub struct Exported {
    pub path: PathBuf,
    /// Files copied in (overrides, for a `.mrpack`).
    pub copied: usize,
    /// Files listed by their download.
    pub linked: usize,
    /// The worlds in it, by folder.
    pub worlds: Vec<String>,
}

/// Writes the instance whose game folder is `home` to `out`. `worlds` are folders of its
/// `saves/`; `linked` comes from [`linkable`] (empty for a zip).
pub fn export(
    home: &Path,
    name: &str,
    format: Format,
    worlds: &[String],
    linked: &[Linked],
    out: &Path,
) -> Result<Exported, String> {
    let mut files: Vec<(String, PathBuf)> = Vec::new();
    for f in FILES {
        if home.join(f).is_file() {
            files.push((f.to_string(), home.join(f)));
        }
    }
    for folder in FOLDERS {
        walk(home, &home.join(folder), &[], &mut files);
    }
    for world in worlds {
        let dir = home.join("saves").join(world);
        if world.contains(['/', '\\']) || !dir.join("level.dat").is_file() {
            return Err(format!("{name} has no world {world}."));
        }
        walk(home, &dir, &PER_PLAYER, &mut files);
    }
    let linked: Vec<&Linked> = match format {
        Format::Mrpack => linked.iter().collect(),
        Format::Zip => Vec::new(),
    };
    files.retain(|(rel, _)| !linked.iter().any(|l| l.path == *rel));
    if files.is_empty() && linked.is_empty() {
        return Err(format!(
            "{name} has nothing to share yet: no settings, packs, mods or worlds."
        ));
    }
    if let Some(dir) = out.parent() {
        fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    let part = out.with_extension("part");
    let written = write(&part, name, format, &files, &linked);
    let done = written.and_then(|()| fs::rename(&part, out).map_err(|e| e.to_string()));
    if let Err(e) = done {
        let _ = fs::remove_file(&part);
        return Err(format!("Could not export {name}: {e}"));
    }
    Ok(Exported {
        path: out.to_path_buf(),
        copied: files.len(),
        linked: linked.len(),
        worlds: worlds.to_vec(),
    })
}

fn write(
    path: &Path,
    name: &str,
    format: Format,
    files: &[(String, PathBuf)],
    linked: &[&Linked],
) -> Result<(), String> {
    let mut zip = zip::ZipWriter::new(fs::File::create(path).map_err(|e| e.to_string())?);
    let options = zip::write::SimpleFileOptions::default();
    let prefix = match format {
        Format::Mrpack => {
            zip.start_file(INDEX, options).map_err(|e| e.to_string())?;
            let index = serde_json::to_vec_pretty(&index(name, linked)).unwrap();
            zip.write_all(&index).map_err(|e| e.to_string())?;
            "overrides/"
        }
        Format::Zip => "",
    };
    for (rel, file) in files {
        let bytes = match rel.ends_with("/level.dat") {
            true => shareable_level(file)?,
            false => fs::read(file).map_err(|e| format!("{}: {e}", file.display()))?,
        };
        zip.start_file(format!("{prefix}{rel}"), options)
            .map_err(|e| e.to_string())?;
        zip.write_all(&bytes).map_err(|e| e.to_string())?;
    }
    zip.finish().map_err(|e| e.to_string())?;
    Ok(())
}

/// `modrinth.index.json`: format 1, the files by their downloads, and the Minecraft version.
fn index(name: &str, linked: &[&Linked]) -> serde_json::Value {
    let files: Vec<serde_json::Value> = linked
        .iter()
        .map(|l| {
            serde_json::json!({
                "path": l.path,
                "hashes": { "sha1": l.file.sha1, "sha512": l.file.sha512 },
                "env": { "client": "required", "server": "unsupported" },
                "downloads": [l.file.url],
                "fileSize": l.file.size,
            })
        })
        .collect();
    serde_json::json!({
        "formatVersion": 1,
        "game": "minecraft",
        "versionId": "1",
        "name": name,
        "summary": format!("{name}, exported from Horizon."),
        "files": files,
        "dependencies": { "minecraft": GAME },
    })
}

/// Every file under `dir`, relative to `home` with `/`, leaving out the names in `skip` at
/// `dir`'s top and dot files.
fn walk(home: &Path, dir: &Path, skip: &[&str], out: &mut Vec<(String, PathBuf)>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    let mut entries: Vec<_> = entries.flatten().collect();
    entries.sort_by_key(|e| e.file_name());
    for e in entries {
        let name = e.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') || skip.contains(&name.as_str()) {
            continue;
        }
        let path = e.path();
        if path.is_dir() {
            walk(home, &path, &[], out);
        } else if let Ok(rel) = path.strip_prefix(home) {
            out.push((rel.to_string_lossy().replace('\\', "/"), path));
        }
    }
}

/// A world's level.dat without the player's UUID; unreadable, it goes as it is.
fn shareable_level(file: &Path) -> Result<Vec<u8>, String> {
    let bytes = fs::read(file).map_err(|e| format!("{}: {e}", file.display()))?;
    let mut raw = Vec::new();
    if flate2::read::GzDecoder::new(&bytes[..])
        .take(64 << 20)
        .read_to_end(&mut raw)
        .is_err()
    {
        return Ok(bytes);
    }
    let Ok((mut root, _)) = nbt::read_named(&raw, Limits::default()) else {
        return Ok(bytes);
    };
    if let Some(Tag::Compound(data)) = root.tag.get_mut("Data") {
        if let Some(Tag::Compound(player)) = data.get_mut(&"Player".into()) {
            player.remove(&"UUID".into());
        }
    }
    let raw = nbt::write_named(&root, Limits::default()).map_err(|e| e.to_string())?;
    let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    gz.write_all(&raw).map_err(|e| e.to_string())?;
    gz.finish().map_err(|e| e.to_string())
}

/// The resource packs the browser installed from Modrinth that are still the file it checked:
/// with `lookup` (the source's file for an entry), what a `.mrpack` can list by download.
pub fn linkable(home: &Path, lookup: &dyn Fn(&Entry) -> Option<FileRef>) -> Vec<Linked> {
    Installed::load(home)
        .entries
        .iter()
        .filter(|e| e.source == SourceId::Modrinth && e.kind == Kind::ResourcePack)
        .filter(|e| {
            let Ok(bytes) = fs::read(home.join(&e.file)) else {
                return false;
            };
            let sum: String = sha2::Sha512::digest(&bytes)
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect();
            e.checksum.strip_prefix("SHA-512:") == Some(sum.as_str())
        })
        .filter_map(|e| {
            let file = lookup(e)?;
            let same = file.sha512.as_deref() == e.checksum.strip_prefix("SHA-512:");
            (same && file.sha1.is_some()).then(|| Linked {
                path: e.file.clone(),
                file,
            })
        })
        .collect()
}

/// An entry's file as Modrinth serves it (asks Modrinth: for [`linkable`]).
pub fn from_modrinth(e: &Entry) -> Option<FileRef> {
    let sum = e.checksum.strip_prefix("SHA-512:")?;
    crate::browser::modrinth::version_files(&e.version)
        .ok()?
        .into_iter()
        .find(|f| f.sha512.as_deref() == Some(sum))
}

/// A free path for `name.<extension>` in `dir`: "Name (2).mrpack" when taken.
pub fn free_path(dir: &Path, name: &str, format: Format) -> PathBuf {
    let ext = format.extension();
    let clean: String = name
        .chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '-',
            c => c,
        })
        .collect();
    (1..)
        .map(|n| match n {
            1 => dir.join(format!("{clean}.{ext}")),
            n => dir.join(format!("{clean} ({n}).{ext}")),
        })
        .find(|p| !p.exists())
        .unwrap()
}

#[cfg(test)]
#[path = "export/tests.rs"]
mod tests;
