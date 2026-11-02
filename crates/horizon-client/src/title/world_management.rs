//! World-list operations. Metadata edits preserve every other NBT field;
//! backups use vanilla's ZIP layout and never include the session lock.
use crate::save::{read_dat, write_dat};
use horizon_world::nbt::Tag;
use std::{
    fs, io,
    path::{Path, PathBuf},
};

fn world_path(saves: &Path, folder: &str) -> crate::Result<PathBuf> {
    if Path::new(folder).components().count() != 1
        || !matches!(
            Path::new(folder).components().next(),
            Some(std::path::Component::Normal(_))
        )
    {
        return Err("Invalid world folder".into());
    }
    let path = saves.join(folder);
    if !fs::symlink_metadata(&path)?.is_dir() {
        return Err("World folder is not a directory".into());
    }
    Ok(path)
}

/// EditWorldScreen holds a validated LevelStorageAccess while editing or
/// backing up. File-command callers need the same ownership for the operation.
fn world_access(saves: &Path, folder: &str) -> crate::Result<(PathBuf, crate::save::SessionLock)> {
    let world = world_path(saves, folder)?;
    let lock =
        crate::save::SessionLock::acquire_validated(&world, &saves.join("allowed_symlinks.txt"))?;
    Ok((world, lock))
}

pub fn rename(saves: &Path, folder: &str, name: &str) -> crate::Result<()> {
    let (world, _lock) = world_access(saves, folder)?;
    let path = world.join("level.dat");
    let mut root = read_dat(&path).ok_or("Could not read level.dat")?;
    let Some(Tag::Compound(data)) = root.get_mut("Data") else {
        return Err("Missing level data".into());
    };
    data.insert("LevelName".into(), Tag::String(name.trim().into()));
    write_dat(&path, root)
}

pub(super) fn reset_icon(saves: &Path, folder: &str) -> crate::Result<()> {
    let (world, _lock) = world_access(saves, folder)?;
    fs::remove_file(world.join("icon.png"))?;
    Ok(())
}

/// `LevelStorageAccess.deleteLevel`. The world's
/// session lock is taken first, so a world open in this or another instance is
/// refused rather than pulled from under it; everything but the lock goes,
/// then the lock is released and removed with the folder. A failed pass (a
/// file briefly held by a virus scanner, say) is retried up to five times,
/// half a second apart, as vanilla does. Only the named folder directly in
/// `saves` is touched: symbolic links inside it are removed, never followed.
pub fn delete(saves: &Path, folder: &str) -> crate::Result<()> {
    let world = world_path(saves, folder)?;
    let lock = crate::save::SessionLock::acquire(&world)?;
    let lock_path = world.join("session.lock");
    log::info!("Deleting level {folder}");
    for attempt in 1.. {
        match remove_contents(&world, &lock_path) {
            Ok(()) => break,
            Err(e) if attempt >= 5 => return Err(e.into()),
            Err(e) => {
                log::warn!("Failed to delete {}: {e}", world.display());
                std::thread::sleep(std::time::Duration::from_millis(500));
            }
        }
    }
    drop(lock);
    match fs::remove_file(&lock_path) {
        Err(e) if e.kind() != io::ErrorKind::NotFound => return Err(e.into()),
        _ => {}
    }
    fs::remove_dir(&world)?;
    Ok(())
}

fn remove_contents(world: &Path, lock: &Path) -> io::Result<()> {
    for entry in fs::read_dir(world)? {
        let entry = entry?;
        let path = entry.path();
        if path == lock {
            continue;
        }
        // `file_type` does not follow links, and `remove_dir_all` never does.
        if entry.file_type()?.is_dir() {
            fs::remove_dir_all(&path)?;
        } else {
            fs::remove_file(&path)?;
        }
    }
    Ok(())
}

pub fn backups_path(saves: &Path) -> PathBuf {
    saves.parent().unwrap_or(saves).join("backups")
}

pub(super) fn open_folder(path: &Path) -> crate::Result<()> {
    fs::create_dir_all(path)?;
    let path = fs::canonicalize(path)?;
    let program = if cfg!(target_os = "windows") {
        "explorer"
    } else if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    };
    // Reap the desktop opener without blocking the UI thread.
    let mut child = std::process::Command::new(program)
        .arg(path)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()?;
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}

pub fn backup(saves: &Path, folder: &str) -> crate::Result<u64> {
    let (world, _lock) = world_access(saves, folder)?;
    let backups = backups_path(saves);
    fs::create_dir_all(&backups)?;
    let base = format!(
        "{}_{}",
        chrono::Local::now().format("%Y-%m-%d_%H-%M-%S"),
        folder
    );
    let mut index = 0;
    let (path, file) = loop {
        let name = if index == 0 {
            format!("{base}.zip")
        } else {
            format!("{base} ({index}).zip")
        };
        let path = backups.join(name);
        match fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
        {
            Ok(file) => break (path, file),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => index += 1,
            Err(e) => return Err(e.into()),
        }
    };
    let result = (|| {
        let mut zip = zip::ZipWriter::new(file);
        write_tree(&mut zip, &world, &world, folder)?;
        Ok(zip.finish()?.metadata()?.len())
    })();
    if result.is_err() {
        let _ = fs::remove_file(path);
    }
    result
}

fn write_tree(
    zip: &mut zip::ZipWriter<fs::File>,
    root: &Path,
    dir: &Path,
    folder: &str,
) -> crate::Result<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        let kind = entry.file_type()?;
        if kind.is_symlink() {
            return Err(format!("Cannot back up symbolic link: {}", path.display()).into());
        }
        if kind.is_dir() {
            write_tree(zip, root, &path, folder)?;
        } else if kind.is_file() && entry.file_name() != "session.lock" {
            let relative = path.strip_prefix(root)?;
            let name = format!("{folder}/{}", relative.to_string_lossy().replace('\\', "/"));
            zip.start_file(
                name,
                zip::write::SimpleFileOptions::default()
                    .compression_method(zip::CompressionMethod::Deflated),
            )?;
            io::copy(&mut fs::File::open(path)?, zip)?;
        }
    }
    Ok(())
}
