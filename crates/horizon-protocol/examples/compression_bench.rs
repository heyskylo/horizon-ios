//! Replay a horizon-bots --capture as 50 identical connections. This measures framing and
//! compression on one thread, not actual server throughput or network latency.
use flate2::{Compress, Compression, FlushCompress, Status};
use horizon_protocol::buf::write_varint;
use std::{error::Error, time::Instant};

fn main() -> Result<(), Box<dyn Error>> {
    let path = std::env::args()
        .nth(1)
        .ok_or("usage: compression_bench CAPTURE")?;
    let data = std::fs::read(path)?;
    let mut packets = Vec::new();
    let mut at = 0;
    while at + 12 <= data.len() {
        let id = i32::from_le_bytes(data[at + 4..at + 8].try_into()?);
        let len = u32::from_le_bytes(data[at + 8..at + 12].try_into()?) as usize;
        let Some(payload) = data.get(at + 12..at + 12 + len) else {
            break;
        };
        let mut body = Vec::new();
        write_varint(&mut body, id);
        body.extend_from_slice(payload);
        packets.push(body);
        at += 12 + len;
    }
    if packets.is_empty() {
        return Err("empty capture".into());
    }
    for level in [6, 2] {
        let start = Instant::now();
        let mut bytes = 0usize;
        let mut z = Compress::new(Compression::new(level), true);
        for _ in 0..50 {
            for body in &packets {
                let mut out = Vec::with_capacity(body.len() + (body.len() >> 10) + 72);
                if body.len() < 256 {
                    out.push(0);
                    out.extend_from_slice(body);
                } else {
                    write_varint(&mut out, body.len() as i32);
                    z.reset();
                    let begin = z.total_in();
                    let mut flush = FlushCompress::None;
                    loop {
                        let offset = (z.total_in() - begin) as usize;
                        if offset == body.len() {
                            flush = FlushCompress::Finish;
                        }
                        if out.capacity() - out.len() < 64 {
                            out.reserve(32 << 10);
                        }
                        if z.compress_vec(&body[offset..], &mut out, flush)? == Status::StreamEnd {
                            break;
                        }
                    }
                }
                let mut prefix = Vec::new();
                write_varint(&mut prefix, out.len() as i32);
                bytes += prefix.len() + out.len();
            }
        }
        println!(
            "level={level} packets={} connections=50 bytes={bytes} seconds={:.3}",
            packets.len(),
            start.elapsed().as_secs_f64()
        );
    }
    Ok(())
}
