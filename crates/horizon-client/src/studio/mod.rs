//! Horizon In-Game Video Studio.
//!
//! Provides native session packet recording (`.mcpr` format).

pub mod mcpr;
pub mod recorder;

pub use mcpr::{McprReader, McprWriter, Metadata, TimestampedPacket};
pub use recorder::{RecorderStatus, StudioRecorder, generate_recording_filename};
