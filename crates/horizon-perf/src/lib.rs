//! The dev-only reader for `.hzperf` captures (docs/perf-platform.md §2.2).

pub mod cli;
pub mod error;
pub mod follow;
mod health;
mod inflate;
pub mod read;
pub mod summary;
pub mod watch;

pub use error::Error;
pub use follow::{Entry, Follower};
pub use read::read_capture;
pub use summary::{DEFAULT_BUDGET, SummaryOptions, summarize};
