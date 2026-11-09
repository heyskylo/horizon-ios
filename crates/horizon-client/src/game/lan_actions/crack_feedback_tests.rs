use super::*;
use crate::game::{Audio, Options, Progress, Settings, load_resources};
use horizon_client::{
    chunk::Chunk,
    lan::{Joined, NetConfig, Out, PlayerSender, guest::Guest, hub::HubConfig},
};
use horizon_protocol::packets::configuration::ClientInformation;
use horizon_world::registry::BlockStateId;
use std::sync::{Arc, atomic::AtomicI32, mpsc};

fn observer(conn: u32, position: [f64; 3], dimension: &str) -> (Guest, mpsc::Receiver<Out>) {
    let (tx, rx) = mpsc::channel();
    let mut guest = Guest::new(
        Joined {
            conn,
            uuid: u128::from(conn),
            name: format!("Crack{conn}"),
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
        position,
        2,
    );
    guest.placed = true;
    guest.loaded = true;
    guest.dimension = dimension.into();
    guest.on_ground = true;
    (guest, rx)
}

fn cracks(rx: &mpsc::Receiver<Out>) -> Vec<(i32, [i32; 3], u8)> {
    let mut result = Vec::new();
    for out in rx.try_iter() {
        let bodies = match out {
            Out::Packet(body) => vec![body],
            Out::Packets(bodies) => bodies,
            Out::Disconnect(_) => panic!("unexpected disconnect"),
        };
        for body in bodies {
            let mut reader = horizon_protocol::buf::Reader::new(&body);
            let id = reader.varint().unwrap();
            if let C::BlockDestruction(packet) = C::decode(id, reader.rest()).unwrap() {
                result.push((packet.id, packet.pos, packet.progress));
            }
        }
    }
    result
}

/// Exercise actual packet handling, dig feedback, and encoded observer packets without
/// starting a listener, a server loop, a window, or audio output.
#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn crack_packets_follow_start_clock_and_strict_observer_radius() {
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
    let scratch =
        std::env::temp_dir().join(format!("horizon-crack-feedback-{}", std::process::id()));
    std::fs::create_dir_all(&scratch).unwrap();
    game.world_root = scratch.clone();
    game.world
        .chunks
        .insert((0, 0), Arc::new(Chunk::empty(0, 0)));
    let pos = [0, 100, 0];
    let stone = BlockStateId(
        game.res
            .facts()
            .states
            .iter()
            .position(|f| &*f.name == "minecraft:stone")
            .unwrap() as u32,
    );
    game.world.set(pos[0], pos[1], pos[2], stone).unwrap();
    game.lan_host(
        super::super::lan::LanSettings {
            port: 0,
            guest_commands: true,
            force_game_mode: false,
        },
        NetConfig {
            listen: false,
            announce: false,
            ..NetConfig::loopback(0, "crack-test".into(), 6)
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
            max_players: 6,
            integrated: false,
            levels: Default::default(),
        },
    )
    .unwrap();
    let mut host = game.lan.take().unwrap();
    let mut receivers = Vec::new();
    for (index, (position, dimension)) in [
        ([0.5, 100.0, 0.5], "minecraft:overworld"),
        ([31.999, 100.0, 0.0], "minecraft:overworld"),
        ([32.0, 100.0, 0.0], "minecraft:overworld"),
        ([40.0, 100.0, 0.0], "minecraft:overworld"),
        ([0.0, 100.0, 0.0], "minecraft:the_nether"),
    ]
    .into_iter()
    .enumerate()
    {
        let (guest, rx) = observer(index as u32 + 1, position, dimension);
        host.hub.guests.push(guest);
        receivers.push(rx);
    }
    let action = |action| {
        S::PlayerAction(play::PlayerAction {
            action,
            pos,
            direction: 1,
            sequence: 1,
        })
    };
    host.hub.ticks = 100;
    game.lan_guest_packet(&mut host, 1, action(0));
    let mut trace = cracks(&receivers[1]);
    assert_eq!(trace, [(1, pos, 0)], "START sends stage zero immediately");
    for tick in 101..=103 {
        host.hub.ticks = tick;
        host.hub.guests[0].inventory[0] =
            (tick != 102).then(|| ItemStack::new("minecraft:diamond_pickaxe", 1));
        game.as_guest(&mut host, 1, |g| {
            g.forced_hit = Some(Hit {
                pos,
                face: [0, 1, 0],
                point: [0.5, 100.5, 0.5],
            });
            g.tick_mining().unwrap();
            g.forced_hit = None;
        });
        game.guest_dig_feedback(&mut host, 1);
        game.guest_dig_feedback(&mut host, 1);
    }
    // Stone: hand 1/1.5/100; diamond pickaxe 8/1.5/30. Each tick
    // recomputes current speed against elapsed START time, rather than summing it.
    let ticks = cracks(&receivers[1]);
    assert_eq!(ticks, [(1, pos, 3), (1, pos, 0), (1, pos, 7)]);
    trace.extend(ticks);
    game.lan_guest_packet(&mut host, 1, action(3));
    let stop = cracks(&receivers[1]);
    assert_eq!(stop, [(1, pos, 255)], "accepted STOP clears the crack");
    trace.extend(stop);
    game.world.set(pos[0], pos[1], pos[2], stone).unwrap();
    host.hub.guests[0].inventory[0] = None;
    game.lan_guest_packet(&mut host, 1, action(0));
    game.lan_guest_packet(&mut host, 1, action(2));
    let abort = cracks(&receivers[1]);
    assert_eq!(
        abort,
        [(1, pos, 0), (1, pos, 255)],
        "ABORT clears the crack"
    );
    trace.extend(abort);
    // A restricted START must not publish the previous dig's mining state for
    // a different target, nor replace its START clock.
    game.lan_guest_packet(&mut host, 1, action(0));
    assert_eq!(cracks(&receivers[1]), [(1, pos, 0)]);
    let start = host.play[&1].dig_start;
    host.hub.guests[0].game_mode = 2;
    let other = [1, 100, 0];
    game.world.set(other[0], other[1], other[2], stone).unwrap();
    game.lan_guest_packet(
        &mut host,
        1,
        S::PlayerAction(play::PlayerAction {
            action: 0,
            pos: other,
            direction: 1,
            sequence: 2,
        }),
    );
    assert!(cracks(&receivers[1]).is_empty());
    assert_eq!(host.play[&1].digging.map(|(pos, _)| pos), Some(pos));
    assert_eq!(host.play[&1].dig_start, start);
    game.lan_guest_packet(&mut host, 1, action(2));
    assert_eq!(cracks(&receivers[1]), [(1, pos, 255)]);
    game.lan_guest_packet(&mut host, 1, action(0));
    assert!(cracks(&receivers[1]).is_empty(), "fresh restricted START");
    host.hub.guests[0].game_mode = 1;
    game.lan_guest_packet(&mut host, 1, action(0));
    assert!(cracks(&receivers[1]).is_empty(), "creative START");
    host.hub.guests[0].game_mode = 0;
    let torch = BlockStateId(
        game.res
            .facts()
            .states
            .iter()
            .position(|f| &*f.name == "minecraft:torch")
            .unwrap() as u32,
    );
    game.world.set(pos[0], pos[1], pos[2], torch).unwrap();
    game.lan_guest_packet(&mut host, 1, action(0));
    assert!(cracks(&receivers[1]).is_empty(), "instant START");
    for (index, rx) in receivers.iter().enumerate() {
        assert!(
            cracks(rx).is_empty(),
            "unexpected crack for observer {index}"
        );
    }
    if let Some(path) = std::env::var_os("HORIZON_CRACK_TRACE") {
        let stages: Vec<_> = trace.iter().map(|(_, _, stage)| *stage).collect();
        std::fs::write(path, serde_json::to_vec(&stages).unwrap()).unwrap();
    }
    drop(host);
    drop(game);
    std::fs::remove_dir_all(scratch).unwrap();
}
