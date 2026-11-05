//! Files only their owner may read: the control API's and the game's control endpoint (each holds a
//! token) and the management protocol's generated key. Written whole to a temporary file this
//! process creates with owner-only permissions, then renamed over the path, so the result never
//! keeps an old file's wider permissions and never writes through a link left at the path.
use std::io::{self, Write};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

/// Writes `text` to `path`, readable and writable by its owner only (0600 on Unix; on Windows the
/// folder's inherited permissions, as for any file). Replaces what was at `path`, a link included
/// (the link itself, not what it points to).
pub fn write_private(path: &Path, text: impl AsRef<[u8]>) -> io::Result<()> {
    static WRITES: AtomicU64 = AtomicU64::new(0);
    let name = path
        .file_name()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "not a file path"))?;
    let tmp = path.with_file_name(format!(
        ".{}.{}.{}.tmp",
        name.to_string_lossy(),
        std::process::id(),
        WRITES.fetch_add(1, Ordering::Relaxed)
    ));
    write_through_temp(path, &tmp, text.as_ref())
}

fn write_through_temp(path: &Path, tmp: &Path, text: &[u8]) -> io::Result<()> {
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    // A collision is not ours to clean up. Only remove a temporary file after
    // this process successfully created it (including after a later write error).
    let mut file = options.open(tmp)?;
    let written = file.write_all(text).and_then(|()| file.sync_all());
    drop(file);
    let result = written.and_then(|()| std::fs::rename(tmp, path));
    if result.is_err() {
        let _ = std::fs::remove_file(tmp);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> std::path::PathBuf {
        let dir =
            std::env::temp_dir().join(format!("horizon-private-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn writes_whole_and_leaves_no_temporary_file() {
        let dir = scratch("whole");
        let path = dir.join("api.json");
        write_private(&path, "one").unwrap();
        write_private(&path, "two").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "two");
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn an_existing_temporary_file_is_not_removed() {
        let dir = scratch("collision");
        let path = dir.join("api.json");
        let temporary = dir.join("existing.tmp");
        std::fs::write(&path, "old token").unwrap();
        std::fs::write(&temporary, "unrelated existing file").unwrap();
        assert!(write_through_temp(&path, &temporary, b"new token").is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "old token");
        assert_eq!(
            std::fs::read_to_string(&temporary).unwrap(),
            "unrelated existing file"
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_failed_replace_removes_only_the_temporary_file_we_created() {
        let dir = scratch("failed-replace");
        let path = dir.join("not-a-file");
        let temporary = dir.join("owned.tmp");
        std::fs::create_dir(&path).unwrap();
        assert!(write_through_temp(&path, &temporary, b"secret").is_err());
        assert!(!temporary.exists());
        assert!(path.is_dir());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn an_old_readable_file_becomes_owner_only() {
        use std::os::unix::fs::PermissionsExt;
        let dir = scratch("mode");
        let path = dir.join("api.json");
        std::fs::write(&path, "old token").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        write_private(&path, "new token").unwrap();
        let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn a_link_at_the_path_is_replaced_not_followed() {
        let dir = scratch("link");
        let elsewhere = dir.join("elsewhere.txt");
        std::fs::write(&elsewhere, "untouched").unwrap();
        let path = dir.join("api.json");
        std::os::unix::fs::symlink(&elsewhere, &path).unwrap();
        write_private(&path, "token").unwrap();
        assert_eq!(std::fs::read_to_string(&elsewhere).unwrap(), "untouched");
        assert!(
            !std::fs::symlink_metadata(&path)
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "token");
        std::fs::remove_dir_all(dir).unwrap();
    }
}
