use super::*;

/// `ThrownExperienceBottle.onHit` uses `awardWithDirection`: every fresh
/// orb starts 0.25 out from the face it hit, or back along the bottle's
/// motion after an entity hit, moving that way (#11388).
#[test]
#[ignore = "needs prepared resources (HORIZON_TEST_RESOURCES)"]
fn bottle_orbs_start_away_from_what_it_hit() {
    let mut game = super::super::native_machines::tests::server_game();
    let bottle = ItemStack::new("minecraft:experience_bottle", 1);
    let at = Vec3::new(8.5, 70.5, 8.5);
    // (hit face, the bottle's motion, the axis and sign the orbs leave by)
    for (face, motion, axis, sign) in [
        (
            Some(([9, 70, 8], Direction::West)),
            Vec3::new(1.0, 0.0, 0.0),
            0,
            -1.0,
        ),
        (None, Vec3::new(0.0, 0.0, 1.0), 2, -1.0),
    ] {
        game.orbs.clear();
        game.splash(
            &bottle,
            at,
            None,
            face,
            0,
            &PotionOwner::default(),
            false,
            motion,
        );
        assert!(!game.orbs.is_empty());
        let at = [at.x, at.y, at.z];
        for o in game.orbs.iter() {
            let p = [o.position.x, o.position.y, o.position.z];
            let v = [o.velocity.x, o.velocity.y, o.velocity.z];
            assert!((p[axis] - (at[axis] + sign * 0.25)).abs() < 1e-9, "{p:?}");
            assert!(v[axis] * sign >= 0.0, "{v:?}");
        }
    }
}
