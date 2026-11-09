//! The camera native mods move (`native_camera_pose`, `native_camera_shake`)
//! and the first-person hand they pose: each value bounded, a value that
//! isn't finite keeping vanilla's.

/// The camera shake native mods ask for (`native_camera_shake`): turns in
/// degrees, and an offset in blocks along the camera's right, up and back.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct NativeShake {
    pub pitch: f32,
    pub yaw: f32,
    pub roll: f32,
    pub offset: [f32; 3],
}

/// A mod's camera (`native_camera_pose`): each value that isn't finite keeps vanilla's from `view`.
pub(super) fn posed_camera(
    view: (glam::DVec3, f32, f32),
    position: [f64; 3],
    yaw: f32,
    pitch: f32,
) -> (glam::DVec3, f32, f32) {
    let position = std::array::from_fn(|i| {
        if position[i].is_finite() {
            position[i]
        } else {
            view.0[i]
        }
    });
    (
        glam::DVec3::from_array(position),
        if yaw.is_finite() { yaw } else { view.1 },
        if pitch.is_finite() { pitch } else { view.2 },
    )
}

/// The largest turn a shake gives, in degrees, on each axis.
const SHAKE_MAX_DEGREES: f32 = 10.0;
/// The largest offset a shake gives, in blocks, along each axis.
const SHAKE_MAX_OFFSET: f32 = 0.5;

/// The pose a native mod asks for the first-person hand, each out-of-range
/// or non-finite component replaced by its identity.
pub(in crate::game) fn hand_pose_matrix(values: [f32; 7]) -> glam::Mat4 {
    let valid = |value: f32, min: f32, max: f32, default: f32| {
        if value.is_finite() && (min..=max).contains(&value) {
            value
        } else {
            default
        }
    };
    let [x, y, z, pitch, yaw, roll, scale] = values;
    glam::Mat4::from_translation(glam::Vec3::new(
        valid(x, -16.0, 16.0, 0.0),
        valid(y, -16.0, 16.0, 0.0),
        valid(z, -16.0, 16.0, 0.0),
    )) * glam::Mat4::from_rotation_x(valid(pitch, -3600.0, 3600.0, 0.0).to_radians())
        * glam::Mat4::from_rotation_y(valid(yaw, -3600.0, 3600.0, 0.0).to_radians())
        * glam::Mat4::from_rotation_z(valid(roll, -3600.0, 3600.0, 0.0).to_radians())
        * glam::Mat4::from_scale(glam::Vec3::splat(valid(scale, 0.01, 100.0, 1.0)))
}

/// `shake` scaled by `scale` (Distortion Effects) and bounded on each axis;
/// a value that isn't finite counts as none.
pub(super) fn bounded_shake(shake: NativeShake, scale: f32) -> NativeShake {
    let scale = if scale.is_finite() {
        scale.clamp(0.0, 1.0)
    } else {
        1.0
    };
    let turn = |v: f32| {
        if v.is_finite() {
            (v * scale).clamp(-SHAKE_MAX_DEGREES, SHAKE_MAX_DEGREES)
        } else {
            0.0
        }
    };
    let shift = |v: f32| {
        if v.is_finite() {
            (v * scale).clamp(-SHAKE_MAX_OFFSET, SHAKE_MAX_OFFSET)
        } else {
            0.0
        }
    };
    NativeShake {
        pitch: turn(shake.pitch),
        yaw: turn(shake.yaw),
        roll: turn(shake.roll),
        offset: shake.offset.map(shift),
    }
}

/// The pose a bounded shake multiplies into the projection, after the view
/// bobbing: the offset, then roll, pitch and yaw.
pub(super) fn shake_pose(shake: NativeShake) -> glam::Mat4 {
    let [x, y, z] = shake.offset;
    glam::Mat4::from_translation(glam::Vec3::new(-x, -y, -z))
        * glam::Mat4::from_rotation_z(shake.roll.to_radians())
        * glam::Mat4::from_rotation_x(shake.pitch.to_radians())
        * glam::Mat4::from_rotation_y(shake.yaw.to_radians())
}
