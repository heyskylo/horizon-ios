use super::{loaded_motion, position, rotation};
use horizon_core::geometry::Vec3;
use horizon_world::nbt::Tag;

/// `Entity.load` for a record with a saved position: clamp coordinates before
/// rebuilding the box. Reject invalid positions; motion over 10 resets.
pub fn boat_from_nbt(tag: &Tag) -> Option<(Vec3, Vec3, f32)> {
    let p = position(tag)?;
    let p = Vec3::new(
        p.x.clamp(-30_000_512.0, 30_000_512.0),
        p.y.clamp(-20_000_000.0, 20_000_000.0),
        p.z.clamp(-30_000_512.0, 30_000_512.0),
    );
    p.is_finite()
        .then(|| (p, loaded_motion(tag), rotation(tag)[0]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_clamps_each_position_axis_and_only_motion_over_ten() {
        let doubles = |v: [f64; 3]| Tag::List(v.map(|v| Tag::Double(v.to_bits())).to_vec());
        for (raw, expected) in [
            (
                [40_000_000.0, 30_000_000.0, -40_000_000.0],
                [30_000_512.0, 20_000_000.0, -30_000_512.0],
            ),
            (
                [-40_000_000.0, -30_000_000.0, 40_000_000.0],
                [-30_000_512.0, -20_000_000.0, 30_000_512.0],
            ),
            ([1.25, -64.0, -3.5], [1.25, -64.0, -3.5]),
        ] {
            let tag = Tag::compound([
                ("Pos", doubles(raw)),
                ("Motion", doubles([10.0, -10.0, 10.00001])),
            ]);
            let (p, m, _) = boat_from_nbt(&tag).unwrap();
            assert_eq!(p.components(), expected);
            assert_eq!(m, Vec3::new(10.0, -10.0, 0.0));
        }
        let tag = Tag::compound([("Pos", doubles([0.0, f64::NAN, 0.0]))]);
        assert!(boat_from_nbt(&tag).is_none());
    }
}
