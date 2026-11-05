//! Client resource-pack namespaces, separate from content-mod registry ownership.
use std::{fs, path::Path};

/// The valid namespace directories in an effective client asset root.
pub fn client(root: &Path) -> Vec<String> {
    let Ok(entries) = fs::read_dir(root.join("assets")) else {
        return Vec::new();
    };
    let mut names: Vec<_> = entries
        .flatten()
        .filter(|entry| entry.path().is_dir())
        .filter_map(|entry| entry.file_name().into_string().ok())
        .filter(|name| {
            !name.is_empty()
                && name != "."
                && name != ".."
                && name
                    .bytes()
                    .all(|b| matches!(b, b'a'..=b'z' | b'0'..=b'9' | b'_' | b'-' | b'.'))
        })
        .collect();
    names.sort();
    names
}
