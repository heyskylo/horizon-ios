use super::*;
use horizon_client::{
    chunk::Chunk,
    lan::{Joined, NetConfig, PlayerSender, guest::Guest, hub::HubConfig, playerdata},
    motion::PlayerMotion,
};
use horizon_core::position::ChunkPos;
use horizon_protocol::packets::configuration::ClientInformation;
use horizon_world::nbt::Tag;
use std::sync::{Arc, atomic::AtomicI32, mpsc};

fn check(tag: &Tag, grace: i32, particles: bool, phase: &str, failures: &mut Vec<String>) {
    let expected = [
        (
            "Brain",
            Tag::compound([("memories", Tag::Compound(Default::default()))]),
        ),
        ("current_impulse_context_reset_grace_time", Tag::Int(grace)),
        (
            "spawn_extra_particles_on_fall",
            Tag::Byte(i8::from(particles)),
        ),
    ];
    for (key, value) in expected {
        if tag.get(key) != Some(&value) {
            failures.push(format!(
                "{phase}: {key}: {:?}, expected {value:?}",
                tag.get(key)
            ));
        }
    }
}

fn raw(marker: &str, grace: i32, particles: bool) -> Tag {
    Tag::compound([
        ("h157_marker", Tag::String(marker.into())),
        ("current_impulse_context_reset_grace_time", Tag::Int(grace)),
        (
            "spawn_extra_particles_on_fall",
            Tag::Byte(i8::from(particles)),
        ),
    ])
}

#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES and scratch homes/cwd"]
fn actual_player_save_tracks_mutable_body_and_keeps_guest_context() {
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
    options.execution_role = execution_role::ExecutionRole::IntegratedServer;
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
    game.world_root = std::env::current_dir().unwrap().join("h157-owner");
    std::fs::create_dir_all(&game.world_root).unwrap();
    game.world
        .chunks
        .insert((0, 0), Arc::new(Chunk::empty(0, 0)));
    game.block_ticks
        .add_empty_chunk(ChunkPos::new(0, 0))
        .unwrap();
    game.player = Some(PlayerMotion::new(&game.motion, Vec3::new(8.5, 200.0, 8.5)).unwrap());
    game.build_registries();
    game.set_allow_commands(true);
    game.set_mode_id(1);
    let owner_uuid = game.level.ensure_player_uuid();
    assert!(game.run_command("tick freeze"));
    assert!(game.clock.frozen());
    let clock = game.world.game_time;
    let tracker = (game.sculk.tracker, game.sculk.tracker_loaded);
    let mut failures = Vec::new();
    let initial = game.player_nbt().expect("actual owner getter");
    assert_eq!(
        initial.get("UUID"),
        Some(&Tag::IntArray(owner_uuid.to_vec()))
    );
    assert_eq!(initial.get("Health"), Some(&Tag::Float(20.0_f32.to_bits())));
    assert_eq!(initial.get("playerGameType"), Some(&Tag::Int(1)));
    check(&initial, 0, false, "new owner", &mut failures);
    game.level.player_extra = Some(raw("owner", 99, false));
    {
        let body = game.player.as_mut().unwrap();
        body.impulse.apply_grace_time(37);
        body.spawn_extra_particles_on_fall = true;
        assert_eq!(body.impulse.reset_grace_time, 37);
        assert!(body.spawn_extra_particles_on_fall);
    }
    let live = game.player_nbt().unwrap();
    check(&live, 37, true, "live nondefault beats raw", &mut failures);
    check(
        &game.level.player_tag(),
        37,
        true,
        "actual saved writer",
        &mut failures,
    );
    assert_eq!(live.get("h157_marker"), Some(&Tag::String("owner".into())));
    {
        let body = game.player.as_mut().unwrap();
        body.impulse.reset();
        body.spawn_extra_particles_on_fall = false;
        assert_eq!(body.impulse.reset_grace_time, 0);
    }
    check(
        &game.player_nbt().unwrap(),
        0,
        false,
        "live reset",
        &mut failures,
    );
    // Without a body, the old file is retained until a real body supplies a snapshot.
    game.player = None;
    game.sync_level();
    check(
        &game.level.player_tag(),
        99,
        false,
        "no body raw retention",
        &mut failures,
    );
    game.player = Some(PlayerMotion::new(&game.motion, Vec3::new(8.5, 200.0, 8.5)).unwrap());
    assert_eq!(game.player.as_ref().unwrap().impulse.reset_grace_time, 0);
    assert!(!game.player.as_ref().unwrap().spawn_extra_particles_on_fall);
    check(
        &game.player_nbt().unwrap(),
        0,
        false,
        "new body replaces snapshot",
        &mut failures,
    );
    game.player.as_mut().unwrap().impulse.apply_grace_time(37);
    game.player.as_mut().unwrap().spawn_extra_particles_on_fall = true;
    check(
        &game.player_nbt().unwrap(),
        37,
        true,
        "owner before guest",
        &mut failures,
    );
    let net = NetConfig {
        announce: false,
        ..NetConfig::lan(0, "h157-context".into(), 2)
    };
    game.lan_host(
        lan::LanSettings {
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
            name: "H157Guest".into(),
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
        [12.5, 200.0, 8.5],
        2,
    );
    guest.placed = true;
    guest.loaded = true;
    guest.saved = Some(raw("guest", 88, true));
    let mut body = PlayerMotion::new(&game.motion, Vec3::new(12.5, 200.0, 8.5)).unwrap();
    body.impulse.apply_grace_time(7);
    assert_eq!(body.impulse.reset_grace_time, 7);
    assert!(!body.spawn_extra_particles_on_fall);
    guest.motion = Some(Box::new(body));
    game.lan.as_mut().unwrap().hub.guests.push(guest);
    let guest_tag = game
        .for_player(commands::Target::Guest(2), |g| {
            assert_eq!(g.player.as_ref().unwrap().impulse.reset_grace_time, 7);
            assert!(!g.player.as_ref().unwrap().spawn_extra_particles_on_fall);
            g.player_nbt().unwrap()
        })
        .expect("actual as_guest context");
    assert_eq!(
        guest_tag.get("UUID"),
        Some(&Tag::IntArray(vec![0, 0, 0, 2]))
    );
    assert_eq!(
        guest_tag.get("h157_marker"),
        Some(&Tag::String("guest".into()))
    );
    check(&guest_tag, 7, false, "active guest body", &mut failures);
    assert_eq!(game.player.as_ref().unwrap().impulse.reset_grace_time, 37);
    assert!(game.player.as_ref().unwrap().spawn_extra_particles_on_fall);
    // Read the real writer before another sync can hide a context leak.
    let restored = game.level.player_tag();
    assert_eq!(
        restored.get("h157_marker"),
        Some(&Tag::String("owner".into()))
    );
    check(
        &restored,
        37,
        true,
        "owner snapshot restored",
        &mut failures,
    );
    let mut saved_guest = horizon_client::save::Level::default();
    let guest = &game.lan.as_ref().unwrap().hub.guests[0];
    assert_eq!(guest.motion.as_ref().unwrap().impulse.reset_grace_time, 7);
    playerdata::store(guest, &mut saved_guest, None);
    check(
        &saved_guest.player_tag(),
        7,
        false,
        "guest store reads its live body",
        &mut failures,
    );
    assert_eq!(game.world.game_time, clock);
    assert_eq!(
        (game.sculk.tracker, game.sculk.tracker_loaded),
        tracker,
        "external-owned tracker must stay untouched"
    );
    eprintln!(
        "H157 actual mutable bodies, reset/new-body, owner/guest, writer and frozen-clock admissions complete"
    );
    assert!(
        failures.is_empty(),
        "actual saved-player projection failures: {failures:#?}"
    );
}
