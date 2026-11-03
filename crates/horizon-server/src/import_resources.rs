//! `horizon-server import-resources DIR`: puts a folder of prepared game files (made on the owner's own
//! computer by the launcher or `horizon-prepare`) where the server reads them, so a headless machine
//! without Java needs no `prepare`. docs/server.md, section "Game files".
//!
//! The folder is checked first (the same checks the engine makes at start, plus the `minecraft` version
//! stamp in `horizon-resources.json` against the pinned version), copied beside its destination, and
//! moved in with a rename. The previous copy is put back if any step fails.

use crate::engine::{self, GAME_VERSION};
use std::{
    io,
    path::{Path, PathBuf},
};

/// Where the prepared files go: the installer's `game-files/horizon-client/<version>` (what the release's
/// `.local/horizon-client` links to), or `resources/` in the server folder when there is no installer.
pub fn destination(dir: &Path) -> PathBuf {
    match crate::update::Install::find() {
        Some(install) => install
            .root
            .join("game-files/horizon-client")
            .join(GAME_VERSION),
        None => dir.join("resources"),
    }
}

/// `import-resources DIR`.
pub fn command(dir: &Path, args: &[String]) -> Result<(), String> {
    let [source] = args else {
        return Err(
            "import-resources needs the prepared folder to copy (one with horizon-resources.json)"
                .into(),
        );
    };
    let target = destination(dir);
    import(Path::new(source), &target, &mut || Ok(()))?;
    println!("Imported the game files into {}", target.display());
    println!(
        "Next: `horizon-server start` (or `horizon-server restart` if it is running) to start on them."
    );
    Ok(())
}

/// What is wrong with `source` as the server's game files, each as one line; empty when it is fit.
pub fn problems(source: &Path) -> Vec<String> {
    if !source.is_dir() {
        return vec![format!("{} is not a folder", source.display())];
    }
    let manifest = source.join("horizon-resources.json");
    let Ok(text) = std::fs::read_to_string(&manifest) else {
        return vec![format!(
            "{} has no horizon-resources.json: it is not a prepared folder (the launcher writes one, or \
             `horizon-prepare --prepare`)",
            source.display()
        )];
    };
    let mut out = Vec::new();
    match serde_json::from_str::<serde_json::Value>(&text) {
        Ok(manifest) => match manifest.get("minecraft").and_then(|v| v.as_str()) {
            Some(GAME_VERSION) => {}
            Some(other) => out.push(format!(
                "prepared for Minecraft {other}, this server needs {GAME_VERSION}: prepare {GAME_VERSION} \
                 and import that"
            )),
            None => out.push("horizon-resources.json names no Minecraft version".into()),
        },
        Err(e) => out.push(format!("horizon-resources.json is not valid JSON ({e})")),
    }
    // The file and checksum checks the engine makes at start; its message lists one problem per line.
    if let Err(e) = engine::check_prepared(source) {
        out.extend(
            e.lines()
                .filter(|l| l.starts_with("  "))
                .map(|l| l.trim().to_owned())
                // The version problem above already covers an unreadable or invalid manifest.
                .filter(|l| !l.starts_with("horizon-resources.json")),
        );
    }
    out
}

/// Checks `source`, copies it next to `target`, and swaps it in; `target` keeps its old content on any
/// failure. `each_file` runs after every copied file (a test makes it fail partway).
pub fn import(
    source: &Path,
    target: &Path,
    each_file: &mut dyn FnMut() -> io::Result<()>,
) -> Result<(), String> {
    let found = problems(source);
    if !found.is_empty() {
        return Err(format!(
            "{} can't be used as the server's game files:\n  {}\nNothing was changed.",
            source.display(),
            found.join("\n  ")
        ));
    }
    let parent = target
        .parent()
        .ok_or_else(|| format!("{} has no parent folder", target.display()))?;
    std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
    let name = target
        .file_name()
        .map_or_else(|| "resources".into(), |n| n.to_string_lossy().into_owned());
    let pid = std::process::id();
    let fresh = parent.join(format!(".{name}.importing-{pid}"));
    let old = parent.join(format!(".{name}.previous-{pid}"));
    let _ = std::fs::remove_dir_all(&fresh);
    let copied = copy_tree(source, &fresh, each_file)
        .map_err(|e| format!("copying {}: {e}", source.display()))
        .and_then(|()| {
            // Copied bytes must still pass the same check before they replace anything.
            let found = problems(&fresh);
            if found.is_empty() {
                Ok(())
            } else {
                Err(format!("the copy is incomplete:\n  {}", found.join("\n  ")))
            }
        });
    if let Err(e) = copied {
        let _ = std::fs::remove_dir_all(&fresh);
        return Err(format!("{e}\nThe previous game files are untouched."));
    }
    let had_old = target.symlink_metadata().is_ok();
    if had_old && let Err(e) = std::fs::rename(target, &old) {
        let _ = std::fs::remove_dir_all(&fresh);
        return Err(format!(
            "{}: {e}\nThe previous game files are untouched.",
            target.display()
        ));
    }
    if let Err(e) = std::fs::rename(&fresh, target) {
        if had_old {
            let _ = std::fs::rename(&old, target);
        }
        let _ = std::fs::remove_dir_all(&fresh);
        return Err(format!(
            "{}: {e}\nThe previous game files are untouched.",
            target.display()
        ));
    }
    if had_old {
        let _ = std::fs::remove_dir_all(&old);
    }
    Ok(())
}

/// Copies the folder `from` to the new folder `to`, following links, calling `each_file` after each file.
fn copy_tree(
    from: &Path,
    to: &Path,
    each_file: &mut dyn FnMut() -> io::Result<()>,
) -> io::Result<()> {
    std::fs::create_dir_all(to)?;
    let mut entries: Vec<_> = std::fs::read_dir(from)?.collect::<Result<_, _>>()?;
    entries.sort_by_key(|e| e.file_name());
    for entry in entries {
        let (src, dst) = (entry.path(), to.join(entry.file_name()));
        if std::fs::metadata(&src)?.is_dir() {
            copy_tree(&src, &dst, each_file)?;
        } else {
            std::fs::copy(&src, &dst)?;
            each_file()?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("horizon-import-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn put(path: &Path, bytes: &[u8]) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, bytes).unwrap();
    }

    /// A prepared folder the engine's own check accepts, stamped `minecraft`; `tag` marks its content.
    fn prepared(root: &Path, minecraft: &str, tag: &str) -> PathBuf {
        let dir = root.join(format!("prepared-{tag}"));
        for file in [
            "facts.json",
            "block-tags.json",
            "registries.json",
            "commands.json",
        ] {
            put(&dir.join(file), format!("{{\"{tag}\": 1}}").as_bytes());
        }
        put(&dir.join("assets/minecraft/font/default.json"), b"{}");
        put(
            &dir.join("assets/minecraft/textures/block/stone.png"),
            tag.as_bytes(),
        );
        put(
            &dir.join("horizon-resources.json"),
            format!("{{\"schema\": 1, \"minecraft\": \"{minecraft}\"}}").as_bytes(),
        );
        dir
    }

    fn tag(target: &Path) -> String {
        std::fs::read_to_string(target.join("assets/minecraft/textures/block/stone.png")).unwrap()
    }

    fn leftovers(parent: &Path) -> Vec<String> {
        std::fs::read_dir(parent)
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| n.starts_with('.'))
            .collect()
    }

    #[test]
    fn a_good_folder_is_copied_whole_and_replaces_the_old_one() {
        let root = scratch("good");
        let source = prepared(&root, GAME_VERSION, "new");
        let target = root.join("game-files/horizon-client").join(GAME_VERSION);
        put(&target.join("stale.txt"), b"old");
        import(&source, &target, &mut || Ok(())).unwrap();
        assert_eq!(tag(&target), "new");
        assert!(target.join("horizon-resources.json").is_file());
        assert!(
            !target.join("stale.txt").exists(),
            "the old copy is replaced, not merged"
        );
        assert!(engine::check_prepared(&target).is_ok());
        assert!(leftovers(target.parent().unwrap()).is_empty());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_wrong_version_is_refused_and_nothing_changes() {
        let root = scratch("version");
        let source = prepared(&root, "26.2", "new");
        let old = prepared(&root, GAME_VERSION, "old");
        let target = root.join("target");
        import(&old, &target, &mut || Ok(())).unwrap();
        let e = import(&source, &target, &mut || Ok(())).unwrap_err();
        assert!(
            e.contains("prepared for Minecraft 26.2") && e.contains(GAME_VERSION),
            "{e}"
        );
        assert_eq!(tag(&target), "old");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_missing_file_or_manifest_is_named() {
        let root = scratch("missing");
        let source = prepared(&root, GAME_VERSION, "new");
        std::fs::remove_file(source.join("registries.json")).unwrap();
        let target = root.join("target");
        let e = import(&source, &target, &mut || Ok(())).unwrap_err();
        assert!(e.contains("registries.json is missing"), "{e}");
        assert!(!target.exists());
        std::fs::remove_file(source.join("horizon-resources.json")).unwrap();
        let e = import(&source, &target, &mut || Ok(())).unwrap_err();
        assert!(e.contains("no horizon-resources.json"), "{e}");
        let e = import(&root.join("nope"), &target, &mut || Ok(())).unwrap_err();
        assert!(e.contains("is not a folder"), "{e}");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_failure_partway_keeps_the_old_copy_and_leaves_no_temp_folder() {
        let root = scratch("partway");
        let old = prepared(&root, GAME_VERSION, "old");
        let source = prepared(&root, GAME_VERSION, "new");
        let target = root.join("target");
        import(&old, &target, &mut || Ok(())).unwrap();
        let mut copied = 0;
        let e = import(&source, &target, &mut || {
            copied += 1;
            if copied == 3 {
                Err(io::Error::other("disk full"))
            } else {
                Ok(())
            }
        })
        .unwrap_err();
        assert!(e.contains("disk full") && e.contains("untouched"), "{e}");
        assert_eq!(copied, 3);
        assert_eq!(tag(&target), "old");
        assert!(engine::check_prepared(&target).is_ok());
        assert!(leftovers(&root).is_empty(), "{:?}", leftovers(&root));
        let _ = std::fs::remove_dir_all(&root);
    }
}
