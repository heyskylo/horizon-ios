//! Horizon In-Game Video Studio.

pub mod mcpr;

pub use mcpr::{
    McprReader, McprWriter, Metadata, RecoveryReport, TimestampedPacket, recover_crashed_recordings,
};
