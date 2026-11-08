//! 3D Spline Path and Camera Frustum Gizmo Renderer
//!
//! Renders the Catmull-Rom camera path spline in 3D world space (yellow line)
//! and wireframe frustum pyramids at each keyframe position to visualize camera
//! orientation, field of view, and roll.

use super::spline::{CameraKeyframe, CameraTrack};
use crate::gizmos::Gizmos;

/// Studio path colors matching in-game video editor theme.
pub const PATH_YELLOW: u32 = 0xFFFF_D700; // Gold / Yellow spline
pub const FRUSTUM_COLOR: u32 = 0xFF40_E0D0; // Turquoise / Cyan frustum
pub const FRUSTUM_SELECTED_COLOR: u32 = 0xFFFF_4500; // Orange-red selected frustum
pub const KEYFRAME_NODE_COLOR: u32 = 0xFFFF_FFFF; // White apex node
pub const LINE_WIDTH: f32 = 2.0;

/// Render the camera spline path and keyframe frustums into the given `Gizmos` collection.
pub fn render_camera_path(
    track: &CameraTrack,
    gizmos: &mut Gizmos,
    selected_index: Option<usize>,
    step_ms: u32,
) {
    if track.keyframes.is_empty() {
        return;
    }

    // 1. Draw sampled spline curve between keyframes
    if track.keyframes.len() >= 2 {
        let sampled = track.sample_path(step_ms.max(20));
        for window in sampled.windows(2) {
            let p0 = window[0];
            let p1 = window[1];
            gizmos.line(
                [p0.x, p0.y, p0.z],
                [p1.x, p1.y, p1.z],
                PATH_YELLOW,
                LINE_WIDTH,
            );
        }
    }

    // 2. Draw wireframe camera frustum at each keyframe location
    for (idx, kf) in track.keyframes.iter().enumerate() {
        let is_selected = selected_index == Some(idx);
        let color = if is_selected {
            FRUSTUM_SELECTED_COLOR
        } else {
            FRUSTUM_COLOR
        };

        render_camera_frustum(kf, gizmos, color);
    }
}

/// Draws a wireframe camera pyramid/frustum with an "up" indicator notch at the keyframe pose.
pub fn render_camera_frustum(kf: &CameraKeyframe, gizmos: &mut Gizmos, color: u32) {
    let apex = glam::DVec3::from_array(kf.position);
    let quat = kf.rotation_quat();

    // In Minecraft camera conventions:
    // View vector points forward. In local coordinates: forward is -Z.
    let dist = 1.0;
    // Scale aspect ratio ~16:9, scaled by FOV
    let half_fov_rad = (kf.fov * 0.5).to_radians();
    let hh = dist * (half_fov_rad.tan() as f64).clamp(0.2, 1.2);
    let hw = hh * (16.0 / 9.0);

    let local_corners = [
        glam::DVec3::new(-hw, hh, -dist),                      // 0: Top-Left
        glam::DVec3::new(hw, hh, -dist),                       // 1: Top-Right
        glam::DVec3::new(hw, -hh, -dist),                      // 2: Bottom-Right
        glam::DVec3::new(-hw, -hh, -dist),                     // 3: Bottom-Left
        glam::DVec3::new(0.0, hh + 0.15 * hh.max(0.3), -dist), // 4: Top apex marker
    ];

    let world_corners: Vec<[f64; 3]> = local_corners
        .iter()
        .map(|&c| {
            let rot_c = quat.as_dquat() * c;
            let w = apex + rot_c;
            [w.x, w.y, w.z]
        })
        .collect();

    let apex_pt = kf.position;

    // Connect apex to 4 corners
    for i in 0..4 {
        gizmos.line(apex_pt, world_corners[i], color, LINE_WIDTH);
    }

    // Connect the 4 rectangular base corners
    gizmos.line(world_corners[0], world_corners[1], color, LINE_WIDTH);
    gizmos.line(world_corners[1], world_corners[2], color, LINE_WIDTH);
    gizmos.line(world_corners[2], world_corners[3], color, LINE_WIDTH);
    gizmos.line(world_corners[3], world_corners[0], color, LINE_WIDTH);

    // Up indicator triangle (Top-Left -> Top marker -> Top-Right)
    gizmos.line(
        world_corners[0],
        world_corners[4],
        KEYFRAME_NODE_COLOR,
        LINE_WIDTH,
    );
    gizmos.line(
        world_corners[4],
        world_corners[1],
        KEYFRAME_NODE_COLOR,
        LINE_WIDTH,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_render_camera_path_lines_generated() {
        let mut track = CameraTrack::new();
        track.insert(CameraKeyframe::new(0, [0.0, 64.0, 0.0], 0.0, 0.0, 70.0));
        track.insert(CameraKeyframe::new(
            1000,
            [10.0, 65.0, 10.0],
            45.0,
            0.0,
            70.0,
        ));

        let mut gizmos = Gizmos::default();
        render_camera_path(&track, &mut gizmos, None, 100);

        // Should have spline segments + frustum lines for 2 keyframes
        // Each frustum has 4 + 4 + 2 = 10 lines
        assert!(!gizmos.lines.is_empty());
        assert!(gizmos.lines.len() >= 20);
    }
}
