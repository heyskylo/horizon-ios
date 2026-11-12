//! Packet framing (Varint21FrameDecoder / Varint21LengthFieldPrepender) and
//! zlib compression above a threshold (CompressionEncoder / CompressionDecoder).
use crate::{
    Error, Result,
    buf::{Reader, write_varint},
};
use flate2::{Compress, Compression, Decompress, FlushCompress, FlushDecompress, Status};
use std::cell::RefCell;

/// Frames are at most three VarInt bytes long (21 bits).
pub const MAX_FRAME: usize = (1 << 21) - 1;
/// CompressionDecoder.MAXIMUM_UNCOMPRESSED_LENGTH.
pub const MAX_UNCOMPRESSED: usize = 8_388_608;

/// Build the frame for one packet body (`id` + payload already concatenated):
/// length prefix, and with compression enabled the uncompressed-length
/// field (0 when below the threshold).
pub fn encode(body: &[u8], threshold: Option<usize>) -> Result<Vec<u8>> {
    Ok(prefixed(&contents(body, threshold)?))
}

/// What [`encode`] puts after the length prefix: the same for every
/// connection with the same threshold.
pub fn contents(body: &[u8], threshold: Option<usize>) -> Result<Vec<u8>> {
    let inner = match threshold {
        None => body.to_vec(),
        Some(t) if body.len() < t => {
            let mut v = Vec::with_capacity(body.len() + 1);
            v.push(0);
            v.extend_from_slice(body);
            v
        }
        Some(_) => {
            // The data length, then the stream: zlib's bound for it is a
            // little over the body's length.
            let mut v = Vec::with_capacity(body.len() + (body.len() >> 10) + 72);
            write_varint(&mut v, body.len() as i32);
            deflate_into(body, &mut v)?;
            v
        }
    };
    if inner.len() > MAX_FRAME {
        return Err(Error::new(format!(
            "packet too large: {} bytes",
            inner.len()
        )));
    }
    Ok(inner)
}

thread_local! {
    /// This thread's zlib compressor, reset for each packet. A new one per
    /// packet allocated and cleared zlib's window and hash tables (some
    /// 300 KiB) every time, which cost more than compressing a small packet.
    static DEFLATER: RefCell<Option<Compress>> = const { RefCell::new(None) };
}

/// `body` as one zlib stream at level 2, appended to `out`: the
/// bytes a new `ZlibEncoder` writes for it, from a reset compressor.
fn deflate_into(body: &[u8], out: &mut Vec<u8>) -> Result<()> {
    DEFLATER.with_borrow_mut(|deflater| {
        let z = deflater.get_or_insert_with(|| Compress::new(Compression::new(2), true));
        z.reset();
        let start = z.total_in();
        // ZlibEncoder's calls: the whole input without a flush, then finish.
        let mut flush = FlushCompress::None;
        loop {
            let at = (z.total_in() - start) as usize;
            if at == body.len() {
                flush = FlushCompress::Finish;
            }
            if out.capacity() - out.len() < 64 {
                out.reserve(32 << 10);
            }
            let status = z
                .compress_vec(&body[at..], out, flush)
                .map_err(|e| Error::new(format!("compression failed: {e}")))?;
            if status == Status::StreamEnd {
                return Ok(());
            }
        }
    })
}

/// A frame: the length prefix, then the [`contents`].
pub fn prefixed(contents: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(contents.len() + 3);
    write_varint(&mut out, contents.len() as i32);
    out.extend_from_slice(contents);
    out
}

/// The length prefix at the front of `bytes`, once it is complete: its own
/// size and the frame's length.
pub fn header(bytes: &[u8]) -> Result<Option<(usize, usize)>> {
    let mut len = 0usize;
    for i in 0..3 {
        let Some(&b) = bytes.get(i) else {
            return Ok(None);
        };
        len |= usize::from(b & 0x7F) << (7 * i);
        if b & 0x80 == 0 {
            if len == 0 {
                // Varint21FrameDecoder: a frame with no bytes is a
                // corrupted frame, not an empty packet.
                return Err(Error::new("Frame length cannot be zero"));
            }
            return Ok(Some((i + 1, len)));
        }
    }
    Err(Error::new("length wider than 21 bits"))
}

/// Inflates compressed frames, keeping its state from one to the next
/// (`CompressionDecoder`'s one `Inflater`).
pub struct Inflater(Decompress);
impl Default for Inflater {
    fn default() -> Self {
        Self(Decompress::new(true))
    }
}
impl Inflater {
    /// The zlib stream at the front of `input`, which must hold exactly `size` bytes.
    fn inflate(&mut self, input: &[u8], size: usize) -> Result<Vec<u8>> {
        self.0.reset(true);
        // One byte more than declared: a longer stream fills it. Reserved as the output grows, not
        // all at once: a few bytes declaring 8 MiB would otherwise allocate 8 MiB per frame.
        // DEFLATE expands at most about 1032:1.
        let limit = size + 1;
        let mut out =
            Vec::with_capacity(limit.min(input.len().saturating_mul(1032).saturating_add(64)));
        let status = loop {
            let consumed = usize::try_from(self.0.total_in()).unwrap_or(input.len());
            let status = self
                .0
                .decompress_vec(
                    input.get(consumed..).unwrap_or_default(),
                    &mut out,
                    FlushDecompress::Finish,
                )
                .map_err(|e| Error::new(format!("Badly compressed packet - {e}")))?;
            // Done, past the limit, or stopped with room left (the input ran out).
            if matches!(status, Status::StreamEnd)
                || out.len() >= limit
                || out.len() < out.capacity()
            {
                break status;
            }
            let more = out.len().max(4096).min(limit - out.len());
            out.reserve_exact(more);
        };
        if out.len() != size || !matches!(status, Status::StreamEnd) {
            return Err(Error::new(format!(
                "Badly compressed packet - actual length {} does not match declared size {size}",
                out.len()
            )));
        }
        Ok(out)
    }
}

/// A frame's packet body: where it starts in the frame, or inflated.
pub enum Body {
    Plain(usize),
    Inflated(Vec<u8>),
}

/// `CompressionDecoder`: the body of a frame, inflated when its data length says so.
pub fn body(frame: &[u8], threshold: Option<usize>, inflater: &mut Inflater) -> Result<Body> {
    let Some(threshold) = threshold else {
        return Ok(Body::Plain(0));
    };
    let mut r = Reader::new(frame);
    let size = r.varint()?;
    if size == 0 {
        return Ok(Body::Plain(r.position()));
    }
    let size = usize::try_from(size).map_err(|_| Error::new("negative uncompressed length"))?;
    if size < threshold {
        return Err(Error::new(format!(
            "Badly compressed packet - size of {size} is below server threshold of {threshold}"
        )));
    }
    if size > MAX_UNCOMPRESSED {
        return Err(Error::new(format!(
            "Badly compressed packet - size of {size} is larger than protocol maximum"
        )));
    }
    inflater
        .inflate(&frame[r.position()..], size)
        .map(Body::Inflated)
}

#[cfg(test)]
mod tests {
    use super::*;
    use flate2::write::ZlibEncoder;
    use std::io::Write;

    /// What a new encoder for every packet wrote, before the compressor was kept.
    fn fresh_contents(body: &[u8]) -> Vec<u8> {
        let mut v = Vec::new();
        write_varint(&mut v, body.len() as i32);
        let mut z = ZlibEncoder::new(v, Compression::new(2));
        z.write_all(body).unwrap();
        z.finish().unwrap()
    }

    /// The kept compressor writes the bytes a new one does, whatever it
    /// compressed before: small, large (several output blocks), repetitive
    /// and random bodies, in an order that puts each size after the others.
    #[test]
    fn a_reused_compressor_writes_the_same_bytes() {
        let mut seed = 0x9e37_79b9_7f4a_7c15u64;
        let mut random = move || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed
        };
        let mut bodies: Vec<Vec<u8>> = Vec::new();
        for n in [256usize, 257, 300, 1000, 4096, 40_000, 70_000, 300_000] {
            bodies.push((0..n).map(|i| (i * 7 % 13) as u8).collect());
            bodies.push((0..n).map(|_| random() as u8).collect());
            // Runs and noise, like a chunk's sections and light.
            bodies.push(
                (0..n)
                    .map(|i| {
                        if i % 512 < 300 {
                            0
                        } else {
                            random() as u8 & 15
                        }
                    })
                    .collect(),
            );
        }
        for round in 0..2 {
            for body in &bodies {
                assert_eq!(
                    contents(body, Some(256)).unwrap(),
                    fresh_contents(body),
                    "{} bytes, round {round}",
                    body.len()
                );
            }
            bodies.reverse();
        }
    }

    #[test]
    fn large_bodies_inflate_whole_and_short_streams_are_refused() {
        // 2 MiB of zeros deflate to a few KiB: inflated whole as the buffer grows.
        let body = vec![0u8; 2 << 20];
        let frame = contents(&body, Some(256)).unwrap();
        let mut inflater = Inflater::default();
        let Body::Inflated(out) = super::body(&frame, Some(256), &mut inflater).unwrap() else {
            panic!("compressed");
        };
        assert!(out == body);
        // A few bytes declaring 8 MiB: refused (and no longer 8 MiB reserved for them).
        let mut lying = Vec::new();
        write_varint(&mut lying, MAX_UNCOMPRESSED as i32);
        lying.extend_from_slice(&frame[3..20]);
        assert!(super::body(&lying, Some(256), &mut inflater).is_err());
        // The same inflater still reads a good frame after.
        assert!(matches!(
            super::body(&frame, Some(256), &mut inflater).unwrap(),
            Body::Inflated(_)
        ));
    }

    /// Split one complete frame off the front of `buffer`, if available.
    fn split(buffer: &mut Vec<u8>) -> Result<Option<Vec<u8>>> {
        let Some((start, len)) = header(buffer)? else {
            return Ok(None);
        };
        if buffer.len() < start + len {
            return Ok(None);
        }
        let frame = buffer[start..start + len].to_vec();
        buffer.drain(..start + len);
        Ok(Some(frame))
    }

    /// The packet body (id + payload) of a frame.
    fn decompress(frame: Vec<u8>, threshold: Option<usize>) -> Result<Vec<u8>> {
        Ok(match body(&frame, threshold, &mut Inflater::default())? {
            Body::Plain(at) => frame[at..].to_vec(),
            Body::Inflated(body) => body,
        })
    }

    #[test]
    fn uncompressed_frames_round_trip() {
        let body = vec![0x00, 1, 2, 3];
        let framed = encode(&body, None).unwrap();
        assert_eq!(framed, [4, 0, 1, 2, 3]);
        let mut buffer = framed.clone();
        buffer.extend_from_slice(&framed[..2]);
        assert_eq!(split(&mut buffer).unwrap(), Some(body));
        assert_eq!(split(&mut buffer).unwrap(), None);
        assert_eq!(buffer, [4, 0]);
    }

    #[test]
    fn compression_threshold_like_vanilla() {
        // Below the threshold: a zero data length, then the raw body.
        let small = vec![7u8; 10];
        let framed = encode(&small, Some(256)).unwrap();
        assert_eq!(&framed[..2], &[11, 0]);
        let mut buffer = framed;
        let frame = split(&mut buffer).unwrap().unwrap();
        assert_eq!(decompress(frame, Some(256)).unwrap(), small);
        // At or above: the uncompressed size and a zlib stream.
        let big: Vec<u8> = (0..5000u32).map(|i| (i % 7) as u8).collect();
        let framed = encode(&big, Some(256)).unwrap();
        assert!(framed.len() < big.len());
        let mut buffer = framed;
        let frame = split(&mut buffer).unwrap().unwrap();
        assert_eq!(Reader::new(&frame).varint().unwrap(), 5000);
        assert_eq!(decompress(frame, Some(256)).unwrap(), big);
    }

    #[test]
    fn rejects_bad_compressed_lengths() {
        // Declared size below the threshold is a protocol error.
        let mut frame = vec![10];
        let mut z = ZlibEncoder::new(Vec::new(), Compression::default());
        z.write_all(&[1; 10]).unwrap();
        frame.extend(z.finish().unwrap());
        assert!(decompress(frame, Some(256)).is_err());
        // Declared size that does not match the stream.
        let mut frame = Vec::new();
        write_varint(&mut frame, 300);
        let mut z = ZlibEncoder::new(Vec::new(), Compression::default());
        z.write_all(&[1; 299]).unwrap();
        frame.extend(z.finish().unwrap());
        assert!(decompress(frame, Some(256)).is_err());
    }

    /// One inflater serves every frame, also after a bad one.
    #[test]
    fn the_inflater_is_reused() {
        let mut inflater = Inflater::default();
        let frame = |body: &[u8], declared: usize| {
            let mut frame = Vec::new();
            write_varint(&mut frame, declared as i32);
            let mut z = ZlibEncoder::new(frame, Compression::default());
            z.write_all(body).unwrap();
            z.finish().unwrap()
        };
        for n in [300usize, 70_000, 256, 5000] {
            let body: Vec<u8> = (0..n).map(|i| (i * 7 % 13) as u8).collect();
            match body_of(&frame(&body, n), &mut inflater) {
                Ok(Body::Inflated(out)) => assert_eq!(out, body),
                _ => panic!("{n} bytes did not inflate"),
            }
            // A stream longer than declared, then one shorter.
            assert!(body_of(&frame(&body, n - 1), &mut inflater).is_err());
            assert!(body_of(&frame(&body, n + 1), &mut inflater).is_err());
        }
    }

    fn body_of(frame: &[u8], inflater: &mut Inflater) -> Result<Body> {
        body(frame, Some(256), inflater)
    }

    #[test]
    fn a_noncanonical_length_prefix_still_reads() {
        // The data-length VarInt is read, not measured: zero as `0x80 0x00`.
        assert_eq!(
            decompress(vec![0x80, 0x00, 7, 1, 2], Some(256)).unwrap(),
            [7, 1, 2]
        );
        // And a compressed packet whose size was written wide: 300.
        let body = vec![9; 300];
        let mut frame = vec![0xAC, 0x82, 0x00];
        let mut z = ZlibEncoder::new(Vec::new(), Compression::default());
        z.write_all(&body).unwrap();
        frame.extend(z.finish().unwrap());
        assert_eq!(decompress(frame, Some(256)).unwrap(), body);
    }
}
