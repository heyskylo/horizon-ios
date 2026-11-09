//! `FileDownload`: the latest world's link fetched as the tar.gz whose root entry is
//! `world/`, unpacked into the game's `saves` folder under a free name, then the resource
//! pack the archive pointed at checked against its SHA-1 and saved beside the world
//! (`resourcepacks/resources.zip`). `DownloadTask`'s prepare request lives in `realms.rs`;
//! this module starts at `FileDownload.download`.
//!
//! The tar reader handles what `TarArchiveInputStream` accepts for a Realms upload: ustar
//! headers, a `prefix`/`name` split, pax extended headers (`x`, `g`) and GNU long names
//! (`L`). Nothing is ever extracted outside the new world folder: an entry that does not
//! sit under `world/`, or climbs above it, is skipped as vanilla's `startsWith` check does.
use super::client::WorldDownload;
use horizon_world::nbt::Tag;
use sha1::{Digest, Sha1};
use std::fs::{self, File};
use std::io::{self, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Duration;

/// `RealmsDownloadLatestWorldScreen.checkDownloadSize`'s `5368709120L`.
pub const SIZE_LIMIT: u64 = 5 * 1024 * 1024 * 1024;

/// `FileDownload`'s `Duration.ofMinutes(2)` connect and response timeouts.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(120);
const READ_TIMEOUT: Duration = Duration::from_secs(120);

/// `SharedConstants.ILLEGAL_FILE_CHARACTERS`, in its order.
const ILLEGAL_FILE_CHARACTERS: [char; 15] = [
    '/', '\n', '\r', '\t', '\0', '\u{c}', '`', '?', '*', '\\', '<', '>', '|', '"', ':',
];

/// `FileDownload.INVALID_FILE_NAMES`: a file name Windows refuses.
const INVALID_FILE_NAMES: [&str; 24] = [
    "CON", "COM", "PRN", "AUX", "CLOCK$", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6",
    "COM7", "COM8", "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

/// The screen's numbers: the current file's total and bytes, and its cancel and extract
/// flags. The resource pack's download restarts the bytes as `DownloadStatus` does.
#[derive(Default)]
pub struct DownloadProgress {
    session: Option<std::sync::Arc<super::transport::Cancellation>>,
    total: AtomicU64,
    written: AtomicU64,
    cancelled: AtomicBool,
    extracting: AtomicBool,
}

impl std::fmt::Debug for DownloadProgress {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DownloadProgress")
            .field("cancelled", &self.is_cancelled())
            .finish_non_exhaustive()
    }
}

impl DownloadProgress {
    pub(crate) fn managed(session: std::sync::Arc<super::transport::Cancellation>) -> Self {
        Self {
            session: Some(session),
            ..Self::default()
        }
    }

    pub fn total(&self) -> u64 {
        self.total.load(Ordering::Relaxed)
    }
    pub fn written(&self) -> u64 {
        self.written.load(Ordering::Relaxed)
    }
    /// `downloadStatus.bytesWritten != 0`: the bar and the status wait for a first byte.
    pub fn started(&self) -> bool {
        self.written() > 0
    }
    /// `(double)bytesWritten / totalBytes`, capped at 1.
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
            || self.session.as_ref().is_some_and(|s| s.cancelled())
    }
    pub fn is_extracting(&self) -> bool {
        self.extracting.load(Ordering::Relaxed)
    }
    /// A new file starts (`DownloadCountingOutputStream` begins at zero).
    fn begin_file(&self, total: u64) {
        self.total.store(total, Ordering::Relaxed);
        self.written.store(0, Ordering::Relaxed);
    }
    fn add_written(&self, bytes: u64) {
        self.written.fetch_add(bytes, Ordering::Relaxed);
    }
    fn set_extracting(&self) {
        self.extracting.store(true, Ordering::Relaxed);
    }
}

/// Why a download stopped, as the screen tells it (vanilla's `errorMessage` is always
/// `mco.download.failed`; the cause stays in the log).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DownloadError {
    Cancelled,
    Failed(String),
}

impl DownloadError {
    fn failed(e: impl std::fmt::Display) -> Self {
        Self::Failed(e.to_string())
    }
}

/// `FileDownload.contentLength`: the link's `Content-Length`, 0 when the HEAD fails (vanilla
/// keeps an empty `OptionalLong` as 0 and downloads anyway).
pub fn content_length(link: &str) -> u64 {
    content_length_managed(link, None)
}

pub(crate) fn content_length_managed(
    link: &str,
    session: Option<std::sync::Arc<super::transport::Cancellation>>,
) -> u64 {
    if link.is_empty() {
        return 0;
    }
    match request("HEAD", link, session.as_ref()) {
        Ok(response) => response
            .header("Content-Length")
            .and_then(|value| value.trim().parse().ok())
            .unwrap_or(0),
        Err(e) => {
            log::warn!("Realms: could not read the download's size: {e}");
            0
        }
    }
}

/// `FileDownload`: the world archive, then the resource pack the service named.
pub fn download_world(
    download: &WorldDownload,
    world_name: &str,
    saves: &Path,
    progress: &DownloadProgress,
) -> Result<(), DownloadError> {
    if progress.is_cancelled() {
        return Err(DownloadError::Cancelled);
    }
    let archive = temp_path("backup", ".tar.gz");
    let result = (|| {
        fetch_to(&download.download_link, &archive, progress)?;
        // `finishWorldDownload`: a short body is not an error, it just does not unpack.
        let mut world = None;
        if progress.written() >= progress.total() && !progress.is_cancelled() {
            progress.set_extracting();
            world = Some(extract_archive(
                &archive,
                world_name.trim(),
                saves,
                progress,
            )?);
        }
        if progress.is_cancelled() {
            return Err(DownloadError::Cancelled);
        }
        if !download.resource_pack_url.is_empty() && !download.resource_pack_hash.is_empty() {
            let world = world.ok_or_else(|| DownloadError::failed("no world was unpacked"))?;
            let pack = temp_path("resources", ".tar.gz");
            let result = fetch_to(&download.resource_pack_url, &pack, progress).and_then(|()| {
                // `finishResourcePackDownload`: a short body is checked nowhere.
                if progress.written() >= progress.total() {
                    copy_resource_pack(&pack, &world, &download.resource_pack_hash)
                } else {
                    Ok(())
                }
            });
            let _ = fs::remove_file(&pack);
            result?;
        }
        Ok(())
    })();
    let _ = fs::remove_file(&archive);
    if let Err(e) = &result {
        log::info!("Realms: the world download stopped: {e:?}");
    }
    result
}

fn agent(session: Option<&std::sync::Arc<super::transport::Cancellation>>) -> ureq::Agent {
    let builder = ureq::AgentBuilder::new()
        .timeout_connect(CONNECT_TIMEOUT)
        .timeout_read(READ_TIMEOUT)
        .redirects(0)
        .user_agent(&format!("Java/{}", horizon_core::version::JAVA_VERSION));
    match session {
        Some(cancel) => builder.tls_connector(std::sync::Arc::new(super::transport::connector(
            cancel.clone(),
        ))),
        None => builder,
    }
    .build()
}

/// A download may follow redirects, but never converts an HTTPS link to HTTP.
fn request(
    method: &str,
    link: &str,
    session: Option<&std::sync::Arc<super::transport::Cancellation>>,
) -> io::Result<super::transport::Response> {
    let mut link = link.to_owned();
    for _ in 0..=5 {
        let response = if link.starts_with("http://") {
            let user_agent = format!("Java/{}", horizon_core::version::JAVA_VERSION);
            super::transport::http(
                method,
                &link,
                &[("User-Agent", &user_agent)],
                None,
                session.cloned().unwrap_or_default(),
                CONNECT_TIMEOUT,
                READ_TIMEOUT,
            )?
        } else if link.starts_with("https://") {
            let response = match agent(session).request(method, &link).call() {
                Ok(r) | Err(ureq::Error::Status(_, r)) => r,
                Err(e) => return Err(io::Error::other(e.to_string())),
            };
            let status = response.status();
            let headers = response
                .headers_names()
                .into_iter()
                .filter_map(|k| response.header(&k).map(|v| (k.clone(), v.to_owned())))
                .collect();
            super::transport::Response {
                status,
                headers,
                body: response.into_reader(),
            }
        } else {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "invalid Realms download URL",
            ));
        };
        if !matches!(response.status, 301 | 302 | 303 | 307 | 308) {
            return Ok(response);
        }
        let location = response
            .header("Location")
            .ok_or_else(|| io::Error::other("missing redirect target"))?;
        let parsed = ureq::get(&link)
            .request_url()
            .map_err(|_| io::Error::other("invalid download URL"))?;
        let next = parsed
            .as_url()
            .join(location)
            .map_err(|_| io::Error::other("invalid redirect target"))?
            .to_string();
        if link.starts_with("https://") && !next.starts_with("https://") {
            return Err(io::Error::other("insecure Realms redirect"));
        }
        link = next;
    }
    Err(io::Error::other("too many Realms redirects"))
}

/// `FileDownload.download(DownloadStatus, HttpClient, url, target)`: a 200 body to `target`,
/// counted as it is written.
fn fetch_to(link: &str, target: &Path, progress: &DownloadProgress) -> Result<(), DownloadError> {
    let response =
        request("GET", link, progress.session.as_ref()).map_err(DownloadError::failed)?;
    if response.status != 200 {
        return Err(DownloadError::failed(format!(
            "the link answered {}",
            response.status
        )));
    }
    let total = response
        .header("Content-Length")
        .and_then(|value| value.trim().parse().ok())
        .unwrap_or(0);
    progress.begin_file(total);
    let mut reader = response.body;
    let mut file = File::create(target).map_err(DownloadError::failed)?;
    let mut buffer = [0u8; 64 * 1024];
    loop {
        if progress.is_cancelled() {
            return Err(DownloadError::Cancelled);
        }
        let read = reader.read(&mut buffer).map_err(DownloadError::failed)?;
        if read == 0 {
            break;
        }
        file.write_all(&buffer[..read])
            .map_err(DownloadError::failed)?;
        progress.add_written(read as u64);
    }
    file.flush().map_err(DownloadError::failed)?;
    Ok(())
}

/// `FileDownload.finishResourcePackDownload`: the SHA-1 check, then the file beside the
/// world (`LevelResource.MAP_RESOURCE_FILE`).
fn copy_resource_pack(pack: &Path, world: &Path, expected: &str) -> Result<(), DownloadError> {
    let actual = sha1_file(pack).map_err(DownloadError::failed)?;
    if actual != expected {
        log::error!("Realms: the resource pack's hash is {actual}, not {expected}");
        return Err(DownloadError::failed("the resource pack's hash"));
    }
    let target = world.join("resourcepacks/resources.zip");
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent).map_err(DownloadError::failed)?;
    }
    fs::copy(pack, &target).map_err(DownloadError::failed)?;
    Ok(())
}

fn sha1_file(path: &Path) -> io::Result<String> {
    let mut hasher = Sha1::new();
    io::copy(&mut File::open(path)?, &mut hasher)?;
    Ok(hex(&hasher.finalize()))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn temp_path(prefix: &str, suffix: &str) -> PathBuf {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let n = NEXT.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!("{prefix}-{}-{n}{suffix}", std::process::id()))
}

/// `FileDownload.untarGzipArchive`: the archive into `saves/<free name>`, then
/// `renameAndDropPlayer`. Returns the world folder.
pub fn extract_archive(
    archive: &Path,
    name: &str,
    saves: &Path,
    progress: &DownloadProgress,
) -> Result<PathBuf, DownloadError> {
    fs::create_dir_all(saves).map_err(DownloadError::failed)?;
    let folder = world_folder(saves, name);
    let world = saves.join(&folder);
    fs::create_dir_all(&world).map_err(DownloadError::failed)?;
    let file = File::open(archive).map_err(DownloadError::failed)?;
    let mut tar = TarReader::new(flate2::read::GzDecoder::new(BufReader::new(file)));
    loop {
        if progress.is_cancelled() {
            return Err(DownloadError::Cancelled);
        }
        let Some(header) = tar.next().map_err(DownloadError::failed)? else {
            break;
        };
        let Some(relative) = under_world(&header.name) else {
            log::warn!(
                "Realms: an archive entry outside world/ was skipped: {}",
                header.name
            );
            continue;
        };
        let destination = world.join(&relative);
        if !destination.starts_with(&world) {
            log::warn!(
                "Realms: an archive entry climbing out of the world was skipped: {}",
                header.name
            );
            continue;
        }
        if header.directory {
            fs::create_dir_all(&destination).map_err(DownloadError::failed)?;
        } else {
            if let Some(parent) = destination.parent() {
                fs::create_dir_all(parent).map_err(DownloadError::failed)?;
            }
            let mut out = File::create(&destination).map_err(DownloadError::failed)?;
            tar.copy_contents(header.size, &mut out)
                .map_err(DownloadError::failed)?;
        }
        tar.skip_padding(header.size)
            .map_err(DownloadError::failed)?;
    }
    // `validateAndCreateAccess`: a world holding a symbolic link is not renamed (vanilla
    // logs `Failed to download file`); the folder stays as extracted either way.
    match crate::title::world_management::symlinks(saves, &folder) {
        Ok(links) if links.is_empty() => rename_and_drop_player(&world, &folder),
        Ok(_) => log::warn!("Realms: the downloaded world holds a symbolic link"),
        Err(e) => log::warn!("Realms: could not check the downloaded world: {e}"),
    }
    Ok(world)
}

/// `LevelStorageAccess.renameAndDropPlayer`: the folder's name as the world's, and no
/// singleplayer owner. A failure only logs, as vanilla's does.
fn rename_and_drop_player(world: &Path, name: &str) {
    let path = world.join("level.dat");
    let Some(mut root) = crate::save::read_dat(&path) else {
        log::warn!("Realms: could not read the downloaded world's level.dat");
        return;
    };
    let Some(Tag::Compound(data)) = root.get_mut("Data") else {
        log::warn!("Realms: the downloaded world has no level data");
        return;
    };
    data.insert("LevelName".into(), Tag::String(name.trim().into()));
    data.remove(&"singleplayer_uuid".into());
    if let Err(e) = crate::save::write_dat(&path, root) {
        log::warn!("Realms: could not name the downloaded world: {e}");
    }
}

/// `FileDownload.untarGzipArchive`'s name: the characters a folder cannot hold become `_`,
/// the reserved Windows names gain underscores, an empty name is `Realm`, and a name the
/// saves folder or a world already uses counts up (`Realm`, `Realm-2`, ...).
fn world_folder(saves: &Path, name: &str) -> String {
    let mut name: String = name
        .chars()
        .map(|c| if matches!(c, '.' | '/' | '"') { '_' } else { c })
        .collect();
    if name.is_empty() {
        name = "Realm".into();
    }
    for c in ILLEGAL_FILE_CHARACTERS {
        name = name.replace(c, "_");
    }
    name = free_file_name(&name);
    let lower = name.to_lowercase();
    let mut number: u32 = 1;
    for id in level_ids(saves) {
        if id.to_lowercase().starts_with(&lower) {
            match trailing_number(&id) {
                Some(found) => number = number.max(found),
                None => number += 1,
            }
        }
    }
    if acceptable(saves, &name) && number <= 1 {
        return name;
    }
    let mut candidate = format!(
        "{name}{}",
        if number == 1 {
            String::new()
        } else {
            format!("-{number}")
        }
    );
    if !acceptable(saves, &candidate) {
        // Vanilla loops until a name is acceptable; a folder that refuses every name (a
        // read-only saves directory) must not spin a worker forever.
        for _ in 0..1000 {
            number += 1;
            candidate = format!("{name}-{number}");
            if acceptable(saves, &candidate) {
                break;
            }
        }
    }
    candidate
}

/// `FileDownload.findAvailableFolderName`: a reserved DOS name gains an underscore a side.
fn free_file_name(folder: &str) -> String {
    match INVALID_FILE_NAMES
        .iter()
        .find(|invalid| folder.eq_ignore_ascii_case(invalid))
    {
        Some(_) => format!("_{folder}_"),
        None => folder.to_owned(),
    }
}

/// `.*-([0-9]+)$`: the number a world's folder name ends with.
fn trailing_number(id: &str) -> Option<u32> {
    let (_, digits) = id.rsplit_once('-')?;
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    digits.parse().ok()
}

/// `LevelStorageSource.findLevelCandidates`: the folders holding a level.dat.
fn level_ids(saves: &Path) -> Vec<String> {
    let Ok(entries) = fs::read_dir(saves) else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter(|entry| {
            entry.file_type().is_ok_and(|kind| kind.is_dir())
                && (entry.path().join("level.dat").is_file()
                    || entry.path().join("level.dat_old").is_file())
        })
        .filter_map(|entry| entry.file_name().into_string().ok())
        .collect()
}

/// `LevelStorageSource.isNewLevelIdAcceptable`: the folder can be made.
fn acceptable(saves: &Path, id: &str) -> bool {
    let path = saves.join(id);
    match fs::create_dir(&path) {
        Ok(()) => fs::remove_dir(&path).is_ok(),
        Err(_) => false,
    }
}

/// `Path.of("world").relativize(entry)` normalized, or none when the entry is not under
/// `world/` (vanilla's `startsWith` check drops those).
fn under_world(name: &str) -> Option<PathBuf> {
    let mut parts: Vec<&str> = Vec::new();
    for part in name.split(['/', '\\']) {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            // On Windows `PathBuf::push` of a part with a drive prefix (`C:`) replaces the
            // path, and `name:stream` names an alternate data stream: neither stays in the
            // world folder, so the entry is skipped.
            part if part.contains(':') => return None,
            part => parts.push(part),
        }
    }
    if parts.first() != Some(&"world") {
        return None;
    }
    let mut path = PathBuf::new();
    for part in &parts[1..] {
        path.push(part);
    }
    Some(path)
}

/// A tar entry as `TarArchiveInputStream` reports one, after pax/GNU headers are applied.
struct Header {
    name: String,
    size: u64,
    directory: bool,
}

struct TarReader<R: Read> {
    inner: R,
}

impl<R: Read> TarReader<R> {
    fn new(inner: R) -> Self {
        Self { inner }
    }

    /// The next entry, folding pax (`x`, `g`) and GNU long-name (`L`, `K`) headers into it.
    fn next(&mut self) -> io::Result<Option<Header>> {
        let mut pax_path: Option<String> = None;
        let mut long_name: Option<String> = None;
        loop {
            let mut block = [0u8; 512];
            if !read_block(&mut self.inner, &mut block)? {
                return Ok(None);
            }
            if block.iter().all(|byte| *byte == 0) {
                return Ok(None);
            }
            let kind = block[156];
            let size = parse_size(&block[124..136]);
            match kind {
                b'x' | b'X' => {
                    let data = read_data(&mut self.inner, size)?;
                    pax_path = pax_path_of(&data);
                    continue;
                }
                b'g' => {
                    skip_data(&mut self.inner, size)?;
                    continue;
                }
                b'L' => {
                    let data = read_data(&mut self.inner, size)?;
                    long_name = Some(c_string(&data));
                    continue;
                }
                b'K' => {
                    skip_data(&mut self.inner, size)?;
                    continue;
                }
                _ => {}
            }
            let name = pax_path
                .or(long_name)
                .unwrap_or_else(|| header_name(&block));
            return Ok(Some(Header {
                name,
                size,
                directory: kind == b'5',
            }));
        }
    }

    /// The entry's contents, exactly `size` bytes.
    fn copy_contents(&mut self, size: u64, out: &mut impl Write) -> io::Result<()> {
        let copied = io::copy(&mut (&mut self.inner).take(size), out)?;
        if copied != size {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "the archive ended inside an entry",
            ));
        }
        Ok(())
    }

    /// The zero padding after the contents.
    fn skip_padding(&mut self, size: u64) -> io::Result<()> {
        skip_padding(&mut self.inner, size)
    }
}

/// A 512-byte header block: false at the archive's end.
fn read_block(reader: &mut impl Read, block: &mut [u8; 512]) -> io::Result<bool> {
    let mut filled = 0;
    while filled < block.len() {
        match reader.read(&mut block[filled..])? {
            0 if filled == 0 => return Ok(false),
            0 => {
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "the archive ended inside a header",
                ));
            }
            read => filled += read,
        }
    }
    Ok(true)
}

fn header_name(block: &[u8; 512]) -> String {
    let name = c_string(&block[0..100]);
    let prefix = c_string(&block[345..500]);
    if prefix.is_empty() {
        name
    } else {
        format!("{prefix}/{name}")
    }
}

fn c_string(field: &[u8]) -> String {
    let end = field
        .iter()
        .position(|byte| *byte == 0)
        .unwrap_or(field.len());
    String::from_utf8_lossy(&field[..end]).into_owned()
}

/// A tar numeric field: octal digits, or the base-256 form for values octal cannot hold.
fn parse_size(field: &[u8]) -> u64 {
    if field.first().is_some_and(|byte| byte & 0x80 != 0) {
        return field
            .iter()
            .skip(1)
            .fold(0u64, |value, byte| (value << 8) | u64::from(*byte));
    }
    let text = field
        .iter()
        .map(|byte| *byte as char)
        .take_while(|c| *c == ' ' || c.is_ascii_digit())
        .collect::<String>();
    let trimmed = text.trim();
    if trimmed.is_empty() {
        0
    } else {
        u64::from_str_radix(trimmed, 8).unwrap_or(0)
    }
}

fn read_data(reader: &mut impl Read, size: u64) -> io::Result<Vec<u8>> {
    // An extended header is a few hundred bytes; a bogus size must not reach an allocation.
    if size > 1 << 20 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "an oversized extended header",
        ));
    }
    let mut data = vec![0u8; size as usize];
    reader.read_exact(&mut data)?;
    skip_padding(reader, size)?;
    Ok(data)
}

fn skip_padding(reader: &mut impl Read, size: u64) -> io::Result<()> {
    let padding = (512 - (size % 512) as usize) % 512;
    if padding > 0 {
        let mut buffer = [0u8; 512];
        reader.read_exact(&mut buffer[..padding])?;
    }
    Ok(())
}

fn skip_data(reader: &mut impl Read, size: u64) -> io::Result<()> {
    let padding = (512 - (size % 512) as usize) % 512;
    let copied = io::copy(&mut reader.take(size + padding as u64), &mut io::sink())?;
    if copied != size + padding as u64 {
        return Err(io::Error::new(
            io::ErrorKind::UnexpectedEof,
            "the archive ended inside an entry",
        ));
    }
    Ok(())
}

/// The `path` record of a pax extended header (its length-prefixed `key=value` lines).
fn pax_path_of(data: &[u8]) -> Option<String> {
    let mut rest = data;
    while !rest.is_empty() {
        let space = rest.iter().position(|byte| *byte == b' ')?;
        let length: usize = std::str::from_utf8(&rest[..space]).ok()?.parse().ok()?;
        if length == 0 || length > rest.len() {
            return None;
        }
        let record = &rest[space + 1..length];
        let record = record.strip_suffix(b"\n").unwrap_or(record);
        if let Ok(text) = std::str::from_utf8(record)
            && let Some(path) = text.strip_prefix("path=")
        {
            return Some(path.to_owned());
        }
        rest = &rest[length..];
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::realms::upload::{UploadProgress, pack_world};

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "realms-download-test-{}-{name}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// A world folder `upload` can pack: a level.dat naming its owner, and one region.
    fn packed_world(dir: &Path, level_name: &str) -> PathBuf {
        let world = dir.join("source");
        fs::create_dir_all(world.join("region")).unwrap();
        fs::write(world.join("region/r.0.0.mca"), vec![7u8; 600]).unwrap();
        let level = Tag::compound([(
            "Data",
            Tag::compound([
                ("LevelName", Tag::String(level_name.into())),
                ("singleplayer_uuid", Tag::IntArray(vec![1, 2, 3, 4])),
                ("DataVersion", Tag::Int(4189)),
            ]),
        )]);
        crate::save::write_dat(&world.join("level.dat"), level).unwrap();
        let archive = pack_world(&world, &UploadProgress::default()).unwrap();
        let kept = dir.join("world.tar.gz");
        fs::copy(archive.path(), &kept).unwrap();
        kept
    }

    /// `FileDownload.untarGzipArchive`: the archive's `world/` becomes a world folder named
    /// after the Realm, its level.dat renamed and its singleplayer owner dropped.
    #[test]
    fn an_archive_unpacks_into_a_named_world() {
        let dir = scratch("unpack");
        let archive = packed_world(&dir, "Old name");
        let saves = dir.join("saves");
        let progress = DownloadProgress::default();
        let world = extract_archive(&archive, "Friends", &saves, &progress).unwrap();
        assert_eq!(world.file_name().unwrap(), "Friends");
        assert_eq!(fs::read(world.join("region/r.0.0.mca")).unwrap().len(), 600);
        let root = crate::save::read_dat(&world.join("level.dat")).unwrap();
        let Some(Tag::Compound(data)) = root.get("Data") else {
            panic!("level data");
        };
        assert_eq!(
            data.get(&"LevelName".into()),
            Some(&Tag::String("Friends".into()))
        );
        assert!(!data.contains_key(&"singleplayer_uuid".into()));
        // The world list counts the name: the next download is `Friends-2`.
        let again = extract_archive(&archive, "Friends", &saves, &progress).unwrap();
        assert_eq!(again.file_name().unwrap(), "Friends-2");
        let _ = fs::remove_dir_all(&dir);
    }

    /// `FileDownload.findAvailableFolderName` and the folder count: illegal characters,
    /// reserved names, an empty name and existing worlds.
    #[test]
    fn a_world_name_is_made_into_a_free_folder() {
        let dir = scratch("names");
        let saves = dir.join("saves");
        fs::create_dir_all(&saves).unwrap();
        fs::create_dir_all(saves.join("Friends-3")).unwrap();
        fs::write(saves.join("Friends-3/level.dat"), b"nbt").unwrap();
        fs::create_dir_all(saves.join("Other")).unwrap();
        assert_eq!(world_folder(&saves, "Friends"), "Friends-4");
        assert_eq!(world_folder(&saves, "Other"), "Other-2");
        assert_eq!(world_folder(&saves, "a/b\\c:d?e"), "a_b_c_d_e");
        assert_eq!(world_folder(&saves, "CON"), "_CON_");
        assert_eq!(world_folder(&saves, ""), "Realm");
        // A folder name with nothing wrong is used as it is.
        assert_eq!(world_folder(&saves, "Fresh"), "Fresh");
        let _ = fs::remove_dir_all(&dir);
    }

    /// One ustar entry in a gzip'd archive, written by hand so a deliberately unsafe name
    /// can be offered (`TarArchiveOutputStream` would never produce one).
    fn one_entry_archive(path: &Path, name: &str, data: &[u8]) {
        let file = File::create(path).unwrap();
        let mut gz = flate2::write::GzEncoder::new(file, flate2::Compression::default());
        write_test_header(&mut gz, name, data.len(), b'0');
        write_test_data(&mut gz, data);
        gz.write_all(&[0u8; 1024]).unwrap();
        gz.finish().unwrap();
    }

    fn write_test_header(gz: &mut impl Write, name: &str, size: usize, kind: u8) {
        let mut header = [0u8; 512];
        header[..name.len()].copy_from_slice(name.as_bytes());
        let text = format!("{size:011o}");
        header[124..135].copy_from_slice(text.as_bytes());
        header[156] = kind;
        header[257..263].copy_from_slice(b"ustar\0");
        header[263..265].copy_from_slice(b"00");
        header[148..156].fill(b' ');
        let checksum: u32 = header.iter().map(|byte| u32::from(*byte)).sum();
        let text = format!("{checksum:06o}");
        header[148..154].copy_from_slice(text.as_bytes());
        header[154] = 0;
        header[155] = b' ';
        gz.write_all(&header).unwrap();
    }

    fn write_test_data(gz: &mut impl Write, data: &[u8]) {
        gz.write_all(data).unwrap();
        let padding = (512 - data.len() % 512) % 512;
        gz.write_all(&vec![0u8; padding]).unwrap();
    }

    /// `TarArchiveInputStream`'s pax `path` record: its own length counts itself.
    fn pax_record(key: &str, value: &str) -> String {
        let body = format!("{key}={value}\n");
        let mut total = body.len() + 2;
        loop {
            let next = total.to_string().len() + 1 + body.len();
            if next == total {
                return format!("{total} {body}");
            }
            total = next;
        }
    }

    /// `FileDownload.untarGzipArchive`'s `startsWith(worldPath)`: an entry outside `world/`
    /// never lands beside the world folder.
    #[test]
    fn an_entry_outside_the_world_is_skipped() {
        let dir = scratch("outside");
        let archive = dir.join("evil.tar.gz");
        one_entry_archive(&archive, "../evil.txt", b"bad");
        let saves = dir.join("saves");
        let world =
            extract_archive(&archive, "Friends", &saves, &DownloadProgress::default()).unwrap();
        assert!(!saves.join("evil.txt").exists());
        assert!(!world.join("evil.txt").exists());
        let _ = fs::remove_dir_all(&dir);
    }

    /// A part with a drive prefix or a stream name would leave the world folder on Windows.
    #[test]
    fn an_entry_with_a_drive_or_stream_part_is_skipped() {
        assert_eq!(under_world("world/C:/evil.txt"), None);
        assert_eq!(under_world("world\\region\\r.0.0.mca:hidden"), None);
        assert_eq!(
            under_world("world/region/r.0.0.mca"),
            Some(PathBuf::from("region").join("r.0.0.mca"))
        );
    }

    /// `RealmsUploadWorldPacker.setLongFileMode(LONGFILE_POSIX)`: a long name arrives in a
    /// pax header (`x`), and the entry after it takes that path.
    #[test]
    fn a_pax_name_is_applied_to_the_next_entry() {
        let dir = scratch("pax");
        let archive = dir.join("pax.tar.gz");
        let long = format!("world/{}", "a".repeat(120));
        let record = pax_record("path", &long);
        let file = File::create(&archive).unwrap();
        let mut gz = flate2::write::GzEncoder::new(file, flate2::Compression::default());
        write_test_header(&mut gz, "././@PaxHeader", record.len(), b'x');
        write_test_data(&mut gz, record.as_bytes());
        write_test_header(&mut gz, "world/truncated", 3, b'0');
        write_test_data(&mut gz, b"nbt");
        gz.write_all(&[0u8; 1024]).unwrap();
        gz.finish().unwrap();
        let saves = dir.join("saves");
        let world =
            extract_archive(&archive, "Friends", &saves, &DownloadProgress::default()).unwrap();
        assert_eq!(fs::read(world.join("a".repeat(120))).unwrap(), b"nbt");
        let _ = fs::remove_dir_all(&dir);
    }
}
