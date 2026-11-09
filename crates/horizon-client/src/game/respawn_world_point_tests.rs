use crate::game::{Audio, Game, Options, Progress, Settings, load_resources};
use horizon_client::lan::{Joined, NetConfig, Out, PlayerSender, guest::Guest, hub::HubConfig};
use horizon_core::geometry::Vec3;
use horizon_protocol::packets::{configuration::ClientInformation, play::Clientbound};
use std::sync::{Arc, atomic::AtomicI32, mpsc};

/// PlayerList.respawn sends world respawn data, independently of the personal landing.
#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn guest_respawn_keeps_world_information_distinct_from_personal_landing() {
    check_respawn(false);
}

#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn guest_respawn_reads_the_active_loaded_generator_sea_level() {
    check_respawn(true);
}

fn check_respawn(check_sea_level: bool) {
    let root = std::path::PathBuf::from(std::env::var_os("HORIZON_TEST_RESOURCES").unwrap());
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
    options.execution_role = crate::game::execution_role::ExecutionRole::IntegratedServer;
    let mut game = Game::new(
        &loaded.resources,
        loaded.pack,
        loaded.gui.unwrap(),
        options,
        settings,
        Audio::silent(&root),
        None,
    )
    .unwrap();
    let scratch = std::env::temp_dir().join(format!(
        "horizon-respawn-point-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&scratch).unwrap();
    game.world_root = scratch.clone();
    let world = [0, 100, 0];
    let personal = [5.5, 101.0, 5.5];
    game.level.world_spawn = Some(world);
    game.spawn = Some(Vec3::new(0.5, 100.0, 0.5));
    game.lan_host(
        super::super::lan::LanSettings {
            port: 0,
            guest_commands: true,
            force_game_mode: false,
        },
        NetConfig {
            announce: false,
            ..NetConfig::loopback(0, "respawn-test".into(), 2)
        },
        HubConfig {
            default_game_mode: 0,
            guest_commands: true,
            force_game_mode: false,
            hardcore: false,
            view_distance: 2,
            simulation_distance: 2,
            dimension: "minecraft:overworld".into(),
            allow_flight: false,
            seed: 0,
            max_players: 2,
            integrated: false,
            levels: Default::default(),
        },
    )
    .unwrap();
    let (tx, rx) = mpsc::channel();
    let mut guest = Guest::new(
        Joined {
            conn: 2,
            uuid: 330,
            name: "RespawnGuest".into(),
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
        [0.5, 100.0, 0.5],
        2,
    );
    guest.dimension = "minecraft:overworld".into();
    guest.placed = true;
    guest.loaded = true;
    guest.spawn = Some(personal);
    guest.health = 0.0;
    guest.dead = true;
    let mut host = game.lan.take().unwrap();
    host.hub.guests.push(guest);
    // Replacing the active generator between calls must not leave a cached
    // value in the hub or its context. Both generators use real bundle data.
    let variants: &[Option<bool>] = if check_sea_level {
        &[Some(true), Some(false), Some(true)]
    } else {
        &[None]
    };
    for &flat in variants {
        if let Some(flat) = flat {
            use horizon_client::vanillagen::VanillaTerrain;
            let terrain = if flat {
                VanillaTerrain::for_flat(
                    &root.join("worldgen"),
                    &Default::default(),
                    game.pack.assets.registry,
                    game.res.biomes(),
                    12345,
                    &horizon_world::worldgen::generator::FlatSettings {
                        layers: vec![("minecraft:stone".into(), 1)],
                        biome: "minecraft:plains".into(),
                        structure_overrides: Some(vec![]),
                        features: false,
                        lakes: false,
                    },
                    false,
                )
            } else {
                VanillaTerrain::new(
                    &root.join("worldgen"),
                    game.pack.assets.registry,
                    game.res.biomes(),
                    12345,
                )
            }
            .unwrap();
            game.end.set_locator(Some(Arc::new(terrain)));
        }
        host.hub.guests[0].health = 0.0;
        host.hub.guests[0].dead = true;
        let clock = game.world.game_time;
        game.guest_respawn(&mut host, 2);
        assert_eq!(game.world.game_time, clock);
        let guest = &host.hub.guests[0];
        assert_eq!(guest.uuid, 330);
        assert_eq!(guest.position, personal);
        assert_eq!(guest.health, 20.0);
        assert!(!guest.dead);
        let mut packets = Vec::new();
        for out in rx.try_iter() {
            match out {
                Out::Packet(body) => packets.push(body),
                Out::Packets(bodies) => packets.extend(bodies),
                Out::Disconnect(_) => panic!("unexpected disconnect"),
            }
        }
        let mut sequence = Vec::new();
        let mut points = Vec::new();
        for body in packets {
            let mut r = horizon_protocol::buf::Reader::new(&body);
            let id = r.varint().unwrap();
            match Clientbound::decode(id, r.rest()).unwrap() {
                Clientbound::Respawn(p) => {
                    sequence.push(84);
                    if let Some(flat) = flat {
                        assert_eq!(
                            p.spawn.sea_level,
                            if flat { -63 } else { 63 },
                            "actual guest respawn must use the active loaded generator"
                        );
                    }
                }
                Clientbound::SetDefaultSpawnPosition(p) => {
                    sequence.push(99);
                    points.push((
                        p.pos.dimension,
                        p.pos.pos,
                        p.yaw.to_bits(),
                        p.pitch.to_bits(),
                    ));
                }
                _ => {}
            }
        }
        assert_eq!(sequence, [84, 99, 99]);
        assert_eq!(points, vec![("minecraft:overworld".into(), world, 0, 0); 2]);
    }
    drop(host);
    drop(game);
    std::fs::remove_dir_all(scratch).unwrap();
}

/// A mod's health 0 for a guest dies as `/kill` does: the death message
/// and the death screen, not only a zero in the record.
#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn a_mods_zero_health_kills_a_guest_as_kill_does() {
    let root = std::path::PathBuf::from(std::env::var_os("HORIZON_TEST_RESOURCES").unwrap());
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
    options.execution_role = crate::game::execution_role::ExecutionRole::IntegratedServer;
    let mut game = Game::new(
        &loaded.resources,
        loaded.pack,
        loaded.gui.unwrap(),
        options,
        settings,
        Audio::silent(&root),
        None,
    )
    .unwrap();
    let scratch = std::env::temp_dir().join(format!(
        "horizon-status-death-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&scratch).unwrap();
    game.world_root = scratch.clone();
    game.level.world_spawn = Some([0, 100, 0]);
    game.spawn = Some(Vec3::new(0.5, 100.0, 0.5));
    game.lan_host(
        super::super::lan::LanSettings {
            port: 0,
            guest_commands: true,
            force_game_mode: false,
        },
        NetConfig {
            announce: false,
            ..NetConfig::loopback(0, "status-death-test".into(), 2)
        },
        HubConfig {
            default_game_mode: 0,
            guest_commands: true,
            force_game_mode: false,
            hardcore: false,
            view_distance: 2,
            simulation_distance: 2,
            dimension: "minecraft:overworld".into(),
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
            uuid: 331,
            name: "StatusGuest".into(),
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
        [0.5, 100.0, 0.5],
        2,
    );
    guest.dimension = "minecraft:overworld".into();
    guest.placed = true;
    guest.loaded = true;
    let mut host = game.lan.take().unwrap();
    host.hub.guests.push(guest);
    // Inside the player's own turn, as a tick hook sets it.
    let set = game.as_guest(&mut host, 2, |g| {
        g.native_set_player_status(331, Some(0.0), None, None)
    });
    assert_eq!(set, Some(true));
    assert_eq!(host.hub.guests[0].health, 0.0);
    game.guest_die(&mut host, 0, 2);
    assert!(host.hub.guests[0].dead, "health 0 must kill");
    drop(host);
    drop(game);
    std::fs::remove_dir_all(scratch).unwrap();
}
