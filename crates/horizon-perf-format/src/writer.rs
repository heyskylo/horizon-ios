use std::io::{self, Write};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender, TrySendError};
use std::thread::{self, JoinHandle};

use flate2::Compression;
use flate2::write::GzEncoder;
use prost::Message;

use crate::v1::{self, Capture, Side};

/// `Capture.format` this crate writes: it changes only if the live-follow rules change.
pub const FORMAT: u32 = 1;
/// Messages the writer thread's channel holds before producers start dropping.
pub const CHANNEL_CAPACITY: usize = 4096;
pub const WRITER_THREAD_NAME: &str = "horizon-perf-writer";

/// One element of one repeated top-level field of `Capture`: the unit a writer appends.
#[derive(Clone, Debug, PartialEq)]
pub enum Field {
    Start(v1::Start),
    String(v1::StringEntry),
    History(v1::HealthWindow),
    SlowHistory(v1::SlowEvent),
    World(v1::WorldSnapshot),
    Mark(v1::Mark),
    Warning(v1::Warning),
    Health(v1::HealthSample),
    CounterDef(v1::CounterDef),
    Counters(v1::CounterSample),
    Slow(v1::SlowEvent),
    SpanBlock(v1::SpanBlock),
    Memory(v1::MemorySnapshot),
    Timeline(v1::TimelineChunk),
    Spans(v1::SpanSummary),
    Workers(v1::Workers),
    Samples(v1::SampleProfile),
    Net(v1::NetTraffic),
    Gpu(v1::GpuPasses),
    GpuFrames(v1::GpuFrames),
    Bench(v1::BenchResult),
    Stop(v1::Stop),
}

impl Field {
    pub fn side(&self) -> Side {
        match self {
            Field::Start(m) => m.side(),
            Field::String(m) => m.side(),
            Field::History(m) => m.side(),
            Field::SlowHistory(m) | Field::Slow(m) => m.side(),
            Field::World(m) => m.side(),
            Field::Mark(m) => m.side(),
            Field::Warning(m) => m.side(),
            Field::Health(m) => m.side(),
            Field::CounterDef(m) => m.side(),
            Field::Counters(m) => m.side(),
            Field::SpanBlock(m) => m.side(),
            Field::Memory(m) => m.side(),
            Field::Timeline(m) => m.side(),
            Field::Spans(m) => m.side(),
            Field::Workers(m) => m.side(),
            Field::Samples(m) => m.side(),
            Field::Net(m) => m.side(),
            Field::Gpu(m) => m.side(),
            Field::GpuFrames(m) => m.side(),
            Field::Bench(m) => m.side(),
            Field::Stop(m) => m.side(),
        }
    }

    fn into_capture(self) -> Capture {
        let mut c = Capture::default();
        match self {
            Field::Start(m) => c.start.push(m),
            Field::String(m) => c.strings.push(m),
            Field::History(m) => c.history.push(m),
            Field::SlowHistory(m) => c.slow_history.push(m),
            Field::World(m) => c.world.push(m),
            Field::Mark(m) => c.marks.push(m),
            Field::Warning(m) => c.warnings.push(m),
            Field::Health(m) => c.health.push(m),
            Field::CounterDef(m) => c.counter_defs.push(m),
            Field::Counters(m) => c.counters.push(m),
            Field::Slow(m) => c.slow.push(m),
            Field::SpanBlock(m) => c.span_blocks.push(m),
            Field::Memory(m) => c.memory.push(m),
            Field::Timeline(m) => c.timeline.push(m),
            Field::Spans(m) => c.spans.push(m),
            Field::Workers(m) => c.workers.push(m),
            Field::Samples(m) => c.samples.push(m),
            Field::Net(m) => c.net.push(m),
            Field::Gpu(m) => c.gpu.push(m),
            Field::GpuFrames(m) => c.gpu_frames.push(m),
            Field::Bench(m) => c.bench.push(m),
            Field::Stop(m) => c.stop.push(m),
        }
        c
    }
}

/// Writes one capture as a single gzip stream, synchronously, with a sync flush after every field.
pub struct CaptureWriter<W: Write> {
    gz: GzEncoder<W>,
    buf: Vec<u8>,
}

impl<W: Write> CaptureWriter<W> {
    /// Starts the stream with `format`, flushed, so even an empty capture inflates to a whole field.
    pub fn new(out: W) -> io::Result<Self> {
        let mut writer = Self {
            gz: GzEncoder::new(out, Compression::default()),
            buf: Vec::new(),
        };
        writer.append(&Capture {
            format: FORMAT,
            ..Capture::default()
        })?;
        Ok(writer)
    }

    /// Appends one whole top-level field, then sync-flushes it through to `out`.
    pub fn write(&mut self, field: Field) -> io::Result<()> {
        self.append(&field.into_capture())
    }

    fn append(&mut self, single: &Capture) -> io::Result<()> {
        self.buf.clear();
        single.encode(&mut self.buf).map_err(io::Error::other)?;
        self.gz.write_all(&self.buf)?;
        self.gz.flush()
    }

    /// Appends bytes that are whole top-level fields already (another side's stream), then sync-flushes.
    pub fn write_raw(&mut self, fields: &[u8]) -> io::Result<()> {
        self.gz.write_all(fields)?;
        self.gz.flush()
    }

    pub fn get_ref(&self) -> &W {
        self.gz.get_ref()
    }

    /// Ends the deflate stream and writes the gzip trailer.
    pub fn finish(self) -> io::Result<W> {
        self.gz.finish()
    }
}

#[derive(Default)]
struct Dropped([AtomicU64; 3]);

impl Dropped {
    fn slot(&self, side: Side) -> &AtomicU64 {
        &self.0[(side as usize).min(2)]
    }
}

enum Item {
    Field(Field),
    Raw(Vec<u8>),
}

/// What became of bytes offered to [`FieldSender::try_send_raw`].
#[derive(Debug, PartialEq, Eq)]
pub enum RawSend {
    Sent,
    /// The channel had no room; the bytes come back to try again.
    Full(Vec<u8>),
    /// The writer is gone.
    Closed,
}

/// The producers' end of a writer thread's bounded channel.
#[derive(Clone)]
pub struct FieldSender {
    tx: SyncSender<Item>,
    dropped: Arc<Dropped>,
}

impl FieldSender {
    /// Queues `field` without waiting: a full channel drops it and counts it for its side; a `Stop` alone waits for room.
    pub fn send(&self, field: Field) -> bool {
        if let Field::Stop(_) = field {
            return self.tx.send(Item::Field(field)).is_ok();
        }
        match self.tx.try_send(Item::Field(field)) {
            Ok(()) => true,
            Err(TrySendError::Full(Item::Field(field))) => {
                self.dropped
                    .slot(field.side())
                    .fetch_add(1, Ordering::Relaxed);
                false
            }
            Err(TrySendError::Full(Item::Raw(_)) | TrySendError::Disconnected(_)) => false,
        }
    }

    /// Queues bytes that are whole top-level fields, never dropping them: a full channel hands them back.
    pub fn try_send_raw(&self, fields: Vec<u8>) -> RawSend {
        match self.tx.try_send(Item::Raw(fields)) {
            Ok(()) => RawSend::Sent,
            Err(TrySendError::Full(Item::Raw(fields))) => RawSend::Full(fields),
            Err(_) => RawSend::Closed,
        }
    }

    /// Fields of `side` dropped so far because the channel was full.
    pub fn dropped(&self, side: Side) -> u64 {
        self.dropped.slot(side).load(Ordering::Relaxed)
    }
}

/// A `CaptureWriter` on its own thread, fed through a channel of `CHANNEL_CAPACITY` fields.
pub struct WriterThread<W> {
    sender: FieldSender,
    handle: JoinHandle<io::Result<W>>,
}

impl<W: Write + Send + 'static> WriterThread<W> {
    pub fn spawn(out: W) -> io::Result<Self> {
        Self::spawn_with(out, || {})
    }

    /// [`WriterThread::spawn`], running `on_start` on the writer thread first.
    pub fn spawn_with(out: W, on_start: impl FnOnce() + Send + 'static) -> io::Result<Self> {
        let (tx, rx) = mpsc::sync_channel::<Item>(CHANNEL_CAPACITY);
        let dropped = Arc::new(Dropped::default());
        let counts = Arc::clone(&dropped);
        let handle = thread::Builder::new()
            .name(WRITER_THREAD_NAME.into())
            .spawn(move || {
                on_start();
                run(out, rx, &counts)
            })?;
        Ok(Self {
            sender: FieldSender { tx, dropped },
            handle,
        })
    }

    pub fn sender(&self) -> FieldSender {
        self.sender.clone()
    }

    /// Writes what is queued and closes the stream; waits until every other `FieldSender` is dropped.
    pub fn finish(self) -> io::Result<W> {
        drop(self.sender);
        self.handle
            .join()
            .map_err(|_| io::Error::other("the capture writer thread panicked"))?
    }
}

fn run<W: Write>(out: W, rx: Receiver<Item>, dropped: &Dropped) -> io::Result<W> {
    let mut writer = CaptureWriter::new(out)?;
    for item in rx {
        let field = match item {
            Item::Raw(fields) => {
                writer.write_raw(&fields)?;
                continue;
            }
            Item::Field(field) => field,
        };
        let field = match field {
            Field::Stop(mut stop) => {
                let count = dropped.slot(stop.side()).load(Ordering::Relaxed);
                stop.overhead
                    .get_or_insert_with(Default::default)
                    .dropped_messages += count;
                Field::Stop(stop)
            }
            field => field,
        };
        writer.write(field)?;
    }
    writer.finish()
}
