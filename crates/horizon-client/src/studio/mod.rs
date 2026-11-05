//! Horizon In-Game Video Studio.
//!
//! Provides native session packet recording (`.mcpr` format).

pub mod mcpr;
pub mod recorder;

pub use mcpr::{
    McprReader, McprWriter, Metadata, RecoveryReport, TimestampedPacket, recover_crashed_recordings,
};
pub use recorder::{RecorderStatus, StudioRecorder, generate_recording_filename};
