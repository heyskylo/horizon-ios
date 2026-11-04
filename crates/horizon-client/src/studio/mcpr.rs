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
    io::{self, BufReader, BufWriter, Cursor, Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
};

/// High-level session metadata stored in `metaData.json`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Metadata {
    pub singleplayer: bool,
    pub server_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub custom_server_name: Option<String>,
    pub duration: u32,
    pub date: u64,
    pub mcversion: String,
    pub file_format: String,
    pub file_format_version: u32,
    pub protocol: i32,
    pub generator: String,
    pub self_id: i32,
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
            mcversion: "26.3".to_owned(),
            file_format: "MCPR".to_owned(),
            file_format_version: 14,
            protocol: horizon_protocol::PROTOCOL_VERSION,
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

/// Writer for generating `.mcpr` replay files.
pub struct McprWriter {
    dest_path: PathBuf,
    temp_tmcpr_path: PathBuf,
    writer: Option<BufWriter<File>>,
    metadata: Metadata,
    packet_count: usize,
    bytes_written: u64,
}

impl McprWriter {
    /// Create a new replay writer that writes packets to a temporary stream file
    /// and finalizes into `dest_path` when finished.
    pub fn new(dest_path: PathBuf, metadata: Metadata) -> io::Result<Self> {
        if let Some(parent) = dest_path.parent() {
            fs::create_dir_all(parent)?;
        }
        let temp_tmcpr_path = dest_path.with_extension("tmcpr.tmp");
        let file = File::create(&temp_tmcpr_path)?;
        let writer = Some(BufWriter::with_capacity(64 * 1024, file));
        Ok(Self {
            dest_path,
            temp_tmcpr_path,
            writer,
            metadata,
            packet_count: 0,
            bytes_written: 0,
        })
    }

    /// Record a packet at the given elapsed timestamp.
    pub fn write_packet(&mut self, timestamp_ms: u32, id: i32, payload: &[u8]) -> io::Result<()> {
        let writer = self
            .writer
            .as_mut()
            .ok_or_else(|| io::Error::other("writer already closed"))?;
        let mut varint_buf = Vec::with_capacity(5);
        write_varint(&mut varint_buf, id);
        let packet_length = (varint_buf.len() + payload.len()) as u32;

        writer.write_all(&timestamp_ms.to_be_bytes())?;
        writer.write_all(&packet_length.to_be_bytes())?;
        writer.write_all(&varint_buf)?;
        writer.write_all(payload)?;

        self.packet_count += 1;
        self.bytes_written += 8 + packet_length as u64;
        Ok(())
    }

    pub fn packet_count(&self) -> usize {
        self.packet_count
    }

    pub fn bytes_written(&self) -> u64 {
        self.bytes_written
    }

    /// Complete recording, write `metaData.json`, package into `.mcpr` ZIP,
    /// and clean up temporary files.
    pub fn finish(mut self, duration_ms: u32) -> io::Result<PathBuf> {
        if let Some(mut w) = self.writer.take() {
            w.flush()?;
        }

        self.metadata.duration = duration_ms;

        let mcpr_file = File::create(&self.dest_path)?;
        let mut zip = zip::ZipWriter::new(mcpr_file);
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);

        zip.start_file("metaData.json", options)?;
        let meta_json = serde_json::to_vec_pretty(&self.metadata)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        zip.write_all(&meta_json)?;

        zip.start_file("recording.tmcpr", options)?;
        let mut tmcpr_reader = BufReader::new(File::open(&self.temp_tmcpr_path)?);
        io::copy(&mut tmcpr_reader, &mut zip)?;

        zip.finish().map_err(io::Error::other)?;

        let _ = fs::remove_file(&self.temp_tmcpr_path);
        Ok(self.dest_path.clone())
    }
}

impl Drop for McprWriter {
    fn drop(&mut self) {
        if self.temp_tmcpr_path.exists() {
            let _ = fs::remove_file(&self.temp_tmcpr_path);
        }
    }
}

/// Reader for opening and streaming `.mcpr` replay files.
pub struct McprReader {
    pub metadata: Metadata,
    tmcpr_data: Cursor<Vec<u8>>,
    packet_index: Vec<(u32, u64)>, // (timestamp_ms, byte_offset in tmcpr)
}

impl McprReader {
    /// Open an `.mcpr` replay archive from disk.
    pub fn open(path: &Path) -> io::Result<Self> {
        let file = File::open(path)?;
        let mut archive = zip::ZipArchive::new(file)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;

        let metadata: Metadata = {
            let mut meta_file = archive
                .by_name("metaData.json")
                .map_err(|e| io::Error::new(io::ErrorKind::NotFound, e))?;
            let mut meta_bytes = Vec::new();
            meta_file.read_to_end(&mut meta_bytes)?;
            serde_json::from_slice(&meta_bytes)
                .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?
        };

        let tmcpr_bytes = {
            let mut tmcpr_file = archive
                .by_name("recording.tmcpr")
                .map_err(|e| io::Error::new(io::ErrorKind::NotFound, e))?;
            let mut bytes = Vec::new();
            tmcpr_file.read_to_end(&mut bytes)?;
            bytes
        };

        let mut packet_index = Vec::new();
        let mut cursor = Cursor::new(&tmcpr_bytes);
        let total_len = tmcpr_bytes.len() as u64;

        while cursor.position() + 8 <= total_len {
            let offset = cursor.position();
            let mut header = [0u8; 8];
            if cursor.read_exact(&mut header).is_err() {
                break;
            }
            let timestamp_ms = u32::from_be_bytes([header[0], header[1], header[2], header[3]]);
            let length = u32::from_be_bytes([header[4], header[5], header[6], header[7]]) as u64;

            packet_index.push((timestamp_ms, offset));
            if cursor.seek(SeekFrom::Current(length as i64)).is_err() {
                break;
            }
        }

        let tmcpr_data = Cursor::new(tmcpr_bytes);
        Ok(Self {
            metadata,
            tmcpr_data,
            packet_index,
        })
    }

    /// Total number of packets indexed in the recording.
    pub fn total_packets(&self) -> usize {
        self.packet_index.len()
    }

    /// Read the next timestamped packet from the current position.
    pub fn next_packet(&mut self) -> io::Result<Option<TimestampedPacket>> {
        let mut header = [0u8; 8];
        match self.tmcpr_data.read_exact(&mut header) {
            Ok(()) => {}
            Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => return Ok(None),
            Err(e) => return Err(e),
        }

        let timestamp_ms = u32::from_be_bytes([header[0], header[1], header[2], header[3]]);
        let length = u32::from_be_bytes([header[4], header[5], header[6], header[7]]) as usize;

        let mut data = vec![0u8; length];
        self.tmcpr_data.read_exact(&mut data)?;

        let mut reader = Reader::new(&data);
        let id = reader
            .varint()
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        let payload = reader.rest().to_vec();

        Ok(Some(TimestampedPacket {
            timestamp_ms,
            id,
            payload,
        }))
    }

    /// Seek to the packet closest to or preceding `target_timestamp_ms`.
    pub fn seek_to_timestamp(&mut self, target_timestamp_ms: u32) -> io::Result<Option<u32>> {
        if self.packet_index.is_empty() {
            return Ok(None);
        }
        let idx = match self
            .packet_index
            .binary_search_by_key(&target_timestamp_ms, |&(ts, _)| ts)
        {
            Ok(exact) => exact,
            Err(insert_idx) => insert_idx.saturating_sub(1),
        };

        let (ts, offset) = self.packet_index[idx];
        self.tmcpr_data.seek(SeekFrom::Start(offset))?;
        Ok(Some(ts))
    }

    /// Reset stream reading to the beginning.
    pub fn rewind(&mut self) -> io::Result<()> {
        self.tmcpr_data.seek(SeekFrom::Start(0))?;
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
}
