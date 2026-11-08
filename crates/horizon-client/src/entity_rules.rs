//! PURPOSE: `Entity`'s gravity and fluid-push rules, decided once for every kind.
//! WHY: each kind used to check its own `NoGravity` flag and pick its own
//! number, so a kind that forgot the check ignored the flag (#6481).
//! REFS: vanilla `Entity.isNoGravity`, `getDefaultGravity`, `getGravity`,
//! `applyGravity`, `isPushedByFluid`, `getAirDrag`; issue #6509, slice #6544.
//! LOGS: none. READS-BY: the kinds' ticks in this crate and in `game/`.
//! ENDS-WHEN: kinds share one stored `Entity` state and this folds into it.
use horizon_core::geometry::Vec3;

/// `Entity.scale(getAirDrag())`: 0.98F, widened before it scales double motion.
pub fn scale_air_drag(velocity: Vec3) -> Vec3 {
    let drag = f64::from(0.98_f32);
    Vec3::new(velocity.x * drag, velocity.y * drag, velocity.z * drag)
}

/// The rules vanilla's `Entity` answers for every kind. A kind overrides one
/// only where its vanilla class does; the defaults are `Entity`'s.
pub trait EntityRules {
    /// `Entity.isNoGravity` (`NoGravity` in the save).
    fn is_no_gravity(&self) -> bool {
        false
    }
    /// `Entity.getDefaultGravity`.
    fn get_default_gravity(&self) -> f64 {
        0.0
    }
    /// `Entity.getGravity`: none with `NoGravity`, else the kind's default.
    /// `applyGravity` subtracts this from the vertical velocity.
    fn get_gravity(&self) -> f64 {
        if self.is_no_gravity() {
            0.0
        } else {
            self.get_default_gravity()
        }
    }
    /// `Entity.isPushedByFluid`.
    fn is_pushed_by_fluid(&self) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn four_ticks_of_air_drag_match_vanilla_0_98f() {
        // Issue #6814: 0.1 × 0.98F⁴ is 0.09223682318…, not 0.1 × 0.98⁴ as f64.
        let mut v = Vec3::new(0.1, 0.0, 0.0);
        for _ in 0..4 {
            v = scale_air_drag(v);
        }
        assert_eq!(v.x.to_bits(), 0x3fb7_9cd5_1b0c_538e);
        assert_eq!(v.y.to_bits(), 0);
        assert_eq!(v.z.to_bits(), 0);
    }
}
