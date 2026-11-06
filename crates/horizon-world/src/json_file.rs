//! Reading a prepared JSON file with the file named in the error.
//!
//! `serde_json` reports only where in the document it failed ("expected value at
//! line 1 column 1"), which says nothing about which of the prepared folder's
//! many files a player should prepare again. These helpers put the path in front
//! of that message.

use serde::de::{Deserialize, DeserializeOwned};
use std::{fs, io, path::Path};

/// `path`'s bytes; an I/O error names the file.
pub fn read_bytes(path: &Path) -> io::Result<Vec<u8>> {
    fs::read(path).map_err(|e| named(path, e))
}

/// `bytes` as JSON; a parse error names `path`.
pub fn parse<'de, T: Deserialize<'de>>(path: &Path, bytes: &'de [u8]) -> io::Result<T> {
    serde_json::from_slice(bytes).map_err(|e| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("{}: {e}", path.display()),
        )
    })
}

/// `path` as JSON; an I/O or parse error names the file.
pub fn read<T: DeserializeOwned>(path: &Path) -> io::Result<T> {
    parse(path, &read_bytes(path)?)
}

/// An I/O error whose message starts with the path it happened to.
fn named(path: &Path, error: io::Error) -> io::Error {
    io::Error::new(error.kind(), format!("{}: {error}", path.display()))
}

/// A path for tests: a fresh folder under the system temporary directory.
#[cfg(test)]
fn test_dir(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("horizon-json-file-{tag}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_parse_error_names_the_file() {
        let dir = test_dir("parse");
        let empty = dir.join("gameplay-facts.json");
        fs::write(&empty, b"").unwrap();
        let error = read::<serde_json::Value>(&empty).unwrap_err().to_string();
        assert!(error.contains("gameplay-facts.json"), "{error}");

        let broken = dir.join("block-tags.json");
        fs::write(&broken, br#"{"a": "#).unwrap();
        let error = read::<serde_json::Value>(&broken).unwrap_err().to_string();
        assert!(error.contains("block-tags.json"), "{error}");

        let missing = dir.join("attributes.json");
        let error = read::<serde_json::Value>(&missing).unwrap_err().to_string();
        assert!(error.contains("attributes.json"), "{error}");
        let _ = fs::remove_dir_all(&dir);
    }
}
