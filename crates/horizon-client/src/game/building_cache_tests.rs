//! The players' boxes the guests' turns of one tick share (`building_cache`):
//! a guest teleported, killed or put in spectator mode in the middle of the
//! tick blocks placement where it is now, as when each turn built the boxes
//! afresh, not at its box from the start of the tick.
use super::*;
use horizon_client::{
    chunk::Chunk,
    lan::{Joined, NetConfig, PlayerSender, guest::Guest, hub::HubConfig},
    motion::PlayerMotion,
};
use horizon_protocol::packets::configuration::ClientInformation;
use std::sync::{Arc, atomic::AtomicI32, mpsc};

type Boxes = Vec<([f64; 3], [f64; 3])>;

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
        game.block_ticks
            .add_empty_chunk(horizon_core::position::ChunkPos::new(x, 0))
            .unwrap();
    }
    assert!(game.run_command("fill 0 99 0 31 99 15 stone"));
    game.player = Some(PlayerMotion::new(&game.motion, Vec3::new(2.5, 100.0, 2.5)).unwrap());
    let net = NetConfig {
        announce: false,
        ..NetConfig::lan(0, "building-cache-test".into(), 3)
    };
    game.lan_host(
        super::lan::LanSettings {
            port: 0,
            guest_commands: true,
            force_game_mode: false,
        },
        net,
        HubConfig {
            default_game_mode: 0,
            guest_commands: true,
            force_game_mode: false,
            hardcore: false,
            view_distance: 2,
            simulation_distance: 2,
            dimension: Dim::Overworld.id().into(),
            allow_flight: false,
            seed: 0,
            max_players: 3,
            integrated: false,
            levels: Default::default(),
        },
    )
    .unwrap();
    for (conn, name, x) in [(2, "Mover", 8.5), (3, "Builder", 14.5)] {
        let (tx, _rx) = mpsc::channel();
        let mut guest = Guest::new(
            Joined {
                conn,
                uuid: conn as u128,
                name: name.into(),
                properties: Vec::new(),
                info: ClientInformation::new(2),
                brand: "test".into(),
                sender: PlayerSender::new(tx, Arc::default()),
                latency: Arc::new(AtomicI32::new(0)),
                address: "127.0.0.1:25565".parse().unwrap(),
                expected: false,
                configured: Default::default(),
            },
            conn as i32,
            [x, 100.0, 8.5],
            0,
        );
        guest.placed = true;
        guest.loaded = true;
        game.lan.as_mut().unwrap().hub.guests.push(guest);
    }
    game
}

/// The other players' boxes in the Builder's turn: what blocks its placing.
fn seen_by_builder(game: &mut Game<'_>) -> Boxes {
    let mut host = game.lan.take().unwrap();
    let seen = game.as_guest(&mut host, 3, |g| g.lan_building_boxes.boxes(Dim::Overworld));
    game.lan = Some(host);
    seen.unwrap()
}

/// The Builder's view at the start of a tick, after `command` (and, with
/// `turns`, the guests' turns that follow it in the tick), and the same
/// without the shared boxes.
fn the_mover_changed_mid_tick(command: &str, turns: bool) -> (Boxes, Boxes, Boxes) {
    let mut game = game();
    super::lan_actions::building_cache(game.lan.as_ref());
    let before = seen_by_builder(&mut game);
    assert!(game.run_command(command), "{command}");
    if turns {
        let mut host = game.lan.take().unwrap();
        game.tick_guests_inner(&mut host);
        game.lan = Some(host);
    }
    let cached = seen_by_builder(&mut game);
    super::lan_actions::building_cache(None);
    let fresh = seen_by_builder(&mut game);
    (before, cached, fresh)
}

fn mover_at(boxes: &Boxes, x: f64) -> bool {
    boxes
        .iter()
        .any(|(min, max)| min[0] <= x && x <= max[0] && min[1] <= 100.5 && 100.5 <= max[1])
}

#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn a_teleported_guest_blocks_where_it_went_not_where_the_tick_began() {
    let (before, cached, fresh) = the_mover_changed_mid_tick("tp Mover 24.5 100 8.5", false);
    assert!(mover_at(&before, 8.5));
    assert!(!mover_at(&cached, 8.5) && mover_at(&cached, 24.5));
    assert_eq!(cached, fresh);
}

#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn a_guest_killed_mid_tick_no_longer_blocks() {
    // The kill takes its health; the guest dies in its own turn of the tick.
    let (before, cached, fresh) = the_mover_changed_mid_tick("kill Mover", true);
    assert!(mover_at(&before, 8.5));
    assert!(!mover_at(&cached, 8.5));
    assert_eq!(cached, fresh);
}

#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn a_guest_put_in_spectator_mid_tick_no_longer_blocks() {
    let (before, cached, fresh) = the_mover_changed_mid_tick("gamemode spectator Mover", false);
    assert!(mover_at(&before, 8.5));
    assert!(!mover_at(&cached, 8.5));
    assert_eq!(cached, fresh);
}
