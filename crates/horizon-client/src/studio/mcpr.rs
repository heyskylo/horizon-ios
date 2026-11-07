//! `.mcpr` Replay File Container Specification & I/O
//!
//! An `.mcpr` file is a ZIP archive containing:
//! - `metaData.json`: metadata about the recording session (version, duration, player UUIDs, etc.)
//! - `recording.tmcpr`: binary stream of chronological timestamped network packets:
//!   `[timestamp_ms: u32 big-endian][packet_length: u32 big-endian][packet_data: VarInt(packet_id) + payload]`
//! - Optional `recording.tmcpr.crc32`: CRC32 of the packet stream.

use horizon_protocol::buf::{Reader, write_varint};
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File, OpenOptions},
    io::{self, BufReader, BufWriter, Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
};

/// The most `metaData.json` may inflate to: real files are a few hundred bytes.
const MAX_METADATA_BYTES: u64 = 1 << 20;
/// The most `recording.tmcpr` may inflate to. The reader streams to disk, but
/// still caps archive expansion to protect disk space from hostile archives.
const MAX_STREAM_BYTES: u64 = 1 << 30;
/// Keep one seek anchor for every group of this many packets.
const STREAM_INDEX_STRIDE: usize = 1024;
/// How much recording time a stream and its metadata may hold in memory before
/// both reach disk. A crash costs at most this many milliseconds of packets.
const FLUSH_INTERVAL_MS: u32 = 1_000;

/// High-level session metadata stored in `metaData.json`.
///
/// Only the format fields are required: ReplayMod leaves out `serverName` for
/// single player and older format versions lack `protocol`, so those read as
/// their defaults rather than rejecting the file.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Metadata {
    #[serde(default)]
    pub singleplayer: bool,
    #[serde(default)]
    pub server_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub custom_server_name: Option<String>,
    #[serde(default)]
    pub duration: u32,
    #[serde(default)]
    pub date: u64,
    pub mcversion: String,
    pub file_format: String,
    pub file_format_version: u32,
    /// The network protocol the packet ids belong to; `None` when the file
    /// doesn't say, which playback has to treat as unknown.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub protocol: Option<i32>,
    #[serde(default)]
    pub generator: String,
    #[serde(default)]
    pub self_id: i32,
    #[serde(default)]
    pub players: Vec<String>,
}

impl Default for Metadata {
    fn default() -> Self {
        Self {
            singleplayer: true,
            server_name: "Local World".to_owned(),
            custom_server_name: None,
            duration: 0,
            date: 0,
            mcversion: horizon_core::VANILLA_VERSION.to_owned(),
            file_format: "MCPR".to_owned(),
            file_format_version: 14,
            protocol: Some(horizon_protocol::PROTOCOL_VERSION),
            generator: "Horizon Studio".to_owned(),
            self_id: 1,
            players: Vec::new(),
        }
    }
}

impl Metadata {
    /// Construct new metadata with server name and Minecraft version.
    pub fn new(server_name: String, singleplayer: bool, mcversion: String) -> Self {
        Self {
            singleplayer,
            server_name,
            mcversion,
            ..Default::default()
        }
    }
}

/// A raw timestamped clientbound network packet from a replay stream.
#[derive(Clone, Debug, PartialEq)]
pub struct TimestampedPacket {
    /// Milliseconds since recording start.
    pub timestamp_ms: u32,
    /// Protocol packet ID.
    pub id: i32,
    /// Unframed packet payload bytes (excluding the VarInt packet ID).
    pub payload: Vec<u8>,
}

impl TimestampedPacket {
    pub fn new(timestamp_ms: u32, id: i32, payload: Vec<u8>) -> Self {
        Self {
            timestamp_ms,
            id,
            payload,
        }
    }
}

/// A file name beside `dest` that no other writer in this or another process
/// picks: `<dest>.<pid>-<nanos>.<suffix>`.
fn sibling(dest: &Path, suffix: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    let mut name = dest.file_name().unwrap_or_default().to_os_string();
    name.push(format!(".{}-{nanos}.{suffix}", std::process::id()));
    dest.with_file_name(name)
}

fn create_private_file(path: &Path) -> io::Result<File> {
    let mut options = OpenOptions::new();
    options.read(true).write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options.open(path)
}

pub(super) fn write_private_file(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let mut options = OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        file.set_permissions(fs::Permissions::from_mode(0o600))?;
    }
    file.write_all(bytes)?;
    Ok(())
}

/// The metadata beside a stream: `<stream>.meta.json`.
///
/// It is written when recording starts, long before the archive exists, so a
/// crash leaves enough to package the stream with
/// [`recover_crashed_recordings`].
fn metadata_beside(stream: &Path) -> PathBuf {
    let mut name = stream.file_name().unwrap_or_default().to_os_string();
    name.push(".meta.json");
    stream.with_file_name(name)
}

/// Writer for generating `.mcpr` replay files.
///
/// Packets go to a stream file beside the destination
/// (`<name>.mcpr.<pid>-<nanos>.tmcpr.tmp`) and the session metadata to
/// `<stream>.meta.json`. Both are flushed together about once a second of
/// recording, so a crash loses little and leaves what [`recover_crashed_recordings`]
/// packages. The stream is only removed once [`finish`](Self::finish) has put the
/// finished archive in place, so a failed finish or a writer dropped without one
/// leaves the packets on disk.
pub struct McprWriter {
    dest_path: PathBuf,
    temp_tmcpr_path: PathBuf,
    temp_meta_path: PathBuf,
    writer: Option<BufWriter<File>>,
    metadata: Metadata,
    packet_count: usize,
    /// Bytes of whole frames in the stream: what `finish` packages.
    bytes_written: u64,
    /// The latest timestamp written: the duration a recovery can vouch for.
    last_timestamp_ms: u32,
    /// The timestamp of the last time the stream and metadata reached disk.
    last_flush_ms: u32,
    /// A write failed: the stream may end in a partial frame, and nothing more
    /// is appended after it.
    failed: bool,
}

impl McprWriter {
    /// Create a new replay writer that writes packets to a temporary stream file
    /// and finalizes into `dest_path` when finished.
    ///
    /// Any recording a crash left in the destination's folder is packaged first
    /// (see [`recover_crashed_recordings`]).
    pub fn new(dest_path: PathBuf, metadata: Metadata) -> io::Result<Self> {
        if let Some(parent) = dest_path.parent() {
            fs::create_dir_all(parent)?;
            if !parent.as_os_str().is_empty() {
                match recover_crashed_recordings(parent) {
                    Ok(report) if !report.recovered.is_empty() => log::info!(
                        "Studio: recovered {} recording(s) a crash left behind",
                        report.recovered.len()
                    ),
                    Ok(_) => {}
                    Err(e) => log::warn!("Studio: could not look for crashed recordings: {e}"),
                }
            }
        }
        let temp_tmcpr_path = sibling(&dest_path, "tmcpr.tmp");
        let temp_meta_path = metadata_beside(&temp_tmcpr_path);
        let file = File::create_new(&temp_tmcpr_path)?;
        // A writer holds its stream locked until the file closes, which the
        // operating system does even after a crash. Recovery uses that to tell
        // a leftover from a recording still being written.
        if let Err(e) = file.try_lock() {
            // Filesystems without locks still record; recovery falls back to the
            // pid in the stream's name there.
            log::warn!("Studio: recording stream cannot be locked: {e}");
        }
        let writer = Some(BufWriter::with_capacity(64 * 1024, file));
        let mut this = Self {
            dest_path,
            temp_tmcpr_path,
            temp_meta_path,
            writer,
            metadata,
            packet_count: 0,
            bytes_written: 0,
            last_timestamp_ms: 0,
            last_flush_ms: 0,
            failed: false,
        };
        if let Err(e) = this.write_sidecar() {
            // Without metadata a crash would be recovered with defaults only:
            // start over rather than record something that can't be identified.
            this.writer = None;
            let _ = fs::remove_file(&this.temp_tmcpr_path);
            return Err(e);
        }
        Ok(this)
    }

    /// Record a packet at the given elapsed timestamp.
    ///
    /// After an error the writer refuses further packets; `finish` still
    /// packages every frame written whole before it.
    pub fn write_packet(&mut self, timestamp_ms: u32, id: i32, payload: &[u8]) -> io::Result<()> {
        if self.failed {
            return Err(io::Error::other("an earlier write failed"));
        }
        let writer = self
            .writer
            .as_mut()
            .ok_or_else(|| io::Error::other("writer already closed"))?;
        let mut frame = Vec::with_capacity(13 + payload.len());
        frame.extend_from_slice(&timestamp_ms.to_be_bytes());
        frame.extend_from_slice(&[0; 4]);
        write_varint(&mut frame, id);
        frame.extend_from_slice(payload);
        let packet_length = u32::try_from(frame.len() - 8)
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "packet too large"))?;
        frame[4..8].copy_from_slice(&packet_length.to_be_bytes());

        if let Err(e) = writer.write_all(&frame) {
            self.failed = true;
            return Err(e);
        }
        self.packet_count += 1;
        self.bytes_written += frame.len() as u64;
        self.last_timestamp_ms = self.last_timestamp_ms.max(timestamp_ms);

        // Let the packets and the metadata beside them reach disk together, so
        // the sidecar never describes a stream that is further ahead.
        if self.last_timestamp_ms.wrapping_sub(self.last_flush_ms) >= FLUSH_INTERVAL_MS {
            self.last_flush_ms = self.last_timestamp_ms;
            if let Err(e) = self.flush() {
                self.failed = true;
                return Err(e);
            }
        }
        Ok(())
    }

    pub fn packet_count(&self) -> usize {
        self.packet_count
    }

    pub fn bytes_written(&self) -> u64 {
        self.bytes_written
    }

    /// The stream file holding the packets until `finish` succeeds.
    pub fn stream_path(&self) -> &Path {
        &self.temp_tmcpr_path
    }

    /// The metadata file holding the session until `finish` succeeds.
    pub fn metadata_path(&self) -> &Path {
        &self.temp_meta_path
    }

    /// Flush the stream to disk and rewrite the metadata beside it.
    ///
    /// A crash after this loses at most the packets written since, and recovery
    /// knows what the recording is. A write error is reported after the stream
    /// flush; a metadata error only logs, since the packets are still sound.
    pub fn flush(&mut self) -> io::Result<()> {
        if let Some(w) = self.writer.as_mut() {
            w.flush()?;
        }
        if let Err(e) = self.write_sidecar() {
            log::warn!(
                "Studio: could not update {}: {e}",
                self.temp_meta_path.display()
            );
        }
        Ok(())
    }

    fn write_sidecar(&self) -> io::Result<()> {
        let mut metadata = self.metadata.clone();
        metadata.duration = self.last_timestamp_ms;
        let json = serde_json::to_vec_pretty(&metadata)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        write_private_file(&self.temp_meta_path, &json)
    }

    /// Complete recording, write `metaData.json`, package into `.mcpr` ZIP,
    /// and clean up temporary files.
    ///
    /// The archive is written to a temporary file beside `dest_path`, synced and
    /// renamed over it, so an existing file at `dest_path` stays whole until the
    /// new one is complete. On error the stream file is kept and the error names it.
    pub fn finish(mut self, duration_ms: u32) -> io::Result<PathBuf> {
        if let Err(e) = self.flush() {
            // A failed flush leaves a partial frame at the end at worst: only
            // `bytes_written` are packaged below.
            log::warn!("Studio: recording flush failed: {e}");
            self.failed = true;
        }
        // Close the stream before packaging reads it again: on Windows the
        // writer's lock would refuse that read.
        self.writer = None;
        self.metadata.duration = duration_ms;

        let part = sibling(&self.dest_path, "mcpr.part");
        match self.package(&part) {
            Ok(()) => {
                let _ = fs::remove_file(&self.temp_tmcpr_path);
                let _ = fs::remove_file(&self.temp_meta_path);
                Ok(self.dest_path.clone())
            }
            Err(e) => {
                let _ = fs::remove_file(&part);
                Err(io::Error::new(
                    e.kind(),
                    format!(
                        "could not finish the recording ({e}); its packets are kept in {}",
                        self.temp_tmcpr_path.display()
                    ),
                ))
            }
        }
    }

    fn package(&self, part: &Path) -> io::Result<()> {
        package_stream(
            &self.metadata,
            &self.temp_tmcpr_path,
            self.bytes_written,
            part,
        )?;
        fs::rename(part, &self.dest_path)
    }
}

/// Package `bytes` of `stream` and `metadata` into the archive at `part`.
///
/// The caller syncs and renames it into place; nothing here touches `stream`.
fn package_stream(metadata: &Metadata, stream: &Path, bytes: u64, part: &Path) -> io::Result<()> {
    let mut zip = zip::ZipWriter::new(File::create_new(part)?);
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);

    zip.start_file("metaData.json", options)?;
    let meta_json = serde_json::to_vec_pretty(metadata)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    zip.write_all(&meta_json)?;

    // Zip64, so a recording past 4 GiB still finishes.
    zip.start_file("recording.tmcpr", options.large_file(true))?;
    let mut input = BufReader::new(File::open(stream)?).take(bytes);
    let mut checksum = crc32fast::Hasher::new();
    let mut copied = 0u64;
    let mut buffer = [0; 64 * 1024];
    loop {
        let count = input.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        checksum.update(&buffer[..count]);
        zip.write_all(&buffer[..count])?;
        copied += count as u64;
    }
    if copied != bytes {
        return Err(io::Error::new(
            io::ErrorKind::UnexpectedEof,
            "the stream file is shorter than what was written",
        ));
    }
    zip.start_file("recording.tmcpr.crc32", options)?;
    write!(zip, "{:08x}", checksum.finalize())?;

    let file = zip.finish().map_err(io::Error::other)?;
    file.sync_all()?;
    drop(file);
    Ok(())
}

impl Drop for McprWriter {
    fn drop(&mut self) {
        // An empty stream holds nothing worth keeping; one with packets stays
        // for a later recovery (see the type's doc).
        if self.packet_count == 0 && self.temp_tmcpr_path.exists() {
            self.writer = None;
            let _ = fs::remove_file(&self.temp_tmcpr_path);
            let _ = fs::remove_file(&self.temp_meta_path);
        }
    }
}

/// The fixed head of the frame at `offset`: `(timestamp_ms, body length, bytes of
/// the id VarInt)`, or `None` when the header is short or the id does not fit in
/// the body. [`frame_at`] and crash recovery both read a frame's shape through
/// this, so they agree on what a whole frame is.
fn frame_head(data: &[u8], offset: usize) -> Option<(u32, usize, usize)> {
    let header = data.get(offset..offset.checked_add(8)?)?;
    let timestamp_ms = u32::from_be_bytes([header[0], header[1], header[2], header[3]]);
    let length = u32::from_be_bytes([header[4], header[5], header[6], header[7]]) as usize;
    // A VarInt is at most five bytes, and the id must fit in the body.
    let id_bytes = data.get(offset + 8..offset + 8 + length.min(5))?;
    let mut reader = Reader::new(id_bytes);
    reader.varint().ok()?;
    Some((timestamp_ms, length, id_bytes.len() - reader.rest().len()))
}

/// Byte positions for one complete packet frame in the temporary stream.
struct FrameLocation {
    timestamp_ms: u32,
    id: i32,
    payload_start: u64,
    payload_end: u64,
    next: u64,
}

/// Read one whole frame from a seekable stream. A partial header/body/ID ends
/// the readable stream, matching crash recovery's handling of truncated data.
fn frame_at<R: Read + Seek>(
    stream: &mut R,
    offset: u64,
    end: u64,
) -> io::Result<Option<FrameLocation>> {
    if end.saturating_sub(offset) < 8 {
        return Ok(None);
    }
    stream.seek(SeekFrom::Start(offset))?;
    let mut header = [0; 8];
    stream.read_exact(&mut header)?;
    let timestamp_ms = u32::from_be_bytes(header[..4].try_into().unwrap());
    let length = u32::from_be_bytes(header[4..].try_into().unwrap()) as u64;
    let body_start = offset + 8;
    let Some(next) = body_start.checked_add(length) else {
        return Ok(None);
    };
    if length == 0 || next > end {
        return Ok(None);
    }
    let id_bytes_len = length.min(5) as usize;
    let mut id_bytes = [0; 5];
    stream.read_exact(&mut id_bytes[..id_bytes_len])?;
    let mut packet = Reader::new(&id_bytes[..id_bytes_len]);
    let Ok(id) = packet.varint() else {
        return Ok(None);
    };
    let payload_start = body_start + (id_bytes_len - packet.rest().len()) as u64;
    stream.seek(SeekFrom::Start(next))?;
    Ok(Some(FrameLocation {
        timestamp_ms,
        id,
        payload_start,
        payload_end: next,
        next,
    }))
}

struct SparseIndexEntry {
    timestamp_ms: u32,
    frame_offset: u64,
    group_start: u64,
}

/// Disk-backed inflated stream removed when the reader closes.
struct TemporaryStream {
    path: PathBuf,
    reader: BufReader<File>,
}

impl TemporaryStream {
    fn create() -> io::Result<Self> {
        let mut last_error = None;
        for attempt in 0..16 {
            let nanos = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |duration| duration.as_nanos());
            let path = std::env::temp_dir().join(format!(
                "horizon-mcpr-{}-{nanos}-{attempt}.tmcpr",
                std::process::id()
            ));
            match create_private_file(&path) {
                Ok(file) => {
                    return Ok(Self {
                        path,
                        reader: BufReader::with_capacity(64 * 1024, file),
                    });
                }
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                    last_error = Some(error);
                }
                Err(error) => return Err(error),
            }
        }
        Err(last_error.unwrap_or_else(|| io::Error::other("could not create stream temp file")))
    }
}

impl Drop for TemporaryStream {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

/// Reader for opening and streaming `.mcpr` replay files.
pub struct McprReader {
    pub metadata: Metadata,
    stream: TemporaryStream,
    /// Where `next_packet` reads next.
    position: u64,
    /// The end of the last whole frame: the stream stops here.
    end: u64,
    /// Sparse frame index; timestamps are normalized to never go backwards.
    packet_index: Vec<SparseIndexEntry>,
    total_packets: usize,
}

fn read_entry(archive: &mut zip::ZipArchive<File>, name: &str, max: u64) -> io::Result<Vec<u8>> {
    let entry = archive
        .by_name(name)
        .map_err(|e| io::Error::new(io::ErrorKind::NotFound, e))?;
    let mut bytes = Vec::new();
    entry.take(max + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > max {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("{name} is larger than {max} bytes"),
        ));
    }
    Ok(bytes)
}

impl McprReader {
    /// Open an `.mcpr` replay archive from disk.
    pub fn open(path: &Path) -> io::Result<Self> {
        let file = File::open(path)?;
        let mut archive = zip::ZipArchive::new(file)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;

        let meta_bytes = read_entry(&mut archive, "metaData.json", MAX_METADATA_BYTES)?;
        let metadata: Metadata = serde_json::from_slice(&meta_bytes)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        if metadata.file_format != "MCPR" {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "not an MCPR recording (fileFormat {:?})",
                    metadata.file_format
                ),
            ));
        }

        let mut stream_entry = archive
            .by_name("recording.tmcpr")
            .map_err(|e| io::Error::new(io::ErrorKind::NotFound, e))?;
        if stream_entry.size() > MAX_STREAM_BYTES {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("recording.tmcpr is larger than {MAX_STREAM_BYTES} bytes"),
            ));
        }
        let mut stream = TemporaryStream::create()?;
        let mut stream_size = 0u64;
        let mut stream_crc = crc32fast::Hasher::new();
        let mut buffer = [0; 64 * 1024];
        loop {
            let count = stream_entry.read(&mut buffer)?;
            if count == 0 {
                break;
            }
            stream_size = stream_size.checked_add(count as u64).ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidData, "recording.tmcpr is too large")
            })?;
            if stream_size > MAX_STREAM_BYTES {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("recording.tmcpr is larger than {MAX_STREAM_BYTES} bytes"),
                ));
            }
            stream_crc.update(&buffer[..count]);
            stream.reader.get_mut().write_all(&buffer[..count])?;
        }
        let actual_crc = stream_crc.finalize();
        drop(stream_entry);
        if let Ok(crc_entry) = archive.by_name("recording.tmcpr.crc32") {
            let mut expected = String::new();
            crc_entry.take(64).read_to_string(&mut expected)?;
            let expected = expected.trim();
            let actual = format!("{actual_crc:08x}");
            if expected.len() != 8 || !expected.eq_ignore_ascii_case(&actual) {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "recording.tmcpr CRC32 mismatch",
                ));
            }
        }
        // A recording that stopped mid-write ends in a frame that isn't whole:
        // indexing and playback both stop before it.
        let mut packet_index = Vec::new();
        let mut offset = 0u64;
        let mut latest = 0;
        let mut group_start = 0u64;
        let mut total_packets = 0usize;
        while let Some(frame) = frame_at(&mut stream.reader, offset, stream_size)? {
            if total_packets == 0 || frame.timestamp_ms > latest {
                latest = frame.timestamp_ms;
                group_start = offset;
            }
            if total_packets.is_multiple_of(STREAM_INDEX_STRIDE) {
                packet_index.push(SparseIndexEntry {
                    timestamp_ms: latest,
                    frame_offset: offset,
                    group_start,
                });
            }
            total_packets += 1;
            offset = frame.next;
        }
        stream.reader.seek(SeekFrom::Start(0))?;

        Ok(Self {
            metadata,
            stream,
            position: 0,
            end: offset,
            packet_index,
            total_packets,
        })
    }

    /// Total number of packets indexed in the recording.
    pub fn total_packets(&self) -> usize {
        self.total_packets
    }

    /// Read the next timestamped packet from the current position, or `None`
    /// at the end of the last whole frame.
    pub fn next_packet(&mut self) -> io::Result<Option<TimestampedPacket>> {
        if self.position >= self.end {
            return Ok(None);
        }
        let Some(frame) = frame_at(&mut self.stream.reader, self.position, self.end)? else {
            return Ok(None);
        };
        let payload_len =
            usize::try_from(frame.payload_end - frame.payload_start).map_err(|_| {
                io::Error::new(io::ErrorKind::InvalidData, "packet payload is too large")
            })?;
        let mut payload = vec![0; payload_len];
        self.stream
            .reader
            .seek(SeekFrom::Start(frame.payload_start))?;
        self.stream.reader.read_exact(&mut payload)?;
        self.position = frame.next;
        Ok(Some(TimestampedPacket {
            timestamp_ms: frame.timestamp_ms,
            id: frame.id,
            payload,
        }))
    }

    /// Seek to the first packet of the latest timestamp at or before
    /// `target_timestamp_ms` (the first packet when the target is earlier than
    /// all of them), and return that timestamp.
    ///
    /// Starting at the first packet of a millisecond keeps the packets that
    /// arrived together, such as a chunk and the entities in it, together.
    pub fn seek_to_timestamp(&mut self, target_timestamp_ms: u32) -> io::Result<Option<u32>> {
        let Some(first) = self.packet_index.first() else {
            return Ok(None);
        };
        let after = self
            .packet_index
            .partition_point(|entry| entry.timestamp_ms <= target_timestamp_ms);
        let anchor = &self.packet_index[after.saturating_sub(1)];
        let mut chosen_timestamp = anchor.timestamp_ms;
        let mut chosen_position = anchor.group_start;
        let mut latest = anchor.timestamp_ms;
        let mut offset = anchor.frame_offset;
        while let Some(frame) = frame_at(&mut self.stream.reader, offset, self.end)? {
            latest = latest.max(frame.timestamp_ms);
            if latest > target_timestamp_ms {
                break;
            }
            if latest > chosen_timestamp {
                chosen_timestamp = latest;
                chosen_position = offset;
            }
            offset = frame.next;
        }
        if after == 0 {
            chosen_timestamp = first.timestamp_ms;
            chosen_position = first.group_start;
        }
        self.position = chosen_position;
        Ok(Some(chosen_timestamp))
    }

    /// Reset stream reading to the beginning.
    pub fn rewind(&mut self) -> io::Result<()> {
        self.position = 0;
        self.stream.reader.seek(SeekFrom::Start(0))?;
        Ok(())
    }
}

/// What one [`recover_crashed_recordings`] pass did.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RecoveryReport {
    /// The archives written from crashed recordings.
    pub recovered: Vec<PathBuf>,
    /// Leftover streams whose writer is still running: left alone.
    pub skipped: Vec<PathBuf>,
    /// Leftover streams that hold no whole frame: kept beside their metadata.
    pub kept: Vec<PathBuf>,
}

/// A leftover stream, cut to the frames it holds whole.
#[derive(Default)]
struct WholeStream {
    /// Bytes of whole frames: what can be packaged.
    bytes: u64,
    packets: usize,
    last_timestamp_ms: u32,
}

/// Package the recordings a crash left in `dir`.
///
/// Every `<name>.mcpr.<pid>-<nanos>.tmcpr.tmp` becomes `<name>.mcpr` (or a
/// `.recovered` name when that one is taken), cut to its last whole frame and
/// stamped with the metadata written beside the stream when recording began. A
/// stream whose writer may still be alive is skipped, and a stream with no whole
/// frame is kept with its metadata and logged: recovery never deletes what it
/// cannot package.
pub fn recover_crashed_recordings(dir: &Path) -> io::Result<RecoveryReport> {
    let mut report = RecoveryReport::default();
    for entry in fs::read_dir(dir)? {
        let path = entry?.path();
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        // Only our own streams: `<name>.mcpr.<pid>-<nanos>.tmcpr.tmp`.
        let Some(rest) = name.strip_suffix(".tmcpr.tmp") else {
            continue;
        };
        let Some((dest_name, owner)) = rest.rsplit_once('.') else {
            continue;
        };
        let Some((pid_text, nanos)) = owner.split_once('-') else {
            continue;
        };
        let (Ok(pid), Ok(_)) = (pid_text.parse::<u32>(), nanos.parse::<u128>()) else {
            continue;
        };

        if writer_may_be_alive(&path, pid) {
            log::info!(
                "Studio: keeping {}: its writer is still running",
                path.display()
            );
            report.skipped.push(path);
            continue;
        }

        let metadata = match fs::read(metadata_beside(&path))
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        {
            Some(metadata) => metadata,
            None => {
                log::warn!(
                    "Studio: {} has no metadata; recovering it with defaults",
                    path.display()
                );
                Metadata {
                    date: modified_ms(&path),
                    ..Default::default()
                }
            }
        };
        let dest = path.with_file_name(dest_name);
        match recover_stream(&path, &dest, metadata) {
            Ok((archive, packets)) => {
                log::info!(
                    "Studio: recovered {} packet(s) from {} into {}",
                    packets,
                    path.display(),
                    archive.display()
                );
                report.recovered.push(archive);
            }
            Err(e) => {
                log::warn!(
                    "Studio: could not recover {} ({e}); keeping it",
                    path.display()
                );
                report.kept.push(path);
            }
        }
    }
    Ok(report)
}

/// Whether the writer of `stream` may still be running.
///
/// Err on the side of yes: packaging a live recording would take the stream from
/// under its writer.
fn writer_may_be_alive(stream: &Path, pid: u32) -> bool {
    if pid == std::process::id() {
        return true;
    }
    #[cfg(target_os = "linux")]
    {
        // A process with that pid is running, but only the writer itself holds
        // the stream locked: an unlocked one means the pid was reused.
        Path::new("/proc").join(pid.to_string()).exists() && !stream_is_unlocked(stream)
    }
    #[cfg(not(target_os = "linux"))]
    {
        // Without a pid check, ask the file: a writer holds its stream locked
        // until it closes it, and the operating system drops the lock when the
        // process dies.
        !stream_is_unlocked(stream)
    }
}

/// Whether no process holds `stream` open with an exclusive lock.
fn stream_is_unlocked(stream: &Path) -> bool {
    let Ok(file) = OpenOptions::new().read(true).write(true).open(stream) else {
        return false;
    };
    file.try_lock().is_ok()
}

/// Cut a leftover stream to its last whole frame, as [`frame_at`] defines one,
/// without holding the whole stream in memory.
fn whole_stream(path: &Path) -> io::Result<WholeStream> {
    let file = File::open(path)?;
    let total = file.metadata()?.len();
    let mut reader = BufReader::new(file);
    let mut whole = WholeStream::default();
    let mut offset = 0u64;
    // A frame's eight header bytes and enough of the body to hold its id.
    let mut probe = [0u8; 13];
    while total - offset >= 8 {
        reader.read_exact(&mut probe[..8])?;
        let length = u32::from_be_bytes([probe[4], probe[5], probe[6], probe[7]]) as u64;
        let head = 8 + length.min(5);
        // The whole body has to be there, or the frame was cut short.
        if total - offset < 8 + length {
            break;
        }
        reader.read_exact(&mut probe[8..head as usize])?;
        let Some((timestamp_ms, _, _)) = frame_head(&probe[..head as usize], 0) else {
            break;
        };
        offset += 8 + length;
        whole.bytes = offset;
        whole.packets += 1;
        whole.last_timestamp_ms = whole.last_timestamp_ms.max(timestamp_ms);
        reader.seek_relative((length - (head - 8)) as i64)?;
    }
    Ok(whole)
}

/// Write `dest` from the whole frames of `stream`, and remove the leftover once
/// the archive is in place.
fn recover_stream(
    stream: &Path,
    dest: &Path,
    mut metadata: Metadata,
) -> io::Result<(PathBuf, usize)> {
    let whole = whole_stream(stream)?;
    if whole.packets == 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "it holds no whole frame",
        ));
    }
    metadata.duration = whole.last_timestamp_ms;
    let dest = free_name(dest);
    let part = sibling(&dest, "mcpr.part");
    if let Err(e) = package_stream(&metadata, stream, whole.bytes, &part) {
        let _ = fs::remove_file(&part);
        return Err(e);
    }
    if let Err(e) = fs::rename(&part, &dest) {
        let _ = fs::remove_file(&part);
        return Err(e);
    }
    let _ = fs::remove_file(stream);
    let _ = fs::remove_file(metadata_beside(stream));
    Ok((dest, whole.packets))
}

/// `dest`, or a `.recovered` name beside it when something already fills it.
fn free_name(dest: &Path) -> PathBuf {
    if !dest.exists() {
        return dest.to_owned();
    }
    let name = dest
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned();
    let stem = name.strip_suffix(".mcpr").unwrap_or(&name);
    let mut n = 1;
    loop {
        let candidate = dest.with_file_name(if n == 1 {
            format!("{stem}.recovered.mcpr")
        } else {
            format!("{stem}.recovered-{n}.mcpr")
        });
        if !candidate.exists() {
            return candidate;
        }
        n += 1;
    }
}

/// When a file was last written, in epoch milliseconds as `metaData.json` dates
/// are; `0` when the platform or filesystem does not say.
fn modified_ms(path: &Path) -> u64 {
    fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_millis() as u64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mcpr_roundtrip_metadata_and_packets() {
        let temp_dir = std::env::temp_dir().join(format!("mcpr_test_{}", std::process::id()));
        let _ = fs::create_dir_all(&temp_dir);
        let mcpr_path = temp_dir.join("test_recording.mcpr");

        let meta = Metadata {
            server_name: "Studio Test Server".to_owned(),
            players: vec!["Player1".to_owned(), "Player2".to_owned()],
            ..Default::default()
        };

        let mut writer = McprWriter::new(mcpr_path.clone(), meta).unwrap();
        writer.write_packet(0, 0x01, &[10, 20, 30]).unwrap();
        writer.write_packet(50, 0x24, &[40, 50]).unwrap();
        writer
            .write_packet(100, 0x1A, &[99, 100, 101, 102])
            .unwrap();
        writer.write_packet(500, 0x05, &[]).unwrap();

        assert_eq!(writer.packet_count(), 4);
        let finished_path = writer.finish(550).unwrap();
        assert_eq!(finished_path, mcpr_path);

        let mut reader = McprReader::open(&mcpr_path).unwrap();
        assert_eq!(reader.metadata.server_name, "Studio Test Server");
        assert_eq!(reader.metadata.duration, 550);
        assert_eq!(reader.metadata.players, vec!["Player1", "Player2"]);
        assert_eq!(reader.total_packets(), 4);

        let p0 = reader.next_packet().unwrap().unwrap();
        assert_eq!(p0.timestamp_ms, 0);
        assert_eq!(p0.id, 0x01);
        assert_eq!(p0.payload, vec![10, 20, 30]);
        let p1 = reader.next_packet().unwrap().unwrap();
        assert_eq!(p1.timestamp_ms, 50);
        assert_eq!(p1.id, 0x24);
        assert_eq!(p1.payload, vec![40, 50]);
        let p2 = reader.next_packet().unwrap().unwrap();
        assert_eq!(p2.timestamp_ms, 100);
        assert_eq!(p2.id, 0x1A);
        assert_eq!(p2.payload, vec![99, 100, 101, 102]);
        let p3 = reader.next_packet().unwrap().unwrap();
        assert_eq!(p3.timestamp_ms, 500);
        assert_eq!(p3.id, 0x05);
        assert!(p3.payload.is_empty());
        assert!(reader.next_packet().unwrap().is_none());

        let found_ts = reader.seek_to_timestamp(75).unwrap().unwrap();
        assert_eq!(found_ts, 50);
        let seeked = reader.next_packet().unwrap().unwrap();
        assert_eq!(seeked.timestamp_ms, 50);
        assert_eq!(seeked.id, 0x24);

        let _ = fs::remove_file(mcpr_path);
        let _ = fs::remove_dir(temp_dir);
    }

    /// A scratch folder for one test, removed when it goes out of scope.
    struct Scratch(PathBuf);
    impl Scratch {
        fn new(name: &str) -> Self {
            let dir = std::env::temp_dir().join(format!("mcpr_{name}_{}", std::process::id()));
            let _ = fs::remove_dir_all(&dir);
            fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }
    }
    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn archive(path: &Path, meta: &[u8], tmcpr: &[u8]) {
        let mut zip = zip::ZipWriter::new(File::create(path).unwrap());
        let options = zip::write::SimpleFileOptions::default();
        zip.start_file("metaData.json", options).unwrap();
        zip.write_all(meta).unwrap();
        zip.start_file("recording.tmcpr", options).unwrap();
        zip.write_all(tmcpr).unwrap();
        zip.finish().unwrap();
    }

    fn frame(out: &mut Vec<u8>, ts: u32, length: u32, body: &[u8]) {
        out.extend_from_slice(&ts.to_be_bytes());
        out.extend_from_slice(&length.to_be_bytes());
        out.extend_from_slice(body);
    }

    fn open_stream(scratch: &Scratch, tmcpr: &[u8]) -> McprReader {
        let path = scratch.0.join("r.mcpr");
        archive(
            &path,
            &serde_json::to_vec(&Metadata::default()).unwrap(),
            tmcpr,
        );
        McprReader::open(&path).unwrap()
    }

    /// A recording whose last packet was cut short indexes and plays only the
    /// whole ones, and playback ends cleanly where they end.
    #[test]
    fn a_truncated_last_packet_is_not_indexed() {
        let scratch = Scratch::new("cut");
        let mut tmcpr = Vec::new();
        frame(&mut tmcpr, 7, 3, &[0x01, 10, 20]);
        frame(&mut tmcpr, 9, 100, &[0x02, 1, 2]);
        let mut reader = open_stream(&scratch, &tmcpr);
        assert_eq!(reader.total_packets(), 1);
        assert_eq!(reader.seek_to_timestamp(50).unwrap(), Some(7));
        let first = reader.next_packet().unwrap().unwrap();
        assert_eq!((first.id, first.payload), (0x01, vec![10, 20]));
        assert_eq!(reader.next_packet().unwrap(), None);
    }

    /// A length field far past the end allocates nothing and ends the stream.
    #[test]
    fn a_huge_length_field_ends_the_stream() {
        let scratch = Scratch::new("huge");
        let mut tmcpr = Vec::new();
        frame(&mut tmcpr, 0, u32::MAX, &[0x01, 2, 3, 4]);
        let mut reader = open_stream(&scratch, &tmcpr);
        assert_eq!(reader.total_packets(), 0);
        assert_eq!(reader.next_packet().unwrap(), None);
        assert_eq!(reader.seek_to_timestamp(0).unwrap(), None);
    }

    /// A zero-length frame has no packet id: it is neither counted nor played,
    /// and the stream ends before it.
    #[test]
    fn a_zero_length_frame_ends_the_stream() {
        let scratch = Scratch::new("zero");
        let mut tmcpr = Vec::new();
        frame(&mut tmcpr, 1, 1, &[0x03]);
        frame(&mut tmcpr, 2, 0, &[]);
        frame(&mut tmcpr, 3, 1, &[0x04]);
        let mut reader = open_stream(&scratch, &tmcpr);
        assert_eq!(reader.total_packets(), 1);
        assert_eq!(reader.next_packet().unwrap().unwrap().id, 0x03);
        assert_eq!(reader.next_packet().unwrap(), None);
    }

    /// Seeking lands on the first packet of a millisecond, never in the middle
    /// of packets that share it.
    #[test]
    fn seeking_starts_at_the_first_packet_of_a_millisecond() {
        let scratch = Scratch::new("seek");
        let mut tmcpr = Vec::new();
        frame(&mut tmcpr, 50, 1, &[0x00]);
        for id in 1..=5 {
            frame(&mut tmcpr, 100, 1, &[id]);
        }
        frame(&mut tmcpr, 200, 1, &[0x06]);
        let mut reader = open_stream(&scratch, &tmcpr);
        for (target, ts, id) in [(100, 100, 1), (150, 100, 1), (10, 50, 0), (999, 200, 6)] {
            assert_eq!(
                reader.seek_to_timestamp(target).unwrap(),
                Some(ts),
                "{target}"
            );
            assert_eq!(reader.next_packet().unwrap().unwrap().id, id, "{target}");
        }
    }

    /// Large timestamp groups remain seekable even when sparse anchors land
    /// inside them, while index memory grows with stream size rather than every
    /// packet.
    #[test]
    fn sparse_index_seeks_to_timestamp_group_starts() {
        let scratch = Scratch::new("sparse-index");
        let mut tmcpr = Vec::new();
        for packet in 0..3072u32 {
            let timestamp = u32::from(packet >= 1500) * 10;
            frame(&mut tmcpr, timestamp, 2, &[0x01, packet as u8]);
        }
        let mut reader = open_stream(&scratch, &tmcpr);
        let temp_path = reader.stream.path.clone();
        assert_eq!(reader.total_packets(), 3072);
        assert_eq!(reader.packet_index.len(), 3);

        assert_eq!(reader.seek_to_timestamp(0).unwrap(), Some(0));
        assert_eq!(reader.next_packet().unwrap().unwrap().payload, vec![0x00]);
        assert_eq!(reader.seek_to_timestamp(10).unwrap(), Some(10));
        assert_eq!(reader.next_packet().unwrap().unwrap().payload, vec![220]);

        drop(reader);
        assert!(!temp_path.exists(), "temporary stream is removed on close");
    }

    #[test]
    fn streaming_reader_checks_the_mcpr_stream_crc() {
        let scratch = Scratch::new("crc");
        let path = scratch.0.join("bad-crc.mcpr");
        let mut zip = zip::ZipWriter::new(File::create(&path).unwrap());
        let options = zip::write::SimpleFileOptions::default();
        zip.start_file("metaData.json", options).unwrap();
        zip.write_all(&serde_json::to_vec(&Metadata::default()).unwrap())
            .unwrap();
        zip.start_file("recording.tmcpr", options).unwrap();
        zip.write_all(&[1, 2, 3]).unwrap();
        zip.start_file("recording.tmcpr.crc32", options).unwrap();
        zip.write_all(b"00000000").unwrap();
        zip.finish().unwrap();

        let error = McprReader::open(&path).err().unwrap();
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        assert!(error.to_string().contains("CRC32 mismatch"));
    }

    /// A timestamp that goes backwards is played as stored but indexed at the
    /// time before it, so seeking still finds the right place.
    #[test]
    fn a_backwards_timestamp_does_not_break_seeking() {
        let scratch = Scratch::new("backwards");
        let mut tmcpr = Vec::new();
        frame(&mut tmcpr, 100, 1, &[0x01]);
        frame(&mut tmcpr, 20, 1, &[0x02]);
        frame(&mut tmcpr, 300, 1, &[0x03]);
        let mut reader = open_stream(&scratch, &tmcpr);
        assert_eq!(reader.seek_to_timestamp(250).unwrap(), Some(100));
        let first = reader.next_packet().unwrap().unwrap();
        assert_eq!((first.timestamp_ms, first.id), (100, 0x01));
        assert_eq!(reader.next_packet().unwrap().unwrap().timestamp_ms, 20);
    }

    /// Metadata written by other tools may leave out the fields a recording
    /// can do without; a file that isn't MCPR or has oversized metadata is refused.
    #[test]
    fn metadata_reads_minimal_files_and_refuses_bad_ones() {
        let scratch = Scratch::new("meta");
        let path = scratch.0.join("m.mcpr");
        let minimal = br#"{"mcversion":"1.21.4","fileFormat":"MCPR","fileFormatVersion":14,"customServerName":"My World"}"#;
        archive(&path, minimal, &[]);
        let reader = McprReader::open(&path).unwrap();
        assert_eq!(reader.metadata.protocol, None);
        assert_eq!(reader.metadata.server_name, "");
        assert_eq!(
            reader.metadata.custom_server_name.as_deref(),
            Some("My World")
        );

        archive(
            &path,
            br#"{"mcversion":"1.21.4","fileFormat":"OTHER","fileFormatVersion":1}"#,
            &[],
        );
        assert_eq!(
            McprReader::open(&path).err().unwrap().kind(),
            io::ErrorKind::InvalidData
        );

        let mut big =
            br#"{"mcversion":"1.21.4","fileFormat":"MCPR","fileFormatVersion":14,"generator":""#
                .to_vec();
        big.resize(big.len() + MAX_METADATA_BYTES as usize, b'a');
        big.extend_from_slice(br#""}"#);
        archive(&path, &big, &[]);
        assert_eq!(
            McprReader::open(&path).err().unwrap().kind(),
            io::ErrorKind::InvalidData
        );
    }

    /// A finish that can't put the archive in place keeps the packets and says
    /// where they are; a writer dropped without finishing keeps them too.
    #[test]
    fn a_failed_finish_keeps_the_recording() {
        let scratch = Scratch::new("finish");
        // A non-empty folder at the destination: the final rename fails.
        let dest = scratch.0.join("blocked.mcpr");
        fs::create_dir_all(dest.join("inside")).unwrap();
        let mut writer = McprWriter::new(dest.clone(), Metadata::default()).unwrap();
        writer.write_packet(5, 0x07, &[1, 2, 3]).unwrap();
        let stream = writer.stream_path().to_owned();
        let err = writer.finish(10).unwrap_err();
        assert!(
            err.to_string().contains(&stream.display().to_string()),
            "{err}"
        );
        assert!(stream.exists());
        let leftovers: Vec<_> = fs::read_dir(&scratch.0)
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .filter(|n| n.ends_with(".part"))
            .collect();
        assert!(leftovers.is_empty(), "{leftovers:?}");

        let mut kept = McprWriter::new(scratch.0.join("kept.mcpr"), Metadata::default()).unwrap();
        kept.write_packet(0, 0x01, &[]).unwrap();
        let kept_stream = kept.stream_path().to_owned();
        drop(kept);
        assert!(kept_stream.exists(), "a dropped writer keeps its packets");

        let dropped = McprWriter::new(scratch.0.join("dropped.mcpr"), Metadata::default()).unwrap();
        let empty_stream = dropped.stream_path().to_owned();
        drop(dropped);
        assert!(!empty_stream.exists(), "an empty stream is not kept");
    }

    /// Finishing over an existing recording replaces it only with a complete one.
    #[test]
    fn finishing_replaces_an_existing_recording() {
        let scratch = Scratch::new("replace");
        let dest = scratch.0.join("same.mcpr");
        for id in [0x01, 0x02] {
            let mut writer = McprWriter::new(dest.clone(), Metadata::default()).unwrap();
            writer.write_packet(0, id, &[]).unwrap();
            writer.finish(1).unwrap();
        }
        let mut reader = McprReader::open(&dest).unwrap();
        assert_eq!(reader.next_packet().unwrap().unwrap().id, 0x02);
        assert_eq!(fs::read_dir(&scratch.0).unwrap().count(), 1);
    }

    /// A stream and the metadata a crash leaves: `u32::MAX` is not a pid any
    /// process can have, so recovery takes it for a dead writer.
    fn leftover(scratch: &Scratch, name: &str, meta: Option<&Metadata>, tmcpr: &[u8]) -> PathBuf {
        let stream = scratch
            .0
            .join(format!("{name}.mcpr.{}-77.tmcpr.tmp", u32::MAX));
        fs::write(&stream, tmcpr).unwrap();
        if let Some(meta) = meta {
            fs::write(
                metadata_beside(&stream),
                serde_json::to_vec_pretty(meta).unwrap(),
            )
            .unwrap();
        }
        stream
    }

    /// The recording a crash cut mid-frame comes back up to its last whole
    /// frame, with the metadata written beside it, and the leftover is cleaned up.
    #[test]
    fn a_crashed_recording_recovers_to_its_last_whole_frame() {
        let scratch = Scratch::new("recover");
        let mut tmcpr = Vec::new();
        frame(&mut tmcpr, 10, 3, &[0x01, 10, 20]);
        frame(&mut tmcpr, 20, 2, &[0x02, 30]);
        // The crash cut the third frame after its header and its packet id.
        frame(&mut tmcpr, 30, 40, &[0x03, 1, 2]);
        let meta = Metadata {
            server_name: "Crashed World".to_owned(),
            players: vec!["Steve".to_owned()],
            ..Default::default()
        };
        let stream = leftover(&scratch, "session", Some(&meta), &tmcpr);

        let report = recover_crashed_recordings(&scratch.0).unwrap();
        let archive = scratch.0.join("session.mcpr");
        assert_eq!(report.recovered, vec![archive.clone()]);
        assert!(report.skipped.is_empty(), "{report:?}");
        assert!(report.kept.is_empty(), "{report:?}");
        assert!(!stream.exists(), "a recovered leftover is cleaned up");

        let mut reader = McprReader::open(&archive).unwrap();
        assert_eq!(reader.metadata.server_name, "Crashed World");
        assert_eq!(reader.metadata.players, vec!["Steve".to_owned()]);
        assert_eq!(reader.metadata.duration, 20);
        assert_eq!(reader.total_packets(), 2);
        let first = reader.next_packet().unwrap().unwrap();
        assert_eq!((first.timestamp_ms, first.id), (10, 0x01));
        assert_eq!(first.payload, vec![10, 20]);
        assert_eq!(reader.next_packet().unwrap().unwrap().id, 0x02);
        assert_eq!(reader.next_packet().unwrap(), None);
    }

    /// A leftover from before the sidecar existed is still packaged, with the
    /// format's defaults standing in for the session it can no longer name.
    #[test]
    fn a_leftover_without_metadata_recovers_with_defaults() {
        let scratch = Scratch::new("nometa");
        let mut tmcpr = Vec::new();
        frame(&mut tmcpr, 5, 1, &[0x09]);
        let stream = leftover(&scratch, "bare", None, &tmcpr);

        let report = recover_crashed_recordings(&scratch.0).unwrap();
        let archive = scratch.0.join("bare.mcpr");
        assert_eq!(report.recovered, vec![archive.clone()]);
        assert!(!stream.exists());

        let reader = McprReader::open(&archive).unwrap();
        assert_eq!(reader.metadata.server_name, Metadata::default().server_name);
        assert_eq!(
            reader.metadata.protocol,
            Some(horizon_protocol::PROTOCOL_VERSION)
        );
        assert!(
            reader.metadata.date > 0,
            "the leftover's age dates the file"
        );
        assert_eq!(reader.total_packets(), 1);
    }

    /// A writer that is still running is left alone: its stream stays, and the
    /// metadata written at recording start is there for when it can be recovered.
    #[test]
    fn recovery_leaves_a_live_writers_stream_alone() {
        let scratch = Scratch::new("live");
        let dest = scratch.0.join("live.mcpr");
        let mut writer = McprWriter::new(dest.clone(), Metadata::default()).unwrap();
        writer.write_packet(0, 0x04, &[1]).unwrap();
        let stream = writer.stream_path().to_owned();
        assert!(
            writer.metadata_path().exists(),
            "metadata is written when recording starts"
        );

        let report = recover_crashed_recordings(&scratch.0).unwrap();
        assert_eq!(report.skipped, vec![stream.clone()]);
        assert!(report.recovered.is_empty(), "{report:?}");
        assert!(report.kept.is_empty(), "{report:?}");
        assert!(stream.exists(), "a live stream is not touched");
        assert!(!dest.exists(), "a live recording is not packaged");

        writer.finish(1).unwrap();
        assert!(!stream.exists(), "finish still cleans up");
    }

    /// A stream with no whole frame cannot become a recording, so it and its
    /// metadata are kept rather than deleted.
    #[test]
    fn a_leftover_with_no_whole_frame_is_kept() {
        let scratch = Scratch::new("kept");
        let stream = leftover(&scratch, "broken", Some(&Metadata::default()), &[0, 0, 0]);
        let report = recover_crashed_recordings(&scratch.0).unwrap();
        assert_eq!(report.kept, vec![stream.clone()]);
        assert!(report.recovered.is_empty(), "{report:?}");
        assert!(stream.exists(), "an unrecoverable leftover is kept");
        assert!(metadata_beside(&stream).exists());
    }

    /// Recovery never overwrites a recording that is already there.
    #[test]
    fn recovery_does_not_replace_an_existing_recording() {
        let scratch = Scratch::new("existing");
        let existing = scratch.0.join("taken.mcpr");
        archive(&existing, b"{}", &[]);
        let mut tmcpr = Vec::new();
        frame(&mut tmcpr, 1, 1, &[0x02]);
        let stream = leftover(&scratch, "taken", None, &tmcpr);

        let report = recover_crashed_recordings(&scratch.0).unwrap();
        assert_eq!(
            report.recovered,
            vec![scratch.0.join("taken.recovered.mcpr")]
        );
        assert!(!stream.exists());
        assert!(existing.exists());
    }

    /// The metadata beside a stream follows it while recording, so a crash can
    /// never leave a sidecar describing packets the stream does not hold.
    #[test]
    fn the_metadata_follows_the_stream_it_belongs_to() {
        let scratch = Scratch::new("cadence");
        let mut writer = McprWriter::new(scratch.0.join("c.mcpr"), Metadata::default()).unwrap();
        writer.write_packet(0, 0x01, &[]).unwrap();
        writer
            .write_packet(2 * FLUSH_INTERVAL_MS, 0x02, &[])
            .unwrap();
        let sidecar: Metadata =
            serde_json::from_slice(&fs::read(writer.metadata_path()).unwrap()).unwrap();
        assert_eq!(sidecar.duration, 2 * FLUSH_INTERVAL_MS);
        assert_eq!(sidecar.generator, "Horizon Studio");
    }
}
