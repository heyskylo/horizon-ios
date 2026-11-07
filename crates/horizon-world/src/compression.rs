//! Region codecs, including lz4-java's block framing (not the LZ4 frame format).
use flate2::{Compression as Level, Decompress, FlushDecompress, Status};
use std::{
    io::{self, Read, Write},
    sync::atomic::{AtomicU8, Ordering},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Compression {
    Gzip = 1,
    Zlib = 2,
    None = 3,
    Lz4 = 4,
}

/// The region file versions `RegionFileVersion.register` puts in its table, in
/// registration order: the names server.properties' `region-file-compression`
/// takes, and the ones an invalid value is told to use.
pub const REGION_FILE_NAMES: [&str; 4] = ["gzip", "deflate", "none", "lz4"];

/// `RegionFileVersion.selected`, which starts at `DEFAULT` (VERSION_DEFLATE).
static SELECTED: AtomicU8 = AtomicU8::new(Compression::Zlib as u8);

impl Compression {
    /// `RegionFileVersion.configure`: what `region-file-compression` names
    /// becomes the version every region file written from here on uses,
    /// whichever world it belongs to. An unknown name leaves the selection as
    /// it was, as vanilla logs the value and keeps going. False for a name
    /// that is not one of [`REGION_FILE_NAMES`].
    pub fn configure(name: &str) -> bool {
        let Some(compression) = REGION_FILE_NAMES
            .iter()
            .position(|known| *known == name)
            .map(|i| [Self::Gzip, Self::Zlib, Self::None, Self::Lz4][i])
        else {
            return false;
        };
        SELECTED.store(compression as u8, Ordering::Relaxed);
        true
    }
    /// `RegionFileVersion.getSelected`: the version a chunk or a poi region
    /// is written with.
    pub fn selected() -> Self {
        Self::try_from(SELECTED.load(Ordering::Relaxed)).unwrap_or(Self::Zlib)
    }
}

pub(crate) fn invalid(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.into())
}

impl TryFrom<u8> for Compression {
    type Error = io::Error;
    fn try_from(value: u8) -> io::Result<Self> {
        match value {
            1 => Ok(Self::Gzip),
            2 => Ok(Self::Zlib),
            3 => Ok(Self::None),
            4 => Ok(Self::Lz4),
            _ => Err(invalid(format!(
                "unsupported region compression ID {value}"
            ))),
        }
    }
}

impl Compression {
    pub fn decode(self, bytes: &[u8], max_output: usize) -> io::Result<Vec<u8>> {
        let mut output = Vec::new();
        self.decode_into(bytes, max_output, &mut output)?;
        Ok(output)
    }
    /// [`decode`](Self::decode) into `output`, which keeps what was decoded
    /// before an error: what a stream reader would already have handed out.
    pub fn decode_into(
        self,
        bytes: &[u8],
        max_output: usize,
        output: &mut Vec<u8>,
    ) -> io::Result<()> {
        match self {
            Self::None => {
                if bytes.len() > max_output {
                    return Err(invalid("decompression limit exceeded"));
                }
                output.extend_from_slice(bytes);
                Ok(())
            }
            Self::Gzip => {
                let decoder = flate2::read::MultiGzDecoder::new(bytes);
                decoder
                    .take((max_output as u64).saturating_add(1))
                    .read_to_end(output)?;
                if output.len() > max_output {
                    return Err(invalid("decompression limit exceeded"));
                }
                Ok(())
            }
            Self::Zlib => zlib_into(bytes, max_output, output),
            Self::Lz4 => decode_lz4(bytes, max_output, output),
        }
    }

    pub fn encode(self, bytes: &[u8]) -> io::Result<Vec<u8>> {
        match self {
            Self::None => Ok(bytes.to_vec()),
            Self::Gzip => {
                let mut encoder = flate2::write::GzEncoder::new(Vec::new(), Level::default());
                encoder.write_all(bytes)?;
                encoder.finish()
            }
            Self::Zlib => {
                let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), Level::default());
                encoder.write_all(bytes)?;
                encoder.finish()
            }
            Self::Lz4 => Ok(encode_lz4(bytes)),
        }
    }
}

thread_local! {
    /// This thread's zlib inflater, reset for each chunk instead of a new
    /// one (and its window) allocated every time.
    static INFLATER: std::cell::RefCell<Option<Decompress>> = const { std::cell::RefCell::new(None) };
}

/// The zlib stream in `bytes` decoded onto `output`, at most `max_output`
/// bytes in all. A whole stream that fits is inflated straight into
/// `output`, which grows as it fills (from about eight times the stream, a
/// chunk's usual ratio, never past one byte over the limit); any other
/// stream (damaged, cut short, too long) is decoded again by
/// [`zlib_blocks`], so what an error leaves in `output` stays as it was.
fn zlib_into(bytes: &[u8], max_output: usize, output: &mut Vec<u8>) -> io::Result<()> {
    INFLATER.with_borrow_mut(|inflater| {
        let decoder = inflater.get_or_insert_with(|| Decompress::new(true));
        decoder.reset(true);
        let start = output.len();
        let room = max_output.saturating_sub(start);
        let mut first = Some(bytes.len().saturating_mul(8));
        loop {
            if output.capacity() == output.len() {
                let want = first.take().unwrap_or(output.len() - start).max(8192);
                output.reserve_exact(want.min((start + room).saturating_add(1) - output.len()));
            }
            let before_in = decoder.total_in();
            let before_out = output.len();
            let status =
                decoder.decompress_vec(&bytes[before_in as usize..], output, FlushDecompress::None);
            let fits = output.len() - start <= room;
            match status {
                Ok(Status::StreamEnd) if fits => return Ok(()),
                Ok(_)
                    if fits && (decoder.total_in() != before_in || output.len() != before_out) => {}
                _ => break,
            }
        }
        output.truncate(start);
        decoder.reset(true);
        zlib_blocks(decoder, bytes, max_output, output)
    })
}

/// The zlib stream in `bytes` inflated 8 KiB at a time, each block copied
/// onto `output` while the limit allows: on an error `output` keeps the
/// blocks before it, as a stream reader would have handed them out.
fn zlib_blocks(
    decoder: &mut Decompress,
    bytes: &[u8],
    max_output: usize,
    output: &mut Vec<u8>,
) -> io::Result<()> {
    let mut block = [0; 8192];
    loop {
        let before_in = decoder.total_in();
        let before_out = decoder.total_out();
        let status = decoder
            .decompress(
                &bytes[before_in as usize..],
                &mut block,
                FlushDecompress::None,
            )
            .map_err(|e| invalid(e.to_string()))?;
        let produced = (decoder.total_out() - before_out) as usize;
        if produced > max_output.saturating_sub(output.len()) {
            return Err(invalid("decompression limit exceeded"));
        }
        output.extend_from_slice(&block[..produced]);
        if status == Status::StreamEnd {
            return Ok(());
        }
        if decoder.total_in() == before_in && produced == 0 {
            return Err(invalid("truncated zlib stream"));
        }
    }
}

const MAGIC: &[u8; 8] = b"LZ4Block";
fn checksum(bytes: &[u8]) -> u32 {
    // The Checksum adapter used by the pinned Java library masks to 28 bits.
    xxhash_rust::xxh32::xxh32(bytes, 0x9747_b28c) & 0x0fff_ffff
}

fn decode_lz4(mut bytes: &[u8], max_output: usize, output: &mut Vec<u8>) -> io::Result<()> {
    // `LZ4BlockInputStream.compressedBuffer`: each header is read into it,
    // and a block's bytes over its start; it grows by half when too small.
    // The decompressor may read up to its end, past the block.
    let mut buffer = vec![0; 21];
    loop {
        let header = bytes
            .get(..21)
            .ok_or_else(|| invalid("truncated LZ4 header"))?;
        if &header[..8] != MAGIC {
            return Err(invalid("invalid LZ4 block magic"));
        }
        let method = header[8] & 0xf0;
        if !matches!(method, 0x10 | 0x20) {
            return Err(invalid("invalid LZ4 block method"));
        }
        let compressed = u32::from_le_bytes(header[9..13].try_into().unwrap()) as usize;
        let raw = u32::from_le_bytes(header[13..17].try_into().unwrap()) as usize;
        let expected = u32::from_le_bytes(header[17..21].try_into().unwrap());
        if raw > (1 << (10 + (header[8] & 15)))
            || (raw == 0) != (compressed == 0)
            || (method == 0x10 && raw != compressed)
        {
            return Err(invalid("invalid LZ4 block lengths"));
        }
        buffer[..21].copy_from_slice(header);
        bytes = &bytes[21..];
        if raw == 0 {
            if expected != 0 {
                return Err(invalid("invalid LZ4 terminator checksum"));
            }
            return Ok(());
        }
        if raw > max_output.saturating_sub(output.len()) {
            return Err(invalid("decompression limit exceeded"));
        }
        let block = bytes
            .get(..compressed)
            .ok_or_else(|| invalid("truncated LZ4 payload"))?;
        let start = output.len();
        let decoded = if method == 0x10 {
            output.extend_from_slice(block);
            Ok(())
        } else {
            if buffer.len() < compressed {
                buffer = vec![0; compressed.max(buffer.len() * 3 / 2)];
            }
            buffer[..compressed].copy_from_slice(block);
            output.resize(start + raw, 0);
            // `refill`: the block must be read to its last byte, no further.
            if decompress_lz4_java(&buffer, &mut output[start..]) == Some(compressed) {
                Ok(())
            } else {
                Err(invalid("LZ4 stream is corrupted"))
            }
        }
        .and_then(|()| {
            if checksum(&output[start..]) == expected {
                Ok(())
            } else {
                Err(invalid("LZ4 checksum mismatch"))
            }
        });
        // lz4-java hands out a block only once it has decoded and checked it.
        if let Err(e) = decoded {
            output.truncate(start);
            return Err(e);
        }
        bytes = &bytes[compressed..];
    }
}

/// lz4-java's `LZ4JavaSafeFastDecompressor.decompress`, which
/// `LZ4BlockInputStream` uses: `dest` filled from `src`, returning how many
/// bytes of `src` it read, or `None` where it throws `LZ4Exception`. Its end
/// conditions are its own (a sequence whose literals leave less than 8 bytes of
/// output, or of input past them, must be the last and end the output), and
/// an offset of 0 fills the match with zeros.
fn decompress_lz4_java(src: &[u8], dest: &mut [u8]) -> Option<usize> {
    let (src_end, dest_end) = (src.len(), dest.len());
    if dest_end == 0 {
        return (src.first() == Some(&0)).then_some(1);
    }
    // The run-length bytes after a 15 in the token; a run cut off by the end of
    // the input adds a last 255, as the Java loop does.
    let length = |s: &mut usize, mut len: usize| {
        let mut last = 0xff;
        while *s < src_end {
            last = src[*s];
            *s += 1;
            if last != 0xff {
                break;
            }
            len += 255;
        }
        len + usize::from(last)
    };
    let (mut s, mut d) = (0, 0);
    loop {
        if s >= src_end {
            return None;
        }
        let token = src[s];
        s += 1;
        let mut literals = usize::from(token >> 4);
        if literals == 15 {
            literals = length(&mut s, literals);
        }
        let literal_end = d + literals;
        if dest_end < literal_end + 8 || src_end - s < 8 + literals {
            if literal_end != dest_end || src_end - s < literals {
                return None;
            }
            dest[d..literal_end].copy_from_slice(&src[s..s + literals]);
            return Some(s + literals);
        }
        dest[d..literal_end].copy_from_slice(&src[s..s + literals]);
        s += literals;
        d = literal_end;
        let offset = usize::from(u16::from_le_bytes([src[s], src[s + 1]]));
        s += 2;
        if offset > d {
            return None;
        }
        let mut matched = usize::from(token & 15);
        if matched == 15 {
            matched = length(&mut s, matched);
        }
        let match_end = d + matched + 4;
        if match_end > dest_end {
            return None;
        }
        if offset == 0 {
            dest[d..match_end].fill(0);
        } else {
            for i in d..match_end {
                dest[i] = dest[i - offset];
            }
        }
        d = match_end;
    }
}

fn encode_lz4(bytes: &[u8]) -> Vec<u8> {
    let mut output = Vec::new();
    for block in bytes.chunks(65536) {
        let compressed = lz4_flex::block::compress(block);
        let (token, payload) = if compressed.len() < block.len() {
            (0x26, compressed.as_slice())
        } else {
            (0x16, block)
        };
        output.extend_from_slice(MAGIC);
        output.push(token);
        output.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        output.extend_from_slice(&(block.len() as u32).to_le_bytes());
        output.extend_from_slice(&checksum(block).to_le_bytes());
        output.extend_from_slice(payload);
    }
    output.extend_from_slice(MAGIC);
    output.push(0x16);
    output.extend_from_slice(&[0; 12]);
    output
}

#[cfg(test)]
mod tests {
    use super::*;
    /// lz4-java's `LZ4BlockInputStream` decides which blocks are corrupted
    /// (#5064). Each stream below has a correct checksum; the results are
    /// lz4-java 1.10.1's.
    #[test]
    fn lz4_blocks_are_read_as_lz4_java_reads_them() {
        let hex = |s: &str| -> Vec<u8> {
            (0..s.len())
                .step_by(2)
                .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
                .collect()
        };
        let lz4 = |s: &str| Compression::Lz4.decode(&hex(s), usize::MAX);
        const END: &str = "4c5a34426c6f636b16000000000000000000000000";
        // The issue's stream: its last match ends 2 bytes before the output's end.
        assert!(lz4(&format!(
            "4c5a34426c6f636b2620000000270000002817d90bb112cd89673ffd3d11ad97420600708664345bbd2cb70b00248fc80b0020a52b{END}"
        ))
        .is_err());
        // A literal with less than 8 bytes of input after it must end the output.
        assert!(
            lz4(&format!(
                "4c5a34426c6f636b260500000005000000f5a1570e100301000c{END}"
            ))
            .is_err()
        );
        // An offset of 0 fills the match with zeros.
        assert_eq!(
            lz4(&format!(
                "4c5a34426c6f636b260400000010000000eace86020c000007{END}"
            ))
            .unwrap(),
            [0; 16]
        );
    }
    #[test]
    fn all_codecs_roundtrip_and_enforce_limits() {
        for codec in [
            Compression::Gzip,
            Compression::Zlib,
            Compression::None,
            Compression::Lz4,
        ] {
            for bytes in [
                vec![],
                vec![0; 131073],
                (0..262144).map(|i| (i * 37) as u8).collect(),
            ] {
                let encoded = codec.encode(&bytes).unwrap();
                assert_eq!(codec.decode(&encoded, bytes.len()).unwrap(), bytes);
                if !bytes.is_empty() {
                    assert!(codec.decode(&encoded, bytes.len() - 1).is_err());
                }
                if codec != Compression::None {
                    assert!(
                        codec
                            .decode(&encoded[..encoded.len() - 1], bytes.len())
                            .is_err()
                    );
                }
            }
        }
    }
    /// The zlib decoder before it inflated in place: a new inflater for
    /// each stream, and 8 KiB blocks copied out.
    fn blockwise(bytes: &[u8], max_output: usize, output: &mut Vec<u8>) -> io::Result<()> {
        zlib_blocks(&mut Decompress::new(true), bytes, max_output, output)
    }

    /// Inflating in place keeps what the block decoder kept and fails as it
    /// failed: whole streams, every limit around them, streams cut short and
    /// damaged ones, decoded after bytes already in the output.
    #[test]
    fn zlib_in_place_matches_the_block_decoder() {
        let mut seed = 0x2545_f491_4f6c_dd1du64;
        let mut random = move || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed
        };
        let mut cases = 0;
        for n in [0usize, 1, 8191, 8192, 8193, 20_000, 100_000] {
            let raw: Vec<u8> = (0..n)
                .map(|i| {
                    if i % 700 < 400 {
                        7
                    } else {
                        random() as u8 & 31
                    }
                })
                .collect();
            let encoded = Compression::Zlib.encode(&raw).unwrap();
            let mut streams = vec![encoded.clone()];
            for cut in [1, 2, encoded.len() / 2, encoded.len() - 1] {
                streams.push(encoded[..encoded.len().saturating_sub(cut)].to_vec());
            }
            for at in [
                2,
                3,
                encoded.len() / 3,
                encoded.len() / 2,
                encoded.len() - 5,
            ] {
                let mut damaged = encoded.clone();
                if let Some(b) = damaged.get_mut(at) {
                    *b ^= 0x5a;
                }
                streams.push(damaged);
            }
            for stream in &streams {
                for limit in [
                    0,
                    1,
                    8191,
                    8192,
                    8193,
                    n / 2,
                    n.saturating_sub(1),
                    n,
                    n + 1,
                    n + 9000,
                ] {
                    for before in [0usize, 5] {
                        let mut old = vec![9; before];
                        let mut new = old.clone();
                        let a = blockwise(stream, limit + before, &mut old);
                        let b = Compression::Zlib.decode_into(stream, limit + before, &mut new);
                        assert_eq!(
                            a.map_err(|e| e.to_string()),
                            b.map_err(|e| e.to_string()),
                            "{n} bytes, stream of {}, limit {limit}",
                            stream.len()
                        );
                        assert_eq!(
                            old,
                            new,
                            "{n} bytes, stream of {}, limit {limit}",
                            stream.len()
                        );
                        cases += 1;
                    }
                }
            }
        }
        assert!(cases > 1000);
        // No limit at all.
        let encoded = Compression::Zlib.encode(b"Horizon").unwrap();
        assert_eq!(
            Compression::Zlib.decode(&encoded, usize::MAX).unwrap(),
            b"Horizon"
        );
    }

    /// `RegionFileVersion.configure` and `getSelected`: the names
    /// server.properties' `region-file-compression` takes, the default before
    /// any is set, and an unknown name leaving the selection alone.
    #[test]
    fn the_selected_region_version_follows_the_property() {
        assert_eq!(
            Compression::selected(),
            Compression::Zlib,
            "VERSION_DEFLATE is DEFAULT"
        );
        for (name, want) in [
            ("gzip", Compression::Gzip),
            ("deflate", Compression::Zlib),
            ("none", Compression::None),
            ("lz4", Compression::Lz4),
        ] {
            assert!(Compression::configure(name), "{name}");
            assert_eq!(Compression::selected(), want, "{name}");
        }
        // Vanilla logs the value and keeps the version it had.
        assert!(!Compression::configure("zstd"));
        assert!(!Compression::configure("LZ4"), "the names are exact");
        assert_eq!(Compression::selected(), Compression::Lz4);
        assert!(Compression::configure("deflate"));
        assert_eq!(Compression::selected(), Compression::Zlib);
    }
    #[test]
    fn checksums_reject_corruption() {
        for codec in [Compression::Gzip, Compression::Zlib, Compression::Lz4] {
            let mut encoded = codec.encode(b"Horizon world data").unwrap();
            let i = if codec == Compression::Lz4 {
                17
            } else {
                encoded.len() - 5
            };
            encoded[i] ^= 1;
            assert!(codec.decode(&encoded, 1000).is_err());
        }
    }
}
