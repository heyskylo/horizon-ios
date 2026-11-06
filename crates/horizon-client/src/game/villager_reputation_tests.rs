//! Isolated game glue: no listener, window, audio output or server thread.
use super::*;
use crate::game::{lan, native_machines, trading::MerchantState};
use horizon_client::{
    effects::EffectInstance,
    lan::{
        Lan, NetConfig,
        chunks::ChunkEncoder,
        data::SyncData,
        guest::Guest,
        hub::{Hub, HubConfig},
    },
    merchant::{ItemCost, MerchantOffer, Session},
    mobs::{Damage, Who},
};
use horizon_core::random::LegacyRandom;
use std::sync::Arc;

fn hub(root: &std::path::Path) -> Hub {
    let mut sync = SyncData::empty();
    sync.builtin = horizon_protocol::registry::Builtin::load(
        &horizon_protocol::registry::Builtin::find(root).expect("prepared wire registries"),
    )
    .unwrap();
    let sync = Arc::new(sync);
    let net = NetConfig {
        listen: false,
        announce: false,
        ..NetConfig::lan(0, "reputation-fixture".into(), 2)
    };
    let transport = Lan::start_with(sync.clone(), net).unwrap();
    assert_eq!(transport.port, 0, "fixture never binds a listener");
    let encoder = ChunkEncoder::new([] as [(Arc<str>, bool); 0], &sync.builtin, &[], &[]);
    Hub::new(
        transport,
        sync,
        encoder,
        HubConfig {
            default_game_mode: 0,
            guest_commands: true,
            force_game_mode: false,
            hardcore: false,
            view_distance: 8,
            simulation_distance: 8,
            dimension: "minecraft:overworld".into(),
            allow_flight: false,
            seed: 0,
            max_players: 2,
            integrated: false,
            levels: Default::default(),
        },
    )
}
fn guest(conn: u32, uuid: u128) -> (Guest, std::sync::mpsc::Receiver<horizon_client::lan::Out>) {
    let (tx, rx) = std::sync::mpsc::channel();
    let mut g = Guest::new(
        horizon_client::lan::Joined {
            conn,
            uuid,
            name: format!("fixture-{conn}"),
            properties: Vec::new(),
            info: horizon_protocol::packets::configuration::ClientInformation::new(8),
            brand: "vanilla".into(),
            sender: horizon_client::lan::PlayerSender::new(tx, Arc::default()),
            latency: Arc::new(std::sync::atomic::AtomicI32::new(0)),
            address: "127.0.0.1:1".parse().unwrap(),
            expected: false,
        },
        conn as i32,
        [0.5, 1.0, 0.5],
        8,
    );
    g.placed = true;
    g.dimension = "minecraft:overworld".into();
    g.permission = 4;
    (g, rx)
}
fn offer() -> MerchantOffer {
    MerchantOffer {
        base_cost_a: ItemCost {
            stack: ItemStack::new("minecraft:emerald", 20),
        },
        cost_b: None,
        result: ItemStack::new("minecraft:bread", 1),
        uses: 0,
        max_uses: 12,
        reward_exp: false,
        special_price_diff: 0,
        demand: 0,
        price_multiplier: 0.2,
        xp: 1,
    }
}
fn state() -> MerchantState {
    let mut session = Session::new(vec![offer()], true);
    session.hero = Some(0);
    session.update_special_prices();
    MerchantState {
        session: Box::new(session),
        scroll: 0,
        dragging: false,
        focused: None,
        mob: Some(91),
        name: None,
    }
}
fn price(host: &lan::LanHost) -> (i32, Option<i32>, i32) {
    let s = &host.play[&1].merchant.as_ref().unwrap().merchant.session;
    (s.reputation, s.hero, s.offers[0].special_price_diff)
}
fn villager(game: &Game<'_>, id: u32) -> Mob {
    Mob::new(
        id,
        game.res
            .mob_types()
            .get("minecraft:villager")
            .unwrap()
            .clone(),
        Vec3::new(0.5, 1.0, 0.5),
        0.0,
    )
}
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
    let mut game = native_machines::tests::server_game();
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
    let mut host = lan::reputation_test_host(hub(&game.options.resources));
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
