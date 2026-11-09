//! A pack is a folder or a `.zip` whose root holds a folder named `shaders`; everything else at
//! the root (licences, readmes, decoy folders) is ignored (spec 1.1). Every path a pack uses is
//! relative to that `shaders` folder, written with `/`.

use std::collections::BTreeMap;
use std::fmt;
use std::fs;
use std::io::{self, Cursor, Read};
use std::path::{Path, PathBuf};

/// The most bytes one file of a zipped pack unpacks to: far above any `.properties`, `.lang` or
/// GLSL file, with room for the largest textures packs carry.
pub const MAX_FILE: u64 = 64 << 20;
use std::sync::Arc;

use zip::ZipArchive;

const ROOT: &str = "shaders";
// Resource packs are untrusted. A small ZIP may advertise enormous uncompressed entries.
// Bound both the archive held in RAM and each expanded file before reserving memory.
// These limits also apply to folder packs and to files changed after a pack was opened.
const MAX_ARCHIVE_BYTES: u64 = 512 * 1024 * 1024;
const MAX_ENTRY_BYTES: u64 = 256 * 1024 * 1024;

fn bounded_read(
    mut reader: impl Read,
    declared: u64,
    limit: u64,
    name: &str,
) -> io::Result<Vec<u8>> {
    let too_large = || {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "{name}: exceeds the {} MiB shader-pack read limit",
                limit / (1024 * 1024)
            ),
        )
    };
    if declared > limit {
        return Err(too_large());
    }
    let mut out = Vec::new();
    let mut chunk = [0u8; 8192];
    loop {
        let n = reader.read(&mut chunk)?;
        if n == 0 {
            return Ok(out);
        }
        if out.len() as u64 + n as u64 > limit {
            return Err(too_large());
        }
        out.try_reserve(n)
            .map_err(|_| io::Error::other(format!("{name}: cannot allocate shader-pack data")))?;
        out.extend_from_slice(&chunk[..n]);
    }
}

fn bounded_file(path: &Path, limit: u64) -> io::Result<Vec<u8>> {
    let file = fs::File::open(path)?;
    let size = file.metadata()?.len();
    bounded_read(file, size, limit, &path.to_string_lossy())
}

pub struct Pack {
    name: String,
    files: Files,
    /// The values the player chose for the pack's options, applied to its sources as they are
    /// read (`options::apply`).
    chosen: BTreeMap<String, String>,
}

enum Files {
    /// The pack's `shaders` folder.
    Folder(PathBuf),
    /// The archive and its entries under `shaders/`, by their path inside that folder.
    Zip {
        archive: ZipArchive<Cursor<Arc<[u8]>>>,
        entries: BTreeMap<String, usize>,
    },
}

#[derive(Debug)]
pub enum PackError {
    Io(io::Error),
    Zip(String),
    /// A readable folder or archive with no `shaders` folder at its root.
    NoShadersFolder,
}

impl fmt::Display for PackError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::Io(e) => write!(f, "{e}"),
            Self::Zip(e) => write!(f, "not a readable zip: {e}"),
            Self::NoShadersFolder => f.write_str("no shaders folder at the root of the pack"),
        }
    }
}

impl std::error::Error for PackError {}

impl From<io::Error> for PackError {
    fn from(e: io::Error) -> Self {
        Self::Io(e)
    }
}

impl Pack {
    /// Opens a pack folder or a `.zip` archive.
    pub fn open(path: &Path) -> Result<Self, PackError> {
        let name = path
            .file_name()
            .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
        let files = if path.is_dir() {
            let root = path.join(ROOT);
            if !root.is_dir() {
                return Err(PackError::NoShadersFolder);
            }
            Files::Folder(root)
        } else {
            Self::zip_files(bounded_file(path, MAX_ARCHIVE_BYTES)?)?
        };
        let pack = Self {
            name,
            files,
            chosen: BTreeMap::new(),
        };
        if let Files::Folder(root) = &pack.files {
            for name in pack.all_files() {
                if fs::metadata(root.join(&name))?.len() > MAX_ENTRY_BYTES {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        format!("{name}: exceeds the 256 MiB shader-pack read limit"),
                    )
                    .into());
                }
            }
        }
        Ok(pack)
    }

    /// An archive held in memory; `name` is what [`Pack::name`] returns.
    pub fn from_zip_bytes(name: &str, bytes: Vec<u8>) -> Result<Self, PackError> {
        let files = Self::zip_files(bytes)?;
        Ok(Self {
            name: name.to_owned(),
            files,
            chosen: BTreeMap::new(),
        })
    }

    fn zip_files(bytes: Vec<u8>) -> Result<Files, PackError> {
        if bytes.len() as u64 > MAX_ARCHIVE_BYTES {
            return Err(PackError::Zip(
                "archive exceeds the 512 MiB shader-pack read limit".into(),
            ));
        }
        let mut archive = ZipArchive::new(Cursor::new(Arc::from(bytes)))
            .map_err(|e| PackError::Zip(e.to_string()))?;
        let mut entries = BTreeMap::new();
        for i in 0..archive.len() {
            let Some(name) = archive.name_for_index(i) else {
                continue;
            };
            let name = name
                .map_err(|e| PackError::Zip(e.to_string()))?
                .replace('\\', "/");
            if let Some(inner) = name.strip_prefix(ROOT).and_then(|n| n.strip_prefix('/'))
                && !inner.is_empty()
                && !inner.ends_with('/')
            {
                let size = archive
                    .by_index_raw(i)
                    .map_err(|e| PackError::Zip(e.to_string()))?
                    .size();
                if size > MAX_ENTRY_BYTES {
                    return Err(PackError::Zip(format!(
                        "{inner}: exceeds the 256 MiB shader-pack read limit"
                    )));
                }
                entries.insert(inner.to_owned(), i);
            }
        }
        if entries.is_empty() {
            return Err(PackError::NoShadersFolder);
        }
        Ok(Files::Zip { archive, entries })
    }

    /// The pack with options at the values in `chosen` (name to value, `true` or `false` for a
    /// switch) instead of their defaults. A value the option doesn't list (a file edited by
    /// hand) is left out.
    pub fn with_chosen(mut self, chosen: BTreeMap<String, String>) -> Self {
        let options = crate::options(&self);
        self.chosen = chosen
            .into_iter()
            .filter(|(name, value)| options.get(name).is_some_and(|o| o.values.contains(value)))
            .collect();
        self
    }

    /// The folder or file name the pack was opened from (`BSL.zip`).
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The bytes of a file, by its path inside `shaders` (`lang/en_us.lang`). `None` when it
    /// doesn't exist, the path leaves the folder, or a zipped file unpacks to more than
    /// [`MAX_FILE`].
    pub fn read(&self, path: &str) -> Option<Vec<u8>> {
        self.read_capped(path, MAX_FILE)
    }

    /// Apply a caller's smaller cap without trusting the archive's declared sizes.
    fn read_capped(&self, path: &str, cap: u64) -> Option<Vec<u8>> {
        self.read_capped_checked(path, cap.min(MAX_FILE))
            .ok()
            .flatten()
    }

    /// Like `read`, but preserves resource-limit and I/O errors for loader diagnostics.
    pub fn read_checked(&self, path: &str) -> io::Result<Option<Vec<u8>>> {
        self.read_capped_checked(path, MAX_FILE)
    }

    fn read_capped_checked(&self, path: &str, cap: u64) -> io::Result<Option<Vec<u8>>> {
        let Some(path) = normalize(path) else {
            return Ok(None);
        };
        match &self.files {
            Files::Folder(root) => match bounded_file(&root.join(&path), cap) {
                Ok(bytes) => Ok(Some(bytes)),
                Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
                Err(error) => Err(error),
            },
            Files::Zip { archive, entries } => {
                let Some(&index) = entries.get(&path) else {
                    return Ok(None);
                };
                // A clone shares archive bytes and the central directory; reading needs &mut.
                let mut archive = archive.clone();
                let entry = archive.by_index(index).map_err(io::Error::other)?;
                let size = entry.size();
                bounded_read(entry, size, cap, &path).map(Some)
            }
        }
    }

    /// A text file, decoded as UTF-8 (invalid bytes replaced) without its byte-order mark, with
    /// the chosen option values in its sources.
    pub fn read_text(&self, path: &str) -> Option<String> {
        self.read_text_checked(path).ok().flatten()
    }

    pub fn read_text_checked(&self, path: &str) -> io::Result<Option<String>> {
        let Some(bytes) = self.read_checked(path)? else {
            return Ok(None);
        };
        let text = String::from_utf8_lossy(&bytes);
        let text = text.strip_prefix('\u{feff}').unwrap_or(&text);
        let source = !path.ends_with(".properties") && !path.ends_with(".lang");
        Ok(Some(if source && !self.chosen.is_empty() {
            crate::options::apply(text, &self.chosen)
        } else {
            text.to_owned()
        }))
    }

    pub fn exists(&self, path: &str) -> bool {
        let Some(path) = normalize(path) else {
            return false;
        };
        match &self.files {
            Files::Folder(root) => root.join(path).is_file(),
            Files::Zip { entries, .. } => entries.contains_key(&path),
        }
    }

    /// The names of the files directly inside a folder of the pack (`""` for `shaders` itself),
    /// sorted.
    pub fn list(&self, dir: &str) -> Vec<String> {
        let Some(dir) = normalize(dir) else {
            return Vec::new();
        };
        let mut names: Vec<String> = match &self.files {
            Files::Folder(root) => fs::read_dir(root.join(&dir))
                .into_iter()
                .flatten()
                .flatten()
                .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .collect(),
            Files::Zip { entries, .. } => {
                let prefix = if dir.is_empty() {
                    String::new()
                } else {
                    format!("{dir}/")
                };
                entries
                    .keys()
                    .filter_map(|k| k.strip_prefix(&prefix))
                    .filter(|rest| !rest.contains('/'))
                    .map(str::to_owned)
                    .collect()
            }
        };
        names.sort();
        names
    }

    /// Every file of the pack, by its path inside `shaders`, sorted.
    pub fn all_files(&self) -> Vec<String> {
        let mut names = match &self.files {
            Files::Folder(root) => {
                let mut names = Vec::new();
                let mut dirs = vec![String::new()];
                while let Some(dir) = dirs.pop() {
                    for e in fs::read_dir(root.join(&dir))
                        .into_iter()
                        .flatten()
                        .flatten()
                    {
                        let name = e.file_name().to_string_lossy().into_owned();
                        let path = if dir.is_empty() {
                            name
                        } else {
                            format!("{dir}/{name}")
                        };
                        match e.file_type() {
                            Ok(t) if t.is_dir() => dirs.push(path),
                            Ok(t) if t.is_file() => names.push(path),
                            _ => {}
                        }
                    }
                }
                names
            }
            Files::Zip { entries, .. } => entries.keys().cloned().collect(),
        };
        names.sort();
        names
    }
}

/// `a/./b/../c` → `a/c`; `None` for a path that climbs out of `shaders`.
pub(crate) fn normalize(path: &str) -> Option<String> {
    let mut parts: Vec<&str> = Vec::new();
    for part in path.split(['/', '\\']) {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            _ => parts.push(part),
        }
    }
    Some(parts.join("/"))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::io::Write;

    /// A zip of `(path, contents)` pairs, paths from the archive root.
    pub(crate) fn zip<C: AsRef<[u8]>>(files: &[(&str, C)]) -> Vec<u8> {
        let mut w = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);
        for (path, contents) in files {
            w.start_file(*path, options).unwrap();
            w.write_all(contents.as_ref()).unwrap();
        }
        w.finish().unwrap().into_inner()
    }

    /// A fresh empty folder under the system's temp folder.
    pub(crate) fn temp_dir(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("horizon-shaders-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn a_zipped_file_past_the_cap_reads_as_missing() {
        // A deflate bomb: 64 KiB of spaces zip to almost nothing. Read with a 16 KiB cap here
        // (MAX_FILE in the game), it is refused whatever it declares; the rest of the pack reads.
        let bytes = zip(&[
            ("shaders/shaders.properties", vec![b' '; 64 << 10]),
            ("shaders/lib/common.glsl", b"void main() {}".to_vec()),
        ]);
        assert!(bytes.len() < 4 << 10);
        let pack = Pack::from_zip_bytes("P.zip", bytes).unwrap();
        assert_eq!(pack.read_capped("shaders.properties", 16 << 10), None);
        assert_eq!(
            pack.read_capped("shaders.properties", 64 << 10)
                .unwrap()
                .len(),
            64 << 10
        );
        assert_eq!(pack.read("lib/common.glsl").unwrap(), b"void main() {}");
    }

    #[test]
    fn zip_packs_read_only_the_shaders_folder() {
        let bytes = zip(&[
            ("License.txt", "x"),
            ("shaderFile_A.placebo/shaders.properties", "decoy"),
            ("shaders/shaders.properties", "sun=false"),
            ("shaders/lang/en_US.lang", "a=b"),
            ("shaders/lang/fr_FR.lang", "a=c"),
            ("shaders/lib/common.glsl", ""),
        ]);
        let pack = Pack::from_zip_bytes("P.zip", bytes).unwrap();
        assert_eq!(pack.read_text("shaders.properties").unwrap(), "sun=false");
        assert!(pack.exists("lib/common.glsl"));
        assert!(pack.exists("lib/../shaders.properties"));
        assert!(!pack.exists("../License.txt"));
        assert!(!pack.exists("License.txt"));
        assert_eq!(pack.list("lang"), ["en_US.lang", "fr_FR.lang"]);
        assert_eq!(pack.list(""), ["shaders.properties"]);
    }

    #[test]
    fn archives_without_a_shaders_folder_are_not_packs() {
        let bytes = zip(&[("Pack/shaders/final.fsh", ""), ("readme.txt", "")]);
        assert!(matches!(
            Pack::from_zip_bytes("P.zip", bytes),
            Err(PackError::NoShadersFolder)
        ));
        assert!(matches!(
            Pack::from_zip_bytes("P.zip", b"not a zip".to_vec()),
            Err(PackError::Zip(_))
        ));
    }

    #[test]
    fn folder_packs_read_the_same_way() {
        let dir = temp_dir("folder");
        fs::create_dir_all(dir.join("shaders/lang")).unwrap();
        fs::write(
            dir.join("shaders/lang/en_us.lang"),
            "\u{feff}option.A=Alpha",
        )
        .unwrap();
        fs::write(dir.join("secret.txt"), "outside").unwrap();
        let pack = Pack::open(&dir).unwrap();
        assert_eq!(pack.read_text("lang/en_us.lang").unwrap(), "option.A=Alpha");
        assert_eq!(pack.list("lang"), ["en_us.lang"]);
        assert!(pack.read("../secret.txt").is_none());
        fs::remove_dir_all(&dir).unwrap();
    }
    #[test]
    fn oversized_zip_metadata_is_rejected_before_entry_allocation() {
        let mut bytes = zip(&[("shaders/final.fsh", "void main(){}")]);
        let at = bytes.windows(4).position(|b| b == b"PK\x01\x02").unwrap();
        bytes[at + 24..at + 28].copy_from_slice(&u32::MAX.to_le_bytes());
        let error = match Pack::from_zip_bytes("oversized.zip", bytes) {
            Ok(_) => panic!("oversized advertised entry accepted"),
            Err(error) => error.to_string(),
        };
        assert!(
            error.contains("final.fsh") && error.contains("256 MiB"),
            "{error}"
        );
    }

    #[test]
    fn bounded_reads_limit_actual_decompression_not_only_declared_size() {
        struct NeverRead;
        impl Read for NeverRead {
            fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
                panic!("oversized metadata must be rejected before read");
            }
        }
        assert!(bounded_read(NeverRead, u64::MAX, 64, "test").is_err());
        let bytes = zip(&[("shaders/texture.bin", vec![0u8; 4096])]);
        let mut archive = ZipArchive::new(Cursor::new(bytes)).unwrap();
        let entry = archive.by_index(0).unwrap();
        // Even a lying size cannot cause unbounded expansion.
        assert!(bounded_read(entry, 1, 64, "texture.bin").is_err());
        assert_eq!(
            bounded_read(Cursor::new(b"ok"), 2, 64, "small").unwrap(),
            b"ok"
        );
    }

    #[test]
    fn folder_and_archive_limits_report_clear_errors_without_large_allocations() {
        let dir = temp_dir("size-limits");
        fs::create_dir_all(dir.join("shaders")).unwrap();
        let path = dir.join("shaders/final.fsh");
        fs::write(&path, "void main(){}").unwrap();
        let pack = Pack::open(&dir).unwrap();
        let file = fs::OpenOptions::new().write(true).open(&path).unwrap();
        file.set_len(MAX_ENTRY_BYTES + 1).unwrap();
        let pp = crate::preprocess(&pack, "final.fsh", &crate::Macros::standard());
        assert!(pp.diagnostics.iter().any(|d| d.message.contains("64 MiB")));
        let error = Pack::open(&dir).err().unwrap().to_string();
        assert!(
            error.contains("final.fsh") && error.contains("256 MiB"),
            "{error}"
        );
        let archive = dir.join("large.zip");
        fs::File::create(&archive)
            .unwrap()
            .set_len(MAX_ARCHIVE_BYTES + 1)
            .unwrap();
        assert!(
            Pack::open(&archive)
                .err()
                .unwrap()
                .to_string()
                .contains("512 MiB")
        );
        fs::remove_dir_all(dir).unwrap();
    }
}
