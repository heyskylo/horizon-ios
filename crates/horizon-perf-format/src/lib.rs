//! The `.hzperf` capture contract (docs/perf-platform.md §3): the generated types, the gzip writer, the health recorder and the counter registry.

/// Types generated from `proto/horizon/perf/v1/capture.proto`.
#[allow(clippy::all)]
pub mod v1 {
    include!(concat!(env!("OUT_DIR"), "/horizon.perf.v1.rs"));
}

mod brief;
mod counters;
pub mod files;
mod health;
mod merge;
mod slow;
mod strings;
mod timing;
mod writer;

pub use brief::server_brief;
pub use counters::{
    Counter, CounterReport, CounterReporter, Gauge, Registry, counter, gauge,
    global as counter_registry,
};
pub use health::{Gauges, HealthRecorder, TIERS, Tps};
pub use merge::{MAX_PART_BYTES, MAX_PART_FIELD, MAX_PART_INFLATED, MergeError, PartMerger};
pub use prost::Message as ProtoMessage;
pub use slow::{SLOW_RING_CAPACITY, SLOW_TOP_SPANS, SlowRecord, SlowRing, SlowSpan};
pub use strings::Interner;
pub use timing::TimingRecorder;

pub use writer::{
    CHANNEL_CAPACITY, CaptureWriter, FORMAT, Field, FieldSender, RawSend, WRITER_THREAD_NAME,
    WriterThread,
};
