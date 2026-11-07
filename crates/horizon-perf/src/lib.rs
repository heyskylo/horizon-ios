//! The dev-only reader for `.hzperf` captures (docs/perf-platform.md §2.2).

pub mod bench_diff;
pub mod cli;
pub mod diff;
pub mod error;
pub mod follow;
mod health;
pub mod ids;
mod inflate;
pub mod query;
pub mod read;
pub mod stats;
pub mod summary;
pub mod view;
pub mod watch;

pub use error::Error;
pub use follow::{Entry, Follower};
pub use read::read_capture;
pub use summary::{Baseline, DEFAULT_BUDGET, SummaryOptions, summarize};
