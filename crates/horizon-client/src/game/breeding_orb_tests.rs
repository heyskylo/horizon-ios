use super::*;

/// `Animal.finalizeSpawnChildFromBreeding`: one fresh orb of the parent's
/// 1-to-7 roll, never split (4 is not 3 + 1) or merged (#11388).
#[test]
#[ignore = "needs prepared resources (HORIZON_TEST_RESOURCES)"]
fn breeding_drops_one_orb_of_the_whole_roll() {
    let mut game = super::super::native_machines::tests::server_game();
    let kind = game.res.mob_types().get("minecraft:cow").unwrap().clone();
    game.mobs
        .push(Mob::new(7, kind, Vec3::new(8.5, 64.0, 8.5), 0.0));
    let mut values = std::collections::BTreeSet::new();
    for _ in 0..40 {
        let roll = 1 + game.mobs[0].random.clone().next_int_bound(7).unwrap() as u32;
        let orbs = game.orbs.len();
        game.breeding_orb(7, [8.5, 64.0, 8.5]);
        assert_eq!(game.orbs.len(), orbs + 1, "a fresh orb, not merged");
        assert_eq!(
            game.orbs.last().map(|o| (o.points(), o.count)),
            Some((roll, 1))
        );
        values.insert(roll);
    }
    assert!(values.iter().any(|v| matches!(v, 4..=6)), "{values:?}");
}
