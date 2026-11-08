//! Studio Camera Path Spline & Keyframe Interpolator
//!
//! Provides smooth Catmull-Rom 3D position interpolation and quaternion Slerp
//! rotation for cinematic replay camera flights.

use serde::{Deserialize, Serialize};

/// One camera pose keyframe along a timeline.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CameraKeyframe {
    /// Milliseconds from replay start.
    pub time_ms: u32,
    /// 3D world position [x, y, z].
    pub position: [f64; 3],
    /// Horizontal rotation in degrees (-180..180).
    pub yaw: f32,
    /// Vertical pitch in degrees (-90..90).
    pub pitch: f32,
    /// Camera roll / tilt in degrees.
    pub roll: f32,
    /// Field of view in degrees (e.g. 70.0).
    pub fov: f32,
}

impl CameraKeyframe {
    pub fn new(time_ms: u32, position: [f64; 3], yaw: f32, pitch: f32, fov: f32) -> Self {
        Self {
            time_ms,
            position,
            yaw,
            pitch,
            roll: 0.0,
            fov,
        }
    }

    /// Rotation as a glam Quaternion.
    pub fn rotation_quat(&self) -> glam::Quat {
        let yaw_rad = self.yaw.to_radians();
        let pitch_rad = self.pitch.to_radians();
        let roll_rad = self.roll.to_radians();

        // Standard Minecraft / Horizon camera rotation order: Yaw (Y) * Pitch (X) * Roll (Z)
        glam::Quat::from_rotation_y(-yaw_rad)
            * glam::Quat::from_rotation_x(-pitch_rad)
            * glam::Quat::from_rotation_z(roll_rad)
    }
}

/// Evaluated camera pose at a specific timeline timestamp.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EvaluatedPose {
    pub position: glam::DVec3,
    pub yaw: f32,
    pub pitch: f32,
    pub roll: f32,
    pub fov: f32,
}

/// Sequence of camera keyframes forming a continuous 3D camera trajectory.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct CameraTrack {
    pub keyframes: Vec<CameraKeyframe>,
}

impl CameraTrack {
    pub fn new() -> Self {
        Self {
            keyframes: Vec::new(),
        }
    }

    /// Slice of all sorted keyframes.
    pub fn keyframes(&self) -> &[CameraKeyframe] {
        &self.keyframes
    }

    /// Number of keyframes in track.
    pub fn len(&self) -> usize {
        self.keyframes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.keyframes.is_empty()
    }

    /// Insert or update a keyframe at `keyframe.time_ms`.
    pub fn insert(&mut self, keyframe: CameraKeyframe) {
        if let Some(existing) = self
            .keyframes
            .iter_mut()
            .find(|k| k.time_ms == keyframe.time_ms)
        {
            *existing = keyframe;
        } else {
            self.keyframes.push(keyframe);
            self.keyframes.sort_by_key(|k| k.time_ms);
        }
    }

    /// Remove a keyframe near `time_ms` within `tolerance_ms`.
    pub fn remove_near(&mut self, time_ms: u32, tolerance_ms: u32) -> bool {
        let prev_len = self.keyframes.len();
        self.keyframes
            .retain(|k| k.time_ms.abs_diff(time_ms) > tolerance_ms);
        self.keyframes.len() != prev_len
    }

    /// Clear all keyframes.
    pub fn clear(&mut self) {
        self.keyframes.clear();
    }

    /// Evaluate camera pose at `time_ms`.
    pub fn evaluate(&self, time_ms: u32) -> Option<EvaluatedPose> {
        if self.keyframes.is_empty() {
            return None;
        }
        if self.keyframes.len() == 1 || time_ms <= self.keyframes[0].time_ms {
            let k = &self.keyframes[0];
            return Some(EvaluatedPose {
                position: glam::DVec3::from_array(k.position),
                yaw: k.yaw,
                pitch: k.pitch,
                roll: k.roll,
                fov: k.fov,
            });
        }
        let last = self.keyframes.last().unwrap();
        if time_ms >= last.time_ms {
            return Some(EvaluatedPose {
                position: glam::DVec3::from_array(last.position),
                yaw: last.yaw,
                pitch: last.pitch,
                roll: last.roll,
                fov: last.fov,
            });
        }

        // Find index i where keyframes[i].time_ms <= time_ms < keyframes[i+1].time_ms
        let idx = self
            .keyframes
            .iter()
            .rposition(|k| k.time_ms <= time_ms)
            .unwrap_or(0);

        let k1 = &self.keyframes[idx];
        let k2 = &self.keyframes[idx + 1];

        let dt = (k2.time_ms - k1.time_ms) as f64;
        let t = if dt > 0.0 {
            ((time_ms - k1.time_ms) as f64 / dt).clamp(0.0, 1.0)
        } else {
            0.0
        };

        // 4 points for Catmull-Rom
        let p0 = if idx > 0 {
            glam::DVec3::from_array(self.keyframes[idx - 1].position)
        } else {
            // Extrapolate backwards
            let p1 = glam::DVec3::from_array(k1.position);
            let p2 = glam::DVec3::from_array(k2.position);
            p1 - (p2 - p1)
        };
        let p1 = glam::DVec3::from_array(k1.position);
        let p2 = glam::DVec3::from_array(k2.position);
        let p3 = if idx + 2 < self.keyframes.len() {
            glam::DVec3::from_array(self.keyframes[idx + 2].position)
        } else {
            // Extrapolate forwards
            p2 + (p2 - p1)
        };

        let position = catmull_rom_dvec3(p0, p1, p2, p3, t);

        // Quaternion Slerp for rotation
        let q1 = k1.rotation_quat();
        let q2 = k2.rotation_quat();
        let q_interp = q1.slerp(q2, t as f32);

        // Extract yaw, pitch, roll from interpolated quaternion
        let (yaw, pitch, roll) = quat_to_euler(q_interp, k1.yaw, k2.yaw);

        // Linear interpolation for FOV
        let fov = (k1.fov as f64 + (k2.fov - k1.fov) as f64 * t) as f32;

        Some(EvaluatedPose {
            position,
            yaw,
            pitch,
            roll,
            fov,
        })
    }

    /// Sample points along the spline path at `step_ms` resolution for 3D trajectory rendering.
    pub fn sample_path(&self, step_ms: u32) -> Vec<glam::DVec3> {
        if self.keyframes.len() < 2 {
            return self
                .keyframes
                .iter()
                .map(|k| glam::DVec3::from_array(k.position))
                .collect();
        }

        let start_ms = self.keyframes[0].time_ms;
        let end_ms = self.keyframes.last().unwrap().time_ms;
        let step = step_ms.max(20);

        let mut points = Vec::new();
        let mut cur = start_ms;
        while cur <= end_ms {
            if let Some(pose) = self.evaluate(cur) {
                points.push(pose.position);
            }
            cur += step;
        }
        if let Some(pose) = self.evaluate(end_ms)
            && points.last() != Some(&pose.position)
        {
            points.push(pose.position);
        }
        points
    }
}

/// Standard centripetal Catmull-Rom spline formula for DVec3.
fn catmull_rom_dvec3(
    p0: glam::DVec3,
    p1: glam::DVec3,
    p2: glam::DVec3,
    p3: glam::DVec3,
    t: f64,
) -> glam::DVec3 {
    let t2 = t * t;
    let t3 = t2 * t;
    0.5 * ((2.0 * p1)
        + (-p0 + p2) * t
        + (2.0 * p0 - 5.0 * p1 + 4.0 * p2 - p3) * t2
        + (-p0 + 3.0 * p1 - 3.0 * p2 + p3) * t3)
}

/// Helper converting Glam Quat back into Euler angles (yaw, pitch, roll).
fn quat_to_euler(q: glam::Quat, base_yaw1: f32, base_yaw2: f32) -> (f32, f32, f32) {
    // Forward vector (0, 0, -1) transformed by quaternion
    let forward = q * glam::Vec3::new(0.0, 0.0, -1.0);
    let pitch = -forward.y.asin().to_degrees();

    // Yaw from horizontal forward vector
    let raw_yaw = (-forward.x).atan2(-forward.z).to_degrees();

    // Preserve yaw wrapping continuous with inputs
    let avg_yaw = (base_yaw1 + base_yaw2) * 0.5;
    let mut yaw = raw_yaw;
    while yaw - avg_yaw > 180.0 {
        yaw -= 360.0;
    }
    while yaw - avg_yaw < -180.0 {
        yaw += 360.0;
    }

    // Up vector for roll
    let up = q * glam::Vec3::new(0.0, 1.0, 0.0);
    let right = forward.cross(up).normalize_or_zero();
    let roll = right.y.atan2(up.y).to_degrees();

    (yaw, pitch.clamp(-90.0, 90.0), roll)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_camera_track_evaluation() {
        let mut track = CameraTrack::new();
        assert!(track.is_empty());
        assert_eq!(track.evaluate(0), None);

        track.insert(CameraKeyframe::new(0, [0.0, 64.0, 0.0], 0.0, 0.0, 70.0));
        track.insert(CameraKeyframe::new(
            1000,
            [10.0, 70.0, 20.0],
            90.0,
            10.0,
            80.0,
        ));
        track.insert(CameraKeyframe::new(
            2000,
            [20.0, 64.0, 0.0],
            180.0,
            0.0,
            70.0,
        ));

        assert_eq!(track.len(), 3);

        // Evaluation at exact keyframe points
        let p0 = track.evaluate(0).unwrap();
        assert!((p0.position.x - 0.0).abs() < 1e-4);
        assert!((p0.position.y - 64.0).abs() < 1e-4);
        assert_eq!(p0.fov, 70.0);

        let p1 = track.evaluate(1000).unwrap();
        assert!((p1.position.x - 10.0).abs() < 1e-4);
        assert!((p1.position.y - 70.0).abs() < 1e-4);
        assert_eq!(p1.fov, 80.0);

        // Midpoint evaluation
        let mid = track.evaluate(500).unwrap();
        assert!(mid.position.x > 0.0 && mid.position.x < 10.0);
        assert!(mid.position.y >= 64.0);
        assert!((mid.fov - 75.0).abs() < 1.0);

        // Sample path generates points
        let samples = track.sample_path(100);
        assert!(samples.len() >= 20);
    }
}
