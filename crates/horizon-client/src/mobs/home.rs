//! `Mob.isWithinHome`: the home a mob is restricted to (`home_pos`,
//! `home_radius`).
use crate::mobs::Mob;
use horizon_core::geometry::Vec3;

/// `Mob.isWithinHome(BlockPos)`.
pub fn within_home(c: [i32; 3], r: i32, p: [i32; 3]) -> bool {
    let d = |i: usize| i64::from(c[i] - p[i]);
    let r = i64::from(r);
    d(0) * d(0) + d(1) * d(1) + d(2) * d(2) < r * r
}

impl Mob {
    /// `Mob.isWithinHome(BlockPos.containing(at))`; true with no home.
    pub(super) fn within_home_at(&self, at: Vec3) -> bool {
        let block = [
            at.x.floor() as i32,
            at.y.floor() as i32,
            at.z.floor() as i32,
        ];
        self.ai
            .home
            .is_none_or(|(center, radius)| within_home(center, radius, block))
    }
}
