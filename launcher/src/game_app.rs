//! The game's app bundle on macOS. Started as a bare executable, the game shows in the Dock and
//! in Cmd+Tab as "horizon-client" with the generic executable icon (winit's window icon does
//! nothing there). So the launcher starts it from a small Horizon.app inside its version folder:
//! the Info.plist names it and the icon is the launcher's own. The programs inside are hard links
//! to the version's, so the bundle takes no disk space and the release's signature is untouched.
use std::fs;
use std::io;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

/// The bundle's folder in a version folder.
pub const APP: &str = "Horizon.app";

/// The game, then horizon-ctl, which the game looks for beside itself (Copy Connection Details).
const PROGRAMS: [&str; 2] = ["horizon-client", "horizon-ctl"];

const ICON: &[u8] = include_bytes!("../../packaging/icons/horizon.icns");

/// Its own identifier, so macOS keeps it apart from the launcher (dev.horizon.launcher); the
/// games category lets macOS turn Game Mode on when it plays full screen.
const INFO_PLIST: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
	<key>CFBundleDisplayName</key>
	<string>Horizon</string>
	<key>CFBundleExecutable</key>
	<string>horizon-client</string>
	<key>CFBundleIconFile</key>
	<string>horizon</string>
	<key>CFBundleIdentifier</key>
	<string>dev.horizon.game</string>
	<key>CFBundleInfoDictionaryVersion</key>
	<string>6.0</string>
	<key>CFBundleName</key>
	<string>Horizon</string>
	<key>CFBundlePackageType</key>
	<string>APPL</string>
	<key>LSApplicationCategoryType</key>
	<string>public.app-category.games</string>
	<key>NSHighResolutionCapable</key>
	<true/>
	<key>NSMicrophoneUsageDescription</key>
	<string>__MIC_DESCRIPTION__</string>
</dict>
</plist>
"#;

/// Where `ensure` fills in the localized `NSMicrophoneUsageDescription` (native mods can open the
/// microphone for voice chat; without the key macOS would kill the game instead of asking).
const MIC_DESCRIPTION: &str = "__MIC_DESCRIPTION__";

/// Brings `version`'s Horizon.app up to date and returns the game's program inside it. A program
/// replaced in the version folder (a reinstall, a build copied over) is linked again.
pub fn ensure(version: &Path) -> io::Result<PathBuf> {
    let contents = version.join(APP).join("Contents");
    let programs = contents.join("MacOS");
    fs::create_dir_all(&programs)?;
    fs::create_dir_all(contents.join("Resources"))?;
    // Filled here so the prompt follows the launcher's language: the game reads the bundle as
    // it starts, and the plist is not a place the game can localize.
    let plist = INFO_PLIST.replacen(
        MIC_DESCRIPTION,
        &crate::shortcut::xml(crate::i18n::tr(
            "Native mods can use your microphone for voice chat when you enable it.",
        )),
        1,
    );
    write_if_changed(&contents.join("Info.plist"), plist.as_bytes())?;
    write_if_changed(&contents.join("Resources/horizon.icns"), ICON)?;
    for (i, name) in PROGRAMS.into_iter().enumerate() {
        let (from, to) = (version.join(name), programs.join(name));
        // Releases before horizon-ctl shipped have only the game.
        if i > 0 && !from.exists() {
            continue;
        }
        if !same_file(&from, &to) {
            let _ = fs::remove_file(&to);
            fs::hard_link(&from, &to)?;
        }
    }
    // Native builds read both companions beside current_exe(), including on a reused bundle.
    let (from, to) = (
        version.join("native-mods.json"),
        programs.join("native-mods.json"),
    );
    match fs::metadata(&from) {
        Ok(_) if !same_file(&from, &to) => {
            replace_link(&to, |temp| fs::hard_link(fs::canonicalize(&from)?, temp))?;
        }
        Ok(_) => {}
        Err(e) if e.kind() == io::ErrorKind::NotFound => remove_if_present(&to)?,
        Err(e) => return Err(e),
    }
    for name in ["native-data", "native-content"] {
        sync_native_directory(version, &programs, name)?;
    }
    Ok(programs.join(PROGRAMS[0]))
}

fn sync_native_directory(version: &Path, programs: &Path, name: &str) -> io::Result<()> {
    let present = match fs::metadata(version.join(name)) {
        Ok(m) if m.is_dir() => true,
        Ok(_) => return Err(io::ErrorKind::NotADirectory.into()),
        Err(e) if e.kind() == io::ErrorKind::NotFound => false,
        Err(e) => return Err(e),
    };
    let to = programs.join(name);
    let target = Path::new("../../..").join(name);
    match fs::symlink_metadata(&to) {
        // Never replace or recursively delete an unexpected directory or file.
        Ok(m) if !m.file_type().is_symlink() => return Err(io::ErrorKind::AlreadyExists.into()),
        Ok(_) if present && fs::read_link(&to)? == target => return Ok(()),
        Ok(_) => {}
        Err(e) if e.kind() == io::ErrorKind::NotFound => {}
        Err(e) => return Err(e),
    }
    if present {
        replace_link(&to, |temp| std::os::unix::fs::symlink(&target, temp))
    } else {
        remove_if_present(&to)
    }
}

fn remove_if_present(path: &Path) -> io::Result<()> {
    match fs::remove_file(path) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        result => result,
    }
}

// Rename a fresh link rather than write through a previous hardlink into the version's file.
fn replace_link(to: &Path, link: impl FnOnce(&Path) -> io::Result<()>) -> io::Result<()> {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let temp = to.with_extension(format!(
        "{}-{}.tmp",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    link(&temp)?;
    let result = fs::rename(&temp, to);
    if result.is_err() {
        fs::remove_file(&temp)?;
    }
    result
}

fn write_if_changed(path: &Path, bytes: &[u8]) -> io::Result<()> {
    if fs::read(path).ok().as_deref() != Some(bytes) {
        fs::write(path, bytes)?;
    }
    Ok(())
}

fn same_file(a: &Path, b: &Path) -> bool {
    match (fs::metadata(a), fs::metadata(b)) {
        (Ok(a), Ok(b)) => a.dev() == b.dev() && a.ino() == b.ino(),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn native_version(name: &str) -> PathBuf {
        let version =
            std::env::temp_dir().join(format!("horizon-game-app-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&version);
        fs::create_dir_all(&version).unwrap();
        fs::write(version.join("horizon-client"), "unchanged signed game").unwrap();
        version
    }

    #[test]
    fn native_receipt_refreshes_without_replacing_the_game_and_is_removed_for_vanilla() {
        let version = native_version("receipt");
        let receipt = version.join("native-mods.json");
        let first = br#"{"mods":[{"id":"example","version":"1"}]}"#;
        let second = br#"{"mods":[{"id":"example","version":"2"}]}"#;
        fs::write(&receipt, first).unwrap();
        let program = ensure(&version).unwrap();
        let bundled = program.with_file_name("native-mods.json");
        assert_eq!(fs::read(&bundled).unwrap(), first);
        fs::hard_link(&receipt, version.join("old-receipt")).unwrap();

        // Installers replace the receipt atomically; the executable need not change.
        fs::write(version.join("replacement"), second).unwrap();
        fs::rename(version.join("replacement"), &receipt).unwrap();
        assert_eq!(ensure(&version).unwrap(), program);
        assert_eq!(fs::read(&bundled).unwrap(), second);
        assert_eq!(fs::read(&receipt).unwrap(), second);
        assert_eq!(fs::read(version.join("old-receipt")).unwrap(), first);
        assert!(same_file(&program, &version.join("horizon-client")));
        assert_eq!(fs::read(&program).unwrap(), b"unchanged signed game");

        fs::remove_file(&receipt).unwrap();
        ensure(&version).unwrap();
        assert!(!bundled.exists());
        fs::remove_dir_all(version).unwrap();
    }

    #[test]
    fn native_data_follows_the_version_directory_and_refreshes_only_its_symlink() {
        let version = native_version("data");
        let data = version.join("native-data");
        fs::create_dir_all(data.join("example")).unwrap();
        fs::write(data.join("example/table.bin"), b"first").unwrap();
        let linked = ensure(&version).unwrap().with_file_name("native-data");
        assert_eq!(
            fs::read_link(&linked).unwrap(),
            Path::new("../../../native-data")
        );
        assert_eq!(
            fs::read(linked.join("example/table.bin")).unwrap(),
            b"first"
        );

        fs::create_dir_all(version.join("other-data")).unwrap();
        fs::write(version.join("other-data/keep"), b"untouched").unwrap();
        fs::remove_file(&linked).unwrap();
        std::os::unix::fs::symlink(version.join("other-data"), &linked).unwrap();
        ensure(&version).unwrap();
        assert_eq!(
            fs::read(linked.join("example/table.bin")).unwrap(),
            b"first"
        );
        assert_eq!(
            fs::read(version.join("other-data/keep")).unwrap(),
            b"untouched"
        );

        fs::rename(&data, version.join("old-data")).unwrap();
        fs::create_dir_all(data.join("example")).unwrap();
        fs::write(data.join("example/table.bin"), b"replacement").unwrap();
        ensure(&version).unwrap();
        assert_eq!(
            fs::read(linked.join("example/table.bin")).unwrap(),
            b"replacement"
        );
        fs::rename(&data, version.join("retired-data")).unwrap();
        ensure(&version).unwrap();
        assert!(fs::symlink_metadata(&linked).is_err());
        assert_eq!(
            fs::read(version.join("old-data/example/table.bin")).unwrap(),
            b"first"
        );
        fs::remove_dir_all(version).unwrap();
    }

    #[test]
    fn native_content_is_linked_beside_the_app_executable_and_removed_when_absent() {
        let version = native_version("content");
        let content = version.join("native-content/soundproof/assets/soundproof");
        fs::create_dir_all(&content).unwrap();
        fs::write(content.join("sounds.json"), b"{}").unwrap();
        let linked = ensure(&version).unwrap().with_file_name("native-content");
        assert_eq!(
            fs::read_link(&linked).unwrap(),
            Path::new("../../../native-content")
        );
        assert_eq!(
            fs::read(linked.join("soundproof/assets/soundproof/sounds.json")).unwrap(),
            b"{}"
        );
        fs::rename(version.join("native-content"), version.join("old-content")).unwrap();
        ensure(&version).unwrap();
        assert!(fs::symlink_metadata(linked).is_err());
        fs::remove_dir_all(version).unwrap();
    }

    #[test]
    fn unexpected_bundle_directory_is_preserved_and_reports_an_error() {
        let version = native_version("unexpected");
        fs::create_dir_all(version.join("native-data")).unwrap();
        let unexpected = version.join("Horizon.app/Contents/MacOS/native-data");
        fs::create_dir_all(&unexpected).unwrap();
        fs::write(unexpected.join("keep"), b"user file").unwrap();
        assert_eq!(
            ensure(&version).unwrap_err().kind(),
            io::ErrorKind::AlreadyExists
        );
        assert_eq!(fs::read(unexpected.join("keep")).unwrap(), b"user file");
        assert_eq!(
            fs::read(version.join("horizon-client")).unwrap(),
            b"unchanged signed game"
        );
        fs::remove_dir_all(version).unwrap();
    }

    #[test]
    fn receipt_install_errors_do_not_overwrite_a_directory_or_the_source() {
        let version = native_version("receipt-error");
        fs::write(version.join("native-mods.json"), b"receipt").unwrap();
        let unexpected = version.join("Horizon.app/Contents/MacOS/native-mods.json");
        fs::create_dir_all(&unexpected).unwrap();
        fs::write(unexpected.join("keep"), b"user file").unwrap();
        assert!(ensure(&version).is_err());
        assert_eq!(fs::read(unexpected.join("keep")).unwrap(), b"user file");
        assert_eq!(
            fs::read(version.join("native-mods.json")).unwrap(),
            b"receipt"
        );
        fs::remove_dir_all(version).unwrap();
    }

    #[test]
    fn the_bundle_links_the_version_programs_and_follows_a_replaced_one() {
        let version = std::env::temp_dir().join(format!("horizon-game-app-{}", std::process::id()));
        let _ = fs::remove_dir_all(&version);
        fs::create_dir_all(&version).unwrap();
        fs::write(version.join("horizon-client"), "game 1").unwrap();
        fs::write(version.join("horizon-ctl"), "ctl").unwrap();

        let program = ensure(&version).unwrap();
        assert_eq!(
            program,
            version.join("Horizon.app/Contents/MacOS/horizon-client")
        );
        assert!(same_file(&program, &version.join("horizon-client")));
        assert!(same_file(
            &version.join("Horizon.app/Contents/MacOS/horizon-ctl"),
            &version.join("horizon-ctl")
        ));
        let plist = fs::read_to_string(version.join("Horizon.app/Contents/Info.plist")).unwrap();
        assert!(plist.contains("<string>Horizon</string>"));
        // The mic prompt macOS needs before a native mod may open the microphone: a key with a
        // nonempty localized value, not the placeholder the template ships with.
        let (_, rest) = plist
            .split_once("<key>NSMicrophoneUsageDescription</key>")
            .expect("Info.plist has NSMicrophoneUsageDescription");
        let value = rest
            .split_once("</string>")
            .expect("the key has a string value")
            .0
            .split_once("<string>")
            .expect("an opening tag")
            .1;
        assert!(!value.is_empty());
        assert!(!plist.contains("__MIC_DESCRIPTION__"));
        assert_eq!(
            fs::read(version.join("Horizon.app/Contents/Resources/horizon.icns")).unwrap(),
            ICON
        );

        // A new build copied over the version's game: the bundle runs it, not the old one.
        fs::remove_file(version.join("horizon-client")).unwrap();
        fs::write(version.join("horizon-client"), "game 2").unwrap();
        assert_eq!(
            fs::read_to_string(ensure(&version).unwrap()).unwrap(),
            "game 2"
        );
        fs::remove_dir_all(&version).unwrap();
    }

    #[test]
    fn a_version_without_horizon_ctl_still_gets_its_bundle() {
        let version =
            std::env::temp_dir().join(format!("horizon-game-app-old-{}", std::process::id()));
        let _ = fs::remove_dir_all(&version);
        fs::create_dir_all(&version).unwrap();
        fs::write(version.join("horizon-client"), "game").unwrap();
        assert!(ensure(&version).unwrap().exists());
        assert!(!version
            .join("Horizon.app/Contents/MacOS/horizon-ctl")
            .exists());
        fs::remove_dir_all(&version).unwrap();
    }

    #[test]
    fn a_version_without_the_game_has_no_bundle_program() {
        let version =
            std::env::temp_dir().join(format!("horizon-game-app-none-{}", std::process::id()));
        let _ = fs::remove_dir_all(&version);
        fs::create_dir_all(&version).unwrap();
        assert!(ensure(&version).is_err());
        fs::remove_dir_all(&version).unwrap();
    }

    #[test]
    fn xml_escapes_the_characters_plist_strings_must_not_carry() {
        assert_eq!(
            crate::shortcut::xml(r#"a & b < c > "d""#),
            r#"a &amp; b &lt; c &gt; &quot;d&quot;"#
        );
    }
}
