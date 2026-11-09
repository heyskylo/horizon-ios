//! A write-ahead region transaction. Originals survive every rollback attempt; never start the game
//! with an unfinished journal. A synced commit marker chooses the new state, otherwise recover old.
use super::{Bits, KINDS, STAGING, stage};
use serde::{Deserialize, Serialize};
use std::{fs, io, path::Path};

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Journal {
    name: String,
    kinds: Vec<String>,
}

fn sync_dir(path: &Path) -> io::Result<()> {
    #[cfg(unix)]
    fs::File::open(path)?.sync_all()?;
    // Windows does not offer directory fsync through std. File contents are flushed, and renames
    // use MOVEFILE_WRITE_THROUGH below. Recovery also covers ordinary process termination there.
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}

fn replace(from: &Path, to: &Path) -> io::Result<()> {
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        use windows_sys::Win32::Storage::FileSystem::{
            MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW,
        };
        // std adds the extended-length prefix for long paths; retain that behavior when calling
        // Win32 directly, including a destination that does not exist yet.
        let from = fs::canonicalize(from)?;
        let to = fs::canonicalize(to.parent().unwrap())?.join(to.file_name().unwrap());
        let from: Vec<_> = from.as_os_str().encode_wide().chain(Some(0)).collect();
        let to: Vec<_> = to.as_os_str().encode_wide().chain(Some(0)).collect();
        // Both paths are owned, nul-terminated buffers valid for this synchronous call.
        if unsafe {
            MoveFileExW(
                from.as_ptr(),
                to.as_ptr(),
                MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
    }
    #[cfg(not(windows))]
    fs::rename(from, to)?;
    if from.parent() != to.parent() {
        sync_dir(from.parent().unwrap())?;
    }
    sync_dir(to.parent().unwrap())
}

fn flush(path: &Path) -> io::Result<()> {
    fs::OpenOptions::new().write(true).open(path)?.sync_all()
}

fn clean(folder: &Path) -> io::Result<()> {
    // Detach the whole journal before deleting backups: cleanup must never leave a live journal
    // with only some of its originals after an interruption.
    let complete = folder.join(".horizon-prune-complete");
    if complete.try_exists()? {
        fs::remove_dir_all(&complete)?;
    }
    replace(&folder.join(STAGING), &complete)?;
    fs::remove_dir_all(complete)?;
    sync_dir(folder)
}

pub(super) fn recover(folder: &Path) -> io::Result<()> {
    recover_with(folder, &mut |_| Ok(()))
}

pub(super) fn recover_with(
    folder: &Path,
    checkpoint: &mut impl FnMut(&str) -> io::Result<()>,
) -> io::Result<()> {
    let complete = folder.join(".horizon-prune-complete");
    if complete.try_exists()? {
        fs::remove_dir_all(complete)?;
        sync_dir(folder)?;
    }
    let staged = folder.join(STAGING);
    if !staged.try_exists()? {
        return Ok(());
    }
    let journal = staged.join("journal.json");
    if !journal.try_exists()? {
        // No original file is touched until the complete journal is published.
        return clean(folder);
    }
    let journal: Journal = serde_json::from_slice(&fs::read(journal)?)?;
    let valid_name = horizon_platform::world::region::position_from_path(Path::new(&journal.name))
        .is_ok()
        && Path::new(&journal.name)
            .file_name()
            .is_some_and(|n| n == journal.name.as_str());
    if !valid_name
        || journal.kinds.is_empty()
        || journal.kinds.len() > KINDS.len()
        || journal.kinds.iter().any(|k| !KINDS.contains(&k.as_str()))
        || journal
            .kinds
            .iter()
            .enumerate()
            .any(|(i, k)| journal.kinds[..i].contains(k))
    {
        return Err(io::Error::other(
            "invalid prune journal; recovery files left in place",
        ));
    }
    let committed = match fs::read(staged.join("committed")) {
        Ok(bytes) if bytes == b"committed\n" => true,
        Ok(_) => {
            return Err(io::Error::other(
                "invalid prune commit marker; recovery files left in place",
            ));
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => false,
        Err(e) => return Err(e),
    };
    if !committed {
        // Preflight every backup before touching any destination. A damaged journal fails closed.
        for kind in &journal.kinds {
            fs::File::open(staged.join("old").join(kind).join(&journal.name))?;
        }
        for kind in &journal.kinds {
            let source = staged.join("old").join(kind).join(&journal.name);
            let restored = staged.join(format!("restore-{kind}"));
            fs::copy(&source, &restored)?;
            flush(&restored)?;
            replace(&restored, &folder.join(kind).join(&journal.name))?;
            checkpoint("restored")?;
        }
        // Keep the journal throughout rollback. An interruption here simply repeats that rollback.
    }
    checkpoint("recovered")?;
    clean(folder)
}

pub(super) fn apply(folder: &Path, name: &str, gone: &Bits) -> io::Result<()> {
    apply_with(folder, name, gone, &mut |_| Ok(()))
}

pub(super) fn apply_with(
    folder: &Path,
    name: &str,
    gone: &Bits,
    checkpoint: &mut impl FnMut(&str) -> io::Result<()>,
) -> io::Result<()> {
    recover(folder)?;
    let result = apply_inner(folder, name, gone, checkpoint);
    if let Err(error) = result {
        return match recover(folder) {
            Ok(()) => Err(error),
            Err(recovery) => Err(io::Error::other(format!(
                "{error}; rollback failed: {recovery}; keep recovery files in {}",
                folder.join(STAGING).display()
            ))),
        };
    }
    Ok(())
}

fn apply_inner(
    folder: &Path,
    name: &str,
    gone: &Bits,
    checkpoint: &mut impl FnMut(&str) -> io::Result<()>,
) -> io::Result<()> {
    let staged = folder.join(STAGING);
    fs::create_dir(&staged)?;
    sync_dir(folder)?;
    let mut journal = Journal {
        name: name.to_owned(),
        kinds: Vec::new(),
    };
    for kind in KINDS {
        let source = folder.join(kind).join(name);
        if !source.try_exists()? {
            continue;
        }
        journal.kinds.push(kind.to_owned());
        let old = staged.join("old").join(kind);
        let new = staged.join("new").join(kind);
        fs::create_dir_all(&old)?;
        fs::create_dir_all(&new)?;
        // Keep the original inode: we only replace/unlink destinations, never modify them in
        // place. This also rejects a symlinked entities/POI folder on another filesystem before
        // any destination changes; its replacements and rollback could not be atomic renames.
        fs::hard_link(&source, old.join(name))?;
        flush(&old.join(name))?;
        checkpoint("backup")?;
        stage(&source, &new.join(name), gone)?;
        checkpoint("staged")?;
        sync_dir(&old)?;
        sync_dir(&new)?;
    }
    for name in ["old", "new"] {
        sync_dir(&staged.join(name))?;
    }
    let temporary = staged.join("journal.tmp");
    fs::write(&temporary, serde_json::to_vec(&journal)?)?;
    flush(&temporary)?;
    replace(&temporary, &staged.join("journal.json"))?;
    checkpoint("journal")?;
    for kind in &journal.kinds {
        let new = staged.join("new").join(kind).join(name);
        let destination = folder.join(kind).join(name);
        if new.try_exists()? {
            replace(&new, &destination)?;
        } else {
            // Keep removal as a write-through rename on Windows too. The journal still retains
            // its independent original link until the whole group commits.
            replace(&destination, &staged.join(format!("removed-{kind}")))?;
        }
        checkpoint("replaced")?;
    }
    let commit = staged.join("commit.tmp");
    fs::write(&commit, b"committed\n")?;
    flush(&commit)?;
    replace(&commit, &staged.join("committed"))?;
    checkpoint("committed")?;
    clean(folder)
}
