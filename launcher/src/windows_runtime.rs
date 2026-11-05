//! Keep the game's Windows executable path stable across installed versions.
//! A slot belongs to one game home; its lock is held until the child exits.
use sha2::{Digest, Sha256};
use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
use std::{fs, io, path::Path, process::Command};

const SOURCE: &str = "HORIZON_WINDOWS_RUNTIME_SOURCE";
const SLOT: &str = "HORIZON_WINDOWS_RUNTIME_SLOT";

pub(crate) fn mark(cmd: &mut Command, source: &Path) {
    cmd.env(SOURCE, source);
}

fn value<'a>(cmd: &'a Command, key: &str) -> Option<&'a std::ffi::OsStr> {
    cmd.get_envs().find(|(k, _)| *k == key).and_then(|(_, v)| v)
}

/// Called before adding a pre-launch command or wrapper, after applying the instance.
pub(crate) fn redirect(cmd: Command) -> Command {
    redirect_in(cmd, &crate::backend::data_dir().join("run"))
}

fn redirect_in(cmd: Command, root: &Path) -> Command {
    if value(&cmd, SOURCE).is_none() || value(&cmd, SLOT).is_some() {
        return cmd;
    }
    let home = value(&cmd, "HORIZON_HOME")
        .map(Path::new)
        .or(cmd.get_current_dir())
        .expect("the game command has a home");
    let home = fs::canonicalize(home).unwrap_or_else(|_| home.to_path_buf());
    let key = format!("{:x}", Sha256::digest(home.as_os_str().as_encoded_bytes()));
    let slot = root.join(key);
    let mut out = Command::new(slot.join("game").join(crate::backend::CLIENT));
    out.args(cmd.get_args());
    if let Some(dir) = cmd.get_current_dir() {
        out.current_dir(dir);
    }
    for (key, value) in cmd.get_envs() {
        match value {
            Some(value) => out.env(key, value),
            None => out.env_remove(key),
        };
    }
    out.env(SLOT, slot);
    out
}

/// Prepare under an exclusive lock. Never fall back to a version-specific executable:
/// a locked or incomplete slot is an error, rather than another firewall identity.
pub(crate) fn prepare(cmd: &Command) -> io::Result<Option<crate::backend::RuntimeGuard>> {
    prepare_in(cmd, &crate::backend::data_dir().join("run"))
}

fn prepare_in(cmd: &Command, root: &Path) -> io::Result<Option<crate::backend::RuntimeGuard>> {
    let (Some(source), Some(slot)) = (value(cmd, SOURCE), value(cmd, SLOT)) else {
        return Ok(None);
    };
    let slot = Path::new(slot);
    let valid_key = slot
        .file_name()
        .and_then(|n| n.to_str())
        .is_some_and(|n| n.len() == 64 && n.bytes().all(|b| b.is_ascii_hexdigit()));
    if slot.parent() != Some(root) || !valid_key {
        return Err(io::ErrorKind::InvalidInput.into());
    }
    stage(Path::new(source), slot).map(Some)
}

fn plain_directory(path: &Path) -> io::Result<()> {
    fs::create_dir_all(path)?;
    // A replaced runtime slot or game folder must never redirect cleanup to player data.
    if fs::symlink_metadata(path)?.file_attributes() & 0x400 != 0 {
        return Err(io::ErrorKind::InvalidInput.into());
    }
    Ok(())
}

fn stage(source: &Path, slot: &Path) -> io::Result<crate::backend::RuntimeGuard> {
    plain_directory(slot)?;
    let lock = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(slot.join("run.lock"))?;
    lock.try_lock().map_err(io::Error::other)?;
    // Retain the original exe's protection against deletion/rebuild while playing.
    let source_file = fs::OpenOptions::new()
        .read(true)
        .share_mode(1)
        .open(source.join(crate::backend::CLIENT))?;
    let game = slot.join("game");
    plain_directory(&game)?;
    // Windows refuses this write while an orphaned game still executes the old binary,
    // even if its launcher (and thus the advisory lock) has already exited.
    fs::copy(
        source.join(crate::backend::CLIENT),
        game.join(crate::backend::CLIENT),
    )?;
    for entry in fs::read_dir(&game)? {
        let entry = entry?;
        if entry.file_name() == crate::backend::CLIENT {
            continue;
        }
        if entry.file_type()?.is_dir() {
            fs::remove_dir_all(entry.path())?;
        } else {
            fs::remove_file(entry.path())?;
        }
    }
    // Runtime files (DLLs, native-data and native-mods.json) stay beside the exe.
    // --resources still names the installed version's own resources.
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        if entry.file_name() == crate::backend::CLIENT || entry.file_name() == ".local" {
            continue;
        }
        let to = game.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            crate::backend::copy_dir(&entry.path(), &to)?;
        } else {
            fs::copy(entry.path(), to)?;
        }
    }
    fs::write(
        slot.join("source.json"),
        serde_json::to_vec(&fs::canonicalize(source)?)?,
    )?;
    Ok(crate::backend::RuntimeGuard {
        _slot: lock,
        _source: source_file,
    })
}

pub(crate) fn in_use(source: &Path) -> bool {
    in_use_in(source, &crate::backend::data_dir().join("run"))
}

fn in_use_in(source: &Path, root: &Path) -> bool {
    let source = fs::canonicalize(source).unwrap_or_else(|_| source.to_path_buf());
    let slots = match fs::read_dir(root) {
        Ok(slots) => slots,
        Err(e) => return e.kind() != io::ErrorKind::NotFound,
    };
    slots.filter_map(Result::ok).any(|slot| {
        let recorded = fs::read(slot.path().join("source.json"))
            .ok()
            .and_then(|bytes| serde_json::from_slice::<std::path::PathBuf>(&bytes).ok());
        if recorded.as_ref() != Some(&source) {
            return false;
        }
        let Ok(lock) = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(slot.path().join("run.lock"))
        else {
            return true;
        };
        if lock.try_lock().is_err() {
            return true;
        }
        // If the launcher exited first, Windows still protects the running image.
        fs::OpenOptions::new()
            .write(true)
            .open(slot.path().join("game").join(crate::backend::CLIENT))
            .is_err()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_orphaned_process_keeps_its_runtime_and_source_version_protected() {
        let root = std::env::temp_dir().join(format!("horizon-orphan-{}", std::process::id()));
        let source = root.join("version");
        fs::create_dir_all(&source).unwrap();
        fs::copy(
            Path::new(&std::env::var_os("SystemRoot").unwrap()).join("System32/cmd.exe"),
            source.join(crate::backend::CLIENT),
        )
        .unwrap();
        let mut cmd = Command::new(source.join(crate::backend::CLIENT));
        cmd.current_dir(&root).args(["/D", "/C", "set /p answer="]);
        mark(&mut cmd, &source);
        let mut cmd = redirect_in(cmd, &root.join("run"));
        let guard = prepare_in(&cmd, &root.join("run")).unwrap().unwrap();
        crate::process::hide_console(&mut cmd);
        let mut child = cmd
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::null())
            .spawn()
            .unwrap();
        drop(guard); // Simulate a launcher exiting before its game.
        let running = child.try_wait().unwrap().is_none();
        let protected = in_use_in(&source, &root.join("run"));
        let replacement = prepare_in(&cmd, &root.join("run"));
        let _ = child.kill();
        child.wait().unwrap();
        assert!(running && protected);
        assert!(replacement.is_err());
        assert!(!in_use_in(&source, &root.join("run")));
        drop(prepare_in(&cmd, &root.join("run")).unwrap());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn versions_reuse_a_slot_keep_native_files_and_cannot_replace_a_running_game() {
        let root = std::env::temp_dir().join(format!("horizon-runtime-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        let home = root.join("home");
        fs::create_dir_all(&home).unwrap();
        let command = |version: &str| {
            let source = root.join(version);
            fs::create_dir_all(source.join("native-data/mod")).unwrap();
            fs::write(source.join(crate::backend::CLIENT), version).unwrap();
            fs::write(source.join("native-data/mod/table"), version).unwrap();
            let mut cmd = Command::new(source.join(crate::backend::CLIENT));
            cmd.current_dir(&home)
                .env("HORIZON_HOME", &home)
                .arg("--offscreen");
            mark(&mut cmd, &source);
            redirect_in(cmd, &root.join("run"))
        };
        let first = command("v1");
        let lock = prepare_in(&first, &root.join("run")).unwrap().unwrap();
        assert!(in_use_in(&root.join("v1"), &root.join("run")));
        assert!(fs::remove_file(root.join("v1").join(crate::backend::CLIENT)).is_err());
        let game = Path::new(first.get_program()).parent().unwrap();
        fs::write(game.join("removed.dll"), "old").unwrap();
        let second = command("v2");
        assert_eq!(first.get_program(), second.get_program());
        assert!(prepare_in(&second, &root.join("run")).is_err());
        assert_eq!(fs::read_to_string(first.get_program()).unwrap(), "v1");
        drop(lock);
        assert!(!in_use_in(&root.join("v1"), &root.join("run")));
        let lock = prepare_in(&second, &root.join("run")).unwrap().unwrap();
        assert!(!in_use_in(&root.join("v1"), &root.join("run")));
        assert!(in_use_in(&root.join("v2"), &root.join("run")));
        assert_eq!(fs::read_to_string(second.get_program()).unwrap(), "v2");
        assert_eq!(
            fs::read_to_string(game.join("native-data/mod/table")).unwrap(),
            "v2"
        );
        assert!(!game.join("removed.dll").exists());
        assert_eq!(second.get_current_dir(), Some(home.as_path()));
        assert_eq!(second.get_args().collect::<Vec<_>>(), ["--offscreen"]);
        drop(lock);
        let unrelated = root.join("unrelated");
        fs::create_dir_all(&unrelated).unwrap();
        fs::write(unrelated.join("keep.txt"), "player data").unwrap();
        let mut injected = command("v3");
        injected.env(SLOT, &unrelated);
        assert!(prepare_in(&injected, &root.join("run")).is_err());
        assert_eq!(
            fs::read_to_string(unrelated.join("keep.txt")).unwrap(),
            "player data"
        );
        fs::remove_dir_all(root).unwrap();
    }
}
