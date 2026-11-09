use super::first_spin_target;
use horizon_core::geometry::{Aabb, Vec3};

fn cube(x: f64) -> Aabb {
    Aabb::new(Vec3::new(x, 0.0, 0.0), Vec3::new(x + 1.0, 1.0, 1.0))
}

/// The spin hits the first mob in the swept box, not a removed one, the
/// vehicle, or one out of reach.
#[test]
fn the_spin_picks_the_first_live_mob_in_the_sweep() {
    let swept = Aabb::new(Vec3::new(0.0, 0.0, 0.0), Vec3::new(3.0, 1.0, 1.0));
    let targets = [
        (1, false, cube(10.0)),
        (2, true, cube(1.0)),
        (3, false, cube(1.5)),
        (4, false, cube(2.0)),
    ];
    assert_eq!(
        first_spin_target(swept, None, targets.iter().copied()),
        Some(2)
    );
    assert_eq!(
        first_spin_target(swept, Some(3), targets.iter().copied()),
        Some(3)
    );
    assert_eq!(
        first_spin_target(swept, None, targets[..2].iter().copied()),
        None
    );
}
