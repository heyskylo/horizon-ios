//! `PackSelectionScreen.copyPacks`: copy a dropped pack without replacing an existing one.
use std::{fs, io, path::Path};

fn invalid() -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, "not a regular resource pack")
}

fn copy_tree(from: &Path, to: &Path) -> io::Result<()> {
    let mut directories = vec![(from.to_path_buf(), to.to_path_buf())];
    while let Some((from, to)) = directories.pop() {
        for entry in fs::read_dir(from)? {
            let entry = entry?;
            let kind = entry.file_type()?;
            let target = to.join(entry.file_name());
            if kind.is_symlink() {
                return Err(invalid());
            } else if kind.is_dir() {
                fs::create_dir(&target)?;
                directories.push((entry.path(), target));
            } else if kind.is_file() {
                fs::copy(entry.path(), target)?;
            } else {
                return Err(invalid());
            }
        }
    }
    Ok(())
}

/// Copies a zip or a folder with root `pack.mcmeta` into `dir`. Existing entries are
/// never overwritten; a failed copy removes only the destination this call created.
pub fn copy_pack(from: &Path, dir: &Path) -> io::Result<()> {
    let kind = fs::symlink_metadata(from)?.file_type();
    if kind.is_symlink() {
        return Err(invalid());
    }
    if kind.is_dir() {
        if !from.join("pack.mcmeta").is_file() {
            return Err(invalid());
        }
    } else if !kind.is_file()
        || !from
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("zip"))
        || !crate::zip::Zip::open(from)?.contains("pack.mcmeta")
    {
        return Err(invalid());
    }
    let name = from.file_name().ok_or_else(invalid)?;
    fs::create_dir_all(dir)?;
    // A folder dropped onto its own descendant must not recursively copy itself.
    if kind.is_dir() && dir.canonicalize()?.starts_with(from.canonicalize()?) {
        return Err(invalid());
    }
    let target = dir.join(name);
    let result = if kind.is_dir() {
        fs::create_dir(&target)?;
        copy_tree(from, &target)
    } else {
        let mut source = fs::File::open(from)?;
        let mut output = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&target)?;
        io::copy(&mut source, &mut output).map(|_| ())
    };
    if result.is_err() {
        let cleanup = if kind.is_dir() {
            fs::remove_dir_all(&target)
        } else {
            fs::remove_file(&target)
        };
        if let Err(error) = cleanup {
            log::warn!(
                "could not remove incomplete resource pack {}: {error}",
                target.display()
            );
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn scratch(name: &str) -> std::path::PathBuf {
        let root = std::env::temp_dir().join(format!("horizon-drop-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        root
    }

    fn folder(path: &Path) {
        fs::create_dir_all(path.join("assets/example")).unwrap();
        fs::write(
            path.join("pack.mcmeta"),
            br#"{"pack":{"pack_format":1,"description":"Test"}}"#,
        )
        .unwrap();
        fs::write(path.join("assets/example/test.txt"), b"texture").unwrap();
    }

    #[test]
    fn imports_folders_and_zips_that_discovery_can_list_without_enabling_them() {
        let root = scratch("valid");
        let pack = root.join("My Pack");
        folder(&pack);
        let zip_path = root.join("Other.ZIP");
        let mut zip = zip::ZipWriter::new(fs::File::create(&zip_path).unwrap());
        zip.start_file("pack.mcmeta", zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(&fs::read(pack.join("pack.mcmeta")).unwrap())
            .unwrap();
        zip.finish().unwrap();
        let dir = root.join("resourcepacks");
        copy_pack(&pack, &dir).unwrap();
        copy_pack(&zip_path, &dir).unwrap();
        assert_eq!(
            copy_pack(&zip_path, &dir).unwrap_err().kind(),
            io::ErrorKind::AlreadyExists
        );
        assert_eq!(
            fs::read(dir.join("My Pack/assets/example/test.txt")).unwrap(),
            b"texture"
        );
        assert_eq!(
            fs::read(dir.join("Other.ZIP")).unwrap(),
            fs::read(&zip_path).unwrap()
        );
        let packs = super::super::folder(&dir, super::super::RESOURCE_FORMAT);
        assert_eq!(
            packs.iter().map(|p| p.id.as_str()).collect::<Vec<_>>(),
            ["file/My Pack", "file/Other.ZIP"]
        );
        assert!(super::super::enabled(&packs, &[]).is_empty());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn invalid_inputs_and_collisions_leave_existing_files_untouched() {
        let root = scratch("refused");
        let dir = root.join("resourcepacks");
        let pack = root.join("Pack");
        folder(&pack);
        copy_pack(&pack, &dir).unwrap();
        fs::write(pack.join("assets/example/test.txt"), b"replacement").unwrap();
        assert_eq!(
            copy_pack(&pack, &dir).unwrap_err().kind(),
            io::ErrorKind::AlreadyExists
        );
        assert_eq!(
            fs::read(dir.join("Pack/assets/example/test.txt")).unwrap(),
            b"texture"
        );
        fs::write(root.join("bad.zip"), b"not a zip").unwrap();
        fs::create_dir(root.join("empty")).unwrap();
        assert!(copy_pack(&root.join("bad.zip"), &dir).is_err());
        assert!(copy_pack(&root.join("empty"), &dir).is_err());
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 1);
        assert!(copy_pack(&pack, &pack.join("nested")).is_err());
        assert!(!pack.join("nested/Pack").exists());
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn refuses_symlinks_and_removes_its_partial_copy() {
        let root = scratch("symlink");
        let pack = root.join("Pack");
        folder(&pack);
        std::os::unix::fs::symlink(&pack, pack.join("loop")).unwrap();
        let dir = root.join("resourcepacks");
        assert!(copy_pack(&pack, &dir).is_err());
        assert!(!dir.join("Pack").exists());
        assert!(pack.join("pack.mcmeta").exists());
        std::os::unix::fs::symlink(&pack, root.join("linked")).unwrap();
        assert!(copy_pack(&root.join("linked"), &dir).is_err());
        fs::remove_dir_all(root).unwrap();
    }
}
