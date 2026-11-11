//! A ridden camel's START_RIDING_JUMP: `handlePlayerCommand` calls
//! `Camel.handleStartJump` when `canJump()` (saddled, not refusing to move)
//! and the jump's data is above zero, and it posts `GameEvent.ENTITY_ACTION`
//! from the camel beside its dash sound
//! (tools/test_camel_jump_parity.py measures the same on vanilla 26.3).
use crate::game::*;
use horizon_client::{
    chunk::Chunk,
    lan::{Joined, NetConfig, PlayerSender, guest::Guest, hub::HubConfig},
    motion::PlayerMotion,
};
use horizon_protocol::packets::configuration::ClientInformation;
use std::sync::{Arc, atomic::AtomicI32, mpsc};

const GUEST: &str = "CamelGuest";

/// A host on a stone floor with one connected, placed guest (conn 2).
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
    for x in 0..2 {
        game.world
            .chunks
            .insert((x, 0), Arc::new(Chunk::empty(x, 0)));
    }
    assert!(game.run_command("fill 0 99 0 31 99 15 stone"));
    game.player = Some(PlayerMotion::new(&game.motion, Vec3::new(4.5, 100.0, 6.5)).unwrap());
    let net = NetConfig {
        announce: false,
        ..NetConfig::lan(0, "camel-jump-test".into(), 2)
    };
    game.lan_host(
        crate::game::lan::LanSettings {
            port: 0,
            guest_commands: true,
            force_game_mode: false,
        },
        net,
        HubConfig {
            default_game_mode: 1,
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
        },
    )
    .unwrap();
    let (tx, _rx) = mpsc::channel();
    let mut guest = Guest::new(
        Joined {
            conn: 2,
            uuid: 2,
            name: GUEST.into(),
            properties: Vec::new(),
            info: ClientInformation::new(2),
            brand: "test".into(),
            sender: PlayerSender::new(tx, Arc::default()),
            latency: Arc::new(AtomicI32::new(0)),
            address: "127.0.0.1:25565".parse().unwrap(),
            expected: false,
            configured: Default::default(),
        },
        2,
        [22.5, 101.0, 9.5],
        1,
    );
    guest.placed = true;
    guest.loaded = true;
    // Its view, as the client sent it: straight down.
    guest.pitch = 90.0;
    game.lan.as_mut().unwrap().hub.guests.push(guest);
    game
}

/// Runs `command`: whether it succeeded and what the issuer was told.
fn run(game: &mut Game<'_>, command: &str) -> (bool, Vec<String>) {
    game.chat_capture = Some(Vec::new());
    let ok = game.run_command(command);
    let said = game.chat_capture.take().unwrap_or_default();
    (
        ok,
        said.iter().map(horizon_client::text::Text::plain).collect(),
    )
}

const SENSOR: &str = "11 100 8";

fn scene(game: &mut Game<'_>, saddled: bool) {
    let gear = if saddled {
        r#",equipment:{saddle:{id:"minecraft:saddle",count:1}}"#
    } else {
        ""
    };
    game.sculk_prepare();
    assert!(game.run_command(&format!("setblock {SENSOR} sculk_sensor")));
    game.sculk_sync_index();
    assert!(game.run_command(&format!(
        "summon camel 8.5 100 8.5 {{NoAI:1b,Silent:1b,UUID:[I;51774,0,0,1]{gear}}}"
    )));
    // A new camel stands up for its first 52 ticks (`isInPoseTransition`), so
    // it refuses to move (`Camel.canJump`) until then.
    assert!(game.run_command("tick freeze"));
    for _ in 0..60 {
        assert!(game.run_command("tick step 1"));
        game.tick_server(false).unwrap();
    }
    let (ok, said) = run(
        game,
        &format!("ride {GUEST} mount 0000ca3e-0000-0000-0000-000000000001"),
    );
    assert!(ok, "{said:?}");
    assert!(game.run_command(&format!(
        "execute as {GUEST} on vehicle if entity @s[type=camel]"
    )));
    assert!(
        !game.run_command(&format!("data get block {SENSOR} listener.selector.event")),
        "the sensor is quiet before the jump"
    );
}

fn jump(game: &mut Game<'_>, data: i32) {
    let mut host = game.lan.take().unwrap();
    game.lan_guest_packet(
        &mut host,
        2,
        horizon_protocol::packets::play::Serverbound::PlayerCommand(
            horizon_protocol::packets::play::PlayerCommand {
                id: 2,
                action: 3,
                data,
            },
        ),
    );
    game.lan = Some(host);
}

fn selected(game: &mut Game<'_>) -> Option<String> {
    let (ok, said) = run(
        game,
        &format!("data get block {SENSOR} listener.selector.event.game_event"),
    );
    ok.then(|| said.concat())
}

#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn a_saddled_camels_riding_jump_posts_entity_action() {
    let mut game = game();
    scene(&mut game, true);
    jump(&mut game, 100);
    // The jump is accepted: Camel.handleStartJump's setDashing(true).
    let camel = game
        .mobs
        .iter()
        .find(|m| m.species() == horizon_client::mobs::Species::Camel)
        .expect("the camel");
    assert!(camel.extra.flag2, "the camel dashes");
    let event = selected(&mut game);
    assert!(
        event
            .as_deref()
            .is_some_and(|e| e.ends_with("\"minecraft:entity_action\"")),
        "the camel's handleStartJump posts entity_action: {event:?}"
    );
}

#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn a_jump_without_power_or_saddle_posts_nothing() {
    let mut game = game();
    scene(&mut game, true);
    jump(&mut game, 0);
    assert_eq!(selected(&mut game), None, "no power");
    let mut game = self::game();
    scene(&mut game, false);
    jump(&mut game, 100);
    assert_eq!(selected(&mut game), None, "no saddle");
}

#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn the_scene_sensor_hears_a_mob_beside_it() {
    let mut game = game();
    scene(&mut game, true);
    assert!(game.run_command("summon pig 11.5 100 10.5 {NoAI:1b,Silent:1b,UUID:[I;51774,0,1,1]}"));
    assert!(game.run_command("damage 0000ca3e-0000-0000-0000-000100000001 1"));
    let event = selected(&mut game);
    assert!(
        event
            .as_deref()
            .is_some_and(|e| e.ends_with("\"minecraft:entity_damage\"")),
        "the sensor is indexed: {event:?}"
    );
}

// Added after the baseline: one entity_action per accepted jump, none for a
// refused one (the game events posted, as `dispense_fixture` records them).

fn posted_actions(game: &mut Game<'_>, jumps: &[i32]) -> usize {
    crate::game::dispense_fixture::start_recording();
    for &data in jumps {
        jump(game, data);
    }
    crate::game::dispense_fixture::stop_recording()
        .iter()
        .filter(|line| line.as_str() == "G 8,100,8 minecraft:entity_action")
        .count()
}

#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn control_each_accepted_jump_posts_one_entity_action() {
    let mut game = game();
    scene(&mut game, true);
    assert_eq!(posted_actions(&mut game, &[0]), 0, "refused: no power");
    assert_eq!(posted_actions(&mut game, &[100]), 1, "one accepted jump");
    assert_eq!(
        posted_actions(&mut game, &[100, 0, 100]),
        2,
        "two accepted, one refused"
    );
    let mut game = self::game();
    scene(&mut game, false);
    assert_eq!(posted_actions(&mut game, &[100]), 0, "refused: no saddle");
}
