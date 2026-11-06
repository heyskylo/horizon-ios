//! Isolated game glue: no listener, window, audio output or server thread.
use super::*;
use crate::game::lan::reputation_fixture::{
    game as fixture_game, guest, host as fixture_host, offer, price, state, villager,
};
use horizon_client::{
    effects::EffectInstance,
    mobs::{Damage, Who},
};
use horizon_core::random::LegacyRandom;

fn victim_with_memory(game: &Game<'_>, id: u32) -> Mob {
    let mut victim = villager(game, id);
    let snapshot: Vec<_> = game.mobs.iter().map(|m| m.seen(None)).collect();
    let world = game.mob_world_base(&snapshot);
    let mut random = LegacyRandom::new(0);
    let mut events = Vec::new();
    for _ in 0..20 {
        villager_brain::tick(&mut victim, &world, &mut random, &mut events);
    }
    assert!(villager_brain::murder_witnesses(&mut victim, &world).contains(&91));
    victim
}
#[test]
#[ignore = "requires isolated prepared resources in HORIZON_TEST_RESOURCES"]
fn guest_damage_refreshes_only_its_current_trade_with_restored_hero_and_keeps_removed_attacker_uuid()
 {
    let oracle = std::fs::read_to_string(
        std::env::var_os("HORIZON_REPUTATION_ORACLE").expect("actual vanilla output"),
    )
    .unwrap();
    let prices: Vec<i32> = oracle
        .lines()
        .find(|line| line.starts_with("prices "))
        .unwrap()
        .split_whitespace()
        .skip(1)
        .map(|s| s.parse().unwrap())
        .collect();
    let mut game = fixture_game();
    let host_uuid = [0, 0, 0, 101];
    let a = [0, 0, 0, 121];
    let b = [0, 0, 0, 122];
    game.level.player_uuid = Some(host_uuid);
    let mut merchant = villager(&game, 91);
    merchant
        .extra
        .trade
        .get_or_insert_with(Default::default)
        .offers = vec![offer()];
    merchant.extra.flag2 = true;
    game.mobs.push(merchant);
    let mut host = fixture_host(&game.options.resources);
    let (one, _rx_one) = guest(1, 121);
    let (two, _rx_two) = guest(2, 122);
    host.hub.guests.extend([one, two]);
    host.hub.guests[0]
        .effects
        .add(EffectInstance::new("hero_of_the_village", 400, 0));
    game.lan_open_merchant(&mut host, 1, state());
    assert_eq!(price(&host), (0, Some(0), prices[0]));
    host.hub.guests[0]
        .effects
        .add(EffectInstance::new("hero_of_the_village", 400, 1));
    assert_eq!(
        game.as_guest(&mut host, 2, |g| g.run_command(
            "damage @e[type=minecraft:villager,limit=1] 1 minecraft:player_attack by @s"
        )),
        Some(true)
    );
    assert_eq!(game.villager_reputation_of(91, b), -25);
    assert_eq!(game.villager_reputation_of(91, host_uuid), 0);
    assert_eq!(
        price(&host),
        (0, Some(0), prices[1]),
        "another guest must not refresh this trader's Hero or prices"
    );
    assert_eq!(
        game.as_guest(&mut host, 1, |g| {
            assert_eq!(
                g.hero_amplifier(),
                Some(1),
                "effects are swapped into the acting body"
            );
            g.run_command(
                "damage @e[type=minecraft:villager,limit=1] 2 minecraft:player_attack by @s",
            )
        }),
        Some(true)
    );
    assert_eq!(game.villager_reputation_of(91, a), -25);
    assert_eq!(price(&host), (-25, Some(1), prices[2]));
    assert_eq!(
        game.as_guest(&mut host, 2, |g| g.run_command(
            "damage @e[type=minecraft:villager,limit=1] 3 minecraft:player_attack by @s"
        )),
        Some(true)
    );
    assert_eq!(price(&host), (-25, Some(1), prices[3]));

    let mob_uuid = [0, 0, 7, 19];
    let zombie = Mob::new(
        19,
        game.res
            .mob_types()
            .get("minecraft:zombie")
            .unwrap()
            .clone(),
        Vec3::new(2.0, 1.0, 0.5),
        0.0,
    );
    game.mobs.push(zombie);
    game.persist.uuids.insert(19, mob_uuid);
    let captured = game.damage_attacker_uuid(Some(Who::Mob(19)), None).unwrap();
    game.mobs.retain(|m| m.id != 19);
    game.forget_entity(19);
    assert_eq!(game.damage_attacker_uuid(Some(Who::Mob(19)), None), None);
    assert!(!game.persist.uuids.contains_key(&19));
    game.lan = Some(host);
    let mut victim = victim_with_memory(&game, 93);
    assert!(
        victim.damage(
            100.0,
            Damage::indirect_magic(Some(Who::Mob(19)), [2.0, 1.0, 0.5])
                .with_attacker_uuid(Some(captured))
        )
    );
    assert_eq!(
        villager::ext(&victim).unwrap().gossips.reputation(mob_uuid),
        -25
    );
    game.villager_murdered(&victim);
    assert_eq!(game.villager_reputation_of(91, mob_uuid), -125);
    assert!(
        !game.persist.uuids.contains_key(&19),
        "a departed attacker never receives a fabricated UUID"
    );
    assert_eq!(price(game.lan.as_ref().unwrap()), (-25, Some(1), prices[4]));
    let mut victim = victim_with_memory(&game, 94);
    assert!(victim.damage(
        100.0,
        Damage::player([2.0, 1.0, 0.5]).with_attacker_uuid(Some(a))
    ));
    game.villager_murdered(&victim);
    assert_eq!(game.villager_reputation_of(91, a), -150);
    assert_eq!(
        price(game.lan.as_ref().unwrap()),
        (-150, Some(1), prices[5])
    );
    let cap: Vec<i32> = oracle
        .lines()
        .find(|line| line.starts_with("cap "))
        .unwrap()
        .split_whitespace()
        .skip(1)
        .map(|s| s.parse().unwrap())
        .collect();
    let mut host = game.lan.take().unwrap();
    for amount in 4..=10 {
        assert_eq!(
            game.as_guest(&mut host, 1, |g| g.run_command(&format!(
                "damage @e[type=minecraft:villager,limit=1] {amount} minecraft:player_attack by @s"
            ))),
            Some(true)
        );
    }
    assert_eq!(game.villager_reputation_of(91, a), cap[0]);
    assert_eq!(price(&host), (cap[0], Some(1), cap[1]));
    host.hub.guests[0]
        .effects
        .add(EffectInstance::new("hero_of_the_village", 400, 2));
    assert_eq!(
        game.as_guest(&mut host, 1, |g| g.run_command(
            "damage @e[type=minecraft:villager,limit=1] 11 minecraft:player_attack by @s"
        )),
        Some(true)
    );
    assert_eq!(
        game.villager_reputation_of(91, a),
        cap[0],
        "gossip is already capped"
    );
    assert_eq!(
        price(&host),
        (cap[0], Some(2), cap[2]),
        "the accepted event still refreshes current Hero at the gossip cap"
    );
    let nested: Vec<i32> = oracle
        .lines()
        .find(|line| line.starts_with("nested "))
        .unwrap()
        .split_whitespace()
        .skip(1)
        .map(|s| s.parse().unwrap())
        .collect();
    host.hub.guests[0]
        .effects
        .add(EffectInstance::new("hero_of_the_village", 400, 3));
    let outer = game.enter_guest(&mut host, 1, game.dim).unwrap();
    // Some command paths temporarily put the hub back while a guest is borrowed.
    game.lan = Some(host);
    assert!(game.run_command(
        "damage @e[type=minecraft:villager,limit=1] 12 minecraft:player_attack by @s"
    ));
    let mut host = game.lan.take().unwrap();
    assert!(game.pending_villager_reputation.contains(&91));
    assert_eq!(game.as_guest(&mut host, 2, |_| ()), Some(()));
    assert_eq!(game.lan_acting, Some(1));
    assert_eq!(game.hero_amplifier(), Some(3));
    assert!(
        host.play.get(&1).is_none(),
        "outer merchant is still borrowed"
    );
    assert_eq!(
        host.hub.guests[0].effects.amplifier("hero_of_the_village"),
        None
    );
    assert!(
        game.pending_villager_reputation.contains(&91),
        "an inner return must not consume outer notifications"
    );
    assert_eq!(
        villager::ext(&game.mobs[0]).unwrap().reputation_events,
        vec![a]
    );
    game.leave_guest(&mut host, outer);
    assert!(game.pending_villager_reputation.is_empty());
    assert!(
        villager::ext(&game.mobs[0])
            .unwrap()
            .reputation_events
            .is_empty()
    );
    assert_eq!(price(&host), (nested[0], Some(3), nested[1]));
}
