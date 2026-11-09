//! The dev-only reader for `.hzperf` captures (docs/perf-platform.md §2.2).

pub mod attach;
pub mod bare;
pub mod bench_diff;
pub mod cli;
pub mod diff;
mod diff_calls;
pub mod error;
pub mod export;
pub mod follow;
mod health;
pub mod ids;
mod inflate;
pub mod mcp;
pub mod names;
pub mod ops;
pub mod query;
pub mod read;
pub mod request;
pub mod samples;
#[cfg(not(target_arch = "wasm32"))]
pub mod sharing;
pub mod stats;
pub mod summary;
pub mod view;
#[cfg(feature = "wasm")]
pub mod wasm;
pub mod watch;
pub mod wire;

pub use error::Error;
pub use follow::{Entry, Follower};
pub use read::read_capture;
pub use summary::{Baseline, DEFAULT_BUDGET, SummaryOptions, summarize};
