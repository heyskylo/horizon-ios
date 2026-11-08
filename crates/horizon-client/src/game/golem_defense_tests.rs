//! An iron golem defends a villager from the player it selects:
//! `DefendVillageTargetGoal.canUse` asks the villagers in its box for the
//! reputation of each player in that box (`Villager.getPlayerReputation`,
//! the gossips about that player's UUID), and a player at -100 or below
//! becomes the target (tools/test_golem_defense_guest_parity.py measures the
//! same on vanilla 26.3 with two real players).
use super::*;
use horizon_client::{
    chunk::Chunk,
    lan::{Joined, NetConfig, PlayerSender, guest::Guest, hub::HubConfig},
    motion::PlayerMotion,
};
use horizon_protocol::packets::configuration::ClientInformation;
use std::sync::{Arc, atomic::AtomicI32, mpsc};

const GUEST: &str = "GolemGuest";

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
    game.player = Some(PlayerMotion::new(game.motion, Vec3::new(28.5, 100.0, 10.5)).unwrap());
    let net = NetConfig {
        announce: false,
        ..NetConfig::lan(0, "golem-defense-test".into(), 2)
    };
    game.lan_host(
        super::lan::LanSettings {
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
        [12.5, 100.0, 10.5],
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

const VILLAGER: &str = "00000901-0000-0000-0000-000000000002";

/// The golem, and a villager whose gossips put the guest (UUID 2) at `value`
/// times -5; the host player stands out of the golem's box.
fn scene(game: &mut Game<'_>, value: i32) {
    for command in [
        "summon iron_golem 10.5 100 10.5 {PersistenceRequired:1b,PlayerCreated:0b,UUID:[I;2305,0,0,1]}"
            .to_owned(),
        format!(
            "summon villager 14.5 100 12.5 {{NoAI:1b,PersistenceRequired:1b,UUID:[I;2305,0,0,2],\
             Gossips:[{{Type:\"major_negative\",Value:{value},Target:[I;0,0,0,2]}}]}}"
        ),
        format!("gamemode survival {GUEST}"),
        "tick freeze".to_owned(),
    ] {
        let (ok, said) = run(game, &command);
        assert!(ok, "{command}: {said:?}");
    }
    let (_, gossips) = run(game, &format!("data get entity {VILLAGER} Gossips"));
    assert!(gossips.concat().contains("[I; 0, 0, 0, 2]"), "{gossips:?}");
}

/// Whether the golem targets the guest after `ticks` server ticks.
fn targets_guest(game: &mut Game<'_>, ticks: usize) -> bool {
    for _ in 0..ticks {
        assert!(game.run_command("tick step 1"));
        game.tick_server(false).unwrap();
    }
    let id = game
        .mobs
        .iter()
        .find(|m| m.species() == horizon_client::mobs::Species::IronGolem)
        .expect("the golem")
        .id;
    let golem = game.mobs.iter().find(|m| m.id == id).unwrap();
    golem.ai.target == Some(horizon_client::mobs::Who::Player)
        && game.lan.as_ref().unwrap().mob_players.get(&id) == Some(&2)
}

#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn a_golem_targets_the_guest_its_villager_hates() {
    let mut game = game();
    scene(&mut game, 20);
    assert!(
        targets_guest(&mut game, 40),
        "a reputation of -100 makes the selected guest the target"
    );
}

#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn a_golem_leaves_a_guest_at_minus_95_alone() {
    let mut game = game();
    scene(&mut game, 19);
    assert!(!targets_guest(&mut game, 40), "-95 is above the threshold");
}

#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn control_with_the_guest_as_the_level_player_the_golem_targets_it() {
    // The same scene, the mobs' reputation snapshot keyed by the guest: the
    // goal itself works, only the identity it reads differs.
    let mut game = game();
    game.level.player_uuid = Some([0, 0, 0, 2]);
    scene(&mut game, 20);
    assert!(targets_guest(&mut game, 40));
}

// Added after the baseline: the converse identities. The level's player (the
// host, UUID 1) is hated and the selected guest is not: no defence. Then the
// host is the nearest player: its own gossip makes it the target.
#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn control_a_hated_host_does_not_turn_the_golem_on_the_selected_guest() {
    let mut game = game();
    game.level.player_uuid = Some([0, 0, 0, 1]);
    for command in [
        "summon iron_golem 10.5 100 10.5 {PersistenceRequired:1b,PlayerCreated:0b,UUID:[I;2305,0,0,1]}"
            .to_owned(),
        "summon villager 14.5 100 12.5 {NoAI:1b,PersistenceRequired:1b,UUID:[I;2305,0,0,2],\
         Gossips:[{Type:\"major_negative\",Value:20,Target:[I;0,0,0,1]},\
         {Type:\"minor_positive\",Value:1,Target:[I;0,0,0,2]}]}"
            .to_owned(),
        format!("gamemode survival {GUEST}"),
        "tick freeze".to_owned(),
    ] {
        let (ok, said) = run(&mut game, &command);
        assert!(ok, "{command}: {said:?}");
    }
    assert!(
        !targets_guest(&mut game, 40),
        "the selected guest is not hated"
    );
    // The guest leaves; the host, hated, is the player the golem now sees.
    assert!(game.run_command(&format!("tp {GUEST} 28.5 100 3.5")));
    if let Some(p) = game.player.as_mut() {
        p.position = Vec3::new(12.5, 100.0, 10.5);
    }
    for _ in 0..40 {
        assert!(game.run_command("tick step 1"));
        game.tick_server(false).unwrap();
    }
    let golem = game
        .mobs
        .iter()
        .find(|m| m.species() == horizon_client::mobs::Species::IronGolem)
        .unwrap();
    assert_eq!(golem.ai.target, Some(horizon_client::mobs::Who::Player));
    assert_ne!(
        game.lan.as_ref().unwrap().mob_players.get(&golem.id),
        Some(&2),
        "the target is the host, not the guest"
    );
}
