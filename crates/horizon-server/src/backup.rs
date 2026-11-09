//! Backups and restore (docs/server.md, section 7). A backup is one zip in `backups/`: the world folder and
//! the server's settings and lists. On a running server the game first saves everything and stops writing
//! (`save-off`, `save-all`), and writes again once the zip is done (`save-on`), so players stay online.
//! A restore puts the backup's world in place of the current one, which is moved aside, never deleted.

use crate::config::Config;
use std::{
    fs::File,
    io::{BufWriter, Write},
    path::{Path, PathBuf},
    time::SystemTime,
};
use zip::{CompressionMethod, ZipArchive, ZipWriter, write::SimpleFileOptions};

pub const DIR: &str = "backups";
/// The settings and lists a backup holds beside the world.
const FILES: [&str; 6] = [
    "server.toml",
    "server.properties",
    "ops.json",
    "whitelist.json",
    "banned-players.json",
    "banned-ips.json",
];
/// Automatic backups are named `<time>-auto.zip`; only those are pruned.
const AUTO: &str = "auto";

/// `2026-10-16_18-30-05`, in UTC: sorts by time, and is valid in a file name everywhere.
fn stamp(time: SystemTime) -> String {
    let secs = time
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64);
    let (y, m, d) =
        horizon_platform::server_files::java_properties::civil_from_days(secs.div_euclid(86_400));
    let s = secs.rem_euclid(86_400);
    format!(
        "{y:04}-{m:02}-{d:02}_{:02}-{:02}-{:02}",
        s / 3600,
        s / 60 % 60,
        s % 60
    )
}

/// A backup's file name: the time, and a name the owner gave (letters, digits, `-` and `_` only).
pub fn file_name(time: SystemTime, name: Option<&str>) -> Result<String, String> {
    let name = name.unwrap_or("manual");
    if name.is_empty()
        || name.len() > 64
        || !name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return Err(format!(
            "backup names use letters, digits, - and _ only (got {name:?})"
        ));
    }
    Ok(format!("{}-{name}.zip", stamp(time)))
}

/// Adds a folder to the zip, under `prefix`, skipping the game's session lock.
fn add_dir(zip: &mut ZipWriter<BufWriter<File>>, dir: &Path, prefix: &str) -> Result<(), String> {
    let options = SimpleFileOptions::default()
        .compression_method(CompressionMethod::Deflated)
        .large_file(true);
    let mut entries: Vec<_> = std::fs::read_dir(dir)
        .map_err(|e| format!("{}: {e}", dir.display()))?
        .flatten()
        .collect();
    entries.sort_by_key(|e| e.file_name());
    for entry in entries {
        let name = entry.file_name().to_string_lossy().into_owned();
        let path = entry.path();
        let inner = format!("{prefix}/{name}");
        if path.is_dir() {
            add_dir(zip, &path, &inner)?;
        } else if name != "session.lock" {
            zip.start_file(inner.as_str(), options)
                .map_err(|e| e.to_string())?;
            let mut file = File::open(&path).map_err(|e| format!("{}: {e}", path.display()))?;
            std::io::copy(&mut file, zip).map_err(|e| format!("{}: {e}", path.display()))?;
        }
    }
    Ok(())
}

/// Zips the world and the settings into `backups/<file_name>`. The zip is written under a temporary name
/// and renamed when complete, so a backup that looks finished is one.
pub fn create(dir: &Path, config: &Config, file_name: &str) -> Result<PathBuf, String> {
    let backups = dir.join(DIR);
    std::fs::create_dir_all(&backups).map_err(|e| format!("{}: {e}", backups.display()))?;
    let path = backups.join(file_name);
    let part = backups.join(format!("{file_name}.part"));
    let result = (|| {
        let file = File::create(&part).map_err(|e| format!("{}: {e}", part.display()))?;
        let mut zip = ZipWriter::new(BufWriter::new(file));
        let world = dir.join(&config.world.name);
        if world.is_dir() {
            add_dir(&mut zip, &world, &config.world.name)?;
        }
        let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
        for name in FILES {
            if let Ok(bytes) = std::fs::read(dir.join(name)) {
                zip.start_file(name, options).map_err(|e| e.to_string())?;
                zip.write_all(&bytes).map_err(|e| e.to_string())?;
            }
        }
        zip.finish()
            .map_err(|e| e.to_string())?
            .into_inner()
            .map_err(|e| e.to_string())?
            .sync_all()
            .map_err(|e| e.to_string())
    })();
    match result {
        Ok(()) => {
            std::fs::rename(&part, &path).map_err(|e| e.to_string())?;
            Ok(path)
        }
        Err(e) => {
            let _ = std::fs::remove_file(&part);
            Err(e)
        }
    }
}

pub struct Backup {
    pub name: String,
    pub bytes: u64,
}

/// The finished backups, newest first.
pub fn list(dir: &Path) -> Vec<Backup> {
    let mut out: Vec<Backup> = std::fs::read_dir(dir.join(DIR))
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            name.ends_with(".zip").then(|| Backup {
                bytes: e.metadata().map_or(0, |m| m.len()),
                name,
            })
        })
        .collect();
    out.sort_by(|a, b| b.name.cmp(&a.name));
    out
}

/// Deletes the oldest automatic backups beyond `keep`. Backups made by hand are never deleted.
pub fn prune(dir: &Path, keep: usize) -> Vec<String> {
    let auto_suffix = format!("-{AUTO}.zip");
    list(dir)
        .into_iter()
        .filter(|b| b.name.ends_with(&auto_suffix))
        .skip(keep)
        .filter(|b| std::fs::remove_file(dir.join(DIR).join(&b.name)).is_ok())
        .map(|b| b.name)
        .collect()
}

pub fn auto_name(time: SystemTime) -> String {
    file_name(time, Some(AUTO)).expect("the automatic name is valid")
}

/// The backup a name points at: a file in `backups/`, by its full name or the start of it.
pub fn find(dir: &Path, name: &str) -> Result<String, String> {
    let all = list(dir);
    if let Some(b) = all
        .iter()
        .find(|b| b.name == name || b.name == format!("{name}.zip"))
    {
        return Ok(b.name.clone());
    }
    let matches: Vec<&Backup> = all.iter().filter(|b| b.name.starts_with(name)).collect();
    match matches.as_slice() {
        [one] => return Ok(one.name.clone()),
        [] => {}
        _ => {
            return Err(format!(
                "{name:?} matches {} backups: be more precise",
                matches.len()
            ));
        }
    }
    // The name given to `backup NAME` (or `auto`, `manual`): the newest backup with it.
    let label = format!("-{name}.zip");
    all.iter()
        .find(|b| b.name.ends_with(&label))
        .map(|b| b.name.clone())
        .ok_or_else(|| {
            format!("no backup {name:?} in {DIR}/ (`horizon-server backups` lists them)")
        })
}

/// Puts the backup's world in place of the current one, which goes to `backups/replaced-<time>/`. The
/// server must be stopped. Returns where the old world went.
pub fn restore(dir: &Path, config: &Config, name: &str) -> Result<Option<PathBuf>, String> {
    let zip_path = dir.join(DIR).join(name);
    let scratch = dir.join(".horizon/restore");
    let _ = std::fs::remove_dir_all(&scratch);
    std::fs::create_dir_all(&scratch).map_err(|e| e.to_string())?;
    let file = File::open(&zip_path).map_err(|e| format!("{}: {e}", zip_path.display()))?;
    ZipArchive::new(file)
        .and_then(|mut z| z.extract(&scratch))
        .map_err(|e| format!("{name}: {e}"))?;
    let world = &config.world.name;
    let restored = scratch.join(world);
    if !restored.join("level.dat").is_file() {
        let _ = std::fs::remove_dir_all(&scratch);
        return Err(format!(
            "{name} holds no world named {world:?} (no {world}/level.dat)"
        ));
    }
    let current = dir.join(world);
    let moved = if current.exists() {
        let aside = dir
            .join(DIR)
            .join(format!("replaced-{}", stamp(SystemTime::now())));
        std::fs::create_dir_all(&aside).map_err(|e| e.to_string())?;
        let target = aside.join(world);
        std::fs::rename(&current, &target)
            .map_err(|e| format!("moving the current world aside: {e}"))?;
        Some(target)
    } else {
        None
    };
    std::fs::rename(&restored, &current)
        .map_err(|e| format!("putting the backup in place: {e}"))?;
    let _ = std::fs::remove_dir_all(&scratch);
    Ok(moved)
}

/// A dimension id (`minecraft:the_nether`, or `the_nether` for `minecraft:`) as the id and its folder in the
/// world (`dimensions/<namespace>/<path>`, as the game saves it). Only the characters of a resource id.
pub fn dimension_folder(dimension: &str) -> Result<(String, String), String> {
    let (namespace, path) = dimension
        .split_once(':')
        .unwrap_or(("minecraft", dimension));
    let namespace_ok = !namespace.is_empty()
        && namespace
            .chars()
            .all(|c| matches!(c, 'a'..='z' | '0'..='9' | '_' | '-' | '.'));
    let path_ok = !path.is_empty()
        && path
            .split('/')
            .all(|s| !s.is_empty() && s != "." && s != "..")
        && path
            .chars()
            .all(|c| matches!(c, 'a'..='z' | '0'..='9' | '_' | '-' | '.' | '/'));
    if !namespace_ok || !path_ok {
        return Err(format!(
            "{dimension:?} is not a dimension id (like minecraft:the_nether)"
        ));
    }
    Ok((
        format!("{namespace}:{path}"),
        format!("dimensions/{namespace}/{path}"),
    ))
}

/// Puts one dimension of the backup in place of the current one, leaving the rest of the world (the other
/// dimensions, `level.dat`, the players) as it is. The current dimension goes to
/// `backups/replaced-<time>/<world>/dimensions/...`. The server must be stopped. Returns where the old
/// folder went (None when the world had none).
pub fn restore_dimension(
    dir: &Path,
    config: &Config,
    name: &str,
    dimension: &str,
) -> Result<Option<PathBuf>, String> {
    let (id, folder) = dimension_folder(dimension)?;
    let world = &config.world.name;
    let prefix = format!("{world}/{folder}/");
    let zip_path = dir.join(DIR).join(name);
    let scratch = dir.join(".horizon/restore");
    let _ = std::fs::remove_dir_all(&scratch);
    std::fs::create_dir_all(&scratch).map_err(|e| e.to_string())?;
    let result = (|| {
        let file = File::open(&zip_path).map_err(|e| format!("{}: {e}", zip_path.display()))?;
        let mut zip = ZipArchive::new(file).map_err(|e| format!("{name}: {e}"))?;
        let mut files = 0;
        for i in 0..zip.len() {
            let mut entry = zip.by_index(i).map_err(|e| format!("{name}: {e}"))?;
            let Some(inner) = entry.enclosed_name() else {
                continue;
            };
            if !inner.starts_with(&prefix) {
                continue;
            }
            let out = scratch.join(&inner);
            if entry.is_dir() {
                std::fs::create_dir_all(&out).map_err(|e| e.to_string())?;
                continue;
            }
            if let Some(parent) = out.parent() {
                std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            }
            let mut dest = File::create(&out).map_err(|e| format!("{}: {e}", out.display()))?;
            std::io::copy(&mut entry, &mut dest).map_err(|e| format!("{name}: {e}"))?;
            files += 1;
        }
        if files == 0 {
            return Err(format!(
                "{name} holds nothing of the dimension {id} (no {world}/{folder}/)"
            ));
        }
        Ok(())
    })();
    if let Err(e) = result {
        let _ = std::fs::remove_dir_all(&scratch);
        return Err(e);
    }
    let restored = scratch.join(world).join(&folder);
    let current = dir.join(world).join(&folder);
    let moved = if current.exists() {
        let target = dir
            .join(DIR)
            .join(format!("replaced-{}", stamp(SystemTime::now())))
            .join(world)
            .join(&folder);
        if target.exists() {
            let _ = std::fs::remove_dir_all(&scratch);
            return Err(format!(
                "{} exists already; try again in a second",
                target.display()
            ));
        }
        std::fs::create_dir_all(target.parent().expect("the folder has a parent"))
            .map_err(|e| e.to_string())?;
        std::fs::rename(&current, &target)
            .map_err(|e| format!("moving the current {id} aside: {e}"))?;
        Some(target)
    } else {
        None
    };
    let put = (|| {
        std::fs::create_dir_all(current.parent().expect("the folder has a parent"))?;
        std::fs::rename(&restored, &current)
    })();
    if let Err(e) = put {
        // Put the old folder back: the world must not be left without the dimension.
        if let Some(old) = &moved {
            let _ = std::fs::rename(old, &current);
        }
        let _ = std::fs::remove_dir_all(&scratch);
        return Err(format!("putting the backup's {id} in place: {e}"));
    }
    let _ = std::fs::remove_dir_all(&scratch);
    Ok(moved)
}

/// `every` from server.toml: `"6h"`, `"30m"`, `"1d"`, or `"0"` for no automatic backups.
pub fn interval(every: &str) -> Result<Option<std::time::Duration>, String> {
    let every = every.trim();
    if every == "0" || every.is_empty() {
        return Ok(None);
    }
    let (number, unit) = every.split_at(every.len() - 1);
    let n: u64 = number.trim().parse().map_err(|_| {
        format!("backups.every is {every:?}: use a number and m, h or d, like \"6h\"")
    })?;
    let secs = match unit {
        "m" => 60,
        "h" => 3600,
        "d" => 86_400,
        _ => {
            return Err(format!(
                "backups.every is {every:?}: use a number and m, h or d, like \"6h\""
            ));
        }
    };
    Ok((n > 0).then(|| std::time::Duration::from_secs(n * secs)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_sort_by_time() {
        let t = SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_792_000_000);
        assert_eq!(
            file_name(t, Some("before-update")).unwrap(),
            "2026-10-14_17-46-40-before-update.zip"
        );
        assert!(file_name(t, Some("../x")).is_err());
        assert_eq!(auto_name(t), "2026-10-14_17-46-40-auto.zip");
    }

    #[test]
    fn intervals() {
        assert_eq!(
            interval("6h").unwrap(),
            Some(std::time::Duration::from_secs(21_600))
        );
        assert_eq!(
            interval("30m").unwrap(),
            Some(std::time::Duration::from_secs(1800))
        );
        assert_eq!(interval("0").unwrap(), None);
        assert!(interval("6 hours").is_err());
    }

    #[test]
    fn back_up_and_restore() {
        let dir =
            std::env::temp_dir().join(format!("horizon-server-backup-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let config = Config::default();
        let world = dir.join("world");
        std::fs::create_dir_all(world.join("region")).unwrap();
        std::fs::write(world.join("level.dat"), b"before").unwrap();
        std::fs::write(world.join("region/r.0.0.mca"), vec![7u8; 10_000]).unwrap();
        std::fs::write(world.join("session.lock"), b"x").unwrap();
        std::fs::write(dir.join("server.toml"), crate::config::TEMPLATE).unwrap();
        let t = SystemTime::now();
        let name = file_name(t, None).unwrap();
        create(&dir, &config, &name).unwrap();
        assert_eq!(list(&dir).len(), 1);
        // The world changes, then the backup comes back.
        std::fs::write(world.join("level.dat"), b"after").unwrap();
        let moved = restore(&dir, &config, &find(&dir, &name[..10]).unwrap())
            .unwrap()
            .unwrap();
        assert_eq!(std::fs::read(world.join("level.dat")).unwrap(), b"before");
        assert_eq!(
            std::fs::read(world.join("region/r.0.0.mca")).unwrap().len(),
            10_000
        );
        assert!(!world.join("session.lock").exists());
        assert_eq!(std::fs::read(moved.join("level.dat")).unwrap(), b"after");
        // Only automatic backups are pruned.
        for i in 0..3 {
            let later = t + std::time::Duration::from_secs(10 + i);
            create(&dir, &config, &auto_name(later)).unwrap();
        }
        assert_eq!(prune(&dir, 1).len(), 2);
        let left: Vec<String> = list(&dir).into_iter().map(|b| b.name).collect();
        assert_eq!(left.len(), 2);
        assert!(left.iter().any(|n| n.ends_with("-manual.zip")));
        // A backup is found by its name, or the newest with a label.
        assert_eq!(find(&dir, "manual").unwrap(), name);
        assert_eq!(find(&dir, &left[0]).unwrap(), left[0]);
        assert_eq!(
            find(&dir, left[0].trim_end_matches(".zip")).unwrap(),
            left[0]
        );
        assert_eq!(find(&dir, "auto").unwrap(), left[0]);
        assert!(find(&dir, "nothing").is_err());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn restore_one_dimension_only() {
        let dir = std::env::temp_dir().join(format!("horizon-server-dim-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let config = Config::default();
        let world = dir.join("world");
        let overworld = world.join("dimensions/minecraft/overworld/region/r.0.0.mca");
        let nether = world.join("dimensions/minecraft/the_nether/region/r.0.0.mca");
        for (file, bytes) in [(&overworld, "ow-before"), (&nether, "nether-before")] {
            std::fs::create_dir_all(file.parent().unwrap()).unwrap();
            std::fs::write(file, bytes).unwrap();
        }
        std::fs::write(world.join("level.dat"), b"level-before").unwrap();
        let name = file_name(SystemTime::now(), None).unwrap();
        create(&dir, &config, &name).unwrap();
        std::fs::write(&overworld, "ow-after").unwrap();
        std::fs::write(&nether, "nether-after").unwrap();
        std::fs::write(world.join("level.dat"), b"level-after").unwrap();

        // Unknown or malformed dimensions change nothing.
        assert!(restore_dimension(&dir, &config, &name, "minecraft:the_end").is_err());
        assert!(restore_dimension(&dir, &config, &name, "../world").is_err());
        assert!(restore_dimension(&dir, &config, &name, "minecraft:../x").is_err());
        assert_eq!(std::fs::read(&nether).unwrap(), b"nether-after");

        // `the_nether` means `minecraft:the_nether`.
        let moved = restore_dimension(&dir, &config, &name, "the_nether")
            .unwrap()
            .unwrap();
        assert_eq!(std::fs::read(&nether).unwrap(), b"nether-before");
        assert_eq!(std::fs::read(&overworld).unwrap(), b"ow-after");
        assert_eq!(
            std::fs::read(world.join("level.dat")).unwrap(),
            b"level-after"
        );
        assert_eq!(
            std::fs::read(moved.join("region/r.0.0.mca")).unwrap(),
            b"nether-after"
        );
        assert!(!dir.join(".horizon/restore").exists());

        // A dimension the world lost comes back too, with nothing to put aside.
        std::fs::remove_dir_all(world.join("dimensions/minecraft/the_nether")).unwrap();
        assert!(
            restore_dimension(&dir, &config, &name, "minecraft:the_nether")
                .unwrap()
                .is_none()
        );
        assert_eq!(std::fs::read(&nether).unwrap(), b"nether-before");
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
