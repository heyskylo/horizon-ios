//! Horizon In-Game Video Studio.
//!
//! Provides native session packet recording (`.mcpr` format).

pub mod camera;
pub mod mcpr;
pub mod path_render;
pub mod recorder;
pub mod replay;
pub mod spline;

pub use camera::StudioFreecam;
pub use mcpr::{
    McprReader, McprWriter, Metadata, RecoveryReport, TimestampedPacket, recover_crashed_recordings,
};
pub use path_render::render_camera_path;
pub use recorder::{RecorderStatus, StudioRecorder, generate_recording_filename};
pub use replay::ReplayDriver;
pub use spline::{CameraKeyframe, CameraTrack, EvaluatedPose};
