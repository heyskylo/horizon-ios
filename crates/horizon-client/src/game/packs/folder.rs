//! Resolve paths in the game's filesystem context before handing them to a
//! desktop process, which may not inherit Windows AppData virtualization.
use std::{io, path::Path, process::Command};

pub(super) fn command(dir: &Path) -> io::Result<Command> {
    let program = if cfg!(windows) {
        "explorer"
    } else if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    };
    // canonicalize opens the directory and resolves its physical location.
    // dunce also removes verbatim syntax where safe: Explorer is not a Win32
    // file API and does not handle every extended-length path form.
    #[cfg(windows)]
    let dir = dunce::canonicalize(dir)?;
    let mut command = Command::new(program);
    command.arg(dir);
    Ok(command)
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use std::{
        fs,
        path::PathBuf,
        sync::atomic::{AtomicU64, Ordering},
    };

    static NEXT: AtomicU64 = AtomicU64::new(0);
    struct Scratch(PathBuf);
    impl Scratch {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "horizon-folder-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&path).unwrap();
            Self(path)
        }
    }
    impl Drop for Scratch {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.0).unwrap();
        }
    }

    #[test]
    fn shell_gets_the_existing_folder_as_one_physical_path() {
        let scratch = Scratch::new();
        let folder = scratch.0.join("packs & textures \u{2603}");
        fs::create_dir(&folder).unwrap();
        fs::write(folder.join("marker"), b"pack scan").unwrap();
        let logical = folder.join("..").join("packs & textures \u{2603}");
        let command = command(&logical).unwrap();
        let args: Vec<_> = command.get_args().collect();
        assert_eq!(args.len(), 1);
        let target = Path::new(args[0]);
        assert!(!target.as_os_str().to_string_lossy().starts_with(r"\\?\"));
        assert!(
            !target
                .components()
                .any(|component| component == std::path::Component::ParentDir)
        );
        assert_eq!(fs::read(target.join("marker")).unwrap(), b"pack scan");
        assert_eq!(
            fs::canonicalize(target).unwrap(),
            fs::canonicalize(&folder).unwrap()
        );
        assert_eq!(command.get_program(), "explorer");
    }

    #[test]
    fn missing_folder_is_not_sent_to_explorer() {
        let scratch = Scratch::new();
        assert_eq!(
            command(&scratch.0.join("missing")).unwrap_err().kind(),
            io::ErrorKind::NotFound
        );
    }
}
