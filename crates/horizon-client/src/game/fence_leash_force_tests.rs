//! A NoAI pig leashed to a fence knot is pulled by the leash
//! (`Leashable.tickLeash` → `checkElasticInteractions`): its Motion and yaw at
//! ticks 1, 2 and 5 are vanilla 26.3's exact values
//! (tools/test_fence_leash_force_parity.py measured them on vanilla).
//!
//! The pig is summoned with a saved fence leash (`leash:[I;8,201,0]`), which
//! is only pending. The live holder is admitted from the real path at the
//! tick-1 boundary: the pending tag is restored into the live
//! `Holder::Knot([8,201,0])`, the hosted server gives that knot its entity
//! (`knot_ids`, as it tracks it to a guest), the fence is there, and the knot
//! the leash tick reads (`leash::knot_state`) is at (8.5, 201.375, 0.5).
use super::*;
use horizon_client::{
    chunk::Chunk,
    lan::{Joined, NetConfig, PlayerSender, guest::Guest, hub::HubConfig},
    mobs::leash::{Holder, Pending, knot_state},
    motion::PlayerMotion,
};
use horizon_core::position::ChunkPos;
use horizon_protocol::packets::configuration::ClientInformation;
use std::sync::{Arc, atomic::AtomicI32, mpsc};

const ELASTIC: &str = "00006099-0000-0000-0000-000000000001";
const SLACK: &str = "00006099-0000-0000-0000-000000000002";

/// A host with one placed guest (conn 2), so the server tracks the level and
/// gives its fence knots their entities.
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
    for x in 0..3 {
        game.world
            .chunks
            .insert((x, 0), Arc::new(Chunk::empty(x, 0)));
        game.block_ticks
            .add_empty_chunk(ChunkPos::new(x, 0))
            .unwrap();
    }
    game.player = Some(PlayerMotion::new(&game.motion, Vec3::new(24.5, 201.0, 12.5)).unwrap());
    let net = NetConfig {
        announce: false,
        ..NetConfig::lan(0, "fence-leash-test".into(), 2)
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
            name: "LeashGuest".into(),
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
        [24.5, 201.0, 10.5],
        1,
    );
    guest.placed = true;
    guest.loaded = true;
    game.lan.as_mut().unwrap().hub.guests.push(guest);
    for command in [
        "forceload add 0 0 47 15",
        "fill 0 200 0 47 200 15 stone",
        "tick freeze",
        "setblock 8 201 0 oak_fence",
        "setblock 40 201 0 oak_fence",
        "summon pig 16.5 201 0.5 {NoAI:1b,PersistenceRequired:1b,Health:10f,Rotation:[0f,0f],Motion:[0d,0d,0d],leash:[I;8,201,0],UUID:[I;24729,0,0,1]}",
        "summon pig 43.5 201 0.5 {NoAI:1b,PersistenceRequired:1b,Health:10f,Rotation:[0f,0f],Motion:[0d,0d,0d],leash:[I;40,201,0],UUID:[I;24729,0,0,2]}",
    ] {
        assert!(game.run_command(command), "{command}");
    }
    game
}

fn pig(game: &mut Game<'_>, uuid: &str) -> usize {
    match game.target_with_uuid(horizon_client::save::uuid_from_text(uuid).unwrap()) {
        Some(super::commands::Target::Mob(id)) => {
            game.mobs.iter().position(|m| m.id == id).unwrap()
        }
        other => panic!("{other:?}"),
    }
}

fn step(game: &mut Game<'_>) {
    let before = game.world.game_time;
    assert!(game.run_command("tick step 1"));
    game.tick_server(false).unwrap();
    assert_eq!(game.world.game_time, before + 1, "one tick");
}

/// `(Motion, yaw)` as saved: the velocity and the yaw.
fn vectors(game: &mut Game<'_>, uuid: &str) -> ([f64; 3], f32) {
    let i = pig(game, uuid);
    let m = &game.mobs[i];
    ([m.velocity.x, m.velocity.y, m.velocity.z], m.yaw)
}

#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn a_fence_leash_pulls_a_pig_as_vanilla_does() {
    let mut game = game();
    let (elastic, slack) = (pig(&mut game, ELASTIC), pig(&mut game, SLACK));
    // Before any tick: the saved leash is only pending; the pig is at rest
    // on stone, with gravity.
    for (i, fence) in [(elastic, [8, 201, 0]), (slack, [40, 201, 0])] {
        let m = &game.mobs[i];
        assert_eq!(m.extra.leash_pending, Some(Pending::Knot(fence)), "pending");
        assert_eq!(m.extra.leash, None, "not live yet");
        assert!(m.no_ai && !m.no_gravity);
        assert_eq!(
            ([m.velocity.x, m.velocity.y, m.velocity.z], m.yaw),
            ([0.0; 3], 0.0)
        );
    }
    assert!(game.run_command("execute if block 16 200 0 stone"));
    assert!(game.run_command("execute if block 43 200 0 stone"));
    step(&mut game);
    // The tick-1 boundary: the real restoration bound the live knot.
    for (uuid, fence) in [(ELASTIC, [8, 201, 0]), (SLACK, [40, 201, 0])] {
        let i = pig(&mut game, uuid);
        let leash = game.mobs[i].extra.leash;
        let pending = game.mobs[i].extra.leash_pending;
        let knot = game.knot_ids.get(&fence).copied();
        let state = knot_state(fence);
        eprintln!(
            "tick-1 binding {uuid}: leash={leash:?} pending={pending:?} knot entity={knot:?} at {:?}",
            state.position
        );
        assert_eq!(
            leash,
            Some(Holder::Knot(fence)),
            "the live holder is the fence's knot"
        );
        assert_eq!(pending, None, "restored");
        assert!(knot.is_some(), "the server gave that knot its entity");
        assert_eq!(
            [state.position.x, state.position.y, state.position.z],
            [
                f64::from(fence[0]) + 0.5,
                f64::from(fence[1]) + 0.375,
                f64::from(fence[2]) + 0.5
            ],
            "the knot the leash tick reads"
        );
        assert!(game.run_command(&format!(
            "execute if block {} {} {} oak_fence",
            fence[0], fence[1], fence[2]
        )));
    }
    // The vanilla 26.3 values (two fresh runs, the same bytes).
    let expected: [(u32, [f64; 3], f32); 3] = [
        (
            1,
            [-0.48261553211124286, 0.0, -0.027147122962104232],
            9.049041,
        ),
        (2, [-0.895150152015108, 0.0, -0.05025677894260627], 25.98228),
        (
            5,
            [-1.707933110041183, 0.0, -0.07285963779554705],
            100.52993,
        ),
    ];
    let mut tick = 1;
    for (at, motion, yaw) in expected {
        while tick < at {
            step(&mut game);
            tick += 1;
        }
        assert_eq!(
            vectors(&mut game, SLACK),
            ([0.0; 3], 0.0),
            "slack at tick {at}"
        );
        assert_eq!(
            vectors(&mut game, ELASTIC),
            (motion, yaw),
            "elastic at tick {at}"
        );
    }
}
