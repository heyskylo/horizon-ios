//! `.mcpr` Replay File Container Specification & I/O
//!
//! An `.mcpr` file is a ZIP archive containing:
//! - `metaData.json`: metadata about the recording session (version, duration, player UUIDs, etc.)
//! - `recording.tmcpr`: binary stream of chronological timestamped network packets:
//!   `[timestamp_ms: u32 big-endian][packet_length: u32 big-endian][packet_data: VarInt(packet_id) + payload]`
//! - Optional `markers.json`: timeline markers/chapters.

use horizon_protocol::buf::{Reader, write_varint};
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File},
    io::{self, BufReader, BufWriter, Read, Write},
    path::{Path, PathBuf},
};

/// The most `metaData.json` may inflate to: real files are a few hundred bytes.
const MAX_METADATA_BYTES: u64 = 1 << 20;
/// The most `recording.tmcpr` may inflate to. The reader keeps the stream in
/// memory, so a hostile archive (deflate reaches about 1000:1) must not be able
/// to ask for more than this.
const MAX_STREAM_BYTES: u64 = 1 << 30;

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

/// Writer for generating `.mcpr` replay files.
///
/// Packets go to a stream file beside the destination
/// (`<name>.mcpr.<pid>-<nanos>.tmcpr.tmp`). That file is only removed once
/// [`finish`](Self::finish) has put the finished archive in place, so a failed
/// finish or a writer dropped without one leaves the packets on disk.
pub struct McprWriter {
    dest_path: PathBuf,
    temp_tmcpr_path: PathBuf,
    writer: Option<BufWriter<File>>,
    metadata: Metadata,
    packet_count: usize,
    /// Bytes of whole frames in the stream: what `finish` packages.
    bytes_written: u64,
    /// A write failed: the stream may end in a partial frame, and nothing more
    /// is appended after it.
    failed: bool,
}

impl McprWriter {
    /// Create a new replay writer that writes packets to a temporary stream file
    /// and finalizes into `dest_path` when finished.
    pub fn new(dest_path: PathBuf, metadata: Metadata) -> io::Result<Self> {
        if let Some(parent) = dest_path.parent() {
            fs::create_dir_all(parent)?;
        }
        let temp_tmcpr_path = sibling(&dest_path, "tmcpr.tmp");
        let file = File::create_new(&temp_tmcpr_path)?;
        let writer = Some(BufWriter::with_capacity(64 * 1024, file));
        Ok(Self {
            dest_path,
            temp_tmcpr_path,
            writer,
            metadata,
            packet_count: 0,
            bytes_written: 0,
            failed: false,
        })
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

    /// Complete recording, write `metaData.json`, package into `.mcpr` ZIP,
    /// and clean up temporary files.
    ///
    /// The archive is written to a temporary file beside `dest_path`, synced and
    /// renamed over it, so an existing file at `dest_path` stays whole until the
    /// new one is complete. On error the stream file is kept and the error names it.
    pub fn finish(mut self, duration_ms: u32) -> io::Result<PathBuf> {
        if let Some(mut w) = self.writer.take() {
            // A failed flush leaves a partial frame at the end at worst: only
            // `bytes_written` are packaged below.
            if w.flush().is_err() {
                self.failed = true;
            }
        }
        self.metadata.duration = duration_ms;

        let part = sibling(&self.dest_path, "mcpr.part");
        match self.package(&part) {
            Ok(()) => {
                let _ = fs::remove_file(&self.temp_tmcpr_path);
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
        let mut zip = zip::ZipWriter::new(File::create_new(part)?);
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);

        zip.start_file("metaData.json", options)?;
        let meta_json = serde_json::to_vec_pretty(&self.metadata)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        zip.write_all(&meta_json)?;

        // Zip64, so a recording past 4 GiB still finishes.
        zip.start_file("recording.tmcpr", options.large_file(true))?;
        let mut stream =
            BufReader::new(File::open(&self.temp_tmcpr_path)?).take(self.bytes_written);
        if io::copy(&mut stream, &mut zip)? != self.bytes_written {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "the stream file is shorter than what was written",
            ));
        }

        let file = zip.finish().map_err(io::Error::other)?;
        file.sync_all()?;
        drop(file);
        fs::rename(part, &self.dest_path)
    }
}

impl Drop for McprWriter {
    fn drop(&mut self) {
        // An empty stream holds nothing worth keeping; one with packets stays
        // for a later recovery (see the type's doc).
        if self.packet_count == 0 && self.temp_tmcpr_path.exists() {
            self.writer = None;
            let _ = fs::remove_file(&self.temp_tmcpr_path);
        }
    }
}

/// The frame at `offset`: `(timestamp_ms, id, payload range, next offset)`, or `None` when it
/// isn't whole (a short header, a length past the end, or an id that doesn't
/// fit in its length). The index and `next_packet` both read frames through
/// this, so they agree on where a damaged stream ends.
fn frame_at(data: &[u8], offset: usize) -> Option<(u32, i32, std::ops::Range<usize>, usize)> {
    let header = data.get(offset..offset.checked_add(8)?)?;
    let timestamp_ms = u32::from_be_bytes([header[0], header[1], header[2], header[3]]);
    let length = u32::from_be_bytes([header[4], header[5], header[6], header[7]]) as usize;
    let start = offset + 8;
    let end = start.checked_add(length)?;
    let body = data.get(start..end)?;
    let mut reader = Reader::new(body);
    let id = reader.varint().ok()?;
    let payload_start = end - reader.rest().len();
    Some((timestamp_ms, id, payload_start..end, end))
}

/// Reader for opening and streaming `.mcpr` replay files.
pub struct McprReader {
    pub metadata: Metadata,
    tmcpr_data: Vec<u8>,
    /// Where `next_packet` reads next.
    position: usize,
    /// The end of the last whole frame: the stream stops here.
    end: usize,
    /// `(timestamp_ms, byte_offset)` of each whole frame. Timestamps never go
    /// backwards here: one earlier than the frame before it is indexed at that
    /// frame's time, so seeking stays defined on a damaged file.
    packet_index: Vec<(u32, usize)>,
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

        let tmcpr_data = read_entry(&mut archive, "recording.tmcpr", MAX_STREAM_BYTES)?;

        // A recording that stopped mid-write ends in a frame that isn't whole:
        // indexing and playback both stop before it.
        let mut packet_index = Vec::new();
        let mut offset = 0;
        let mut latest = 0;
        while let Some((timestamp_ms, _, _, next)) = frame_at(&tmcpr_data, offset) {
            latest = latest.max(timestamp_ms);
            packet_index.push((latest, offset));
            offset = next;
        }

        Ok(Self {
            metadata,
            tmcpr_data,
            position: 0,
            end: offset,
            packet_index,
        })
    }

    /// Total number of packets indexed in the recording.
    pub fn total_packets(&self) -> usize {
        self.packet_index.len()
    }

    /// Read the next timestamped packet from the current position, or `None`
    /// at the end of the last whole frame.
    pub fn next_packet(&mut self) -> io::Result<Option<TimestampedPacket>> {
        if self.position >= self.end {
            return Ok(None);
        }
        let Some((timestamp_ms, id, payload, next)) = frame_at(&self.tmcpr_data, self.position)
        else {
            return Ok(None);
        };
        self.position = next;
        Ok(Some(TimestampedPacket {
            timestamp_ms,
            id,
            payload: self.tmcpr_data[payload].to_vec(),
        }))
    }

    /// Seek to the first packet of the latest timestamp at or before
    /// `target_timestamp_ms` (the first packet when the target is earlier than
    /// all of them), and return that timestamp.
    ///
    /// Starting at the first packet of a millisecond keeps the packets that
    /// arrived together, such as a chunk and the entities in it, together.
    pub fn seek_to_timestamp(&mut self, target_timestamp_ms: u32) -> io::Result<Option<u32>> {
        let after = self
            .packet_index
            .partition_point(|&(ts, _)| ts <= target_timestamp_ms);
        let chosen = match after.checked_sub(1) {
            Some(last) => self.packet_index[last].0,
            None => match self.packet_index.first() {
                Some(&(ts, _)) => ts,
                None => return Ok(None),
            },
        };
        let idx = self.packet_index.partition_point(|&(ts, _)| ts < chosen);
        self.position = self.packet_index[idx].1;
        Ok(Some(chosen))
    }

    /// Reset stream reading to the beginning.
    pub fn rewind(&mut self) -> io::Result<()> {
        self.position = 0;
        Ok(())
    }
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
}
