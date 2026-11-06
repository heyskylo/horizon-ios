//! Identical stock-compatible accepted-hit regression for baseline and fixed trees.
use super::reputation_fixture as fixture;
use horizon_client::effects::EffectInstance;

#[test]
#[ignore = "requires explicit prepared resources and isolated fixture home"]
fn every_accepted_guest_hurt_survives_one_reputation_phase_in_the_same_tick() {
    let mut game = fixture::game();
    let owner = [0, 0, 0, 101];
    let a = [0, 0, 0, 121];
    let b = [0, 0, 0, 122];
    game.level.player_uuid = Some(owner);
    game.world.game_time = 40;
    game.world.day_time = 40;
    let mut mob = fixture::villager(&game, 91);
    mob.age = 40;
    mob.extra.flag2 = true;
    mob.extra.trade.get_or_insert_with(Default::default).offers = vec![fixture::offer()];
    let initial_health = mob.health;
    game.mobs.push(mob);
    let mut host = fixture::host(&game.options.resources);
    let (one, _one_output) = fixture::guest(1, 121);
    let (two, _two_output) = fixture::guest(2, 122);
    host.hub.guests.extend([one, two]);
    host.hub.guests[0]
        .effects
        .add(EffectInstance::new("hero_of_the_village", 400, 0));
    game.lan_open_merchant(&mut host, 1, fixture::state());
    assert_eq!(fixture::price(&host), (0, Some(0), -6));
    host.hub.guests[0]
        .effects
        .add(EffectInstance::new("hero_of_the_village", 400, 1));
    for (conn, amount) in [(2, 1), (1, 2), (2, 3)] {
        let line = format!(
            "damage @e[type=minecraft:villager,limit=1] {amount} minecraft:player_attack by @s"
        );
        assert_eq!(
            game.as_guest(&mut host, conn, |g| g.run_command(&line)),
            Some(true),
            "accepted command control: {line}"
        );
        assert_eq!(
            game.mobs[0].health,
            initial_health - amount as f32,
            "accepted damage control"
        );
        assert_eq!(game.world.game_time, 40);
        assert_eq!(game.mobs[0].age, 40);
        if amount == 1 {
            assert_eq!(
                fixture::price(&host),
                (0, Some(0), -6),
                "other attacker must not refresh this trader"
            );
        }
    }
    // Exactly one production reputation phase after the accepted command burst.
    game.lan = Some(host);
    game.villager_reputation_tick();
    let host = game.lan.take().unwrap();
    assert_eq!(game.world.game_time, 40);
    assert_eq!(game.mobs[0].age, 40);
    assert_eq!(
        game.villager_reputation_of(91, b),
        -50,
        "both accepted guest-B hurts must survive the one phase"
    );
    assert_eq!(
        game.villager_reputation_of(91, a),
        -25,
        "the other accepted attacker must survive too"
    );
    assert_eq!(game.villager_reputation_of(91, owner), 0);
    assert_eq!(fixture::price(&host), (-25, Some(1), -2));
}
