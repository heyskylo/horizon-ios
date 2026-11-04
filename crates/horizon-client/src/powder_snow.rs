//! `PowderSnowBlock.entityInside`, shared by living and other entities.
use horizon_core::{
    geometry::{Aabb, Vec3, sweep_intersected_blocks},
    position::BlockPos,
    random::RandomSource,
};

/// Living entities must have their feet in snow; other entities only need
/// their swept box to touch it. Preserve callback order and visit each cell once.
pub fn contacts(
    from: Vec3,
    to: Vec3,
    bounds: Aabb,
    living: bool,
    mut is_snow: impl FnMut(BlockPos) -> bool,
) -> Vec<BlockPos> {
    if living && !is_snow(BlockPos::containing(to)) {
        return Vec::new();
    }
    let epsilon = f64::from(1.0e-5_f32);
    let bounds = bounds.deflate(Vec3::new(epsilon, epsilon, epsilon));
    let mut visited = std::collections::HashSet::new();
    let mut contacts = Vec::new();
    let _ = sweep_intersected_blocks(from, to, bounds, 1_000_000, |p, step| {
        if step >= 16 {
            return false;
        }
        if visited.insert(p) && is_snow(p) {
            contacts.push(p);
        }
        true
    });
    contacts
}

/// The server's movement gate and Boolean/float draws, in Java order.
pub fn particle_velocity(known_movement: Vec3, random: &mut impl RandomSource) -> Option<[f64; 3]> {
    if (known_movement.x == 0.0 && known_movement.z == 0.0) || !random.next_boolean() {
        return None;
    }
    let x = (random.next_float() * 2.0 - 1.0) * 0.083_333_336_f32;
    let z = (random.next_float() * 2.0 - 1.0) * 0.083_333_336_f32;
    Some([f64::from(x), f64::from(0.05_f32), f64::from(z)])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn swept_contacts_cover_nonliving_entities_and_gate_living_feet() {
        let from = Vec3::new(0.5, 0.0, 0.5);
        let to = Vec3::new(3.5, 0.0, 0.5);
        let bounds = Aabb::new(Vec3::new(3.25, 0.0, 0.25), Vec3::new(3.75, 0.5, 0.75));
        let snow = BlockPos::new(1, 0, 0);
        assert_eq!(contacts(from, to, bounds, false, |p| p == snow), vec![snow]);
        assert!(contacts(from, to, bounds, true, |p| p == snow).is_empty());
        let feet = BlockPos::new(3, 0, 0);
        assert_eq!(contacts(to, to, bounds, true, |p| p == feet), vec![feet]);
    }
}
