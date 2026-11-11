//! Actual murder notification, with no server or world tick.
use super::*;
use horizon_core::random::LegacyRandom;

#[test]
#[ignore = "requires explicit isolated prepared resources"]
fn murder_uses_only_the_victims_sensor_memory() {
    let mut game = crate::game::kill_die_vibration_tests::game();
    game.mobs.clear();
    let player = [0, 0, 0, 101];
    game.level.player_uuid = Some(player);
    let kind = game
        .res
        .mob_types()
        .get("minecraft:villager")
        .unwrap()
        .clone();
    let mut victim = Mob::new(91, kind.clone(), Vec3::new(1.0, 100.0, 1.0), 0.0);
    game.mobs
        .push(Mob::new(92, kind, Vec3::new(2.0, 100.0, 1.0), 0.0));
    game.villager_murdered(&victim, true, None);
    assert_eq!(
        game.villager_reputation_of(92, player),
        0,
        "a nearby villager absent from the last scan is not a witness"
    );
    let snapshot: Vec<_> = game.mobs.iter().map(|m| m.seen(None)).collect();
    let world = game.mob_world_base(&snapshot);
    let mut random = LegacyRandom::new(0);
    let mut events = Vec::new();
    for _ in 0..20 {
        villager_brain::tick(&mut victim, &world, &mut random, &mut events);
    }
    assert_eq!(
        villager_brain::murder_witnesses(&mut victim, &world),
        vec![92]
    );
    game.villager_murdered(&victim, true, None);
    assert_eq!(game.villager_reputation_of(92, player), -125);
}
