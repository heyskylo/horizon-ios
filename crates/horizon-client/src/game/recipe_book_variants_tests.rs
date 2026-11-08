//! A recipe with several displays reaches a connected player's recipe book:
//! `ServerRecipeBook.addRecipes` sends a `ClientboundRecipeBookAddPacket.Entry`
//! for each display (`RecipeManager.listDisplaysForRecipe`), so
//! `minecraft:map_cloning`, a transmute recipe with a material count of 1 to 8,
//! sends eight (tools/test_recipe_book_variants_parity.py measures the same on
//! vanilla 26.3 with a real client).
//! `TestCommand.showPos` under `execute as <guest>`: the source player is
//! the guest, and its own `pick` (eyes and view) finds the block, while the
//! message goes to the command's issuer
//! (tools/test_test_pos_guest_parity.py measures the same on vanilla 26.3).
use super::*;
use horizon_client::{
    chunk::Chunk,
    lan::{Joined, NetConfig, PlayerSender, guest::Guest, hub::HubConfig},
    motion::PlayerMotion,
};
use horizon_protocol::packets::configuration::ClientInformation;
use std::sync::{Arc, atomic::AtomicI32, mpsc};

const GUEST: &str = "RecipeGuest";

/// A host on a stone floor with one connected, placed guest (conn 2).
fn game() -> (Game<'static>, mpsc::Receiver<horizon_client::lan::Out>) {
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
    for x in 0..2 {
        game.world
            .chunks
            .insert((x, 0), Arc::new(Chunk::empty(x, 0)));
    }
    assert!(game.run_command("fill 0 99 0 31 99 15 stone"));
    game.player = Some(PlayerMotion::new(game.motion, Vec3::new(28.5, 100.0, 10.5)).unwrap());
    let net = NetConfig {
        announce: false,
        ..NetConfig::lan(0, "recipe-variants-test".into(), 2)
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
    let (tx, rx) = mpsc::channel();
    let mut guest = Guest::new(
        Joined {
            conn: 2,
            uuid: 2,
            name: GUEST.into(),
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
        [12.5, 100.0, 10.5],
        1,
    );
    guest.placed = true;
    guest.loaded = true;
    // Its view, as the client sent it: straight down.
    guest.pitch = 90.0;
    game.lan.as_mut().unwrap().hub.guests.push(guest);
    (game, rx)
}

/// Runs `command`: whether it succeeded and what the issuer was told.
fn run(game: &mut Game<'_>, command: &str) -> (bool, Vec<String>) {
    game.chat_capture = Some(Vec::new());
    let ok = game.run_command(command);
    let said = game.chat_capture.take().unwrap_or_default();
    (
        ok,
        said.iter().map(horizon_client::text::Text::plain).collect(),
    )
}

/// The display ids of every recipe book Add the guest was sent.
fn added(rx: &mpsc::Receiver<horizon_client::lan::Out>) -> Vec<Vec<i32>> {
    use horizon_protocol::packets::play::Clientbound as C;
    let mut result = Vec::new();
    for out in rx.try_iter() {
        let bodies = match out {
            horizon_client::lan::Out::Packet(body) => vec![body],
            horizon_client::lan::Out::Packets(bodies) => bodies,
            horizon_client::lan::Out::Disconnect(_) => panic!("unexpected disconnect"),
        };
        for body in bodies {
            let mut reader = horizon_protocol::buf::Reader::new(&body);
            let id = reader.varint().unwrap();
            if let Ok(C::RecipeBookAdd(add)) = C::decode(id, reader.rest()) {
                result.push(add.entries.iter().map(|e| e.id).collect());
            }
        }
    }
    result
}

fn step(game: &mut Game<'_>, ticks: usize) {
    for _ in 0..ticks {
        assert!(game.run_command("tick step 1"));
        game.tick_server(false).unwrap();
    }
}

fn grant(
    game: &mut Game<'_>,
    rx: &mpsc::Receiver<horizon_client::lan::Out>,
    recipe: &str,
) -> Vec<Vec<i32>> {
    let _ = added(rx);
    let (ok, said) = run(game, &format!("recipe give {GUEST} {recipe}"));
    assert!(ok, "{said:?}");
    // The book's packets go out with a later tick's sync (measured: the next
    // one but one); two ticks take them all.
    step(game, 2);
    added(rx)
}

#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn a_recipe_with_eight_displays_sends_eight_entries() {
    let (mut game, rx) = game();
    assert!(game.run_command("tick freeze"));
    // The join's own book (a new player knows the crafting table) settles on
    // these ticks; then the book is emptied.
    step(&mut game, 3);
    let _ = run(&mut game, &format!("recipe take {GUEST} *"));
    step(&mut game, 2);
    let _ = added(&rx);
    let ordinary = grant(&mut game, &rx, "minecraft:crafting_table");
    assert_eq!(
        ordinary.iter().map(Vec::len).sum::<usize>(),
        1,
        "the ordinary recipe: {ordinary:?}"
    );
    let cloning = grant(&mut game, &rx, "minecraft:map_cloning");
    let ids: Vec<i32> = cloning.concat();
    assert_eq!(ids.len(), 8, "one entry per display: {cloning:?}");
    let mut distinct = ids.clone();
    distinct.sort_unstable();
    distinct.dedup();
    assert_eq!(distinct.len(), 8);
}

// Added after the baseline (the authorized adapter): a recipe with several
// displays still has no ghost (PlaceRecipe is unchanged), and an ordinary
// recipe's ghost is still sent.
fn ghosts(rx: &mpsc::Receiver<horizon_client::lan::Out>) -> usize {
    use horizon_protocol::packets::play::Clientbound as C;
    let mut n = 0;
    for out in rx.try_iter() {
        let bodies = match out {
            horizon_client::lan::Out::Packet(body) => vec![body],
            horizon_client::lan::Out::Packets(bodies) => bodies,
            horizon_client::lan::Out::Disconnect(_) => panic!("unexpected disconnect"),
        };
        for body in bodies {
            let mut reader = horizon_protocol::buf::Reader::new(&body);
            let id = reader.varint().unwrap();
            if let Ok(C::PlaceGhostRecipe(_)) = C::decode(id, reader.rest()) {
                n += 1;
            }
        }
    }
    n
}

#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn control_a_multi_display_recipe_still_has_no_ghost() {
    let (mut game, rx) = game();
    let _ = ghosts(&rx);
    let hub = &mut game.lan.as_mut().unwrap().hub;
    hub.send_ghost_recipe(2, 0, "minecraft:map_cloning");
    assert_eq!(ghosts(&rx), 0, "no ghost for eight displays");
    let hub = &mut game.lan.as_mut().unwrap().hub;
    hub.send_ghost_recipe(2, 0, "minecraft:crafting_table");
    assert_eq!(ghosts(&rx), 1, "the ordinary recipe's ghost");
}
