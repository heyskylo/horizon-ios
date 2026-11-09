//! `TestCommand.showPos` under `execute as <guest>`: the source player is
//! the guest, and its own `pick` (eyes and view) finds the block, while the
//! message goes to the command's issuer
//! (tools/test_test_pos_guest_parity.py measures the same on vanilla 26.3).
use super::*;
use horizon_client::{
    chunk::Chunk,
    lan::{Joined, NetConfig, PlayerSender, guest::Guest, hub::HubConfig},
    motion::PlayerMotion,
};
use horizon_protocol::packets::configuration::ClientInformation;
use std::sync::{Arc, atomic::AtomicI32, mpsc};

const GUEST: &str = "H274Guest";

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
        ..NetConfig::lan(0, "h274-test".into(), 2)
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

#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn test_pos_as_a_guest_uses_the_guest_s_view() {
    let mut game = game();
    // One test box, under the guest (TestCommand.createTestPositionAround:
    // around the host, who stands there for it).
    game.player.as_mut().unwrap().position = Vec3::new(16.5, 100.0, 0.5);
    assert!(run(&mut game, "test create minecraft:always_pass 10 10 10").0);
    // The host stands away from it, looking straight down.
    game.player.as_mut().unwrap().position = Vec3::new(4.5, 101.0, 6.5);
    (game.yaw, game.pitch) = (0.0, 90.0);
    let host = run(&mut game, "test pos");
    assert!(!host.0, "the host's own view finds no test: {host:?}");
    // As the guest: the guest's view finds its box, and the issuer is told.
    assert_eq!(
        run(&mut game, &format!("execute as {GUEST} run test pos")),
        (
            true,
            vec!["Position relative to minecraft:always_pass: 6, 0, 5".to_owned()]
        )
    );
    // The host's place and view are its own again.
    let p = game.player.as_ref().unwrap().position;
    assert_eq!(
        ([p.x, p.y, p.z], game.yaw, game.pitch),
        ([4.5, 101.0, 6.5], 0.0, 90.0)
    );
    let g = game.lan.as_ref().unwrap().hub.guest(2).unwrap();
    assert_eq!((g.position, g.pitch), ([22.5, 101.0, 9.5], 90.0));
    // At the guest, as the host: still the host's view.
    // (execute reports no failure of its own: the issuer is just told nothing.)
    assert_eq!(
        run(&mut game, &format!("execute at {GUEST} run test pos")).1,
        Vec::<String>::new()
    );
}

/// Added controls (after the baseline): a source with no entity, a missing
/// connected player and a guest whose view finds no test are refused as
/// before, and every place and view is back afterwards.
#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn test_pos_refuses_other_sources_and_restores_the_views() {
    let mut game = game();
    game.player.as_mut().unwrap().position = Vec3::new(16.5, 100.0, 0.5);
    assert!(run(&mut game, "test create minecraft:always_pass 10 10 10").0);
    game.player.as_mut().unwrap().position = Vec3::new(4.5, 101.0, 6.5);
    (game.yaw, game.pitch) = (0.0, 90.0);
    let key = |r: commands::R<i32>| format!("{:?}", r.unwrap_err());
    let requires = "permissions.requires.player";
    // An explicit source with no entity (the server's).
    let server = game.server_source();
    assert!(key(game.execute_line("test pos", server)).contains(requires));
    // A connected player that is not there.
    let mut missing = game.player_source();
    missing.entity = Some(commands::Target::Guest(99));
    assert!(key(game.execute_line("test pos", missing)).contains(requires));
    // The guest looking up: its own view finds no test.
    game.lan.as_mut().unwrap().hub.guest_mut(2).unwrap().pitch = -90.0;
    let mut guest = game.player_source();
    guest.entity = Some(commands::Target::Guest(2));
    assert!(
        key(game.execute_line("test pos", guest))
            .contains("commands.test.error.no_test_containing_pos")
    );
    let p = game.player.as_ref().unwrap().position;
    assert_eq!(
        ([p.x, p.y, p.z], game.yaw, game.pitch),
        ([4.5, 101.0, 6.5], 0.0, 90.0)
    );
    let g = game.lan.as_ref().unwrap().hub.guest(2).unwrap();
    assert_eq!((g.position, g.pitch), ([22.5, 101.0, 9.5], -90.0));
    assert_eq!(game.dim, Dim::Overworld);
}
