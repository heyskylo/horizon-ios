use super::*;
use horizon_client::{
    chunk::Chunk,
    lan::{Joined, NetConfig, Out, PlayerSender, guest::Guest, hub::HubConfig},
};
use horizon_core::position::ChunkPos;
use horizon_protocol::packets::configuration::ClientInformation;
use std::sync::{Arc, atomic::AtomicI32, mpsc};

fn scene() -> (Game<'static>, mpsc::Receiver<Out>) {
    let mut g = super::super::native_machines::tests::server_game();
    g.build_registries();
    g.set_allow_commands(true);
    let stone = g
        .pack
        .assets
        .registry
        .default_state("minecraft:stone")
        .unwrap()
        .0;
    let mut chunk = Chunk::empty(0, 0);
    for x in 0..16 {
        for z in 0..16 {
            chunk.set(x, 99, z, stone);
        }
    }
    g.world.chunks.insert((0, 0), Arc::new(chunk));
    g.block_ticks.add_empty_chunk(ChunkPos::new(0, 0)).unwrap();
    assert!(g.run_command("forceload add 0 0"));
    assert!(g.run_command("tick freeze"));
    assert!(g.run_command("summon oak_boat .5 100 2.5 {UUID:[I;7,7,7,7]}"));
    g.lan_host(
        super::super::lan::LanSettings {
            port: 0,
            guest_commands: true,
            force_game_mode: false,
        },
        NetConfig {
            listen: false,
            announce: false,
            ..NetConfig::loopback(0, "boat-holder-unit".into(), 2)
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
            conn: 1,
            uuid: 1,
            name: "BoatHolder".into(),
            properties: Vec::new(),
            info: ClientInformation::new(2),
            brand: "unit".into(),
            sender: PlayerSender::new(tx, Arc::default()),
            latency: Arc::new(AtomicI32::new(0)),
            address: "127.0.0.1:1".parse().unwrap(),
            expected: false,
            configured: Default::default(),
        },
        1,
        [0.5, 100.0, 0.5],
        2,
    );
    guest.placed = true;
    guest.loaded = true;
    guest.load_timeout = 0;
    guest.dimension = "minecraft:overworld".into();
    guest.inventory[0] = Some(ItemStack::new("minecraft:lead", 1));
    g.lan.as_mut().unwrap().hub.guests.push(guest);
    g.sync_tick_chunks();
    g.ticked_entities_through = g.next_entity;
    (g, rx)
}
#[derive(Debug, PartialEq)]
struct Snapshot {
    known: [f64; 3],
    previous: [f64; 3],
    position: [f64; 3],
    moved: bool,
    using: Option<(usize, u32)>,
    yaw: f32,
    pitch: f32,
    hand: Option<ItemStack>,
}
fn snapshot(g: &Game<'_>) -> Snapshot {
    let h = g.lan.as_ref().unwrap();
    let p = &h.hub.guests[0];
    Snapshot {
        known: p.known_movement,
        previous: p.previous_position,
        position: p.position,
        moved: p.position_this_tick,
        using: h.play.get(&1).and_then(|p| p.using),
        yaw: p.yaw,
        pitch: p.pitch,
        hand: p.inventory[0].clone(),
    }
}
fn group(g: &mut Game<'_>, packets: Vec<S>, token: i32) {
    let p = &mut g.lan.as_mut().unwrap().hub.guests[0];
    for packet in packets {
        p.inbox.push_back((packet, 0));
    }
    // Real Hub queue/collision/event dispatch, before the receipt marker.
    g.tick_lan_packets();
    assert!(g.lan.as_ref().unwrap().hub.guests[0].inbox.is_empty());
    let before = snapshot(g);
    assert!(before.hand.is_none(), "original lead was consumed");
    assert!(before.using.is_none());
    let mut host = g.lan.take().unwrap();
    let (yaw, pitch) = (host.hub.guests[0].yaw, host.hub.guests[0].pitch);
    g.lan_guest_packet(
        &mut host,
        1,
        S::UseItem(play::UseItem {
            hand: 0,
            sequence: token,
            yaw,
            pitch,
        }),
    );
    g.lan = Some(host);
    let after = snapshot(g);
    assert_eq!(
        after, before,
        "empty marker changed actual movement/use bookkeeping at {token}"
    );
    println!("marker {token}: before={before:?}, after={after:?}");
    // The existing network tick flushes receipts and resets its own packet
    // gate outside a level pass; no movement field is assigned by this test.
    g.tick_lan(false);
}
fn walk_stop(assert_trajectory: bool) {
    let (mut g, _rx) = scene();
    for _ in 0..2 {
        g.tick_boats();
    }
    let id = g.boats[0].id;
    group(
        &mut g,
        vec![S::Interact(play::Interact {
            id: id as i32,
            hand: 0,
            location: [0.; 3],
            secondary: false,
        })],
        1,
    );
    assert!(g.boats[0].leash.is_some());
    assert_eq!(g.boats[0].leash_player, Some([0, 0, 0, 1]));
    g.tick_boats();
    let mut x = 0.5;
    for tick in 1..=30 {
        x += 0.4;
        group(
            &mut g,
            vec![
                S::MovePlayerPos(play::MovePlayerPos {
                    x,
                    y: 100.,
                    z: 0.5,
                    flags: 1,
                }),
                S::ClientTickEnd(horizon_protocol::packets::Empty),
            ],
            tick + 1,
        );
        assert_eq!(
            g.lan.as_ref().unwrap().hub.guests[0].position,
            [x, 100.0, 0.5]
        );
        g.tick_boats();
    }
    for tick in 1..=10 {
        group(
            &mut g,
            vec![S::ClientTickEnd(horizon_protocol::packets::Empty)],
            31 + tick,
        );
        assert_eq!(
            g.lan.as_ref().unwrap().hub.guests[0].known_movement,
            [0.; 3]
        );
        g.tick_boats();
    }
    assert!(g.boats[0].leash.is_some());
    assert!(g.lan.as_ref().unwrap().play[&1].using.is_none());
    if assert_trajectory {
        let p = g.boats[0].position;
        let rounded = [p.x, p.y, p.z].map(|v| format!("{v:.4}").parse::<f64>().unwrap());
        assert_eq!(
            rounded,
            [6.0676, 100., 1.3736],
            "original Java four-decimal still projection; full position {p:?}"
        );
    }
}
#[test]
#[ignore = "needs prepared resources and isolated homes/cwd"]
fn empty_markers_preserve_actual_walk_and_still_movement_bookkeeping() {
    walk_stop(false);
}
#[test]
#[ignore = "needs prepared resources and isolated homes/cwd"]
fn boat_tick_uses_stopped_players_known_movement() {
    walk_stop(true);
}
#[test]
#[ignore = "needs prepared resources and isolated homes/cwd"]
fn walking_then_still_tick_end_preserves_the_accepted_displacement() {
    let (mut g, _rx) = scene();
    let id = g.boats[0].id;
    group(
        &mut g,
        vec![S::Interact(play::Interact {
            id: id as i32,
            hand: 0,
            location: [0.0; 3],
            secondary: false,
        })],
        1,
    );
    group(
        &mut g,
        vec![
            S::MovePlayerPos(play::MovePlayerPos {
                x: 0.9,
                y: 100.0,
                z: 0.5,
                flags: 1,
            }),
            S::ClientTickEnd(horizon_protocol::packets::Empty),
        ],
        2,
    );
    let walking = snapshot(&g);
    assert_eq!(walking.known, [0.4, 0.0, 0.0]);
    assert_eq!(walking.previous, [0.5, 100.0, 0.5]);
    assert_eq!(walking.position, [0.9, 100.0, 0.5]);
    group(
        &mut g,
        vec![S::ClientTickEnd(horizon_protocol::packets::Empty)],
        3,
    );
    let still = snapshot(&g);
    assert_eq!(still.known, [0.0; 3]);
    assert_eq!(still.previous, walking.previous);
    assert_eq!(still.position, walking.position);
    assert!(!still.moved);
}
