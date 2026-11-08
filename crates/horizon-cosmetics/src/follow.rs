//! How a companion follows its wearer: a smoothed lag toward its anchor, and the two rules that
//! keep it out of the way (out of a cone in front of a first-person camera, clear of the head).

use glam::{DVec3, Vec3};

/// Half the angle of the cone in front of a first-person camera a companion never enters
/// (degrees): the middle of the view, crosshair included.
pub const VIEW_CONE: f32 = 35.0;
/// Farther than this from its anchor (blocks), a companion jumps there instead of flying: the
/// wearer teleported, or came into view.
pub const SNAP: f64 = 8.0;

/// A companion's place in the world and the way it faces.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Follow {
    pub pos: DVec3,
    /// Degrees, as an entity's yaw.
    pub yaw: f32,
}

impl Follow {
    /// A companion already at `target`, facing `yaw`.
    pub fn at(target: DVec3, yaw: f32) -> Self {
        Self { pos: target, yaw }
    }

    /// Moves toward `target` facing toward `yaw` over `dt` seconds, as a first-order lag with a
    /// time constant of `lag` ticks: frame-rate independent, never past the target.
    pub fn step(&mut self, target: DVec3, yaw: f32, lag: f32, dt: f32) {
        if self.pos.distance_squared(target) > SNAP * SNAP || !self.pos.is_finite() {
            *self = Self::at(target, yaw);
            return;
        }
        let k = 1.0 - (-dt.max(0.0) * 20.0 / lag.max(1.0)).exp();
        self.pos += (target - self.pos) * f64::from(k);
        let turn = (yaw - self.yaw + 180.0).rem_euclid(360.0) - 180.0;
        self.yaw += turn * k;
    }
}

/// `p` (blocks from a first-person camera) moved out of the cone of `VIEW_CONE` around `forward`
/// (unit), keeping its distance: turned toward the cone's edge on its own side, or toward
/// `side` (unit, the camera's left say) when it sits right on the axis.
pub fn out_of_view(p: Vec3, forward: Vec3, side: Vec3) -> Vec3 {
    let length = p.length();
    if length < 1e-6 {
        return side * 0.5;
    }
    let along = p.dot(forward);
    let cone = VIEW_CONE.to_radians();
    if along <= length * cone.cos() {
        return p;
    }
    let across = p - forward * along;
    let out = across.try_normalize().unwrap_or(side);
    // Just past the edge, so rounding keeps it out.
    let edge = cone + 0.01;
    (forward * edge.cos() + out * edge.sin()) * length
}

/// `p` pushed straight away from `head` until it is at least `min` from it (all in blocks).
pub fn clear_of(p: Vec3, head: Vec3, min: f32) -> Vec3 {
    let d = p - head;
    let length = d.length();
    if length >= min {
        return p;
    }
    let away = d.try_normalize().unwrap_or(Vec3::Y);
    head + away * min
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn follows_with_a_lag_that_does_not_depend_on_the_frame_rate() {
        let target = DVec3::new(1.0, 0.0, 0.0);
        // A lag of 8 ticks: after 8 ticks (0.4 s) it has come 1 - 1/e of the way.
        let mut slow = Follow::at(DVec3::ZERO, 0.0);
        let mut fast = slow;
        for _ in 0..24 {
            slow.step(target, 90.0, 8.0, 1.0 / 60.0);
        }
        for _ in 0..96 {
            fast.step(target, 90.0, 8.0, 1.0 / 240.0);
        }
        let expected = 1.0 - (-1.0_f64).exp();
        assert!((slow.pos.x - expected).abs() < 1e-4, "{}", slow.pos.x);
        assert!((fast.pos.x - expected).abs() < 1e-4, "{}", fast.pos.x);
        assert!((slow.yaw - 90.0 * expected as f32).abs() < 1e-2);
        // A long frame never carries it past.
        slow.step(target, 90.0, 8.0, 10.0);
        assert!(slow.pos.x <= 1.0 && (slow.pos.x - 1.0).abs() < 1e-6);
    }

    #[test]
    fn turns_the_short_way_and_snaps_after_a_teleport() {
        let mut f = Follow::at(DVec3::ZERO, 170.0);
        f.step(DVec3::ZERO, -170.0, 1.0, 10.0);
        // Through 180, not back through 0.
        assert!((f.yaw - 190.0).abs() < 1e-3, "{}", f.yaw);
        let far = DVec3::new(100.0, 64.0, -3.0);
        f.step(far, 0.0, 40.0, 0.001);
        assert_eq!(f, Follow::at(far, 0.0));
    }

    #[test]
    fn stays_out_of_the_middle_of_a_first_person_view() {
        let forward = Vec3::NEG_Z;
        let left = Vec3::NEG_X;
        let cone = VIEW_CONE.to_radians().cos();
        // Right in front, a little to the right: pushed out to the right, as far away.
        let p = Vec3::new(0.1, 0.0, -1.0);
        let q = out_of_view(p, forward, left);
        assert!(q.normalize().dot(forward) < cone);
        assert!((q.length() - p.length()).abs() < 1e-5);
        assert!(q.x > 0.0 && q.y.abs() < 1e-6);
        // Dead centre: toward the side given.
        assert!(out_of_view(Vec3::new(0.0, 0.0, -1.0), forward, left).x < 0.0);
        // Already beside or behind: unchanged.
        for p in [Vec3::new(1.0, 0.0, -0.5), Vec3::new(0.0, 0.0, 1.0)] {
            assert_eq!(out_of_view(p, forward, left), p);
        }
    }

    #[test]
    fn keeps_its_distance_from_the_head() {
        let head = Vec3::new(0.0, 1.5, 0.0);
        let p = clear_of(Vec3::new(0.2, 1.5, 0.0), head, 0.75);
        assert!((p - Vec3::new(0.75, 1.5, 0.0)).length() < 1e-6);
        assert_eq!(
            clear_of(Vec3::new(1.0, 1.5, 0.0), head, 0.75),
            Vec3::new(1.0, 1.5, 0.0)
        );
    }
}
