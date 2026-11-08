//! Game files travel between a computer and a phone as one ZIP archive.
//!
//! A prepared tree is around 589 MB across five thousand files (`horizon-prepare`), a phone's app
//! bundle is read-only at runtime and CI refuses to put assets in a bundle at all, and iOS's Files
//! app neither shows nor lets you create a dotfolder -- so neither "copy the folder in" nor "ship it
//! inside the `.ipa`" works. What does work is one archive that the game unpacks when it arrives,
//! through either door: dropped into `Documents` over a cable, or posted to the phone over Wi-Fi.
//!
//! The archive is understood by what sits at its root, which is also what a person gets when they
//! compress a folder themselves: `horizon-resources.json` is the prepared game, `level.dat` is a
//! world, `pack.mcmeta` is a resource pack. [`pack`] writes exactly that shape, so the two ends
//! cannot drift apart, and [`Pack`] refuses an archive that is none of those before it writes a
//! single file.

pub mod download;
pub mod http;
pub mod screen;

use std::{
    fs::{self, File},
    io::{self, Read},
    path::{Path, PathBuf},
};
use zip::{CompressionMethod, ZipArchive, write::SimpleFileOptions, write::ZipWriter};

use crate::Result;

/// The largest archive accepted, the most it may unpack to, and the most files it may hold. All
/// three sit far above a real prepared tree (~589 MB, ~10k files), so they are not a guess about
/// what a player sends: they are what stops one request from a stranger on the same Wi-Fi filling
/// the phone's storage or opening a million entries.
const MAX_PACKED: u64 = 2 << 30;
const MAX_UNPACKED: u64 = 4 << 30;
const MAX_ENTRIES: usize = 50_000;

/// What an archive holds, named for the file that says so.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// The prepared tree: `horizon-resources.json` and everything `horizon-prepare` writes beside it.
    Resources,
    /// One world folder: `level.dat` and its chunks.
    World,
    /// One resource pack, which stays an archive rather than being unpacked.
    ResourcePack,
}

impl Kind {
    pub const ALL: [Kind; 3] = [Kind::Resources, Kind::World, Kind::ResourcePack];

    /// The entry that identifies this kind, wherever the archive puts it.
    pub fn marker(self) -> &'static str {
        match self {
            Kind::Resources => "horizon-resources.json",
            Kind::World => "level.dat",
            Kind::ResourcePack => "pack.mcmeta",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Kind::Resources => "game files",
            Kind::World => "world",
            Kind::ResourcePack => "resource pack",
        }
    }

    /// The folder under the game home that holds this kind; on a phone, `Documents/` and this.
    pub fn folder(self) -> &'static str {
        match self {
            Kind::Resources => "resources",
            Kind::World => "saves",
            Kind::ResourcePack => "resourcepacks",
        }
    }
}

/// Where an import ended up, for the screen and the log to describe.
#[derive(Debug, Clone)]
pub struct Installed {
    pub kind: Kind,
    pub path: PathBuf,
    pub files: usize,
    pub bytes: u64,
}

/// Where the prepared files are, of the two places they can be: the folder beside the executable,
/// and the folder an import unpacked into the sandbox's home (`Documents/resources`, the one the
/// Files app can reach). `None` when neither holds `horizon-resources.json` -- on a desktop the
/// ordinary answer, which leaves the caller's `.local/horizon-client/26.3` in charge.
///
/// A phone needs the second stop because it has no other door: the bundle is read while the game
/// runs, so the game cannot put its own assets where the first stop looks, and `.local/...` is a
/// dotfolder iOS neither shows nor lets a player create. A desktop is not asked (`home` is `None`)
/// because a `resources` folder under its working directory means something else there.
pub fn resources_root(sibling: &Path, home: Option<&Path>) -> Option<PathBuf> {
    let has = |dir: &Path| dir.join(Kind::Resources.marker()).exists();
    if has(sibling) {
        return Some(sibling.to_owned());
    }
    home.map(|home| home.join(Kind::Resources.folder()))
        .filter(|dir| has(dir))
}

/// Whether a game that found nothing to load should wait on screen instead of giving up. On a phone,
/// always: there is nothing else on the device to say what is missing, and a silent exit teaches a
/// player nothing. Anywhere else it has to be asked for (`HORIZON_IMPORT_SCREEN`), which is how the
/// screen is driven and tested on a machine with no phone in it.
pub fn waits_on_screen() -> bool {
    cfg!(any(target_os = "ios", target_os = "android"))
        || std::env::var_os("HORIZON_IMPORT_SCREEN").is_some()
}

/// The folder an import writes into: `Documents` on a phone, where the Files app and a cable both
/// reach; on Linux the game's home (`HORIZON_HOME`, else `~/.local/share/horizon`), where a start
/// downloads the game files once and finds them after; and the working directory everywhere else, and
/// in a run that asked for the screen (`HORIZON_IMPORT_SCREEN`), which points the game there.
pub fn home() -> PathBuf {
    if cfg!(target_os = "linux") && std::env::var_os("HORIZON_IMPORT_SCREEN").is_none() {
        return crate::logging::game_home();
    }
    std::env::current_dir().unwrap_or_default()
}

/// Names that are not part of what a person meant to send: the droppings a desktop leaves in a zip,
/// and a running world's `session.lock`, which says "someone is inside" rather than anything worth
/// importing. Left in, the droppings would also hide the one folder everything shares at the top,
/// which is what lets [`Pack`] see through archives made by a desktop's Compress.
fn is_junk(name: &str) -> bool {
    let last = name.rsplit('/').next().unwrap_or(name);
    name.starts_with("__MACOSX/")
        || matches!(
            last,
            ".DS_Store" | "Thumbs.db" | "desktop.ini" | "session.lock"
        )
}

/// The name an archive carries for itself, with anything that would turn it into a path or a
/// terminal escape removed. It arrives from a stranger's file: it is a label, not an instruction.
fn sanitize(name: &str) -> String {
    let kept: String = name
        .chars()
        .filter(|c| c.is_alphanumeric() || matches!(c, '-' | '_' | '.' | ' '))
        .collect();
    let trimmed = kept.trim_matches('.').trim();
    if trimmed.is_empty() {
        "imported".to_owned()
    } else {
        trimmed.chars().take(64).collect()
    }
}

/// Where an entry belongs inside the destination, or `None` when it belongs nowhere: no `..`, no
/// absolute path, no drive letter. A bad name fails the whole archive instead of quietly dropping
/// the entry, so a player hears that the archive is strange rather than playing a half-imported
/// world.
fn inside(name: &str, prefix: &str) -> Option<PathBuf> {
    if name.starts_with('/') || name.contains(':') {
        return None;
    }
    let rest = if prefix.is_empty() {
        name
    } else {
        name.strip_prefix(prefix)?
    };
    let mut out = PathBuf::new();
    for part in rest.split('/') {
        match part {
            "" | "." => continue,
            ".." => return None,
            _ => out.push(part),
        }
    }
    (!out.as_os_str().is_empty()).then_some(out)
}

/// The one folder every entry shares one level below `prefix`, if there is one: what compressing a
/// folder produces, as opposed to compressing its contents.
fn common_folder(names: &[String], prefix: &str) -> Option<String> {
    let names: Vec<&str> = names
        .iter()
        .filter_map(|name| name.strip_prefix(prefix))
        .filter(|rest| rest.contains('/'))
        .collect();
    let first = names.first()?.split('/').next()?;
    names
        .iter()
        .all(|name| name.starts_with(first) && name[first.len()..].starts_with('/'))
        .then(|| first.to_owned())
}

/// Does one of the kinds' markers sit directly under `prefix`?
fn has_marker(names: &[String], prefix: &str) -> Option<Kind> {
    Kind::ALL.iter().copied().find(|kind| {
        names.iter().any(|name| {
            name.strip_prefix(prefix)
                .is_some_and(|rest| rest == kind.marker())
        })
    })
}

/// One archive, opened and understood.
pub struct Pack {
    archive: ZipArchive<File>,
    path: PathBuf,
    /// The folder or file name the game gives what arrived: the folder the markers live under, else
    /// the archive's own stem.
    item: String,
    /// Every entry above the root the game reads, including the trailing `/`: `""`, `"Horizon/"`, or
    /// `"horizon-client/26.3/"` for somebody who compressed one folder too many.
    prefix: String,
    pub kind: Kind,
}

impl Pack {
    /// Look inside an archive and say what it is, writing nothing. Only the central directory is
    /// read, so this is cheap even for a prepared tree.
    pub fn open(path: &Path) -> Result<Self> {
        if fs::metadata(path)?.len() > MAX_PACKED {
            return Err("the archive is larger than this will unpack".into());
        }
        let archive = ZipArchive::new(File::open(path)?)?;
        // `name_for_index` borrows the archive, so the list is finished before anything is read out.
        let mut names = Vec::new();
        for index in 0..archive.len() {
            if let Some(name) = archive.name_for_index(index) {
                let name = name?.replace('\\', "/");
                if !name.ends_with('/') && !is_junk(&name) {
                    names.push(name);
                }
            }
        }
        if names.len() > MAX_ENTRIES {
            return Err("the archive holds more files than this will unpack".into());
        }
        if names.is_empty() {
            return Err("the archive holds nothing".into());
        }
        names.sort();
        // Descend through at most two shared folders looking for the marker, because compressing
        // `.local/horizon-client` rather than `26.3` is a mistake a careful person still makes.
        let mut prefix = String::new();
        let mut kind = has_marker(&names, "");
        for _ in 0..2 {
            if kind.is_some() {
                break;
            }
            let Some(folder) = common_folder(&names, &prefix) else {
                break;
            };
            prefix.push_str(&folder);
            prefix.push('/');
            kind = has_marker(&names, &prefix);
        }
        let kind = kind.ok_or_else(|| {
            format!(
                "{} is not Horizon's game files: it holds none of {}",
                path.display(),
                Kind::ALL
                    .iter()
                    .map(|k| k.marker())
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        })?;
        let stem = path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        let item = sanitize(
            prefix
                .trim_end_matches('/')
                .rsplit('/')
                .next()
                .filter(|last| !last.is_empty())
                .unwrap_or(&stem),
        );
        Ok(Self {
            archive,
            path: path.to_owned(),
            item,
            prefix,
            kind,
        })
    }

    /// Read every entry out into `into`, which is created here. Anything that goes wrong removes
    /// `into` again: the game must never see a half tree.
    fn unpack(&mut self, into: &Path) -> Result<(usize, u64)> {
        fs::create_dir_all(into)?;
        let mut files = 0;
        let mut written = 0u64;
        for index in 0..self.archive.len() {
            let file = self.archive.by_index(index)?;
            let name = file.name()?.replace('\\', "/");
            let failure = |message: String| {
                let _ = fs::remove_dir_all(into);
                Err::<(usize, u64), _>(message.into())
            };
            // Not `ZipArchive::extract`, which would create these: an archive from a stranger must
            // not be able to point a later write somewhere else in the container.
            if file.is_symlink() {
                return failure(format!("the archive holds a symbolic link: {name}"));
            }
            if name.ends_with('/') || is_junk(&name) {
                continue;
            }
            let Some(relative) = inside(&name, &self.prefix) else {
                return failure(format!("the archive holds a path that escapes it: {name}"));
            };
            let target = into.join(relative);
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent)?;
            }
            let mut out = File::create(&target)?;
            // The limit is on bytes actually written, not on what the archive claims: a header can
            // say anything at all. The extra byte is what tells "exactly the limit" from "past it"
            // without a second pass.
            let copied = io::copy(&mut file.take(MAX_UNPACKED - written + 1), &mut out)?;
            if written + copied > MAX_UNPACKED {
                return failure("the archive unpacks to more than this will hold".to_owned());
            }
            written += copied;
            files += 1;
        }
        Ok((files, written))
    }

    /// Put the archive where the game reads it, under `home` (`Documents` on a phone), and consume
    /// it: a phone does not have 589 MB to spend twice, and what arrives is a copy of something that
    /// is still on the computer that sent it.
    pub fn install(mut self, home: &Path) -> Result<Installed> {
        let archive = self.path.clone();
        let folder = Path::new(self.kind.folder());
        let (path, files, bytes) = match self.kind {
            // A pack stays an archive: the game reads zips straight out of `resourcepacks`
            // (`packs::discover`), so unpacking one would only lose what vanilla players expect.
            Kind::ResourcePack => {
                let dest = unique(&home.join(folder).join(format!("{}.zip", self.item)));
                move_file(&archive, &dest)?;
                let bytes = fs::metadata(&dest)?.len();
                (dest, 1, bytes)
            }
            // A world is never written over: what would be overwritten is the player's own.
            Kind::World => {
                let dest = unique(&home.join(folder).join(&self.item));
                let (files, bytes) = self.unpack(&dest)?;
                fs::remove_file(&archive)?;
                (dest, files, bytes)
            }
            Kind::Resources => {
                let scratch = home.join("resources.unpacking");
                let _ = fs::remove_dir_all(&scratch);
                let (files, bytes) = self.unpack(&scratch)?;
                let dest = home.join(folder);
                publish(&scratch, &dest)?;
                fs::remove_file(&archive)?;
                (dest, files, bytes)
            }
        };
        Ok(Installed {
            kind: self.kind,
            path,
            files,
            bytes,
        })
    }

    /// The archive itself, for a caller that only wanted to name it.
    pub fn path(&self) -> &Path {
        &self.path
    }
}

/// Move a finished tree into place, with the old one put aside rather than deleted: the battery can
/// die between the two renames, and `resources.previous` is something [`recover`] can put back,
/// while a half-deleted folder is not. The copy is what costs the disk, and it goes as soon as the
/// new tree is in place.
fn publish(scratch: &Path, dest: &Path) -> Result<()> {
    let previous = dest.with_file_name(format!(
        "{}.previous",
        dest.file_name().unwrap_or_default().to_string_lossy()
    ));
    let _ = fs::remove_dir_all(&previous);
    if dest.exists() {
        fs::rename(dest, &previous)?;
    }
    fs::rename(scratch, dest)?;
    horizon_core::prepared_files::invalidate(dest);
    let _ = fs::remove_dir_all(&previous);
    Ok(())
}

/// Finish an install that a crash or a flat battery left half done, and report what it put back.
/// Called at startup, before the game looks for its resources.
pub fn recover(home: &Path) -> Result<Option<PathBuf>> {
    let scratch = home.join("resources.unpacking");
    let dest = home.join(Kind::Resources.folder());
    let previous = home.join("resources.previous");
    if scratch.is_dir() && !dest.exists() {
        fs::rename(&scratch, &dest)?;
        horizon_core::prepared_files::invalidate(&dest);
        return Ok(Some(dest));
    }
    if !dest.exists() && previous.is_dir() {
        fs::rename(&previous, &dest)?;
        horizon_core::prepared_files::invalidate(&dest);
        return Ok(Some(dest));
    }
    // Both landed: the new tree is in place, so what is left over is scratch and the old copy.
    let _ = fs::remove_dir_all(&scratch);
    let _ = fs::remove_dir_all(&previous);
    Ok(None)
}

/// `path`, or `path (2)` if it is taken, then `path (3)` and so on.
fn unique(path: &Path) -> PathBuf {
    if !path.exists() {
        return path.to_owned();
    }
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let extension = path.extension().and_then(|e| e.to_str());
    let mut index = 2;
    loop {
        let name = match extension {
            Some(extension) => format!("{stem} ({index}).{extension}"),
            None => format!("{stem} ({index})"),
        };
        let candidate = path.with_file_name(name);
        if !candidate.exists() {
            return candidate;
        }
        index += 1;
    }
}

/// Rename, with a copy when the two ends are on different volumes: a pack pulled off a USB stick is
/// still a pack the player meant to install.
fn move_file(from: &Path, to: &Path) -> Result<()> {
    if let Some(parent) = to.parent() {
        fs::create_dir_all(parent)?;
    }
    if fs::rename(from, to).is_ok() {
        return Ok(());
    }
    fs::copy(from, to)?;
    fs::remove_file(from)?;
    Ok(())
}

/// Say what an archive is without touching what it contains.
pub fn classify(path: &Path) -> Result<(Kind, String)> {
    let pack = Pack::open(path)?;
    Ok((pack.kind, pack.item))
}

/// Write a folder as an archive the phone accepts. The folder's own name becomes the single
/// top-level directory, which is the shape a desktop's Compress produces and what [`Pack::open`]
/// reads back, so an archive made here and one made by hand mean the same thing.
pub fn pack(folder: &Path, out: &Path, mut progress: impl FnMut(usize, u64)) -> Result<Installed> {
    let kind = Kind::ALL
        .iter()
        .copied()
        .find(|kind| folder.join(kind.marker()).is_file())
        .ok_or_else(|| {
            format!(
                "{} holds none of the three kinds: no {} at its root",
                folder.display(),
                Kind::ALL
                    .iter()
                    .map(|k| k.marker())
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        })?;
    let top = folder
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .ok_or("the folder has no name to give the archive")?;
    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
    let file = File::create(out)?;
    let written = (|| -> Result<(usize, u64)> {
        let mut zip = ZipWriter::new(file);
        let counts = write_tree(&mut zip, &top, folder, folder, options, &mut progress)?;
        zip.finish()?;
        Ok(counts)
    })();
    match written {
        Ok((files, bytes)) => Ok(Installed {
            kind,
            path: out.to_owned(),
            files,
            bytes,
        }),
        Err(error) => {
            let _ = fs::remove_file(out);
            Err(error)
        }
    }
}

/// Walk `dir` into `zip` under `top`. Symlinks are refused on the way out for the same reason
/// [`Pack::unpack`] refuses them on the way in, and a running world's `session.lock` is left out
/// because it means "someone is in here", not anything worth importing.
fn write_tree(
    zip: &mut ZipWriter<File>,
    top: &str,
    root: &Path,
    dir: &Path,
    options: SimpleFileOptions,
    progress: &mut impl FnMut(usize, u64),
) -> Result<(usize, u64)> {
    let mut files = 0;
    let mut bytes = 0;
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        let kind = entry.file_type()?;
        if kind.is_symlink() {
            return Err(format!("not packing the symbolic link {}", path.display()).into());
        }
        if kind.is_dir() {
            let (more, written) = write_tree(zip, top, root, &path, options, progress)?;
            files += more;
            bytes += written;
            continue;
        }
        if !kind.is_file() {
            continue;
        }
        let relative = path
            .strip_prefix(root)?
            .to_string_lossy()
            .replace('\\', "/");
        if is_junk(&relative) {
            continue;
        }
        zip.start_file(format!("{top}/{relative}"), options)?;
        let mut source = File::open(&path)?;
        bytes += io::copy(&mut source, zip)?;
        files += 1;
        if files % 256 == 0 {
            progress(files, bytes);
        }
    }
    Ok((files, bytes))
}

/// The phone's other door: archives that arrive with no network involved, dropped into `Documents`
/// by the Files app, by Finder's or iTunes' File Sharing, or by a manager someone installed for the
/// purpose. Every zip there is tried; ours are installed and consumed; anything that is not ours is
/// left alone with a word in the log, and anything that was ours but would not unpack is renamed so
/// the next launch does not fail in the same place all over again.
pub fn scan(home: &Path) -> Vec<Installed> {
    let mut installed = Vec::new();
    let Ok(entries) = fs::read_dir(home) else {
        return installed;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() || path.extension().and_then(|e| e.to_str()) != Some("zip") {
            continue;
        }
        let pack = match Pack::open(&path) {
            Ok(pack) => pack,
            // Somebody else's archive, or not an archive: not ours to touch.
            Err(error) => {
                log::info!("Horizon: leaving {} alone: {error}", path.display());
                continue;
            }
        };
        match pack.install(home) {
            Ok(placed) => {
                log::info!(
                    "Horizon: imported {} {} to {} ({} files, {} bytes)",
                    placed.kind.label(),
                    placed
                        .path
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy(),
                    placed.path.display(),
                    placed.files,
                    placed.bytes
                );
                installed.push(placed);
            }
            Err(error) => {
                let parked = path.with_extension("zip.failed");
                log::error!(
                    "Horizon: {} would not import: {error}; leaving it as {}",
                    path.display(),
                    parked.file_name().unwrap_or_default().to_string_lossy()
                );
                let _ = fs::rename(&path, &parked);
            }
        }
    }
    installed
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn temp_home(tag: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("horizon-import-test-{}-{tag}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// A test archive, written by hand because the point of these tests is what a stranger can send.
    fn write_archive(path: &Path, entries: &[(impl AsRef<str>, impl AsRef<str>)]) {
        let mut zip = ZipWriter::new(File::create(path).unwrap());
        let options = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
        for (name, body) in entries {
            zip.start_file(name.as_ref(), options).unwrap();
            zip.write_all(body.as_ref().as_bytes()).unwrap();
        }
        zip.finish().unwrap();
    }

    const PREPARED: &[(&str, &str)] = &[
        ("horizon-resources.json", r#"{"schema":7}"#),
        ("facts.json", "{}"),
        ("assets/minecraft/textures/block/grass.png", "png"),
    ];

    #[test]
    fn prepared_files_unpack_into_the_folder_the_game_reads() {
        let home = temp_home("prepared");
        let archive = home.join("horizon.zip");
        write_archive(&archive, PREPARED);
        let installed = Pack::open(&archive).unwrap().install(&home).unwrap();
        assert_eq!(installed.kind, Kind::Resources);
        assert_eq!(installed.files, 3);
        assert!(home.join("resources/horizon-resources.json").is_file());
        assert!(
            home.join("resources/assets/minecraft/textures/block/grass.png")
                .is_file()
        );
        // The archive is a copy of what the computer still has, so it does not stay.
        assert!(!archive.exists());
        assert!(!home.join("resources.unpacking").exists());
        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn replacing_game_files_invalidates_the_previous_prepared_cache() {
        let home = temp_home("prepared-cache-replace");
        let root = home.join("resources");
        fs::create_dir_all(root.join("assets/minecraft/textures/block")).unwrap();
        fs::write(root.join("horizon-resources.json"), r#"{"schema":7}"#).unwrap();
        let member = "assets/minecraft/textures/block/grass.png";
        fs::write(root.join(member), b"old pixels").unwrap();
        horizon_core::prepared_files::write(&root).unwrap();
        let previous = horizon_core::prepared_files::open(&root).unwrap();
        let archive = home.join("new.zip");
        write_archive(&archive, PREPARED);
        Pack::open(&archive).unwrap().install(&home).unwrap();
        assert_eq!(
            horizon_core::prepared_files::read(&root.join(member)).unwrap(),
            b"png"
        );
        assert_eq!(previous.read(member).unwrap().unwrap(), b"old pixels");
        horizon_core::prepared_files::invalidate(&root);
        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn desktop_prepared_binary_is_optional_after_mobile_import() {
        let home = temp_home("desktop-prepared-cache");
        let desktop = home.join("desktop");
        fs::create_dir_all(desktop.join("assets/minecraft/textures/block")).unwrap();
        fs::write(desktop.join("horizon-resources.json"), r#"{"schema":7}"#).unwrap();
        let member = "assets/minecraft/textures/block/grass.png";
        fs::write(desktop.join(member), b"portable pixels").unwrap();
        horizon_core::prepared_files::write(&desktop).unwrap();
        let archive = home.join("prepared.zip");
        pack(&desktop, &archive, |_, _| {}).unwrap();
        let installed = Pack::open(&archive).unwrap().install(&home).unwrap();
        assert!(
            installed
                .path
                .join(horizon_core::prepared_files::FILE_NAME)
                .is_file()
        );
        // The cache names its original root; the original files remain portable.
        assert!(horizon_core::prepared_files::open(&installed.path).is_none());
        assert_eq!(
            horizon_core::prepared_files::read(&installed.path.join(member)).unwrap(),
            b"portable pixels"
        );
        horizon_core::prepared_files::invalidate(&installed.path);
        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn a_folder_wrapped_archive_means_the_same_thing() {
        // Compressing a folder on a desktop puts the folder's name in front of every entry.
        let home = temp_home("wrapped");
        let archive = home.join("Horizon-26.3.zip");
        let entries: Vec<(String, &str)> = PREPARED
            .iter()
            .map(|(name, body)| (format!("Horizon-26.3/{name}"), *body))
            .collect();
        write_archive(&archive, &entries);
        let (kind, item) = classify(&archive).unwrap();
        assert_eq!(kind, Kind::Resources);
        assert_eq!(item, "Horizon-26.3");
        Pack::open(&archive).unwrap().install(&home).unwrap();
        assert!(home.join("resources/horizon-resources.json").is_file());
        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn one_folder_too_many_still_reads() {
        // Compressing `.local/horizon-client` instead of `26.3` puts the manifest two levels down.
        let home = temp_home("double");
        let archive = home.join("horizon-client.zip");
        let entries: Vec<(String, &str)> = PREPARED
            .iter()
            .map(|(name, body)| (format!("horizon-client/26.3/{name}"), *body))
            .collect();
        write_archive(&archive, &entries);
        let (kind, item) = classify(&archive).unwrap();
        assert_eq!(kind, Kind::Resources);
        assert_eq!(item, "26.3");
        Pack::open(&archive).unwrap().install(&home).unwrap();
        assert!(home.join("resources/horizon-resources.json").is_file());
        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn desktop_droppings_do_not_hide_the_folder() {
        let home = temp_home("droppings");
        let archive = home.join("world.zip");
        write_archive(
            &archive,
            &[
                ("MyWorld/level.dat", "nbt"),
                ("MyWorld/session.lock", "locked"),
                ("__MACOSX/MyWorld/._level.dat", "junk"),
                (".DS_Store", "junk"),
            ],
        );
        let installed = Pack::open(&archive).unwrap().install(&home).unwrap();
        assert_eq!(installed.kind, Kind::World);
        assert_eq!(installed.files, 1);
        assert_eq!(installed.path, home.join("saves/MyWorld"));
        assert!(home.join("saves/MyWorld/level.dat").is_file());
        assert!(!home.join("saves/MyWorld/session.lock").exists());
        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn a_world_is_never_written_over() {
        let home = temp_home("collision");
        fs::create_dir_all(home.join("saves/MyWorld")).unwrap();
        fs::write(home.join("saves/MyWorld/level.dat"), "mine").unwrap();
        let archive = home.join("world.zip");
        write_archive(&archive, &[("MyWorld/level.dat", "theirs")]);
        let installed = Pack::open(&archive).unwrap().install(&home).unwrap();
        assert_eq!(installed.path.file_name().unwrap(), "MyWorld (2)");
        assert_eq!(
            fs::read(home.join("saves/MyWorld/level.dat")).unwrap(),
            b"mine".to_vec()
        );
        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn a_pack_stays_an_archive() {
        let home = temp_home("pack");
        let archive = home.join("faithful.zip");
        write_archive(&archive, &[("pack.mcmeta", "{}"), ("assets/x.png", "p")]);
        let installed = Pack::open(&archive).unwrap().install(&home).unwrap();
        assert_eq!(installed.kind, Kind::ResourcePack);
        assert!(installed.path.ends_with("resourcepacks/faithful.zip"));
        assert!(!home.join("resourcepacks/faithful").exists());
        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn paths_that_escape_are_refused() {
        assert_eq!(inside("a/b.png", ""), Some(PathBuf::from("a/b.png")));
        assert_eq!(
            inside("Top/a/b.png", "Top/"),
            Some(PathBuf::from("a/b.png"))
        );
        assert!(inside("../out", "").is_none());
        assert!(inside("a/../../out", "").is_none());
        assert!(inside("/etc/passwd", "").is_none());
        assert!(inside("C:/Windows/system.ini", "").is_none());
        assert!(inside("", "").is_none());
    }

    #[test]
    fn a_swap_left_half_done_is_finished() {
        let home = temp_home("swap");
        fs::create_dir_all(home.join("resources.unpacking")).unwrap();
        fs::write(
            home.join("resources.unpacking/horizon-resources.json"),
            "{}",
        )
        .unwrap();
        assert_eq!(recover(&home).unwrap(), Some(home.join("resources")));
        assert!(home.join("resources/horizon-resources.json").is_file());
        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn a_crash_between_the_two_renames_puts_the_old_tree_back() {
        let home = temp_home("previous");
        fs::create_dir_all(home.join("resources.previous")).unwrap();
        fs::write(home.join("resources.previous/facts.json"), "{}").unwrap();
        assert_eq!(recover(&home).unwrap(), Some(home.join("resources")));
        assert!(home.join("resources/facts.json").is_file());
        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn a_leftover_scratch_does_not_bury_a_good_tree() {
        // A leftover scratch beside a good tree is dropped, not promoted over it.
        let home = temp_home("stale-scratch");
        fs::create_dir_all(home.join("resources")).unwrap();
        fs::write(home.join("resources/facts.json"), "{}").unwrap();
        fs::create_dir_all(home.join("resources.unpacking")).unwrap();
        assert!(recover(&home).unwrap().is_none());
        assert!(!home.join("resources.unpacking").exists());
        assert!(home.join("resources/facts.json").is_file());
        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn replacing_the_game_files_moves_the_old_tree_out_afterwards() {
        let home = temp_home("replace");
        fs::create_dir_all(home.join("resources")).unwrap();
        fs::write(home.join("resources/old.json"), "old").unwrap();
        let archive = home.join("horizon.zip");
        write_archive(&archive, PREPARED);
        Pack::open(&archive).unwrap().install(&home).unwrap();
        assert!(home.join("resources/horizon-resources.json").is_file());
        assert!(!home.join("resources/old.json").exists());
        assert!(!home.join("resources.previous").exists());
        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn anything_else_is_refused_without_touching_the_home() {
        let home = temp_home("unknown");
        let archive = home.join("holiday-photos.zip");
        write_archive(&archive, &[("beach.jpg", "x"), ("sunset.jpg", "y")]);
        let error = classify(&archive).unwrap_err().to_string();
        assert!(error.contains("not Horizon's game files"), "{error}");
        assert_eq!(scan(&home).len(), 0);
        // Somebody else's files are not ours to rename or delete.
        assert!(archive.exists());
        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn an_archive_that_is_ours_but_will_not_unpack_is_parked() {
        let home = temp_home("parked");
        let archive = home.join("horizon.zip");
        write_archive(
            &archive,
            &[
                ("horizon-resources.json", "{}"),
                (
                    "../outside.txt",
                    "written beside the archive, not inside it",
                ),
            ],
        );
        let error = Pack::open(&archive)
            .unwrap()
            .install(&home)
            .unwrap_err()
            .to_string();
        assert!(error.contains("escapes"), "{error}");
        assert!(!home.join("resources").exists());
        assert!(!home.join("outside.txt").exists());
        // The next launch must not fail in the same place all over again, so it is put aside.
        assert!(scan(&home).is_empty());
        assert!(!archive.exists());
        assert!(home.join("horizon.zip.failed").is_file());
        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn a_real_folder_round_trips_through_the_archive() {
        let home = temp_home("round");
        let source = home.join("Horizon-26.3");
        fs::create_dir_all(source.join("assets/minecraft/models")).unwrap();
        fs::write(source.join("horizon-resources.json"), r#"{"schema":7}"#).unwrap();
        fs::write(source.join("assets/minecraft/models/block.json"), "{}").unwrap();
        fs::write(source.join("session.lock"), "someone is inside").unwrap();
        let archive = home.join("out.zip");
        let packed = pack(&source, &archive, |_, _| {}).unwrap();
        assert_eq!(packed.kind, Kind::Resources);
        assert_eq!(packed.files, 2);
        fs::remove_dir_all(&source).unwrap();
        let installed = Pack::open(&archive).unwrap().install(&home).unwrap();
        assert_eq!(installed.files, 2);
        assert!(home.join("resources/horizon-resources.json").is_file());
        assert!(
            home.join("resources/assets/minecraft/models/block.json")
                .is_file()
        );
        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn the_phone_reads_the_folder_an_import_wrote() {
        let dir = temp_home("root");
        let sibling = dir.join("bundle");
        let home = dir.join("Documents");
        fs::create_dir_all(&home).unwrap();
        // Nothing prepared anywhere: the caller's own fallback, and nothing from here.
        assert_eq!(resources_root(&sibling, None), None);
        assert_eq!(resources_root(&sibling, Some(&home)), None);
        fs::create_dir_all(home.join("resources")).unwrap();
        fs::write(home.join("resources/horizon-resources.json"), "{}").unwrap();
        assert_eq!(
            resources_root(&sibling, Some(&home)),
            Some(home.join("resources"))
        );
        // A desktop never looks there: its working directory is a checkout, not a sandbox.
        assert_eq!(resources_root(&sibling, None), None);
        // Assets shipped in the bundle win, as they always did.
        fs::create_dir_all(&sibling).unwrap();
        fs::write(sibling.join("horizon-resources.json"), "{}").unwrap();
        assert_eq!(resources_root(&sibling, Some(&home)), Some(sibling));
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn an_archive_dropped_in_documents_is_where_the_game_then_looks() {
        let dir = temp_home("usb");
        let home = dir.join("Documents");
        fs::create_dir_all(&home).unwrap();
        write_archive(&home.join("Horizon-26.3.zip"), PREPARED);
        assert_eq!(scan(&home).len(), 1);
        assert_eq!(
            resources_root(&dir.join("Horizon.app/resources"), Some(&home)).unwrap(),
            home.join("resources")
        );
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn names_are_labels_not_paths() {
        assert_eq!(sanitize("../../etc"), "etc");
        assert_eq!(sanitize(".."), "imported");
        assert_eq!(sanitize(" My World 26.3 "), "My World 26.3");
        // An escape sequence is dropped with the brackets: what is left cannot paint a terminal.
        assert_eq!(sanitize("o\u{1b}[2J"), "o2J");
        assert_eq!(sanitize(&"a".repeat(100)).len(), 64);
    }
}
