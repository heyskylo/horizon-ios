//! Studio Freecam & Spectator Flight Controller
//!
//! Provides detached 6-DOF camera flight with velocity smoothing, mouse look,
//! and integration with the camera spline keyframe tracks.

use super::spline::{CameraKeyframe, CameraTrack};

/// Flying freecam mode and controller for studio recording and replay inspection.
#[derive(Clone, Debug, PartialEq)]
pub struct StudioFreecam {
    /// Whether freecam is active and controlling the view.
    pub active: bool,
    /// 3D world position [x, y, z].
    pub position: glam::DVec3,
    /// Horizontal look angle in degrees (-180..180).
    pub yaw: f32,
    /// Vertical look angle in degrees (-90..90).
    pub pitch: f32,
    /// Camera roll in degrees.
    pub roll: f32,
    /// Field of view in degrees.
    pub fov: f32,
    /// Flying speed in blocks per second (default 15.0).
    pub base_speed: f64,
    /// Current velocity for smooth movement damping.
    pub velocity: glam::DVec3,
    /// Whether camera is currently driven automatically by keyframe spline path.
    pub locked_to_path: bool,
    /// The camera spline track.
    pub track: CameraTrack,
}

impl Default for StudioFreecam {
    fn default() -> Self {
        Self {
            active: false,
            position: glam::DVec3::ZERO,
            yaw: 0.0,
            pitch: 0.0,
            roll: 0.0,
            fov: 70.0,
            base_speed: 15.0,
            velocity: glam::DVec3::ZERO,
            locked_to_path: false,
            track: CameraTrack::new(),
        }
    }
}

impl StudioFreecam {
    pub fn new() -> Self {
        Self::default()
    }

    /// Activate freecam initialized at the player's current eye position and look angle.
    pub fn activate(&mut self, position: glam::DVec3, yaw: f32, pitch: f32, fov: f32) {
        self.active = true;
        self.position = position;
        self.yaw = yaw;
        self.pitch = pitch;
        self.roll = 0.0;
        self.fov = fov;
        self.velocity = glam::DVec3::ZERO;
        self.locked_to_path = false;
    }

    /// Deactivate freecam and return camera control to normal gameplay.
    pub fn deactivate(&mut self) {
        self.active = false;
        self.locked_to_path = false;
        self.velocity = glam::DVec3::ZERO;
    }

    /// Rotate camera by mouse motion delta.
    pub fn turn(&mut self, dyaw: f32, dpitch: f32) {
        if !self.active || self.locked_to_path {
            return;
        }
        self.yaw = (self.yaw + dyaw).rem_euclid(360.0);
        if self.yaw > 180.0 {
            self.yaw -= 360.0;
        }
        self.pitch = (self.pitch + dpitch).clamp(-90.0, 90.0);
    }

    /// Adjust flight speed using mouse wheel scrolling.
    pub fn adjust_speed(&mut self, wheel_steps: i32) {
        if wheel_steps > 0 {
            self.base_speed = (self.base_speed * 1.25).min(200.0);
        } else if wheel_steps < 0 {
            self.base_speed = (self.base_speed / 1.25).max(1.0);
        }
    }

    /// Update freecam position with movement inputs (WASD, Space, Shift).
    pub fn update_flight_motion(
        &mut self,
        forward: f64,
        strafe: f64,
        vertical: f64,
        fast_boost: bool,
        delta_secs: f64,
    ) {
        if !self.active || self.locked_to_path {
            return;
        }

        let speed_mult = if fast_boost { 2.5 } else { 1.0 };
        let target_speed = self.base_speed * speed_mult;

        // Calculate view direction vectors
        let yaw_rad = (self.yaw as f64).to_radians();
        let pitch_rad = (self.pitch as f64).to_radians();

        let forward_dir = glam::DVec3::new(
            -yaw_rad.sin() * pitch_rad.cos(),
            -pitch_rad.sin(),
            yaw_rad.cos() * pitch_rad.cos(),
        )
        .normalize_or_zero();

        let right_dir = glam::DVec3::new(-yaw_rad.cos(), 0.0, -yaw_rad.sin()).normalize_or_zero();

        let up_dir = glam::DVec3::new(0.0, 1.0, 0.0);

        let mut wish_dir = glam::DVec3::ZERO;
        if forward.abs() > 1e-4 {
            wish_dir += forward_dir * forward;
        }
        if strafe.abs() > 1e-4 {
            wish_dir += right_dir * strafe;
        }
        if vertical.abs() > 1e-4 {
            wish_dir += up_dir * vertical;
        }

        let wish_dir = wish_dir.normalize_or_zero();
        let target_vel = wish_dir * target_speed;

        // Smooth damping
        let blend = (1.0 - (-15.0 * delta_secs).exp()).clamp(0.0, 1.0);
        self.velocity = self.velocity.lerp(target_vel, blend);
        self.position += self.velocity * delta_secs;
    }

    /// Advance or evaluate keyframe spline pose at timeline timestamp `time_ms`.
    pub fn update_spline_playback(&mut self, time_ms: u32) {
        if !self.active || !self.locked_to_path {
            return;
        }
        if let Some(pose) = self.track.evaluate(time_ms) {
            self.position = pose.position;
            self.yaw = pose.yaw;
            self.pitch = pose.pitch;
            self.roll = pose.roll;
            self.fov = pose.fov;
        }
    }

    /// Add a keyframe at the current freecam location and rotation for `time_ms`.
    pub fn add_keyframe_at_current(&mut self, time_ms: u32) {
        let kf = CameraKeyframe {
            time_ms,
            position: [self.position.x, self.position.y, self.position.z],
            yaw: self.yaw,
            pitch: self.pitch,
            roll: self.roll,
            fov: self.fov,
        };
        self.track.insert(kf);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_freecam_activation_and_motion() {
        let mut freecam = StudioFreecam::new();
        assert!(!freecam.active);

        freecam.activate(glam::DVec3::new(10.0, 64.0, 10.0), 0.0, 0.0, 70.0);
        assert!(freecam.active);
        assert_eq!(freecam.position, glam::DVec3::new(10.0, 64.0, 10.0));

        // Turn
        freecam.turn(45.0, -10.0);
        assert_eq!(freecam.yaw, 45.0);
        assert_eq!(freecam.pitch, -10.0);

        // Fly forward for 1 second (facing yaw 0, forward is +Z)
        let mut forward_test = StudioFreecam::new();
        forward_test.activate(glam::DVec3::ZERO, 0.0, 0.0, 70.0);
        forward_test.update_flight_motion(1.0, 0.0, 0.0, false, 0.1);
        assert!(forward_test.position.z > 0.0);

        freecam.update_flight_motion(1.0, 0.0, 0.0, false, 0.1);
        assert!(freecam.position != glam::DVec3::new(10.0, 64.0, 10.0));

        // Speed adjustment
        let initial_speed = freecam.base_speed;
        freecam.adjust_speed(1);
        assert!(freecam.base_speed > initial_speed);
        freecam.adjust_speed(-1);
        assert!((freecam.base_speed - initial_speed).abs() < 1e-4);

        freecam.deactivate();
        assert!(!freecam.active);
    }
}
