use super::ItemStack;
use crate::game::{Audio, Game, Options, Progress, Settings, load_resources};
use horizon_client::chunk::Chunk;
use horizon_client::lan::{Joined, NetConfig, Out, PlayerSender, guest::Guest, hub::HubConfig};
use horizon_core::position::ChunkPos;
use horizon_protocol::packets::{
    configuration::ClientInformation,
    play::{self, Clientbound, Serverbound},
};
use std::sync::{Arc, atomic::AtomicI32, mpsc};

/// handleUseItemOn swings the player's arm, but sends the swing to the
/// player's own client only when the client did not predict it: a block
/// placed (SUCCESS) is the client's own swing, a bed's use
/// (SUCCESS_SERVER) is not (#9887).
#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn only_a_server_only_use_swings_back_to_the_player() {
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
    options.game_dir =
        Some(std::env::temp_dir().join(format!("horizon-swing-echo-unit-{}", std::process::id())));
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
    game.build_registries();
    game.set_allow_commands(true);
    let stone = game
        .pack
        .assets
        .registry
        .default_state("minecraft:stone")
        .unwrap()
        .0;
    let mut chunk = Chunk::empty(0, 0);
    for x in 0..16 {
        for z in 0..16 {
            chunk.set(x, 199, z, stone);
        }
    }
    game.world.chunks.insert((0, 0), Arc::new(chunk));
    game.block_ticks
        .add_empty_chunk(ChunkPos::new(0, 0))
        .unwrap();
    assert!(game.run_command("forceload add 0 0"));
    assert!(game.run_command("setblock 6 200 4 red_bed[facing=north,part=foot]"));
    assert!(game.run_command("setblock 6 200 3 red_bed[facing=north,part=head]"));
    game.lan_host(
        super::super::lan::LanSettings {
            port: 0,
            guest_commands: true,
            force_game_mode: false,
        },
        NetConfig {
            announce: false,
            ..NetConfig::loopback(0, "swing-echo-test".into(), 2)
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
            uuid: 331,
            name: "SwingGuest".into(),
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
        [4.5, 200.0, 4.5],
        2,
    );
    guest.dimension = "minecraft:overworld".into();
    guest.placed = true;
    guest.loaded = true;
    guest.inventory[0] = Some(ItemStack::new("minecraft:stone", 16));
    guest.selected = 0;
    let mut host = game.lan.take().unwrap();
    host.hub.guests.push(guest);
    let id = host.hub.guests[0].entity_id;
    let swings_back = || {
        let mut n = 0;
        for out in rx.try_iter() {
            let bodies = match out {
                Out::Packet(body) => vec![body],
                Out::Packets(bodies) => bodies,
                Out::Disconnect(_) => panic!("unexpected disconnect"),
            };
            for body in bodies {
                let mut r = horizon_protocol::buf::Reader::new(&body);
                let packet_id = r.varint().unwrap();
                if let Ok(Clientbound::SwingAnimation(p)) = Clientbound::decode(packet_id, r.rest())
                    && p.id == id
                {
                    n += 1;
                }
            }
        }
        n
    };
    let use_on = |pos: [i32; 3], cursor: [f32; 3], sequence: i32| {
        Serverbound::UseItemOn(play::UseItemOn {
            hand: 0,
            pos,
            face: 1,
            cursor,
            inside: false,
            world_border: false,
            sequence,
        })
    };
    // BlockItem.place: SUCCESS, the client's own swing.
    game.lan_guest_packet(&mut host, 2, use_on([4, 199, 6], [0.5, 1.0, 0.5], 1));
    assert_eq!(game.world.block(4, 200, 6).0, stone, "the stone is placed");
    assert_eq!(swings_back(), 0, "a placement is not swung back");
    // The swing ends before the next use can start one.
    for _ in 0..8 {
        game.lan_swings(&mut host);
    }
    host.hub.guests[0].selected = 1;
    // BedBlock.useWithoutItem: SUCCESS_SERVER, the server's swing only.
    game.lan_guest_packet(&mut host, 2, use_on([6, 200, 4], [0.5, 0.5625, 0.5], 2));
    assert_eq!(swings_back(), 1, "a bed's use is swung back");
}
