//! `horizon-data.bin`: the prepared folder's small files in one file.
//!
//! A start reads about 22,000 files of the prepared folder (recipes, tags,
//! models, textures...). Each costs a path lookup and an open, a seek on a hard
//! disk and, on Windows, a Defender scan: about 1 ms a file after a signature
//! update, over a minute at the first start (#1961). This file holds the same
//! bytes, written from the folder itself on the owner's machine (never
//! published), and is read through one open handle.
//!
//! A reader asks it first and falls back to the folder for what it doesn't hold
//! (sounds, the top-level tables), and for everything when it is missing,
//! written for another root (a resource pack's or a content mod's mirror of
//! this one), from another preparation (its copy of `horizon-resources.json`
//! differs), or damaged. `HORIZON_NO_DATA_FILE=1` turns it off.
//!
//! Layout, little-endian, every part 8-aligned:
//! - `HZDATA\0\0`, the version (u32), the file count (u32), the lengths (u64)
//!   of the root's path, of `horizon-resources.json` and of the name pool;
//! - the root's canonical path and the bytes of `horizon-resources.json`;
//! - the entries, in name order: name offset (u32) and length (u32) in the
//!   pool, data offset (u64, from the start of the file) and length (u64);
//! - the name pool: `/`-separated paths from the root;
//! - the files' bytes.
use std::{
    fs::{self, File},
    io::{self, BufWriter, Read, Write},
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex, PoisonError,
        atomic::{AtomicUsize, Ordering},
    },
};

/// The file, in the prepared folder's root.
pub const FILE_NAME: &str = "horizon-data.bin";
/// The file it checks its preparation against.
const SOURCE: &str = "horizon-resources.json";
/// The folders it holds, with their subfolders: the resources, and the
/// structure templates world generation loads one by one.
const FOLDERS: [&str; 3] = ["data", "assets", "worldgen/structure"];
const MAGIC: [u8; 8] = *b"HZDATA\0\0";
/// 2: `worldgen/structure` joined the folders (a version 1 file lacks it).
const VERSION: u32 = 2;
const HEADER: usize = 40;
const ENTRY: usize = 24;
// This is an optional acceleration cache. An implausibly large index or member
// falls back to the ordinary files instead of allocating from untrusted lengths.
const MAX_INDEX_BYTES: u64 = 32 << 20;
const MAX_MEMBER_BYTES: u64 = 256 << 20;

type Opened = Vec<(PathBuf, Option<Arc<PreparedFiles>>)>;
static OPENED: Mutex<Opened> = Mutex::new(Vec::new());

/// Whether `name` (from the root) is a file this holds: under its folders,
/// and not a sound (hundreds of MB, read one at a time while playing).
fn holds_file(name: &str) -> bool {
    holds_folder(name) && !name.ends_with(".ogg")
}

/// Whether `dir` (from the root) is one of the folders this holds, or under one.
fn holds_folder(dir: &str) -> bool {
    FOLDERS
        .iter()
        .any(|f| dir == *f || dir.strip_prefix(f).is_some_and(|r| r.starts_with('/')))
}

struct Entry {
    name: (u32, u32),
    data: (u64, u64),
}

/// An open `horizon-data.bin`.
pub struct PreparedFiles {
    file: File,
    names: Box<str>,
    entries: Box<[Entry]>,
}

impl PreparedFiles {
    fn name(&self, entry: &Entry) -> &str {
        &self.names[entry.name.0 as usize..][..entry.name.1 as usize]
    }
    /// The bytes of `name` (`/`-separated, from the root); `None` when this
    /// doesn't hold it (read it from the folder).
    pub fn read(&self, name: &str) -> Option<io::Result<Vec<u8>>> {
        self.read_head(name, u64::MAX)
    }
    /// At most the first `max` bytes of `name`, as [`PreparedFiles::read`].
    pub fn read_head(&self, name: &str, max: u64) -> Option<io::Result<Vec<u8>>> {
        let i = self
            .entries
            .binary_search_by(|e| self.name(e).cmp(name))
            .ok()?;
        let (offset, len) = self.entries[i].data;
        let mut bytes = vec![0; len.min(max) as usize];
        Some(read_at(&self.file, &mut bytes, offset).map(|()| bytes))
    }
    /// The files under `dir` (`/`-separated, from the root) and its
    /// subfolders, in name order, by their names from the root; `None` when
    /// this doesn't hold that folder or has no file in it (list the folder).
    pub fn list(&self, dir: &str) -> Option<Vec<&str>> {
        if !holds_folder(dir) {
            return None;
        }
        let prefix = format!("{dir}/");
        let start = self
            .entries
            .partition_point(|e| self.name(e) < prefix.as_str());
        let names: Vec<&str> = self.entries[start..]
            .iter()
            .map(|e| self.name(e))
            .take_while(|n| n.starts_with(&prefix))
            .collect();
        (!names.is_empty()).then_some(names)
    }
    fn open(root: &Path) -> io::Result<Option<Self>> {
        let path = root.join(FILE_NAME);
        // A mirror's link to another root's file.
        if fs::symlink_metadata(&path)?.file_type().is_symlink() {
            return Ok(None);
        }
        let mut file = File::open(&path)?;
        let size = file.metadata()?.len();
        let mut header = [0; HEADER];
        file.read_exact(&mut header)?;
        let u32_at = |at: usize| u32::from_le_bytes(header[at..at + 4].try_into().unwrap());
        let u64_at = |at: usize| u64::from_le_bytes(header[at..at + 8].try_into().unwrap());
        if header[..8] != MAGIC || u32_at(8) != VERSION {
            return Ok(None);
        }
        let (count, root_len, source_len, pool_len) =
            (u32_at(12) as u64, u64_at(16), u64_at(24), u64_at(32));
        let canonical = fs::canonicalize(root)?;
        // Check the small identity fields before interpreting attacker-controlled offsets.
        if root_len != canonical.as_os_str().as_encoded_bytes().len() as u64
            || source_len > MAX_INDEX_BYTES
            || source_len != fs::metadata(root.join(SOURCE))?.len()
        {
            return Ok(None);
        }
        let index_len = count * ENTRY as u64; // count came from a u32
        let Some(index_end) = [root_len, source_len, pool_len]
            .into_iter()
            .try_fold(HEADER as u64 + index_len, |total, len| {
                total.checked_add(len.checked_next_multiple_of(8)?)
            })
        else {
            return Ok(None);
        };
        if index_end > size || index_end - HEADER as u64 > MAX_INDEX_BYTES {
            return Ok(None);
        }
        let mut index = vec![0; (index_end - HEADER as u64) as usize];
        file.read_exact(&mut index)?;
        let (written_root, rest) = index.split_at(pad(root_len) as usize);
        let (source, rest) = rest.split_at(pad(source_len) as usize);
        let (raw_entries, pool) = rest.split_at(index_len as usize);
        if written_root[..root_len as usize] != *canonical.as_os_str().as_encoded_bytes()
            || source[..source_len as usize] != *fs::read(root.join(SOURCE))?
        {
            return Ok(None);
        }
        let Ok(names) = std::str::from_utf8(&pool[..pool_len as usize]) else {
            return Ok(None);
        };
        let entries: Box<[Entry]> = raw_entries
            .as_chunks::<ENTRY>()
            .0
            .iter()
            .map(|e| {
                let at = |i: usize, n: usize| {
                    let mut b = [0; 8];
                    b[..n].copy_from_slice(&e[i..i + n]);
                    u64::from_le_bytes(b)
                };
                Entry {
                    name: (at(0, 4) as u32, at(4, 4) as u32),
                    data: (at(8, 8), at(16, 8)),
                }
            })
            .collect();
        let files = Self {
            file,
            names: names.into(),
            entries,
        };
        // Every name in the pool, in strict order; every file's bytes inside the file.
        let mut last: Option<&str> = None;
        for e in files.entries.iter() {
            let (at, len) = (e.name.0 as usize, e.name.1 as usize);
            let name = at.checked_add(len).and_then(|end| files.names.get(at..end));
            let in_file = e
                .data
                .0
                .checked_add(e.data.1)
                .is_some_and(|end| end <= size);
            if name.is_none_or(|n| {
                last.is_some_and(|l| l >= n)
                    || !holds_file(n)
                    || n.split('/')
                        .any(|part| part.is_empty() || part == "." || part == "..")
                    || n.contains('\\')
            }) || !in_file
                || e.data.0 < index_end
                || e.data.1 > MAX_MEMBER_BYTES
            {
                return Ok(None);
            }
            last = name;
        }
        Ok(Some(files))
    }
}

/// `root`'s file, opened once per process; `None` when it has no usable one.
pub fn open(root: &Path) -> Option<Arc<PreparedFiles>> {
    let mut opened = OPENED.lock().unwrap_or_else(PoisonError::into_inner);
    if let Some((_, files)) = opened.iter().find(|(r, _)| r == root) {
        return files.clone();
    }
    let files = if std::env::var_os("HORIZON_NO_DATA_FILE").is_some() {
        None
    } else {
        match PreparedFiles::open(root) {
            Ok(files) => files.map(Arc::new),
            Err(e) if e.kind() == io::ErrorKind::NotFound => None,
            Err(e) => {
                eprintln!("Horizon: {} not used: {e}", root.join(FILE_NAME).display());
                None
            }
        }
    };
    opened.push((root.to_owned(), files.clone()));
    files
}

/// A root was replaced or its cache was newly written. Existing readers keep
/// their Arc snapshot, but subsequent opens re-check the current root and file.
pub fn invalidate(root: &Path) {
    let canonical = fs::canonicalize(root).ok();
    OPENED
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .retain(|(path, _)| {
            path != root
                && canonical
                    .as_ref()
                    .is_none_or(|root| fs::canonicalize(path).ok().as_ref() != Some(root))
        });
}

/// Release the engine session's cached handles before returning to its launcher.
pub fn invalidate_all() {
    OPENED
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .clear();
}

/// The file holding `path` (a file or folder under a prepared root) and
/// `path` from that root; `None` when no root above it has a usable file.
pub fn find(path: &Path) -> Option<(Arc<PreparedFiles>, String)> {
    path.ancestors().skip(1).find_map(|root| {
        // Only a root holding one of the folders: no lookup for other paths.
        let rel = path.strip_prefix(root).ok()?;
        let rel = rel.to_str()?.replace(std::path::MAIN_SEPARATOR, "/");
        holds_folder(&rel).then_some(())?;
        Some((open(root)?, rel))
    })
}

/// `path`'s bytes: from its prepared root's data file when that holds it,
/// where a file the data file doesn't have is missing (`NotFound`, without
/// asking the disk: a texture's `.mcmeta` or LabPBR maps, mostly absent);
/// else from the disk.
pub fn read(path: &Path) -> io::Result<Vec<u8>> {
    match find(path) {
        Some((files, rel)) if holds_file(&rel) => files
            .read(&rel)
            .unwrap_or_else(|| Err(io::ErrorKind::NotFound.into())),
        _ => fs::read(path),
    }
}

/// The files under `dir` (a folder of a prepared root) and its subfolders, as
/// `/`-separated paths from `dir` in name order, from the root's data file
/// without listing the disk: empty when it holds none there. `None` when no
/// data file holds `dir` (walk the folder).
pub fn files_under(dir: &Path) -> Option<Vec<String>> {
    let (files, rel) = find(dir)?;
    let names = files.list(&rel).unwrap_or_default();
    Some(
        names
            .into_iter()
            .map(|name| name[rel.len() + 1..].to_owned())
            .collect(),
    )
}

/// At most the first `max` bytes of `path`, from where [`read`] takes them:
/// a header, without reading the whole file.
pub fn read_head(path: &Path, max: u64) -> io::Result<Vec<u8>> {
    match find(path) {
        Some((files, rel)) if holds_file(&rel) => files
            .read_head(&rel, max)
            .unwrap_or_else(|| Err(io::ErrorKind::NotFound.into())),
        _ => {
            let mut bytes = Vec::new();
            io::Read::read_to_end(&mut io::Read::take(File::open(path)?, max), &mut bytes)?;
            Ok(bytes)
        }
    }
}

/// [`read`] as UTF-8 text.
pub fn read_to_string(path: &Path) -> io::Result<String> {
    String::from_utf8(read(path)?).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
}

/// Write `root`'s file from its folders: through a temporary file renamed
/// into place, so a reader sees the old file or the new one. Nothing is
/// written for a root without `horizon-resources.json`.
pub fn write(root: &Path) -> io::Result<()> {
    write_with_readers(root, 1)
}

/// [`write`], reading the folders' files on `readers` threads: a virus scanner
/// delays each file's first open, and scans several at once when several are
/// asked for. The threads run at normal priority: not for a game being played.
pub fn write_with_readers(root: &Path, readers: usize) -> io::Result<()> {
    let source = fs::read(root.join(SOURCE))?;
    let canonical = fs::canonicalize(root)?;
    let root_bytes = canonical.as_os_str().as_encoded_bytes();
    let mut files = Vec::new();
    for folder in FOLDERS {
        walk(&root.join(folder), folder, &mut files)?;
    }
    files.sort_by(|a, b| a.0.cmp(&b.0));
    let pool_len = files.iter().try_fold(0usize, |total, (name, _, _)| {
        total
            .checked_add(name.len())
            .ok_or_else(|| io::Error::other("prepared index too large"))
    })?;
    if pool_len as u64 > MAX_INDEX_BYTES || files.len() as u64 > MAX_INDEX_BYTES / ENTRY as u64 {
        return Err(io::Error::other("prepared index too large"));
    }
    let data_start = HEADER as u64
        + pad(root_bytes.len() as u64)
        + pad(source.len() as u64)
        + (files.len() * ENTRY) as u64
        + pad(pool_len as u64);
    if data_start - HEADER as u64 > MAX_INDEX_BYTES {
        return Err(io::Error::other("prepared index too large"));
    }
    let tmp = root.join(format!("{FILE_NAME}.{}.tmp", std::process::id()));
    remove_abandoned(root);
    let result = (|| {
        let mut out = BufWriter::new(File::create(&tmp)?);
        out.write_all(&MAGIC)?;
        out.write_all(&VERSION.to_le_bytes())?;
        out.write_all(
            &u32::try_from(files.len())
                .map_err(io::Error::other)?
                .to_le_bytes(),
        )?;
        for len in [root_bytes.len(), source.len(), pool_len] {
            out.write_all(&(len as u64).to_le_bytes())?;
        }
        write_padded(&mut out, root_bytes)?;
        write_padded(&mut out, &source)?;
        let (mut name_at, mut data_at) = (0u32, data_start);
        for (name, _, len) in &files {
            out.write_all(&name_at.to_le_bytes())?;
            out.write_all(&(name.len() as u32).to_le_bytes())?;
            out.write_all(&data_at.to_le_bytes())?;
            out.write_all(&len.to_le_bytes())?;
            name_at += name.len() as u32;
            data_at += len;
        }
        write_padded(
            &mut out,
            files
                .iter()
                .flat_map(|(n, _, _)| n.bytes())
                .collect::<Vec<_>>()
                .as_slice(),
        )?;
        // Retain parallel reads without buffering hundreds of large members
        // at once on phones. Large individual members stream with an exact cap.
        let mut start = 0;
        while start < files.len() {
            if files[start].2 > 16 * 1024 * 1024 {
                let (_, path, len) = &files[start];
                let copied = io::copy(&mut File::open(path)?.take(len + 1), &mut out)?;
                if copied != *len {
                    return Err(io::Error::other(format!("{} changed", path.display())));
                }
                start += 1;
                continue;
            }
            let mut end = start;
            let mut bytes = 0;
            while end < files.len()
                && end - start < READ_BATCH
                && bytes + files[end].2 <= 16 * 1024 * 1024
            {
                bytes += files[end].2;
                end += 1;
            }
            for bytes in read_listed(&files[start..end], readers)? {
                out.write_all(&bytes)?;
            }
            start = end;
        }
        out.into_inner()
            .map_err(io::IntoInnerError::into_error)?
            .sync_all()?;
        fs::rename(&tmp, root.join(FILE_NAME))
    })();
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result
}

/// How many files [`write_with_readers`] holds in memory at once.
const READ_BATCH: usize = 512;

/// `files`' bytes in their order, read on `readers` threads.
fn read_listed(files: &[(String, PathBuf, u64)], readers: usize) -> io::Result<Vec<Vec<u8>>> {
    if readers <= 1 {
        return files.iter().map(read_listed_file).collect();
    }
    let next = AtomicUsize::new(0);
    std::thread::scope(|scope| {
        let workers: Vec<_> = (0..readers)
            .map(|_| {
                scope.spawn(|| -> io::Result<Vec<(usize, Vec<u8>)>> {
                    let mut read = Vec::new();
                    loop {
                        let at = next.fetch_add(1, Ordering::Relaxed);
                        let Some(file) = files.get(at) else {
                            return Ok(read);
                        };
                        read.push((at, read_listed_file(file)?));
                    }
                })
            })
            .collect();
        let mut bytes = vec![Vec::new(); files.len()];
        for worker in workers {
            let read = worker
                .join()
                .map_err(|_| io::Error::other("a reader stopped"))??;
            for (at, file) in read {
                bytes[at] = file;
            }
        }
        Ok(bytes)
    })
}

fn read_listed_file((_, path, len): &(String, PathBuf, u64)) -> io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    File::open(path)?.take(len + 1).read_to_end(&mut bytes)?;
    // Changed since it was listed: the offsets would be wrong.
    if bytes.len() as u64 != *len {
        return Err(io::Error::other(format!("{} changed", path.display())));
    }
    Ok(bytes)
}

/// Removes the temporary files of writers that stopped midway (the game or the
/// launcher closed). One still being written on Windows can't be removed; on
/// Unix its writer's rename fails, and a later start writes the file.
fn remove_abandoned(root: &Path) {
    let prefix = format!("{FILE_NAME}.");
    for entry in fs::read_dir(root).into_iter().flatten().flatten() {
        let name = entry.file_name();
        if name
            .to_str()
            .is_some_and(|n| n.starts_with(&prefix) && n.ends_with(".tmp"))
        {
            let _ = fs::remove_file(entry.path());
        }
    }
}

/// The files under `dir` (named `name` from the root) this holds, with their
/// paths and sizes.
fn walk(dir: &Path, name: &str, out: &mut Vec<(String, PathBuf, u64)>) -> io::Result<()> {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e),
    };
    for entry in entries {
        let entry = entry?;
        let path = entry.path();
        let Some(file_name) = entry.file_name().to_str().map(str::to_owned) else {
            // A name a reader of the file couldn't ask for: list the folder instead.
            return Err(io::Error::other(format!("{} is not UTF-8", path.display())));
        };
        let child = format!("{name}/{file_name}");
        let meta = fs::metadata(&path)?;
        if meta.is_dir() {
            walk(&path, &child, out)?;
        } else if holds_file(&child) {
            if meta.len() > MAX_MEMBER_BYTES {
                return Err(io::Error::other("prepared cache member too large"));
            }
            out.push((child, path, meta.len()));
        }
    }
    Ok(())
}

fn pad(len: u64) -> u64 {
    len.next_multiple_of(8)
}

fn write_padded(out: &mut impl Write, bytes: &[u8]) -> io::Result<()> {
    out.write_all(bytes)?;
    out.write_all(&[0; 8][..(pad(bytes.len() as u64) as usize - bytes.len())])
}

#[cfg(unix)]
fn read_at(file: &File, buf: &mut [u8], offset: u64) -> io::Result<()> {
    std::os::unix::fs::FileExt::read_exact_at(file, buf, offset)
}
#[cfg(windows)]
fn read_at(file: &File, mut buf: &mut [u8], mut offset: u64) -> io::Result<()> {
    while !buf.is_empty() {
        match std::os::windows::fs::FileExt::seek_read(file, buf, offset) {
            Ok(0) => return Err(io::ErrorKind::UnexpectedEof.into()),
            Ok(n) => {
                buf = &mut buf[n..];
                offset += n as u64;
            }
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(e) => return Err(e),
        }
    }
    Ok(())
}
#[cfg(not(any(unix, windows)))]
fn read_at(file: &File, buf: &mut [u8], offset: u64) -> io::Result<()> {
    use std::io::Seek;
    static LOCK: Mutex<()> = Mutex::new(());
    let _held = LOCK.lock().unwrap_or_else(PoisonError::into_inner);
    let mut file = file;
    file.seek(io::SeekFrom::Start(offset))?;
    file.read_exact(buf)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A prepared root with a few files, as `horizon-prepare` leaves it.
    fn root(tag: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("hz-prepared-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        for (name, bytes) in [
            (SOURCE, &b"{\"client_sha256\":\"a\"}"[..]),
            ("data/minecraft/recipe/stick.json", b"{\"stick\":1}"),
            ("data/minecraft/recipe/brewing/awkward.json", b"{}"),
            ("data/minecraft/tags/item/logs.json", b"[1,2,3]"),
            ("assets/minecraft/textures/block/stone.png", b"\x89PNG..."),
            ("assets/minecraft/sounds/ambient/cave1.ogg", b"OggS"),
            (
                "worldgen/structure/minecraft/fossil/spine_1.nbt",
                b"\x1f\x8b",
            ),
            ("worldgen/biomes.json", b"{}"),
            ("facts.json", b"{}"),
        ] {
            let path = root.join(name);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, bytes).unwrap();
        }
        root
    }

    #[test]
    fn oversized_members_leave_original_files_available_without_building_a_cache() {
        let root = root("oversized-writer");
        let path = root.join("assets/oversized.bin");
        let file = File::create(&path).unwrap();
        file.set_len(MAX_MEMBER_BYTES + 1).unwrap();
        let error = write(&root).unwrap_err();
        assert!(error.to_string().contains("member too large"));
        assert_eq!(fs::metadata(path).unwrap().len(), MAX_MEMBER_BYTES + 1);
        assert!(!root.join(FILE_NAME).exists());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn malformed_index_lengths_are_rejected_without_panicking() {
        let root = root("overflow");
        write(&root).unwrap();
        let mut bytes = fs::read(root.join(FILE_NAME)).unwrap();
        bytes[32..40].copy_from_slice(&u64::MAX.to_le_bytes());
        fs::write(root.join(FILE_NAME), bytes).unwrap();
        assert!(PreparedFiles::open(&root).unwrap().is_none());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn cache_members_cannot_read_metadata_or_request_giant_allocations() {
        let root = root("member-bounds");
        write(&root).unwrap();
        let original = fs::read(root.join(FILE_NAME)).unwrap();
        let root_len = u64::from_le_bytes(original[16..24].try_into().unwrap());
        let source_len = u64::from_le_bytes(original[24..32].try_into().unwrap());
        let entry = HEADER + pad(root_len) as usize + pad(source_len) as usize;
        let mut bytes = original.clone();
        bytes[entry + 8..entry + 16].copy_from_slice(&0u64.to_le_bytes());
        fs::write(root.join(FILE_NAME), bytes).unwrap();
        assert!(
            PreparedFiles::open(&root).unwrap().is_none(),
            "metadata is not an asset"
        );
        let mut bytes = original;
        bytes[entry + 16..entry + 24].copy_from_slice(&(MAX_MEMBER_BYTES + 1).to_le_bytes());
        fs::write(root.join(FILE_NAME), bytes).unwrap();
        let file = fs::OpenOptions::new()
            .write(true)
            .open(root.join(FILE_NAME))
            .unwrap();
        file.set_len(MAX_MEMBER_BYTES + (1 << 20)).unwrap();
        assert!(
            PreparedFiles::open(&root).unwrap().is_none(),
            "bound member allocations before reads"
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn invalidation_refreshes_positive_and_negative_cache_entries_without_invalidating_readers() {
        let root = root("invalidate");
        assert!(open(&root).is_none());
        write(&root).unwrap();
        invalidate(&root);
        let original = open(&root).unwrap();
        let member = "data/minecraft/recipe/stick.json";
        fs::write(root.join(member), b"new recipe").unwrap();
        write(&root).unwrap();
        invalidate(&root);
        assert_eq!(
            open(&root).unwrap().read(member).unwrap().unwrap(),
            b"new recipe"
        );
        assert_eq!(original.read(member).unwrap().unwrap(), b"{\"stick\":1}");
        invalidate(&root);
        fs::remove_dir_all(root).unwrap();
    }

    /// A writer that stopped midway leaves its temporary file: the next write
    /// removes it, and nothing else.
    #[test]
    fn a_write_removes_what_a_stopped_writer_left() {
        let root = root("abandoned");
        let left = root.join(format!("{FILE_NAME}.1.tmp"));
        let other = root.join("other.tmp");
        fs::write(&left, b"cut short").unwrap();
        fs::write(&other, b"not ours").unwrap();
        write(&root).unwrap();
        assert!(!left.exists());
        assert!(other.exists());
        assert!(PreparedFiles::open(&root).unwrap().is_some());
        fs::remove_dir_all(&root).unwrap();
    }

    /// Several readers write the same file as one, past a batch's worth of files.
    #[test]
    fn readers_write_the_file_one_reader_writes() {
        let root = root("readers");
        for at in 0..READ_BATCH + 3 {
            let name = root.join(format!("data/minecraft/recipe/r{at}.json"));
            fs::write(name, format!("{{\"at\":{at}}}")).unwrap();
        }
        write(&root).unwrap();
        let one = fs::read(root.join(FILE_NAME)).unwrap();
        write_with_readers(&root, 4).unwrap();
        assert!(one == fs::read(root.join(FILE_NAME)).unwrap());
        fs::remove_dir_all(&root).unwrap();
    }

    /// A file that can't be read is an error however many readers there are.
    #[test]
    fn a_reader_error_stops_the_write() {
        let root = root("reader-error");
        let gone = root.join("gone");
        let listed = [("gone".to_owned(), gone, 1)];
        assert!(read_listed(&listed, 1).is_err());
        assert!(read_listed(&listed, 4).is_err());
        fs::remove_dir_all(&root).unwrap();
    }

    /// Every file of its folders reads back as the folder has it; the sounds
    /// and the top-level tables are left to the folder.
    #[test]
    fn the_file_reads_and_lists_what_the_folders_hold() {
        let root = root("reads");
        write(&root).unwrap();
        let files = PreparedFiles::open(&root).unwrap().unwrap();
        for name in [
            "data/minecraft/recipe/stick.json",
            "data/minecraft/recipe/brewing/awkward.json",
            "data/minecraft/tags/item/logs.json",
            "assets/minecraft/textures/block/stone.png",
        ] {
            assert_eq!(
                files.read(name).unwrap().unwrap(),
                fs::read(root.join(name)).unwrap()
            );
        }
        assert!(
            files
                .read("assets/minecraft/sounds/ambient/cave1.ogg")
                .is_none()
        );
        assert!(files.read("facts.json").is_none());
        assert!(files.read("worldgen/biomes.json").is_none());
        assert_eq!(
            files
                .read("worldgen/structure/minecraft/fossil/spine_1.nbt")
                .unwrap()
                .unwrap(),
            b"\x1f\x8b"
        );
        assert!(files.read("data/minecraft/recipe/missing.json").is_none());
        assert_eq!(
            files.list("data/minecraft/recipe").unwrap(),
            [
                "data/minecraft/recipe/brewing/awkward.json",
                "data/minecraft/recipe/stick.json"
            ]
        );
        // `recipe` is not a prefix of `recipes`.
        assert!(files.list("data/minecraft/recipes").is_none());
        assert!(files.list("worldgen").is_none());
        let _ = fs::remove_dir_all(&root);
    }

    /// A file from another preparation, damaged, or a link from another
    /// root's mirror is not used: the folder is read instead.
    #[test]
    fn a_file_that_does_not_match_its_root_is_not_used() {
        let root = root("stale");
        write(&root).unwrap();
        fs::write(root.join(SOURCE), b"{\"client_sha256\":\"b\"}").unwrap();
        assert!(
            PreparedFiles::open(&root).unwrap().is_none(),
            "another preparation"
        );

        write(&root).unwrap();
        let bin = root.join(FILE_NAME);
        let bytes = fs::read(&bin).unwrap();
        fs::write(&bin, &bytes[..bytes.len() - 3]).unwrap();
        assert!(PreparedFiles::open(&root).unwrap().is_none(), "cut short");

        write(&root).unwrap();
        // A mirror of the root (a resource pack's, a content mod's) holding a
        // copy of its file and its horizon-resources.json.
        let mirror =
            std::env::temp_dir().join(format!("hz-prepared-mirror-{}", std::process::id()));
        let _ = fs::remove_dir_all(&mirror);
        fs::create_dir_all(&mirror).unwrap();
        fs::copy(&bin, mirror.join(FILE_NAME)).unwrap();
        fs::copy(root.join(SOURCE), mirror.join(SOURCE)).unwrap();
        assert!(
            PreparedFiles::open(&mirror).unwrap().is_none(),
            "another root's copy"
        );
        #[cfg(unix)]
        {
            fs::remove_file(mirror.join(FILE_NAME)).unwrap();
            std::os::unix::fs::symlink(&bin, mirror.join(FILE_NAME)).unwrap();
            assert!(
                PreparedFiles::open(&mirror).unwrap().is_none(),
                "another root's link"
            );
        }
        let _ = fs::remove_dir_all(&mirror);
        let _ = fs::remove_dir_all(&root);
    }

    /// A folder's files come from the file, without the disk: one added since
    /// isn't listed (as `read` doesn't find it), and a folder it holds nothing
    /// in is empty. Outside its folders, the disk is walked.
    #[test]
    fn files_under_lists_a_folder_from_the_file() {
        let root = root("under");
        write(&root).unwrap();
        fs::write(root.join("assets/minecraft/textures/block/added.png"), b"x").unwrap();
        assert_eq!(
            files_under(&root.join("assets/minecraft/textures")).unwrap(),
            ["block/stone.png"]
        );
        assert_eq!(
            files_under(&root.join("worldgen/structure")).unwrap(),
            ["minecraft/fossil/spine_1.nbt"]
        );
        assert!(
            files_under(&root.join("assets/minecraft/textures/entity"))
                .unwrap()
                .is_empty()
        );
        assert_eq!(files_under(&root.join("worldgen")), None);
        let _ = fs::remove_dir_all(&root);
    }

    /// `find` gives the file and the path from its root for a folder under a
    /// root that has one, and nothing elsewhere.
    #[test]
    fn find_walks_up_to_the_root_with_a_file() {
        let root = root("find");
        write(&root).unwrap();
        let (_, rel) = find(&root.join("data/minecraft/recipe")).unwrap();
        assert_eq!(rel, "data/minecraft/recipe");
        let (_, rel) = find(&root.join("worldgen/structure/minecraft")).unwrap();
        assert_eq!(rel, "worldgen/structure/minecraft");
        // The top-level tables beside the structures stay on the disk.
        assert!(find(&root.join("worldgen")).is_none());
        assert!(find(&root.join("worldgen/biomes.json")).is_none());
        let _ = fs::remove_dir_all(&root);
    }

    /// Under a root with a file, a file it holds reads from it and a file it
    /// doesn't is missing even when the disk has one now; a file outside its
    /// folders, or a sound, reads from the disk.
    #[test]
    fn read_takes_the_file_for_what_it_holds() {
        let root = root("read");
        write(&root).unwrap();
        let stick = root.join("data/minecraft/recipe/stick.json");
        fs::write(&stick, b"changed on disk").unwrap();
        assert_eq!(read(&stick).unwrap(), b"{\"stick\":1}");
        let added = root.join("assets/minecraft/textures/block/stone_n.png");
        fs::write(&added, b"added after").unwrap();
        assert_eq!(read(&added).unwrap_err().kind(), io::ErrorKind::NotFound);
        assert_eq!(read(&root.join("facts.json")).unwrap(), b"{}");
        let sound = root.join("assets/minecraft/sounds/ambient/cave1.ogg");
        assert_eq!(read(&sound).unwrap(), b"OggS");
        assert_eq!(read_to_string(&stick).unwrap(), "{\"stick\":1}");
        // A header: from the file for what it holds, from the disk for the rest.
        assert_eq!(read_head(&stick, 4).unwrap(), b"{\"st");
        assert_eq!(read_head(&stick, 100).unwrap(), b"{\"stick\":1}");
        assert_eq!(read_head(&sound, 2).unwrap(), b"Og");
        assert_eq!(
            read_head(&added, 2).unwrap_err().kind(),
            io::ErrorKind::NotFound
        );
        let _ = fs::remove_dir_all(&root);
    }
}
