//! The server tick's cost per player: `tick_lan` with N players standing on a
//! stone floor, timed over many ticks, so a change to the guests' tick can be
//! measured before and after. Run alone, in an optimized profile:
//! `HORIZON_TEST_RESOURCES=... cargo test --profile fast -p horizon-client
//! fake_player_tick -- --ignored --nocapture` (`HORIZON_BENCH_PLAYERS`,
//! `HORIZON_BENCH_TICKS` and `HORIZON_BENCH_FAKE=1` change it).
use super::*;
use horizon_client::{
    chunk::Chunk,
    lan::{Joined, NetConfig, PlayerSender, guest::Guest, hub::HubConfig},
    motion::PlayerMotion,
};
use horizon_protocol::packets::configuration::ClientInformation;
use std::sync::{Arc, atomic::AtomicI32, mpsc};

fn env(name: &str, default: usize) -> usize {
    std::env::var(name)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

/// A host on a 4x4-chunk stone floor with `players` placed guests on it,
/// fake players when `fake`.
fn game(players: usize, fake: bool) -> (Game<'static>, mpsc::Receiver<horizon_client::lan::Out>) {
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
    for x in 0..4 {
        for z in 0..4 {
            game.world
                .chunks
                .insert((x, z), Arc::new(Chunk::empty(x, z)));
            game.block_ticks
                .add_empty_chunk(horizon_core::position::ChunkPos::new(x, z))
                .unwrap();
        }
    }
    assert!(game.run_command("fill 0 99 0 63 99 63 stone"));
    game.player = Some(PlayerMotion::new(&game.motion, Vec3::new(32.5, 100.0, 32.5)).unwrap());
    let net = NetConfig {
        announce: false,
        ..NetConfig::lan(0, "fake-tick-bench".into(), players + 1)
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
            max_players: players + 1,
            integrated: false,
            levels: Default::default(),
        },
    )
    .unwrap();
    let (tx, rx) = mpsc::channel();
    for i in 0..players {
        let conn = 2 + i as u32;
        let uuid = 0x1000 + i as u128;
        let pos = [
            2.5 + (i % 30) as f64 * 2.0,
            100.0,
            2.5 + (i / 30) as f64 * 2.0,
        ];
        let mut guest = Guest::new(
            Joined {
                conn,
                uuid,
                name: format!("Bench{i}"),
                properties: Vec::new(),
                info: ClientInformation::new(2),
                brand: "test".into(),
                sender: PlayerSender::new(tx.clone(), Arc::default()),
                latency: Arc::new(AtomicI32::new(0)),
                address: "127.0.0.1:25565".parse().unwrap(),
                expected: false,
                configured: Default::default(),
            },
            conn as i32,
            pos,
            0,
        );
        guest.placed = true;
        guest.loaded = true;
        let host = game.lan.as_mut().unwrap();
        host.hub.guests.push(guest);
        if fake {
            assert!(host.hub.set_guest_fake(uuid, true));
        }
    }
    (game, rx)
}

/// Prints the mean `tick_lan` time per tick, and per player, for N players.
#[test]
#[ignore = "a benchmark: needs prepared resources in HORIZON_TEST_RESOURCES"]
fn fake_player_tick() {
    let players = env("HORIZON_BENCH_PLAYERS", 100);
    let ticks = env("HORIZON_BENCH_TICKS", 400);
    let fake = env("HORIZON_BENCH_FAKE", 0) == 1;
    let (mut game, rx) = game(players, fake);
    for _ in 0..40 {
        game.tick_lan(false);
        rx.try_iter().for_each(drop);
    }
    let mut sent = 0usize;
    let start = std::time::Instant::now();
    for _ in 0..ticks {
        game.tick_lan(false);
        sent += rx.try_iter().count();
    }
    let ms = start.elapsed().as_secs_f64() * 1000.0 / ticks as f64;
    println!(
        "fake_player_tick players={players} fake={fake} ticks={ticks}: {ms:.3} ms/tick, \
         {:.1} us/player, {} packets/tick",
        ms * 1000.0 / players.max(1) as f64,
        sent / ticks
    );
}
