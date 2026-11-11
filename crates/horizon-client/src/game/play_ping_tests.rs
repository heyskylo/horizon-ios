//! Registered play-Ping regression with the production sender-flush completion boundary.
use crate::game::{Game, lan::LanSettings};
use horizon_client::lan::{Joined, NetConfig, Out, PlayerSender, guest::Guest, hub::HubConfig};
use horizon_protocol::{
    buf::Reader,
    packets::{
        configuration::ClientInformation,
        play::{Clientbound as C, Serverbound as S},
        status,
    },
};
use std::sync::{Arc, atomic::AtomicI32, mpsc};

fn decode(packet: S) -> (S, usize) {
    let bytes = packet.encode();
    let mut reader = Reader::new(&bytes);
    let id = reader.varint().unwrap();
    (S::decode(id, reader.rest()).unwrap(), bytes.len())
}

fn output(rx: &mpsc::Receiver<Out>) -> Vec<C> {
    rx.try_iter()
        .flat_map(|out| match out {
            Out::Packet(body) => vec![body],
            Out::Packets(bodies) => bodies,
            Out::Disconnect(reason) => panic!("unexpected disconnect: {reason:?}"),
        })
        .map(|body| {
            println!("actual outbound play body={body:02x?}");
            let mut reader = Reader::new(&body);
            let id = reader.varint().unwrap();
            C::decode(id, reader.rest()).unwrap()
        })
        .collect()
}

fn pass(game: &mut Game<'_>, conn: u32, packet: S) {
    let (packet, bytes) = decode(packet);
    if let S::PingRequest(p) = &packet {
        println!(
            "actual decoded play PingRequest conn={conn} value={} bits={:016x}",
            p.time, p.time as u64
        );
    }
    let hub = &game.lan.as_ref().unwrap().hub;
    assert_eq!(hub.guests.len(), 2);
    assert_ne!(hub.guests[0].uuid, hub.guests[1].uuid);
    assert_ne!(hub.guests[0].conn, hub.guests[1].conn);
    let registered = hub.guest(conn).expect("origin connection registered");
    assert_eq!(registered.uuid, u128::from(conn));
    assert!(!registered.fake && !registered.sender.is_fake());
    assert!(registered.left_reason.is_empty());
    assert_eq!(registered.dimension, "minecraft:overworld");
    let guest = game.lan.as_mut().unwrap().hub.guest_mut(conn).unwrap();
    guest.inbox.push_back((packet, bytes));
    guest.inbox_bytes += bytes;
    // Already registered connections: this is a second level pass, without polling sockets.
    game.level_pass.first = false;
    game.level_pass.packets_drained = false;
    let clock_before = game.world.game_time;
    game.tick_lan_packets();
    // tick_lan_packets has ended its Holding scope; production flushes these senders later.
    horizon_client::lan::flush_all(
        game.lan
            .as_ref()
            .unwrap()
            .hub
            .guests
            .iter()
            .map(|g| &g.sender),
    );
    assert_eq!(
        game.world.game_time, clock_before,
        "sender completion adds no world tick"
    );
    println!("actual sender completion conn={conn} world_clock={clock_before}");
    assert!(
        game.level_pass.packets_drained,
        "real Game packet phase completed"
    );
    assert!(
        game.lan
            .as_ref()
            .unwrap()
            .hub
            .guest(conn)
            .unwrap()
            .inbox
            .is_empty(),
        "decoded request was drained by the actual Hub"
    );
}

fn fixture() -> (Game<'static>, [mpsc::Receiver<Out>; 2]) {
    let mut game = crate::game::native_machines::tests::server_game();
    game.build_registries();
    game.lan_host(
        LanSettings {
            port: 0,
            guest_commands: false,
            force_game_mode: false,
        },
        NetConfig {
            announce: false,
            ..NetConfig::loopback(0, "play-ping-test".into(), 2)
        },
        HubConfig {
            default_game_mode: 0,
            guest_commands: false,
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
    let receivers = [1, 2].map(|conn| {
        let (tx, rx) = mpsc::channel();
        let mut guest = Guest::new(
            Joined {
                conn,
                uuid: u128::from(conn),
                name: format!("PingGuest{conn}"),
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
            [0.5, 80.0, 0.5],
            2,
        );
        guest.placed = true;
        guest.loaded = true;
        guest.loading = false;
        guest.load_timeout = 0;
        guest.last_action_tick = 7;
        game.lan.as_mut().unwrap().hub.guests.push(guest);
        rx
    });
    game.lan.as_mut().unwrap().hub.ticks = 123;
    (game, receivers)
}

#[test]
#[ignore = "needs prepared resources and isolated scratch homes/cwd"]
fn registered_play_ping_echoes_exact_long_to_only_its_connection() {
    let (mut game, receivers) = fixture();
    for (turn, time) in [
        1,
        0,
        -1,
        i64::MIN,
        i64::MAX,
        0x0123_4567_89ab_cdef,
        0xfedc_ba98_7654_3210_u64 as i64,
    ]
    .into_iter()
    .enumerate()
    {
        let index = turn % 2;
        let conn = index as u32 + 1;
        pass(&mut game, conn, S::PingRequest(status::Ping { time }));
        let packets = output(&receivers[index]);
        let echoed: Vec<_> = packets
            .iter()
            .filter_map(|p| match p {
                C::PongResponse(p) => Some(p.time),
                _ => None,
            })
            .collect();
        assert_eq!(
            echoed,
            vec![time],
            "missing or incorrect registered Game play-Ping reply"
        );
        assert_eq!(packets.len(), 1, "one request produces exactly one reply");
        assert_eq!(
            game.lan
                .as_ref()
                .unwrap()
                .hub
                .guest(conn)
                .unwrap()
                .last_action_tick,
            7,
            "transport ping does not count as a player action"
        );
        assert!(
            output(&receivers[1 - index]).is_empty(),
            "no reply to the other connection"
        );
    }
}

#[test]
#[ignore = "needs prepared resources and isolated scratch homes/cwd"]
fn dead_guest_play_ping_is_transport_not_an_action() {
    let (mut game, receivers) = fixture();
    game.lan.as_mut().unwrap().hub.guest_mut(1).unwrap().dead = true;
    pass(
        &mut game,
        1,
        S::PingRequest(status::Ping { time: i64::MIN }),
    );
    let packets = output(&receivers[0]);
    assert!(matches!(packets.as_slice(), [C::PongResponse(p)] if p.time == i64::MIN));
    assert!(output(&receivers[1]).is_empty());
    assert_eq!(
        game.lan
            .as_ref()
            .unwrap()
            .hub
            .guest(1)
            .unwrap()
            .last_action_tick,
        7
    );
}

#[test]
#[ignore = "needs prepared resources and isolated scratch homes/cwd"]
fn server_ping_client_pong_keeps_the_existing_prediction_route() {
    let (mut game, receivers) = fixture();
    let hub = &mut game.lan.as_mut().unwrap().hub;
    let mut config = horizon_anticheat::Config::default();
    config.log.clear();
    config.strikes.clear();
    config.prediction.enabled = true;
    hub.set_anticheat(config);
    hub.ticks = 123;
    let holding = horizon_client::lan::Holding::start();
    hub.anticheat_ping();
    drop(holding);
    // Ending Holding is not delivery: independently admit the existing flush boundary first.
    for receiver in &receivers {
        assert!(
            output(receiver).is_empty(),
            "actual server Ping is unavailable before flush"
        );
    }
    println!("actual reverse Ping unavailable after Holding drop, before flush");
    horizon_client::lan::flush_all(hub.guests.iter().map(|g| &g.sender));
    for receiver in &receivers {
        assert!(matches!(output(receiver).as_slice(), [C::Ping(p)] if p.id == 123));
    }
    pass(
        &mut game,
        1,
        S::Pong(horizon_protocol::packets::configuration::Ping { id: 123 }),
    );
    let record = game
        .lan
        .as_mut()
        .unwrap()
        .hub
        .guest_mut(1)
        .unwrap()
        .record
        .as_deref_mut()
        .expect("existing client Pong updates prediction");
    assert_eq!(record.tick_end(0, false).seen, Some(123));
    for receiver in &receivers {
        assert!(output(receiver).is_empty());
    }
}

#[test]
#[ignore = "needs prepared resources and isolated scratch homes/cwd"]
fn held_guest_play_ping_survives_the_acknowledgement_filter() {
    let (mut game, receivers) = fixture();
    let guest = game.lan.as_mut().unwrap().hub.guest_mut(1).unwrap();
    guest.placed = false;
    guest.held = true;
    pass(&mut game, 1, S::PingRequest(status::Ping { time: -1 }));
    assert!(matches!(output(&receivers[0]).as_slice(), [C::PongResponse(p)] if p.time == -1));
    assert!(output(&receivers[1]).is_empty());
    assert_eq!(
        game.lan
            .as_ref()
            .unwrap()
            .hub
            .guest(1)
            .unwrap()
            .last_action_tick,
        7
    );
}

#[test]
#[ignore = "needs prepared resources and isolated scratch homes/cwd"]
fn handover_guest_play_ping_survives_the_acknowledgement_filter() {
    let (mut game, receivers) = fixture();
    let handover = horizon_client::lan::arrivals::Handover {
        id: 0x1234,
        since: 123,
        record: serde_json::json!({"fixture": "handover-state-sentinel"}),
    };
    game.lan
        .as_mut()
        .unwrap()
        .hub
        .guest_mut(1)
        .unwrap()
        .handover = Some(handover.clone());
    pass(
        &mut game,
        1,
        S::PingRequest(status::Ping {
            time: 0xfedc_ba98_7654_3210_u64 as i64,
        }),
    );
    let packets = output(&receivers[0]);
    assert!(
        matches!(packets.as_slice(), [C::PongResponse(p)]
        if p.time == 0xfedc_ba98_7654_3210_u64 as i64),
        "missing matching handover play-Ping reply"
    );
    assert!(output(&receivers[1]).is_empty());
    let guest = game.lan.as_ref().unwrap().hub.guest(1).unwrap();
    assert_eq!(guest.last_action_tick, 7);
    assert_eq!(guest.handover.as_ref(), Some(&handover));
}
