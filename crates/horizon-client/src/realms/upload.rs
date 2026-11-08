//! `RealmsWorldUpload`: the world folder packed as `RealmsUploadWorldPacker` packs it (a tar.gz
//! whose root entry is `world/`), posted by `FileUpload`, then the slot saved with
//! `RealmsClient.updateSlot` once the service took the archive.
//!
//! The packer writes the tar format itself (the `tar` crate is not a dependency): ustar
//! headers, a name split into `prefix`/`name` when it is longer than 100 bytes, and two zero
//! blocks at the end. A path that does not fit ustar's 255 bytes fails the packing rather than
//! uploading a truncated archive.
use super::client::{RealmSlot, RealmsClient, RealmsError, UploadFailure, UploadInfo};
use std::fs::File;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, SystemTime};

/// `RealmsUploadWorldPacker.SIZE_LIMIT`.
pub const SIZE_LIMIT: u64 = 5 * 1024 * 1024 * 1024;

/// The upload screen's numbers: total bytes, bytes handed to the socket, and its cancel.
#[derive(Debug, Default)]
pub struct UploadProgress {
    total: AtomicU64,
    written: AtomicU64,
    cancelled: AtomicBool,
}

impl UploadProgress {
    pub fn total(&self) -> u64 {
        self.total.load(Ordering::Relaxed)
    }
    fn set_total(&self, total: u64) {
        self.total.store(total, Ordering::Relaxed);
    }
    pub fn written(&self) -> u64 {
        self.written.load(Ordering::Relaxed)
    }
    pub fn started(&self) -> bool {
        self.written() > 0
    }
    pub fn completed(&self) -> bool {
        self.total() > 0 && self.written() >= self.total()
    }
    /// `UploadStatus.getPercentage`.
    pub fn percentage(&self) -> f64 {
        let total = self.total();
        if total == 0 {
            return 0.0;
        }
        (self.written() as f64 / total as f64).min(1.0)
    }
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Relaxed);
    }
    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Relaxed)
    }
    /// The body reader's counter and stop flag (`FileUpload`'s `UploadCountingInputStream`).
    pub fn counter(&self) -> &AtomicU64 {
        &self.written
    }
    pub fn stop(&self) -> &AtomicBool {
        &self.cancelled
    }
}

/// Why an upload stopped, as the screen tells it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UploadError {
    /// `RealmsUploadCanceledException`.
    Cancelled,
    /// `RealmsUploadTooLargeException`.
    TooLarge,
    /// `RealmsUploadWorldNotClosedException`.
    WorldNotClosed,
    /// `RealmsUploadFailedException` (a service error keeps its text).
    Failed(String),
}

/// `RealmsWorldUpload.packAndUpload`'s worker: ask where to post, pack, post, save the slot.
pub fn upload_world(
    client: &RealmsClient,
    realm_id: i64,
    slot: &RealmSlot,
    world_dir: &Path,
    progress: &UploadProgress,
) -> Result<(), UploadError> {
    let info = request_upload_info(client, realm_id, progress)?;
    let archive = pack_world(world_dir, progress)?;
    progress.set_total(archive.bytes());
    client
        .upload_world(
            realm_id,
            slot.slot_id,
            &info,
            &slot.options.version,
            archive.path(),
            progress.counter(),
            progress.stop(),
        )
        .map_err(|failure| match failure {
            UploadFailure::Cancelled => UploadError::Cancelled,
            UploadFailure::Failed(message) => UploadError::Failed(message),
        })?;
    client
        .update_slot(realm_id, slot)
        .map_err(|e| UploadError::Failed(e.text()))
}

/// `RealmsWorldUpload.requestUploadInfoWithRetries`: up to 20 tries, waiting the service's
/// `Retry-After` between them; a target that says the world is open is a failure at once.
fn request_upload_info(
    client: &RealmsClient,
    realm_id: i64,
    progress: &UploadProgress,
) -> Result<UploadInfo, UploadError> {
    let mut token: Option<String> = None;
    for _ in 0..20 {
        if progress.is_cancelled() {
            return Err(UploadError::Cancelled);
        }
        match client.request_upload_info(realm_id, token.as_deref()) {
            Ok(Some(info)) => {
                token.clone_from(&info.token);
                if !info.world_closed {
                    return Err(UploadError::WorldNotClosed);
                }
                return Ok(info);
            }
            // `requestUploadInfo` returned null: the next try may answer.
            Ok(None) => {}
            Err(RealmsError::Retry(seconds)) => {
                std::thread::sleep(Duration::from_secs(seconds.min(60)));
            }
            Err(e) => return Err(UploadError::Failed(e.text())),
        }
    }
    Err(UploadError::WorldNotClosed)
}

/// The packed archive, deleted when it goes out of scope (`RealmsWorldUpload`'s `finally`).
#[derive(Debug)]
pub struct Archive {
    path: PathBuf,
    len: u64,
}

impl Archive {
    pub fn path(&self) -> &Path {
        &self.path
    }
    pub fn bytes(&self) -> u64 {
        self.len
    }
}

impl Drop for Archive {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

/// `RealmsUploadWorldPacker.pack`: the folder as a gzip'd tar, `world/` at its root.
pub fn pack_world(world: &Path, progress: &UploadProgress) -> Result<Archive, UploadError> {
    let path = temp_archive_path();
    match write_archive(world, &path, progress) {
        Ok(len) => {
            if len > SIZE_LIMIT {
                let _ = std::fs::remove_file(&path);
                return Err(UploadError::TooLarge);
            }
            Ok(Archive { path, len })
        }
        Err(e) => {
            let _ = std::fs::remove_file(&path);
            Err(e)
        }
    }
}

fn temp_archive_path() -> PathBuf {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let n = NEXT.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "realms-upload-file-{}-{n}.tar.gz",
        std::process::id()
    ))
}

fn write_archive(world: &Path, path: &Path, progress: &UploadProgress) -> Result<u64, UploadError> {
    let file = File::create(path).map_err(|e| UploadError::Failed(e.to_string()))?;
    let gz = flate2::write::GzEncoder::new(file, flate2::Compression::default());
    let mut tar = TarWriter::new(gz);
    add_tree(&mut tar, world, "world", progress)?;
    if progress.is_cancelled() {
        return Err(UploadError::Cancelled);
    }
    let gz = tar
        .finish()
        .map_err(|e| UploadError::Failed(e.to_string()))?;
    let file = gz
        .finish()
        .map_err(|e| UploadError::Failed(e.to_string()))?;
    file.sync_all()
        .map_err(|e| UploadError::Failed(e.to_string()))?;
    drop(file);
    let len = std::fs::metadata(path)
        .map_err(|e| UploadError::Failed(e.to_string()))?
        .len();
    Ok(len)
}

/// `RealmsUploadWorldPacker.addFileToTarGz`: the folder's entry, then its children.
fn add_tree(
    tar: &mut TarWriter<impl Write>,
    path: &Path,
    name: &str,
    progress: &UploadProgress,
) -> Result<(), UploadError> {
    if progress.is_cancelled() {
        return Err(UploadError::Cancelled);
    }
    let metadata = std::fs::metadata(path).map_err(|e| UploadError::Failed(e.to_string()))?;
    let modified = metadata
        .modified()
        .ok()
        .and_then(|t| t.duration_since(SystemTime::UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_secs());
    let directory = metadata.is_dir();
    let size = if directory { 0 } else { metadata.len() };
    tar.header(name, size, directory, modified)
        .map_err(|e| UploadError::Failed(e.to_string()))?;
    if directory {
        let mut children: Vec<PathBuf> = std::fs::read_dir(path)
            .map_err(|e| UploadError::Failed(e.to_string()))?
            .flatten()
            .map(|entry| entry.path())
            .collect();
        // Vanilla uses the folder's own order; sorting keeps the archive reproducible.
        children.sort();
        for child in children {
            let Some(file_name) = child.file_name().and_then(|n| n.to_str()) else {
                continue;
            };
            add_tree(tar, &child, &format!("{name}/{file_name}"), progress)?;
        }
    } else {
        let mut file = File::open(path).map_err(|e| UploadError::Failed(e.to_string()))?;
        let mut buf = [0u8; 64 * 1024];
        loop {
            if progress.is_cancelled() {
                return Err(UploadError::Cancelled);
            }
            let read = file
                .read(&mut buf)
                .map_err(|e| UploadError::Failed(e.to_string()))?;
            if read == 0 {
                break;
            }
            tar.contents(&buf[..read])
                .map_err(|e| UploadError::Failed(e.to_string()))?;
            if tar.written() > SIZE_LIMIT {
                return Err(UploadError::TooLarge);
            }
        }
        tar.pad(size)
            .map_err(|e| UploadError::Failed(e.to_string()))?;
    }
    Ok(())
}

/// A tar writer (`TarArchiveOutputStream`): 512-byte ustar headers, the contents padded to
/// 512, and two zero blocks at the end.
struct TarWriter<W: Write> {
    out: W,
    written: u64,
}

impl<W: Write> TarWriter<W> {
    fn new(out: W) -> Self {
        Self { out, written: 0 }
    }

    fn written(&self) -> u64 {
        self.written
    }

    fn header(&mut self, name: &str, size: u64, directory: bool, mtime: u64) -> io::Result<()> {
        // `TarArchiveEntry` gives a directory's name its trailing slash.
        let name = if directory && !name.ends_with('/') {
            format!("{name}/")
        } else {
            name.to_owned()
        };
        let (prefix, short) = split_name(&name)?;
        let mut header = [0u8; 512];
        write_bytes(&mut header[0..100], short)?;
        write_octal(&mut header[100..108], if directory { 0o755 } else { 0o644 });
        write_octal(&mut header[108..116], 0);
        write_octal(&mut header[116..124], 0);
        write_octal(&mut header[124..136], size);
        write_octal(&mut header[136..148], mtime);
        // The checksum is computed with its own field read as spaces.
        header[148..156].fill(b' ');
        header[156] = if directory { b'5' } else { b'0' };
        header[257..263].copy_from_slice(b"ustar\0");
        header[263..265].copy_from_slice(b"00");
        header[329..337].copy_from_slice(b"0000000\0");
        header[337..345].copy_from_slice(b"0000000\0");
        if let Some(prefix) = prefix {
            write_bytes(&mut header[345..500], prefix)?;
        }
        let checksum: u32 = header.iter().map(|b| u32::from(*b)).sum();
        write_checksum(&mut header[148..156], checksum);
        self.out.write_all(&header)?;
        self.written += 512;
        Ok(())
    }

    fn contents(&mut self, bytes: &[u8]) -> io::Result<()> {
        self.out.write_all(bytes)?;
        self.written += bytes.len() as u64;
        Ok(())
    }

    /// The zero padding after a file's contents (`TarArchiveOutputStream` writes it when the
    /// entry closes).
    fn pad(&mut self, size: u64) -> io::Result<()> {
        let padding = (512 - (size % 512) as usize) % 512;
        self.out.write_all(&[0u8; 512][..padding])?;
        self.written += padding as u64;
        Ok(())
    }

    fn finish(mut self) -> io::Result<W> {
        self.out.write_all(&[0u8; 1024])?;
        self.out.flush()?;
        Ok(self.out)
    }
}

/// A name that fits `name`/`prefix` (100/155 bytes), split at a `/` when it is longer than 100.
fn split_name(name: &str) -> io::Result<(Option<&[u8]>, &[u8])> {
    let bytes = name.as_bytes();
    if bytes.len() <= 100 {
        return Ok((None, bytes));
    }
    let Some(split) = name
        .match_indices('/')
        .map(|(i, _)| i)
        .find(|i| *i <= 155 && bytes.len() - i - 1 <= 100 && bytes.len() - i - 1 > 0)
    else {
        // `TarArchiveOutputStream`'s POSIX mode would write a pax header here; a world folder
        // this deep is refused rather than packed wrong.
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("world path too long for the archive: {name}"),
        ));
    };
    Ok((Some(&bytes[..split]), &bytes[split + 1..]))
}

fn write_bytes(field: &mut [u8], value: &[u8]) -> io::Result<()> {
    if value.len() > field.len() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "tar field too small",
        ));
    }
    field[..value.len()].copy_from_slice(value);
    Ok(())
}

/// A tar numeric field: zero-padded octal and a NUL.
fn write_octal(field: &mut [u8], value: u64) {
    let text = format!("{:0width$o}", value, width = field.len() - 1);
    field[..text.len()].copy_from_slice(text.as_bytes());
    field[field.len() - 1] = 0;
}

/// The checksum: six octal digits, a NUL and a space (`TarArchiveOutputStream`).
fn write_checksum(field: &mut [u8], checksum: u32) {
    let text = format!("{checksum:06o}");
    field[..text.len()].copy_from_slice(text.as_bytes());
    field[text.len()] = 0;
    field[text.len() + 1] = b' ';
}

#[cfg(test)]
mod tests {
    use super::*;

    fn world_dir(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("realms-upload-test-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("region")).unwrap();
        std::fs::write(dir.join("level.dat"), b"nbt").unwrap();
        std::fs::write(dir.join("region/r.0.0.mca"), vec![7u8; 600]).unwrap();
        dir
    }

    /// The archive's entries (name, size), read back through gzip and tar.
    fn entries(archive: &Path) -> Vec<(String, u64)> {
        let file = File::open(archive).unwrap();
        let mut gz = flate2::read::GzDecoder::new(file);
        let mut bytes = Vec::new();
        gz.read_to_end(&mut bytes).unwrap();
        let mut entries = Vec::new();
        let mut offset = 0usize;
        while offset + 512 <= bytes.len() {
            let header = &bytes[offset..offset + 512];
            if header.iter().all(|b| *b == 0) {
                break;
            }
            let text = |field: &[u8]| {
                field
                    .iter()
                    .take_while(|b| **b != 0)
                    .map(|b| *b as char)
                    .collect::<String>()
            };
            let name = text(&header[0..100]);
            let prefix = text(&header[345..500]);
            let size = u64::from_str_radix(text(&header[124..135]).trim(), 8).unwrap_or(0);
            let full = if prefix.is_empty() {
                name
            } else {
                format!("{prefix}/{name}")
            };
            entries.push((full, size));
            offset += 512 + (size as usize).div_ceil(512) * 512;
        }
        entries
    }

    #[test]
    fn the_world_is_packed_under_its_folder_name() {
        let dir = world_dir("pack");
        let progress = UploadProgress::default();
        let archive = pack_world(&dir, &progress).unwrap();
        // `RealmsUploadWorldPacker.addFileToTarGz`: the folder, then its children depth first.
        assert_eq!(
            entries(archive.path()),
            vec![
                ("world/".to_owned(), 0),
                ("world/level.dat".to_owned(), 3),
                ("world/region/".to_owned(), 0),
                ("world/region/r.0.0.mca".to_owned(), 600),
            ]
        );
        assert!(std::fs::metadata(archive.path()).unwrap().len() > 0);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_cancelled_pack_stops_without_an_archive() {
        let dir = world_dir("cancel");
        let progress = UploadProgress::default();
        progress.cancel();
        assert_eq!(
            pack_world(&dir, &progress).unwrap_err(),
            UploadError::Cancelled
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn every_header_carries_the_tar_checksum() {
        let mut tar = TarWriter::new(Vec::new());
        tar.header("world/level.dat", 3, false, 1_700_000_000)
            .unwrap();
        let out = tar.finish().unwrap();
        let header = &out[..512];
        let stored =
            u64::from_str_radix(std::str::from_utf8(&header[148..154]).unwrap(), 8).unwrap();
        // The checksum is the sum of the header with its own field read as spaces.
        let mut copy = header.to_vec();
        copy[148..156].fill(b' ');
        let sum: u64 = copy.iter().map(|b| u64::from(*b)).sum();
        assert_eq!(stored, sum);
        assert_eq!(&header[257..263], b"ustar\0");
        assert_eq!(header[156], b'0');
    }

    #[test]
    fn a_long_path_is_split_into_prefix_and_name() {
        let long = format!("world/{}/region.mca", "a".repeat(90));
        let (prefix, name) = split_name(&long).unwrap();
        let prefix = prefix.expect("a name over 100 bytes needs a prefix");
        // ustar's two fields: prefix at most 155 bytes, name at most 100.
        assert!(prefix.len() <= 155 && name.len() <= 100);
        assert_eq!(
            format!(
                "{}/{}",
                String::from_utf8_lossy(prefix),
                String::from_utf8_lossy(name)
            ),
            long
        );
        // Beyond ustar's 255 bytes the packer refuses rather than truncates.
        assert!(split_name(&format!("world/{}", "b".repeat(300))).is_err());
        // A tar size field is octal: 600 is 1130.
        let mut field = [0u8; 12];
        write_octal(&mut field, 600);
        assert_eq!(&field[..11], b"00000001130");
    }
}
