//! Added after the original old-fail module: context boundaries, not new runtime breadth.
use super::*;
use horizon_client::{
    lan::{Joined, NetConfig, PlayerSender, guest::Guest, hub::HubConfig},
    motion::PlayerMotion,
};
use horizon_core::geometry::Vec3;
use horizon_protocol::packets::{configuration::ClientInformation, play::Clientbound};
use std::sync::{Arc, atomic::AtomicI32, mpsc};

fn chats(receiver: &mpsc::Receiver<horizon_client::lan::Out>) -> Vec<horizon_world::nbt::Tag> {
    let mut result = Vec::new();
    for sent in receiver.try_iter() {
        let bodies = match sent {
            horizon_client::lan::Out::Packet(p) => vec![p],
            horizon_client::lan::Out::Packets(p) => p,
            horizon_client::lan::Out::Disconnect(_) => panic!("unexpected disconnect"),
        };
        for body in bodies {
            let mut reader = horizon_protocol::buf::Reader::new(&body);
            let id = reader.varint().unwrap();
            if let Clientbound::SystemChat(p) = Clientbound::decode(id, reader.rest()).unwrap() {
                result.push(p.content);
            }
        }
    }
    result
}

#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES and scratch homes/cwd"]
fn added_issuer_actor_missing_guest_and_restoration_controls() {
    let mut game = crate::game::native_machines::tests::server_game();
    game.build_registries();
    game.set_allow_commands(true);
    game.level.health = 20.0;
    game.block_ticks
        .add_empty_chunk(horizon_core::position::BlockPos::new(0, 0, 0).chunk())
        .unwrap();
    assert!(game.run_command("forceload add 0 0"));
    assert!(game.run_command("fill 0 199 0 15 199 15 stone"));
    assert!(game.run_command("summon pig 2.5 200.0 2.5 {UUID:[I;0,0,0,99],NoAI:1b}"));
    let actor = game
        .mobs
        .iter()
        .find(|m| game.persist.uuid_of(m.id) == Some([0, 0, 0, 99]))
        .unwrap()
        .id;
    for line in [
        "scoreboard objectives add context dummy",
        "scoreboard players set result context 99",
        "scoreboard players set success context 99",
    ] {
        assert!(game.run_command(line));
    }
    game.player = Some(PlayerMotion::new(game.motion, Vec3::new(1.5, 200.0, 1.5)).unwrap());
    game.lan_host(
        crate::game::lan::LanSettings {
            port: 0,
            guest_commands: true,
            force_game_mode: false,
        },
        NetConfig {
            announce: false,
            ..NetConfig::loopback(0, "test-summary-context".into(), 4)
        },
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
            max_players: 4,
            integrated: false,
            levels: Default::default(),
        },
    )
    .unwrap();
    let mut receivers = Vec::new();
    for conn in [2, 7] {
        let (tx, rx) = mpsc::channel();
        let mut guest = Guest::new(
            Joined {
                conn,
                uuid: u128::from(conn),
                name: format!("Issuer{conn}"),
                properties: Vec::new(),
                info: ClientInformation::new(2),
                brand: "test".into(),
                sender: PlayerSender::new(tx, Arc::default()),
                latency: Arc::new(AtomicI32::new(0)),
                address: "127.0.0.1:25565".parse().unwrap(),
                expected: false,
                configured: Default::default(),
            },
            (1_000 + conn) as i32,
            [8.5, 200.0, 8.5],
            2,
        );
        guest.placed = true;
        guest.loaded = true;
        guest.permission = 2;
        game.lan.as_mut().unwrap().hub.guests.push(guest);
        receivers.push(rx);
    }
    // Added actor/issuer separation: actual execute-as, with the issuer unchanged.
    assert!(game.run_guest_command_told(crate::game::lan_actions::GuestCommand {
        conn: 2, command: "execute as 00000000-0000-0000-0000-000000000063 store result score result context store success score success context run test run minecraft:always_pass".into(),
        permission: 2, integrated: false, quiet: false }).unwrap().0);
    let context = game
        .game_tests
        .runner
        .as_ref()
        .unwrap()
        .summary_source
        .unwrap();
    assert_eq!(context.source.entity, Some(Target::Mob(actor)));
    assert_eq!(context.recipient, Some(Target::Guest(2)));
    assert_eq!(
        game.score.board.score("result", "context").unwrap().value,
        1
    );
    assert_eq!(
        game.score.board.score("success", "context").unwrap().value,
        1
    );
    for _ in 0..8 {
        game.tick_game_tests();
    }
    for g in &game.lan.as_ref().unwrap().hub.guests {
        g.sender.flush();
    }
    let _ = chats(&receivers[0]);
    let _ = chats(&receivers[1]);
    assert!(
        game.run_guest_command_told(crate::game::lan_actions::GuestCommand {
            conn: 2,
            command: "test run minecraft:always_pass".into(),
            permission: 2,
            integrated: false,
            quiet: false
        })
        .unwrap()
        .0
    );
    assert_eq!(
        game.game_tests
            .runner
            .as_ref()
            .unwrap()
            .summary_source
            .unwrap()
            .source
            .entity,
        Some(Target::Guest(2)),
        "added transient Player-alias normalization control"
    );
    game.flush_game_test_output();
    for g in &game.lan.as_ref().unwrap().hub.guests {
        g.sender.flush();
    }
    let _ = chats(&receivers[0]);
    let _ = chats(&receivers[1]);
    game.enter_command_level(Dim::Nether);
    let outer = Source {
        dimension: Dim::Nether,
        pos: [31.0, 200.0, 31.0],
        silent: true,
        ..game.player_source()
    };
    game.command_source = Some(outer);
    game.chat_capture = Some(Vec::new());
    let before = (game.exec.commands, game.exec.callback);
    let marker = Text::literal("issuer-only-context-control");
    let component = component_nbt(&marker.component());
    game.test_source_output(context, marker.clone(), false);
    for g in &game.lan.as_ref().unwrap().hub.guests {
        g.sender.flush();
    }
    assert_eq!(chats(&receivers[0]), [component]);
    assert!(chats(&receivers[1]).is_empty());
    assert!(game.chat_capture.as_ref().unwrap().is_empty());
    assert_eq!(game.dim, Dim::Nether);
    assert_eq!(game.lan_acting, None);
    assert_eq!(
        game.player.as_ref().unwrap().position,
        Vec3::new(1.5, 200.0, 1.5)
    );
    assert_eq!(
        format!("{:?}", game.command_source.unwrap()),
        format!("{outer:?}")
    );
    assert_eq!(before, (game.exec.commands, game.exec.callback));
    // Added missing-recipient control: no host fallback or operator broadcast.
    game.lan
        .as_mut()
        .unwrap()
        .hub
        .guests
        .retain(|g| g.conn != 2);
    game.test_source_output(context, marker, true);
    game.game_tests
        .outbox
        .push(Out::Coordinates(context, vec![Dim::Overworld]));
    game.flush_game_test_output();
    for g in &game.lan.as_ref().unwrap().hub.guests {
        g.sender.flush();
    }
    assert!(chats(&receivers[1]).is_empty());
    assert!(game.chat_capture.as_ref().unwrap().is_empty());
    assert_eq!(game.dim, Dim::Nether);
    assert_eq!(game.lan_acting, None);
    assert_eq!(
        format!("{:?}", game.command_source.unwrap()),
        format!("{outer:?}")
    );
    assert_eq!(before, (game.exec.commands, game.exec.callback));
    assert_eq!(
        game.score.board.score("result", "context").unwrap().value,
        1
    );
    assert_eq!(
        game.score.board.score("success", "context").unwrap().value,
        1
    );
    // Added GUI/verify path-selection controls: no retained command context.
    assert!(
        RunnerOptions::verify(Grid::new((8, 8), 10))
            .summary_source
            .is_none()
    );
    let info = Info::new(
        999,
        "minecraft:always_pass".into(),
        game.test_def("minecraft:always_pass").unwrap(),
        Some((8, 200, 11)),
        Dim::Overworld,
        Rotation::None,
    );
    game.track_and_start_runner(vec![info], false, None, false);
    assert!(
        game.game_tests
            .runner
            .as_ref()
            .unwrap()
            .summary_source
            .is_none()
    );
    assert!(matches!(
        summary_line(None, Text::literal("GUI path"), true),
        Out::Console { .. }
    ));
}
