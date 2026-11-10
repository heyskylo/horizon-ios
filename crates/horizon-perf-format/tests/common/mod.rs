#![allow(dead_code)]

use std::io::{self, Write};
use std::ops::Range;
use std::sync::{Arc, Mutex};

use flate2::{Decompress, FlushDecompress, Status};
use horizon_perf_format::Field;
use horizon_perf_format::v1::Capture;

/// Inflates as much of a possibly truncated gzip file as has arrived, without its trailer.
pub fn inflate_prefix(file: &[u8]) -> Vec<u8> {
    let Some(body) = gzip_body(file) else {
        return Vec::new();
    };
    let mut inflater = Decompress::new(false);
    let mut out = Vec::new();
    loop {
        out.reserve(64 * 1024);
        let (before_in, before_out) = (inflater.total_in(), inflater.total_out());
        let read = before_in as usize;
        let status = inflater
            .decompress_vec(&body[read..], &mut out, FlushDecompress::None)
            .expect("the deflate data is valid as far as it goes");
        let stalled = inflater.total_in() == before_in && inflater.total_out() == before_out;
        if status == Status::StreamEnd || stalled {
            return out;
        }
    }
}

/// The deflate data after an RFC 1952 header, or `None` while the header is still incomplete.
fn gzip_body(file: &[u8]) -> Option<&[u8]> {
    if file.len() < 10 {
        return None;
    }
    assert_eq!(
        &file[..3],
        &[0x1f, 0x8b, 8],
        "gzip magic and deflate method"
    );
    let flags = file[3];
    let mut at = 10;
    if flags & 0x04 != 0 {
        let len = u16::from_le_bytes([*file.get(at)?, *file.get(at + 1)?]) as usize;
        at += 2 + len;
    }
    for bit in [0x08, 0x10] {
        if flags & bit != 0 {
            at += file.get(at..)?.iter().position(|&b| b == 0)? + 1;
        }
    }
    if flags & 0x02 != 0 {
        at += 2;
    }
    file.get(at..)
}

/// Byte ranges of the complete top-level fields at the start of `bytes`; a partial tail is left out.
pub fn complete_fields(bytes: &[u8]) -> Vec<Range<usize>> {
    let mut fields = Vec::new();
    let mut at = 0;
    while at < bytes.len() {
        let mut rest = &bytes[at..];
        let Ok(key) = prost::encoding::decode_varint(&mut rest) else {
            break;
        };
        let end = match key & 7 {
            0 => match prost::encoding::decode_varint(&mut rest) {
                Ok(_) => bytes.len() - rest.len(),
                Err(_) => break,
            },
            2 => match prost::encoding::decode_varint(&mut rest) {
                Ok(len) if (len as usize) <= rest.len() => bytes.len() - rest.len() + len as usize,
                _ => break,
            },
            wire => panic!("a writer never writes wire type {wire}"),
        };
        fields.push(at..end);
        at = end;
    }
    fields
}

/// What decoding the concatenation of `fields` must give, by protobuf's merge rule: repeated fields append.
pub fn push(expected: &mut Capture, field: Field) {
    match field {
        Field::Start(m) => expected.start.push(m),
        Field::String(m) => expected.strings.push(m),
        Field::History(m) => expected.history.push(m),
        Field::SlowHistory(m) => expected.slow_history.push(m),
        Field::Mark(m) => expected.marks.push(m),
        Field::Warning(m) => expected.warnings.push(m),
        Field::Health(m) => expected.health.push(m),
        Field::CounterDef(m) => expected.counter_defs.push(m),
        Field::Counters(m) => expected.counters.push(m),
        Field::Slow(m) => expected.slow.push(m),
        Field::World(m) => expected.world.push(m),
        Field::SpanBlock(m) => expected.span_blocks.push(m),
        Field::Memory(m) => expected.memory.push(m),
        Field::Timeline(m) => expected.timeline.push(m),
        Field::Spans(m) => expected.spans.push(m),
        Field::Workers(m) => expected.workers.push(m),
        Field::Samples(m) => expected.samples.push(m),
        Field::Net(m) => expected.net.push(m),
        Field::Gpu(m) => expected.gpu.push(m),
        Field::GpuFrames(m) => expected.gpu_frames.push(m),
        Field::Bench(m) => expected.bench.push(m),
        Field::Stop(m) => expected.stop.push(m),
    }
}

/// A `Write` whose bytes stay readable through a clone while a writer owns it.
#[derive(Clone, Default)]
pub struct Shared(pub Arc<Mutex<Vec<u8>>>);

impl Shared {
    pub fn bytes(&self) -> Vec<u8> {
        self.0.lock().unwrap().clone()
    }
}

impl Write for Shared {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// xorshift64*: reproducible draws without a dependency.
pub struct Rng(pub u64);

impl Rng {
    pub fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }

    pub fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}
