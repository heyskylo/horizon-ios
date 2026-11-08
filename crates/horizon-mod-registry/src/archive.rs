//! The upload's zip, checked once before any kind's own reader opens it: every
//! reader (registry, launcher, game) then sees the same plain files.
//!
//! Refused: a local header that disagrees with the central directory (one
//! zip that reads as two), two entries for one path, overlapping entries,
//! symlinks and other special files, encrypted entries, and more entries or
//! declared bytes than any kind's reader takes.
use std::{collections::HashSet, path::Path};

/// Entries: a content mod's 65 536 files and their folders.
const MAX_ENTRIES: usize = 100_000;
/// Declared uncompressed bytes: above every kind's own total (a content mod's
/// 512 MiB and a native bundle's 64 MiB).
const MAX_UNCOMPRESSED: u64 = 1 << 30;

/// File type bits of a Unix mode: a regular file, a folder, or none named.
const S_IFMT: u32 = 0o170000;
const S_IFREG: u32 = 0o100000;
const S_IFDIR: u32 = 0o040000;

pub fn check(path: &Path) -> Result<(), String> {
    check_with(path, MAX_ENTRIES, MAX_UNCOMPRESSED)
}

fn check_with(path: &Path, max_entries: usize, max_bytes: u64) -> Result<(), String> {
    let file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let mut zip = zip::ZipArchive::new(file).map_err(|e| format!("not a zip: {e}"))?;
    if zip.len() > max_entries {
        return Err(format!("over {max_entries} entries in the zip"));
    }
    // Duplicate central names, local headers that differ from the central
    // directory (name, sizes, method, flags), entries sharing bytes.
    zip.validate_header_consistency()
        .map_err(|e| e.to_string())?;
    let mut seen = HashSet::new();
    let mut total = 0u64;
    for i in 0..zip.len() {
        let entry = zip.by_index_raw(i).map_err(|e| e.to_string())?;
        let name = entry.name().map_err(|e| e.to_string())?.into_owned();
        if entry
            .unix_mode()
            .is_some_and(|m| ![0, S_IFREG, S_IFDIR].contains(&(m & S_IFMT)))
        {
            return Err(format!("{name:?} is a symlink or special file"));
        }
        if entry.encrypted() {
            return Err(format!("{name:?} is encrypted"));
        }
        // One path on a case-insensitive disk, a file and a folder alike.
        if !seen.insert(name.trim_end_matches('/').to_lowercase()) {
            return Err(format!("{name:?} is in the zip twice"));
        }
        total = total.saturating_add(entry.size());
        if total > max_bytes {
            return Err(format!("the zip expands beyond {} MiB", max_bytes >> 20));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Cursor, Write};
    use zip::{CompressionMethod, ZipWriter, write::SimpleFileOptions};

    fn stored() -> SimpleFileOptions {
        SimpleFileOptions::default().compression_method(CompressionMethod::Stored)
    }

    fn zip_of(files: &[(&str, &[u8])]) -> Vec<u8> {
        let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
        for (name, bytes) in files {
            zip.start_file(*name, stored()).unwrap();
            zip.write_all(bytes).unwrap();
        }
        zip.finish().unwrap().into_inner()
    }

    /// Replaces the `nth` (0-based, `None` = every) occurrence of `from` by
    /// `to` (same length): a name in a local header comes before its central
    /// directory record.
    fn patch(bytes: &mut [u8], from: &[u8], to: &[u8], nth: Option<usize>) {
        assert_eq!(from.len(), to.len());
        let at: Vec<usize> = (0..=bytes.len() - from.len())
            .filter(|&i| &bytes[i..i + from.len()] == from)
            .collect();
        assert!(!at.is_empty());
        for (k, &i) in at.iter().enumerate() {
            if nth.is_none_or(|n| n == k) {
                bytes[i..i + to.len()].copy_from_slice(to);
            }
        }
    }

    fn checked(bytes: &[u8]) -> Result<(), String> {
        checked_with(bytes, MAX_ENTRIES, MAX_UNCOMPRESSED)
    }

    fn checked_with(bytes: &[u8], entries: usize, total: u64) -> Result<(), String> {
        let dir = std::env::temp_dir().join(format!(
            "registry-archive-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("upload.zip");
        std::fs::write(&path, bytes).unwrap();
        let result = check_with(&path, entries, total);
        let _ = std::fs::remove_dir_all(&dir);
        result
    }

    #[test]
    fn plain_files_and_folders_pass() {
        let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
        zip.add_directory("assets/", stored()).unwrap();
        zip.start_file("assets/a.png", stored()).unwrap();
        zip.write_all(b"png").unwrap();
        zip.start_file("horizon-mod.toml", SimpleFileOptions::default())
            .unwrap();
        zip.write_all(b"id = \"a\"").unwrap();
        assert_eq!(checked(&zip.finish().unwrap().into_inner()), Ok(()));
    }

    #[test]
    fn a_symlink_is_refused() {
        let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
        zip.start_file("a.txt", stored()).unwrap();
        zip.write_all(b"x").unwrap();
        zip.add_symlink("assets/link", "/etc/passwd", stored())
            .unwrap();
        let err = checked(&zip.finish().unwrap().into_inner()).unwrap_err();
        assert!(err.contains("\"assets/link\" is a symlink"), "{err}");
    }

    #[test]
    fn a_fifo_is_refused() {
        let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
        zip.start_file("pipe", stored().external_attributes(0o010644 << 16))
            .unwrap();
        let err = checked(&zip.finish().unwrap().into_inner()).unwrap_err();
        assert!(err.contains("special file"), "{err}");
    }

    /// The central directory says `good.txt`, the local header `evil.txt`: a
    /// streaming reader would see another file than the checks did.
    #[test]
    fn a_local_header_that_differs_is_refused() {
        let mut bytes = zip_of(&[("good.txt", b"x")]);
        patch(&mut bytes, b"good.txt", b"evil.txt", Some(0));
        assert!(checked(&bytes).unwrap_err().contains("mismatch"));
    }

    #[test]
    fn two_entries_for_one_path_are_refused() {
        let mut bytes = zip_of(&[("a.txt", b"one"), ("b.txt", b"two")]);
        patch(&mut bytes, b"b.txt", b"a.txt", None);
        assert!(checked(&bytes).unwrap_err().contains("duplicate"));
        // Two cases of one name are one file on macOS and Windows.
        let bytes = zip_of(&[("Data/x.json", b"{}"), ("data/X.json", b"{}")]);
        let err = checked(&bytes).unwrap_err();
        assert!(err.contains("\"data/X.json\" is in the zip twice"), "{err}");
    }

    #[test]
    fn an_encrypted_entry_is_refused() {
        let mut bytes = zip_of(&[("a.txt", b"x")]);
        // Bit 0 of the general purpose flags, in the local header (offset 6)
        // and the central record (offset 8): the headers still agree.
        bytes[6] |= 1;
        let central = (0..bytes.len() - 4)
            .find(|&i| &bytes[i..i + 4] == b"PK\x01\x02")
            .unwrap();
        bytes[central + 8] |= 1;
        assert!(checked(&bytes).unwrap_err().contains("encrypted"));
    }

    #[test]
    fn entries_and_declared_size_are_capped() {
        let bytes = zip_of(&[("a", b""), ("b", b""), ("c", b"")]);
        assert_eq!(checked_with(&bytes, 3, 1 << 20), Ok(()));
        assert!(
            checked_with(&bytes, 2, 1 << 20)
                .unwrap_err()
                .contains("over 2 entries")
        );
        let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
        zip.start_file("big", SimpleFileOptions::default()).unwrap();
        zip.write_all(&vec![0; 3 << 20]).unwrap();
        let bytes = zip.finish().unwrap().into_inner();
        assert!(bytes.len() < 1 << 20);
        assert_eq!(checked_with(&bytes, 10, 4 << 20), Ok(()));
        let err = checked_with(&bytes, 10, 2 << 20).unwrap_err();
        assert!(err.contains("expands beyond 2 MiB"), "{err}");
    }
}
