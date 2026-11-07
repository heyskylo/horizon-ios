//! The `.hzperf` capture contract (docs/perf-platform.md §3): the generated types, the gzip writer, the health recorder and the counter registry.

/// Types generated from `proto/horizon/perf/v1/capture.proto`.
#[allow(clippy::all)]
pub mod v1 {
    include!(concat!(env!("OUT_DIR"), "/horizon.perf.v1.rs"));
}

mod counters;
mod health;
mod slow;
mod strings;
mod writer;

pub use counters::{
    Counter, CounterReport, CounterReporter, Gauge, Registry, counter, gauge,
    global as counter_registry,
};
pub use health::{Gauges, HealthRecorder, TIERS, Tps};
pub use slow::{SLOW_RING_CAPACITY, SLOW_TOP_SPANS, SlowRecord, SlowRing, SlowSpan};
pub use strings::Interner;

pub use writer::{
    CHANNEL_CAPACITY, CaptureWriter, FORMAT, Field, FieldSender, WRITER_THREAD_NAME, WriterThread,
};
