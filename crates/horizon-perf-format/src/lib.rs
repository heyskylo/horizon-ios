//! The `.hzperf` capture contract (docs/perf-platform.md §3): the generated types and the gzip writer.

/// Types generated from `proto/horizon/perf/v1/capture.proto`.
#[allow(clippy::all)]
pub mod v1 {
    include!(concat!(env!("OUT_DIR"), "/horizon.perf.v1.rs"));
}

mod writer;

pub use writer::{
    CHANNEL_CAPACITY, CaptureWriter, FORMAT, Field, FieldSender, WRITER_THREAD_NAME, WriterThread,
};
