use horizon_perf_format::v1::{Attachment, Capture, Side};
use horizon_perf_format::{FORMAT, Field};
use prost::Message;

use crate::error::Error;
use crate::inflate::GzipInflater;

/// One top-level field of a `Capture`, yielded once its last byte has arrived.
#[derive(Clone, Debug, PartialEq)]
pub enum Entry {
    Format(u32),
    Field(Field),
    /// Capture-wide, so it belongs to no side.
    Attachment(Attachment),
    /// A field number this reader does not know; it is skipped, as protobuf says.
    Other {
        number: u32,
    },
}

impl Entry {
    /// Merges this field into `capture` the way protobuf merges concatenated messages.
    pub fn apply(self, capture: &mut Capture) {
        match self {
            Entry::Format(n) => capture.format = n,
            Entry::Other { .. } => {}
            Entry::Attachment(m) => capture.attachments.push(m),
            Entry::Field(Field::Start(m)) => capture.start.push(m),
            Entry::Field(Field::String(m)) => capture.strings.push(m),
            Entry::Field(Field::History(m)) => capture.history.push(m),
            Entry::Field(Field::SlowHistory(m)) => capture.slow_history.push(m),
            Entry::Field(Field::World(m)) => capture.world.push(m),
            Entry::Field(Field::Mark(m)) => capture.marks.push(m),
            Entry::Field(Field::Warning(m)) => capture.warnings.push(m),
            Entry::Field(Field::Health(m)) => capture.health.push(m),
            Entry::Field(Field::CounterDef(m)) => capture.counter_defs.push(m),
            Entry::Field(Field::Counters(m)) => capture.counters.push(m),
            Entry::Field(Field::Slow(m)) => capture.slow.push(m),
            Entry::Field(Field::SpanBlock(m)) => capture.span_blocks.push(m),
            Entry::Field(Field::Memory(m)) => capture.memory.push(m),
            Entry::Field(Field::Timeline(m)) => capture.timeline.push(m),
            Entry::Field(Field::Spans(m)) => capture.spans.push(m),
            Entry::Field(Field::Workers(m)) => capture.workers.push(m),
            Entry::Field(Field::Samples(m)) => capture.samples.push(m),
            Entry::Field(Field::Net(m)) => capture.net.push(m),
            Entry::Field(Field::Gpu(m)) => capture.gpu.push(m),
            Entry::Field(Field::GpuFrames(m)) => capture.gpu_frames.push(m),
            Entry::Field(Field::Bench(m)) => capture.bench.push(m),
            Entry::Field(Field::Stop(m)) => capture.stop.push(m),
        }
    }
}

/// The follow reader: inflates a growing capture incrementally and yields each whole top-level field once.
#[derive(Default)]
pub struct Follower {
    gz: GzipInflater,
    pending: Vec<u8>,
    sides: Vec<(Side, bool)>,
}

impl Follower {
    pub fn new() -> Self {
        Self::default()
    }

    /// Feeds the next bytes of the file, in any split, and returns the fields that became whole.
    pub fn push(&mut self, bytes: &[u8]) -> Result<Vec<Entry>, Error> {
        self.gz.push(bytes, &mut self.pending)?;
        let mut entries = Vec::new();
        let mut at = 0;
        while let Some((number, len)) = next_field(&self.pending[at..])? {
            let entry = decode_entry(number, &self.pending[at..at + len])?;
            at += len;
            self.note(&entry)?;
            entries.push(entry);
        }
        self.pending.drain(..at);
        if self.gz.is_done() && !self.pending.is_empty() {
            return Err(Error::Corrupt("the capture ends inside a field".into()));
        }
        Ok(entries)
    }

    fn note(&mut self, entry: &Entry) -> Result<(), Error> {
        match entry {
            Entry::Format(found) if *found > FORMAT => Err(Error::FormatTooNew {
                found: *found,
                supported: FORMAT,
            }),
            Entry::Field(field) => {
                let side = field.side();
                let stopped = matches!(field, Field::Stop(_));
                match self.sides.iter_mut().find(|(s, _)| *s == side) {
                    Some(known) => known.1 |= stopped,
                    None => self.sides.push((side, stopped)),
                }
                Ok(())
            }
            _ => Ok(()),
        }
    }

    /// The sides that have written a field so far.
    pub fn sides(&self) -> impl Iterator<Item = Side> + '_ {
        self.sides.iter().map(|(side, _)| *side)
    }

    /// Sides seen without a `Stop` yet, the server first.
    pub fn missing_stops(&self) -> Vec<Side> {
        let mut missing: Vec<Side> = self
            .sides
            .iter()
            .filter(|(_, stopped)| !stopped)
            .map(|(side, _)| *side)
            .collect();
        missing.sort_by_key(|side| std::cmp::Reverse(*side as i32));
        missing
    }

    /// True when at least one side has written and every side that has written has stopped.
    pub fn is_finished(&self) -> bool {
        !self.sides.is_empty() && self.sides.iter().all(|(_, stopped)| *stopped)
    }

    /// True once the gzip trailer has arrived.
    pub fn stream_complete(&self) -> bool {
        self.gz.is_done()
    }
}

fn decode_entry(number: u32, bytes: &[u8]) -> Result<Entry, Error> {
    let mut c = Capture::decode(bytes).map_err(|e| Error::Corrupt(e.to_string()))?;
    if number == 1 {
        return Ok(Entry::Format(c.format));
    }
    let field = if let Some(m) = c.start.pop() {
        Field::Start(m)
    } else if let Some(m) = c.strings.pop() {
        Field::String(m)
    } else if let Some(m) = c.history.pop() {
        Field::History(m)
    } else if let Some(m) = c.slow_history.pop() {
        Field::SlowHistory(m)
    } else if let Some(m) = c.world.pop() {
        Field::World(m)
    } else if let Some(m) = c.marks.pop() {
        Field::Mark(m)
    } else if let Some(m) = c.warnings.pop() {
        Field::Warning(m)
    } else if let Some(m) = c.health.pop() {
        Field::Health(m)
    } else if let Some(m) = c.counter_defs.pop() {
        Field::CounterDef(m)
    } else if let Some(m) = c.counters.pop() {
        Field::Counters(m)
    } else if let Some(m) = c.slow.pop() {
        Field::Slow(m)
    } else if let Some(m) = c.span_blocks.pop() {
        Field::SpanBlock(m)
    } else if let Some(m) = c.memory.pop() {
        Field::Memory(m)
    } else if let Some(m) = c.timeline.pop() {
        Field::Timeline(m)
    } else if let Some(m) = c.spans.pop() {
        Field::Spans(m)
    } else if let Some(m) = c.workers.pop() {
        Field::Workers(m)
    } else if let Some(m) = c.samples.pop() {
        Field::Samples(m)
    } else if let Some(m) = c.net.pop() {
        Field::Net(m)
    } else if let Some(m) = c.gpu.pop() {
        Field::Gpu(m)
    } else if let Some(m) = c.gpu_frames.pop() {
        Field::GpuFrames(m)
    } else if let Some(m) = c.bench.pop() {
        Field::Bench(m)
    } else if let Some(m) = c.stop.pop() {
        Field::Stop(m)
    } else if let Some(m) = c.attachments.pop() {
        return Ok(Entry::Attachment(m));
    } else {
        return Ok(Entry::Other { number });
    };
    Ok(Entry::Field(field))
}

/// The field number and byte length of the whole top-level field at the start of `buf`, if it has all arrived.
fn next_field(buf: &[u8]) -> Result<Option<(u32, usize)>, Error> {
    let Some((key, key_len)) = varint(buf)? else {
        return Ok(None);
    };
    let number = key >> 3;
    if number == 0 || number > 0x1fff_ffff {
        return Err(Error::Corrupt(format!("invalid field number {number}")));
    }
    let rest = &buf[key_len..];
    let body = match key & 7 {
        0 => match varint(rest)? {
            Some((_, n)) => n,
            None => return Ok(None),
        },
        1 => 8,
        5 => 4,
        2 => match varint(rest)? {
            Some((len, n)) if len <= i32::MAX as u64 => n + len as usize,
            Some(_) => return Err(Error::Corrupt("a field is larger than 2 GiB".into())),
            None => return Ok(None),
        },
        wire => return Err(Error::Corrupt(format!("unsupported wire type {wire}"))),
    };
    let total = key_len + body;
    Ok((buf.len() >= total).then_some((number as u32, total)))
}

/// A base-128 varint at the start of `buf` and its length, or `None` while it is cut short.
fn varint(buf: &[u8]) -> Result<Option<(u64, usize)>, Error> {
    let mut value = 0u64;
    for (i, &byte) in buf.iter().enumerate().take(10) {
        value |= u64::from(byte & 0x7f) << (7 * i);
        if byte & 0x80 == 0 {
            return Ok(Some((value, i + 1)));
        }
    }
    if buf.len() >= 10 {
        return Err(Error::Corrupt("a varint is longer than 10 bytes".into()));
    }
    Ok(None)
}
