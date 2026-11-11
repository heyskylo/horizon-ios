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

/// `Turtle.TurtleBreedGoal.breed` overrides `Animal.breed`, so a bred turtle
/// never runs `finalizeSpawnChildFromBreeding`'s heart burst (entity event
/// 18); `Frog.spawnChildFromBreeding` still does.
#[test]
#[ignore = "needs prepared resources (HORIZON_TEST_RESOURCES)"]
fn a_bred_turtle_posts_no_heart_burst_but_a_bred_frog_does() {
    let mut game = super::super::native_machines::tests::server_game();
    game.audio.capture = Some(Vec::new());
    for (kind_id, hearts) in [("minecraft:turtle", 0), ("minecraft:frog", 1)] {
        let kind = game.res.mob_types().get(kind_id).unwrap().clone();
        game.mobs
            .push(Mob::new(1, kind.clone(), Vec3::new(8.5, 64.0, 8.5), 0.0));
        game.mobs
            .push(Mob::new(2, kind, Vec3::new(9.5, 64.0, 8.5), 0.0));
        game.lan_entity_events.clear();
        game.breed(1, 2, None);
        let burst = game
            .lan_entity_events
            .iter()
            .filter(|(id, event, _)| *id == 1 && *event == 18)
            .count();
        assert_eq!(
            burst, hearts,
            "{kind_id} heartbeat events: {:?}",
            game.lan_entity_events
        );
        game.mobs.clear();
        game.orbs.clear();
    }
}
