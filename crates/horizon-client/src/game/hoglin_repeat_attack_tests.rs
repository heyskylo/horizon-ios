//! A hoglin that retaliates keeps attacking its own attacker when a
//! neighbouring hoglin is hit in the same tick: `HoglinAi.broadcastAttackTarget`
//! gives each visible adult hoglin the nearer of its current target and the
//! new one (`setAttackTargetIfCloserThanCurrent`)
//! (tools/test_hoglin_repeat_attack_parity.py measures the same on vanilla 26.3).
use super::commands::Target;
use super::*;
use horizon_client::{chunk::Chunk, motion::PlayerMotion};
use horizon_core::position::ChunkPos;
use std::sync::Arc;

const NEAR_HOGLIN: &str = "00006093-0000-0000-0000-000000000003";
const NEAR_PIG: &str = "00006093-0000-0000-0000-00000000000d";
const HOGLIN: &str = "00006093-0000-0000-0000-000000000001";
const PIG: &str = "00006093-0000-0000-0000-00000000000b";

fn game() -> Game<'static> {
    let root = std::path::PathBuf::from(
        std::env::var_os("HORIZON_TEST_RESOURCES")
            .expect("set HORIZON_TEST_RESOURCES to prepared resources"),
    );
    let settings = Settings::default();
    let loaded = load_resources(
        root.clone(),
        None,
        &root,
        &settings,
        std::time::Instant::now(),
        false,
        true,
        &Progress::new(),
        &mut |_, _| unreachable!(),
    )
    .unwrap();
    let mut options = Options::parse(std::iter::empty()).unwrap().unwrap();
    options.resources = root.clone();
    options.execution_role = execution_role::ExecutionRole::IntegratedServer;
    let res: &'static _ = Box::leak(Box::new(loaded.resources));
    let mut game = Game::new(
        res,
        loaded.pack,
        loaded.gui.unwrap(),
        options,
        settings,
        Audio::silent(&root),
        None,
    )
    .unwrap();
    game.build_registries();
    game.set_allow_commands(true);
    for cx in 0..2 {
        game.world
            .chunks
            .insert((cx, 0), Arc::new(Chunk::empty(cx, 0)));
        game.block_ticks
            .add_empty_chunk(ChunkPos::new(cx, 0))
            .unwrap();
    }
    for command in [
        "difficulty normal",
        "forceload add 0 0 31 15",
        "fill 0 99 0 31 99 15 warped_nylium",
        "tick freeze",
    ] {
        assert!(game.run_command(command), "{command}");
    }
    // Two glass pens 16 blocks apart, each with a hoglin and a sturdy NoAI pig.
    for (x0, n) in [(0, 3), (16, 1)] {
        assert!(game.run_command(&format!("fill {} 100 4 {} 102 12 glass", x0 + 4, x0 + 10)));
        assert!(game.run_command(&format!("fill {} 100 5 {} 102 11 air", x0 + 5, x0 + 9)));
        assert!(game.run_command(&format!(
            "summon hoglin {}.5 100 8.5 {{PersistenceRequired:1b,IsImmuneToZombification:1b,Health:40f,UUID:[I;24723,0,0,{n}]}}",
            x0 + 6
        )));
        assert!(game.run_command(&format!(
            "summon pig {}.5 100 8.5 {{NoAI:1b,PersistenceRequired:1b,Silent:1b,attributes:[{{id:\"minecraft:max_health\",base:100d}}],Health:100f,UUID:[I;24723,0,0,{}]}}",
            x0 + 7,
            10 + n
        )));
    }
    game
}

fn health(game: &mut Game<'_>, uuid: &str) -> f32 {
    let target = game
        .target_with_uuid(horizon_client::save::uuid_from_text(uuid).unwrap())
        .expect("the entity is in the level");
    match target {
        Target::Mob(id) => game.mobs.iter().find(|m| m.id == id).unwrap().health,
        other => panic!("{other:?} is not a mob"),
    }
}

fn step(game: &mut Game<'_>, ticks: u32) {
    for _ in 0..ticks {
        assert!(game.run_command("tick step 1"));
        game.tick_server(false).unwrap();
    }
}

#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn a_hoglin_keeps_attacking_its_attacker_beside_another_hit_hoglin() {
    let mut game = game();
    step(&mut game, 40);
    // The neighbour's hoglin first, then this one, in the same tick.
    assert!(game.run_command(&format!(
        "damage {NEAR_HOGLIN} 1 minecraft:mob_attack by {NEAR_PIG}"
    )));
    assert!(game.run_command(&format!("damage {HOGLIN} 1 minecraft:mob_attack by {PIG}")));
    step(&mut game, 5);
    let after_first = health(&mut game, PIG);
    assert!(
        after_first < 100.0,
        "the first retaliation hits ({after_first})"
    );
    let near_first = health(&mut game, NEAR_PIG);
    assert!(
        near_first < 100.0,
        "the neighbour retaliates ({near_first})"
    );
    // MeleeAttack's 40-tick cooldown: two more hits are due within 90 ticks.
    step(&mut game, 90);
    let near_later = health(&mut game, NEAR_PIG);
    assert!(
        near_later < near_first,
        "the neighbour hits again ({near_later})"
    );
    let later = health(&mut game, PIG);
    assert!(
        later < after_first,
        "this hoglin hits its attacker again ({later})"
    );
}

/// Added controls (after the frozen baseline above).
fn mob_id(game: &mut Game<'_>, uuid: &str) -> u32 {
    match game.target_with_uuid(horizon_client::save::uuid_from_text(uuid).unwrap()) {
        Some(Target::Mob(id)) => id,
        other => panic!("{other:?}"),
    }
}

/// `setAttackTargetIfCloserThanCurrent` on the receiver: pacified ignores
/// the broadcast; a nearer current target stays; a nearer (or equally near)
/// incoming one replaces a farther one; none current takes the new one.
#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn a_broadcast_target_replaces_only_a_farther_one() {
    use horizon_client::mobs::{Who, piglin::receive_attack_target};
    let mut game = game();
    let receiver = mob_id(&mut game, HOGLIN);
    let (near, far) = (mob_id(&mut game, PIG), mob_id(&mut game, NEAR_PIG));
    let place = |game: &Game<'_>, id: u32| game.mobs.iter().find(|m| m.id == id).unwrap().position;
    let (near_at, far_at) = (place(&game, near), place(&game, far));
    let m = game.mobs.iter_mut().find(|m| m.id == receiver).unwrap();
    // No current target: the broadcast's is taken.
    m.ai.target = None;
    assert!(receive_attack_target(m, Who::Mob(far), None, Some(far_at)));
    assert_eq!(m.ai.target, Some(Who::Mob(far)), "no current target");
    // Pacified: nothing changes.
    m.ai.target = None;
    m.extra.ints[0] = 200;
    assert!(!receive_attack_target(
        m,
        Who::Mob(near),
        None,
        Some(near_at)
    ));
    assert_eq!(m.ai.target, None, "pacified");
    m.extra.ints[0] = 0;
    // A nearer current target stays.
    m.ai.target = Some(Who::Mob(near));
    receive_attack_target(m, Who::Mob(far), Some(near_at), Some(far_at));
    assert_eq!(m.ai.target, Some(Who::Mob(near)), "nearer current");
    // A nearer incoming target replaces a farther current one.
    m.ai.target = Some(Who::Mob(far));
    receive_attack_target(m, Who::Mob(near), Some(far_at), Some(near_at));
    assert_eq!(m.ai.target, Some(Who::Mob(near)), "nearer incoming");
    // A tie takes the incoming one (`getTargetNearestMe`: current only if strictly nearer).
    m.ai.target = Some(Who::Mob(far));
    receive_attack_target(m, Who::Mob(near), Some(near_at), Some(near_at));
    assert_eq!(m.ai.target, Some(Who::Mob(near)), "tie");
    // A current target that is not in the level counts as absent; an
    // incoming one that is not keeps the current one.
    m.ai.target = Some(Who::Mob(far));
    receive_attack_target(m, Who::Mob(near), None, Some(near_at));
    assert_eq!(m.ai.target, Some(Who::Mob(near)), "current gone");
    receive_attack_target(m, Who::Mob(far), Some(near_at), None);
    assert_eq!(m.ai.target, Some(Who::Mob(near)), "incoming gone");
}

/// The broadcast as the flush applies it (`MobEvent::HoglinAttackTarget`).
fn broadcast(game: &mut Game<'_>, mob: u32, from: u32, target: horizon_client::mobs::Who) {
    let event = horizon_client::mobs::MobEvent::HoglinAttackTarget { mob, from, target };
    game.handle_mob_events(vec![event]).unwrap();
}

fn target_of(game: &Game<'_>, id: u32) -> Option<horizon_client::mobs::Who> {
    game.mobs.iter().find(|m| m.id == id).unwrap().ai.target
}

fn set_target(game: &mut Game<'_>, id: u32, target: Option<horizon_client::mobs::Who>) {
    game.mobs.iter_mut().find(|m| m.id == id).unwrap().ai.target = target;
}

fn place_player(game: &mut Game<'_>, x: f64, z: f64) {
    game.player = Some(PlayerMotion::new(game.motion, Vec3::new(x, 100.0, z)).unwrap());
}

/// A player on either side is placed and compared like a mob
/// (`getTargetNearestMe` compares positions, whoever the targets are).
#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn a_broadcast_player_is_compared_by_distance() {
    use horizon_client::mobs::Who;
    let mut game = game();
    let receiver = mob_id(&mut game, HOGLIN);
    let from = mob_id(&mut game, NEAR_HOGLIN);
    // The receiver at x 22.5, its pig at 23.5, the other pen's pig at 7.5.
    let (pig, other_pig) = (mob_id(&mut game, PIG), mob_id(&mut game, NEAR_PIG));
    // The player 2 blocks away beats the pig 15 blocks away.
    place_player(&mut game, 22.5, 10.5);
    set_target(&mut game, receiver, Some(Who::Mob(other_pig)));
    broadcast(&mut game, receiver, from, Who::Player);
    assert_eq!(
        target_of(&game, receiver),
        Some(Who::Player),
        "a nearer player"
    );
    // A current player 2 blocks away stays against the pig 15 blocks away.
    broadcast(&mut game, receiver, from, Who::Mob(other_pig));
    assert_eq!(
        target_of(&game, receiver),
        Some(Who::Player),
        "a nearer current player"
    );
    // The player 21 blocks away loses to the pig 1 block away, either way round.
    place_player(&mut game, 2.5, 2.5);
    broadcast(&mut game, receiver, from, Who::Mob(pig));
    assert_eq!(
        target_of(&game, receiver),
        Some(Who::Mob(pig)),
        "a farther current player"
    );
    broadcast(&mut game, receiver, from, Who::Player);
    assert_eq!(
        target_of(&game, receiver),
        Some(Who::Mob(pig)),
        "a farther player"
    );
}

/// A host on LAN with one placed guest (conn 2) at `guest_at`.
fn host_with_guest(game: &mut Game<'_>, guest_at: [f64; 3]) {
    use horizon_client::lan::{Joined, NetConfig, PlayerSender, guest::Guest, hub::HubConfig};
    use horizon_protocol::packets::configuration::ClientInformation;
    let net = NetConfig {
        announce: false,
        ..NetConfig::lan(0, "hoglin-broadcast-test".into(), 2)
    };
    let settings = super::lan::LanSettings {
        port: 0,
        guest_commands: true,
        force_game_mode: false,
    };
    let hub = HubConfig {
        default_game_mode: 0,
        guest_commands: true,
        force_game_mode: false,
        hardcore: false,
        view_distance: 2,
        simulation_distance: 2,
        dimension: Dim::Overworld.id().into(),
        allow_flight: false,
        seed: 0,
        max_players: 2,
        integrated: false,
        levels: Default::default(),
    };
    game.lan_host(settings, net, hub).unwrap();
    let (tx, _rx) = std::sync::mpsc::channel();
    let joined = Joined {
        conn: 2,
        uuid: 2,
        name: "HoglinGuest".into(),
        properties: Vec::new(),
        info: ClientInformation::new(2),
        brand: "test".into(),
        sender: PlayerSender::new(tx, Arc::default()),
        latency: Arc::new(std::sync::atomic::AtomicI32::new(0)),
        address: "127.0.0.1:25565".parse().unwrap(),
        expected: false,
        configured: Default::default(),
    };
    let mut guest = Guest::new(joined, 2, guest_at, 0);
    guest.placed = true;
    guest.loaded = true;
    game.lan.as_mut().unwrap().hub.guests.push(guest);
}

/// On LAN, `Who::Player` is each mob's own player (`mob_players`): a hoglin
/// that a guest's attacker is broadcast to compares that guest, not the
/// host, and once it takes the guest it is bound to them.
#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn a_guest_broadcast_is_placed_as_the_guest() {
    use horizon_client::mobs::Who;
    let mut game = game();
    // The host 21 blocks from the receiver, the guest 2 blocks.
    place_player(&mut game, 2.5, 2.5);
    host_with_guest(&mut game, [22.5, 100.0, 10.5]);
    let receiver = mob_id(&mut game, HOGLIN);
    let from = mob_id(&mut game, NEAR_HOGLIN);
    let other_pig = mob_id(&mut game, NEAR_PIG);
    let bound = |game: &Game<'_>| {
        let host = game.lan.as_ref().unwrap();
        host.mob_players.get(&receiver).copied()
    };
    // From a hoglin that sees the host: the host is farther than the pig 15 blocks away.
    set_target(&mut game, receiver, Some(Who::Mob(other_pig)));
    broadcast(&mut game, receiver, from, Who::Player);
    assert_eq!(
        target_of(&game, receiver),
        Some(Who::Mob(other_pig)),
        "the host is farther"
    );
    assert_eq!(bound(&game), None);
    // From a hoglin bound to the guest: the guest is nearer, and the receiver
    // now sees that guest.
    game.lan.as_mut().unwrap().mob_players.insert(from, 2);
    broadcast(&mut game, receiver, from, Who::Player);
    assert_eq!(
        target_of(&game, receiver),
        Some(Who::Player),
        "the guest is nearer"
    );
    assert_eq!(bound(&game), Some(2), "bound to the guest");
}

/// The broadcast's receivers are the sensed visible adult hoglins
/// (`NEAREST_VISIBLE_ADULT_HOGLINS`): not a baby, not one behind stone.
#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn the_sensed_receivers_are_visible_adult_hoglins() {
    let mut game = game();
    // Inside this hoglin's pen: an adult and a baby. Beyond a stone wall: an adult.
    assert!(game.run_command(
        "summon hoglin 24.5 100 10.5 {PersistenceRequired:1b,IsImmuneToZombification:1b,NoAI:1b,UUID:[I;24723,0,0,6]}"
    ));
    assert!(game.run_command(
        "summon hoglin 24.5 100 6.5 {PersistenceRequired:1b,IsImmuneToZombification:1b,Age:-24000,UUID:[I;24723,0,0,4]}"
    ));
    assert!(game.run_command("fill 27 100 0 27 104 15 stone"));
    assert!(game.run_command(
        "summon hoglin 29.5 100 8.5 {PersistenceRequired:1b,IsImmuneToZombification:1b,NoAI:1b,UUID:[I;24723,0,0,5]}"
    ));
    step(&mut game, 40);
    let receiver = mob_id(&mut game, HOGLIN);
    let near = mob_id(&mut game, "00006093-0000-0000-0000-000000000006");
    let baby = mob_id(&mut game, "00006093-0000-0000-0000-000000000004");
    let walled = mob_id(&mut game, "00006093-0000-0000-0000-000000000005");
    let adults = &game
        .mobs
        .iter()
        .find(|m| m.id == receiver)
        .unwrap()
        .extra
        .piglin
        .adults;
    assert!(
        adults.contains(&near),
        "the visible adult neighbour: {adults:?}"
    );
    assert!(!adults.contains(&baby), "not a baby: {adults:?}");
    assert!(
        !adults.contains(&walled),
        "not one behind stone: {adults:?}"
    );
}
