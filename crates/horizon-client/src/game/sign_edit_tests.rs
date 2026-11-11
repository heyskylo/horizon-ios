//! `SignBlockEntity.playerWhoMayEdit`: a sign belongs to one editor at a
//! time, refuses applicators (dyes, ink sacs, honeycomb) while somebody edits
//! it, and opens its editor only on plain text
//! (`SignText.hasEditableText`). See `SignBlock.openTextEdit`,
//! `SignBlock.otherPlayerIsEditingSign`, `SignBlockEntity.tick` and
//! `SignBlockEntity.updateSignText`.
use super::*;
use crate::game::{Audio, Options, Progress, Settings, load_resources};
use horizon_client::{
    chunk::Chunk,
    items::ItemStack,
    lan::{Joined, NetConfig, Out, PlayerSender, guest::Guest, hub::HubConfig},
    signs::SignData,
};
use horizon_core::position::ChunkPos;
use horizon_protocol::packets::{configuration::ClientInformation, play::Clientbound};
use std::sync::{Arc, atomic::AtomicI32, mpsc};

const A: u32 = 2;
const B: u32 = 3;
const A_UUID: u128 = 0x5100_0000_0000_000a;
const B_UUID: u128 = 0x5100_0000_0000_000b;
/// The sign both players are next to; both stand on its front.
const SIGN: [i32; 3] = [4, 100, 4];

/// Two players on one world and what each player's client was sent.
struct Edits {
    game: Game<'static>,
    /// The hub, out of `game.lan` as the game holds it while an action runs.
    host: Option<LanHost>,
    inbox: Vec<(u32, mpsc::Receiver<Out>)>,
    sequence: i32,
}

impl Edits {
    /// A fresh world: the blocks `setup` names, two players beside `SIGN`
    /// (A at x+2.5, B at x-1.5, one block south).
    fn new(setup: &[&str]) -> Self {
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
        options.execution_role = crate::game::execution_role::ExecutionRole::IntegratedServer;
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
        game.world
            .chunks
            .insert((0, 0), Arc::new(Chunk::empty(0, 0)));
        game.block_ticks
            .add_empty_chunk(ChunkPos::new(0, 0))
            .unwrap();
        assert!(game.run_command("forceload add 0 0"));
        for command in setup {
            assert!(game.run_command(command), "{command}");
        }
        game.lan_host(
            crate::game::lan::LanSettings {
                port: 0,
                guest_commands: false,
                force_game_mode: false,
            },
            NetConfig::loopback(0, "sign-edit-test".into(), 4),
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
                max_players: 4,
                integrated: false,
                levels: Default::default(),
            },
        )
        .unwrap();
        let mut host = game.lan.take().unwrap();
        let mut inbox = Vec::new();
        for (conn, uuid, name, at) in [
            (A, A_UUID, "EditA", [6.5, 100.0, 4.5]),
            (B, B_UUID, "EditB", [2.5, 100.0, 4.5]),
        ] {
            let (tx, rx) = mpsc::channel();
            let mut guest = Guest::new(
                Joined {
                    conn,
                    uuid,
                    name: name.into(),
                    properties: Vec::new(),
                    info: ClientInformation::new(12),
                    brand: "test".into(),
                    sender: PlayerSender::new(tx, Arc::default()),
                    latency: Arc::new(AtomicI32::new(0)),
                    address: "127.0.0.1:25565".parse().unwrap(),
                    expected: false,
                    configured: Default::default(),
                },
                conn as i32,
                at,
                2,
            );
            guest.placed = true;
            guest.loaded = true;
            guest.dimension = "minecraft:overworld".into();
            host.hub.guests.push(guest);
            inbox.push((conn, rx));
        }
        Self {
            game,
            host: Some(host),
            inbox,
            sequence: 0,
        }
    }
    fn host(&mut self) -> &mut LanHost {
        self.host
            .as_mut()
            .expect("the hub is out only inside a tick")
    }
    /// Runs `packet` as if `conn`'s client had sent it.
    fn packet(&mut self, conn: u32, packet: S) {
        let mut host = self.host.take().expect("the hub is out only inside a tick");
        self.game.lan_guest_packet(&mut host, conn, packet);
        self.host = Some(host);
    }
    /// `Level.tickBlockEntities`, with the hub where the game keeps it
    /// between actions.
    fn tick(&mut self) {
        self.game.lan = self.host.take();
        self.game.tick_block_entities().unwrap();
        self.host = self.game.lan.take();
    }
    /// `PlayerList.remove` for `conn`.
    fn leave(&mut self, conn: u32) {
        self.host().hub.guests.retain(|g| g.conn != conn);
    }
    /// `ServerGamePacketListenerImpl.handleUseItemOn` from `conn` on the top
    /// face of `pos`.
    fn click(&mut self, conn: u32, pos: [i32; 3]) {
        self.sequence += 1;
        let sequence = self.sequence;
        self.packet(
            conn,
            S::UseItemOn(play::UseItemOn {
                hand: 0,
                pos,
                face: 1,
                cursor: [0.5, 1.0, 0.5],
                inside: false,
                world_border: false,
                sequence,
            }),
        );
    }
    /// `ServerGamePacketListenerImpl.handleSignUpdate` from `conn`.
    fn write(&mut self, conn: u32, pos: [i32; 3], lines: [&str; 4]) {
        self.packet(
            conn,
            S::SignUpdate(play::SignUpdate {
                pos,
                lines: lines.map(String::from),
                front: true,
            }),
        );
    }
    fn sign(&self, pos: [i32; 3]) -> &SignData {
        match self
            .game
            .world
            .block_entities
            .get(&(pos[0], pos[1], pos[2]))
        {
            Some(BlockEntity::Sign(sign)) => sign,
            other => panic!("no sign at {pos:?}: {other:?}"),
        }
    }
    /// Puts `stack` in `conn`'s main hand and selects it.
    fn hold(&mut self, conn: u32, stack: ItemStack) {
        let guest = self
            .host()
            .hub
            .guests
            .iter_mut()
            .find(|g| g.conn == conn)
            .unwrap();
        guest.inventory[0] = Some(stack);
        guest.selected = 0;
    }
    /// The count of the stack `conn` holds.
    fn held(&mut self, conn: u32) -> u32 {
        let guest = self
            .host()
            .hub
            .guests
            .iter()
            .find(|g| g.conn == conn)
            .unwrap();
        guest.inventory[guest.selected]
            .as_ref()
            .map_or(0, |s| s.count)
    }
    /// Moves `conn`'s body to `at`.
    fn move_to(&mut self, conn: u32, at: [f64; 3]) {
        let guest = self
            .host()
            .hub
            .guests
            .iter_mut()
            .find(|g| g.conn == conn)
            .unwrap();
        guest.position = at;
        guest.previous_position = at;
    }
    /// Whether `conn`'s client was sent an `OpenSignEditor` since the last
    /// call.
    fn opened(&mut self, conn: u32) -> bool {
        let (_, rx) = self.inbox.iter().find(|(c, _)| *c == conn).unwrap();
        let mut opened = false;
        for out in rx.try_iter() {
            let bodies = match out {
                Out::Packet(body) => vec![body],
                Out::Packets(bodies) => bodies,
                Out::Disconnect(_) => panic!("a player was disconnected"),
            };
            for body in bodies {
                let mut r = horizon_protocol::buf::Reader::new(&body);
                let id = r.varint().unwrap();
                if let Ok(Clientbound::OpenSignEditor(_)) = Clientbound::decode(id, r.rest()) {
                    opened = true;
                }
            }
        }
        opened
    }
}

fn oak_sign() -> &'static str {
    "setblock 4 100 4 oak_sign"
}

/// `SignBlockEntity.updateSignText` writes only for the sign's own editor,
/// and `SignBlock.openTextEdit` refuses the click of everyone else.
#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn one_sign_belongs_to_one_editor() {
    let mut w = Edits::new(&[oak_sign()]);
    w.click(A, SIGN);
    assert!(w.opened(A), "A's click opened no editor");
    // B's click does nothing at all, as in vanilla.
    w.click(B, SIGN);
    assert!(!w.opened(B), "B opened an editor on the sign A edits");
    // B's write is dropped: the sign has one editor, and it is A.
    w.write(B, SIGN, ["B was here", "", "", ""]);
    assert_eq!(
        w.sign(SIGN).front.lines,
        ["", "", "", ""],
        "B's text went on the sign A edits"
    );
    // A's write lands, and the sign is nobody's again.
    w.write(A, SIGN, ["A was here", "", "", ""]);
    assert_eq!(w.sign(SIGN).front.lines[0], "A was here", "A's text");
    // Now B may open it.
    w.click(B, SIGN);
    assert!(w.opened(B), "the released sign stayed A's");
}

/// `SignBlockEntity.tick`: the holder is dropped once they are out of the
/// block interaction range plus four blocks, or gone from the world
/// (`playerIsTooFarAwayToEdit`, `Level.getPlayerByUUID`).
#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn a_sign_lets_its_editor_go_when_they_walk_away_or_leave() {
    let mut w = Edits::new(&[oak_sign()]);
    w.click(A, SIGN);
    assert!(w.opened(A));
    // A standing there keeps the sign, tick after tick.
    w.tick();
    w.tick();
    w.click(B, SIGN);
    assert!(!w.opened(B), "the tick gave a present editor's sign away");
    // Twelve blocks away is past the block interaction range plus four.
    w.move_to(A, [16.5, 100.0, 4.5]);
    w.tick();
    w.click(B, SIGN);
    assert!(w.opened(B), "the sign stayed A's after A walked away");
    // B's write releases it; A comes back and opens it, then leaves.
    w.write(B, SIGN, ["B was here", "", "", ""]);
    w.move_to(A, [6.5, 100.0, 4.5]);
    w.click(A, SIGN);
    assert!(w.opened(A), "A could not open the released sign");
    w.leave(A);
    w.tick();
    w.click(B, SIGN);
    assert!(w.opened(B), "the sign stayed A's after A left the world");
}

/// `SignBlock.useItemOn`: `otherPlayerIsEditingSign` refuses a dye while
/// somebody edits the sign, so nothing is coloured and no dye is used up.
#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn a_dye_is_refused_while_another_player_edits_the_sign() {
    let mut w = Edits::new(&[
        "setblock 4 100 4 oak_sign{front_text:{messages:[\"hello\",\"\",\"\",\"\"]}}",
    ]);
    w.click(A, SIGN);
    assert!(w.opened(A));
    w.hold(B, ItemStack::new("minecraft:red_dye", 4));
    w.click(B, SIGN);
    assert_eq!(
        w.sign(SIGN).front.color,
        15,
        "the dye coloured a sign someone edits"
    );
    assert_eq!(w.held(B), 4, "the refused dye was used up");
    assert!(!w.opened(B), "the dye click opened an editor");
    // A's write releases the sign; the same click now dyes it red.
    w.write(A, SIGN, ["hello", "", "", ""]);
    w.click(B, SIGN);
    assert_eq!(w.sign(SIGN).front.color, 14, "red");
    assert_eq!(w.held(B), 3, "the applied dye was not used up");
}

/// `SignBlock.useWithoutItem`: `SignText.hasEditableText` refuses the editor
/// on a side holding anything but plain text, while a style or an empty line
/// does not count.
#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn the_editor_opens_only_on_plain_text() {
    // The first line of the side (`ComponentSerialization.CODEC` in NBT: a
    // string is a literal, a compound carries the contents), and whether
    // vanilla opens the editor on it.
    let cases = [
        (r#""""#, true),
        (r#"{text:"Stick",color:"red"}"#, true),
        (r#"{translate:"item.minecraft.stick"}"#, false),
        (r#"{score:{name:"Stick",objective:"signs"}}"#, false),
    ];
    let setup: Vec<String> = cases
        .iter()
        .enumerate()
        .map(|(i, (line, _))| {
            format!(
                "setblock {} 100 4 oak_sign{{front_text:{{messages:[{line},\"\",\"\",\"\"]}}}}",
                2 + i * 2
            )
        })
        .collect();
    let setup: Vec<&str> = setup.iter().map(String::as_str).collect();
    let mut w = Edits::new(&setup);
    for (i, (line, editable)) in cases.iter().enumerate() {
        let pos = [2 + (i as i32) * 2, 100, 4];
        w.click(A, pos);
        assert_eq!(w.opened(A), *editable, "case {line}: the editor");
    }
}

/// `SignBlock.setPlacedBy`: a sign placed with text that is not plain text
/// keeps it and opens no editor.
#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn a_placed_sign_with_a_translated_line_opens_no_editor() {
    let mut w = Edits::new(&["setblock 4 99 4 stone", "setblock 6 99 4 stone"]);
    // The sign item's text (`DataComponents.SIGN_TEXT_FRONT`, which
    // `SignBlockEntity.applyImplicitComponents` puts on the placed sign).
    let mut stack = ItemStack::new("minecraft:oak_sign", 1);
    let front = horizon_client::command::snbt::parse_fully(
        r#"{messages:[{translate:"item.minecraft.stick"},"","",""]}"#,
    )
    .unwrap();
    stack
        .data_mut()
        .set_other("minecraft:sign_text_front", Some(front));
    w.hold(A, stack);
    w.click(A, [4, 99, 4]);
    assert_eq!(w.sign(SIGN).front.lines[0], "Stick", "the sign was placed");
    assert!(!w.opened(A), "an editor opened on a translated line");
    // A plain sign item places a sign the editor opens on.
    w.hold(A, ItemStack::new("minecraft:oak_sign", 1));
    w.click(A, [6, 99, 4]);
    let plain = [6, 100, 4];
    assert_eq!(
        w.sign(plain).front.lines,
        ["", "", "", ""],
        "the plain sign"
    );
    assert!(w.opened(A), "a plain placed sign opened no editor");
}
