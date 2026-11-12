use super::super::super::{
    Audio, Game, Options, Progress, Settings, execution_role::ExecutionRole, load_resources,
};
use super::super::{Kind, Station};
use horizon_client::items::ItemStack;
use horizon_protocol::connection::Pipe;
use std::sync::Arc;

/// A client on a server whose data pack adds `fixture:drill`, which the
/// game's own table doesn't have: the registry sync sent its definition.
fn client_on_a_server_with_a_custom_enchantment() -> Game<'static> {
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
        &mut |_, _| unreachable!("no menu handoff"),
    )
    .unwrap();
    let mut options = Options::parse(std::iter::empty()).unwrap().unwrap();
    options.resources = root.clone();
    options.execution_role = ExecutionRole::Client;
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
    let (client, _server) = Pipe::pair();
    game.connect_local(client, "AnvilTest").unwrap();
    let definition = serde_json::json!({
        "description": "Drill", "supported_items": "minecraft:diamond_pickaxe",
        "weight": 1, "max_level": 3, "min_cost": {"base": 1, "per_level_above_first": 0},
        "max_cost": {"base": 1, "per_level_above_first": 0}, "anvil_cost": 1,
        "slots": ["mainhand"], "effects": {}
    });
    let synced = game
        .stations()
        .enchantments
        .with_entries([("fixture:drill", &definition)])
        .expect("the game's own table lacks fixture:drill");
    game.remote.as_mut().unwrap().tables.enchantments = Some(Arc::new(synced));
    game
}

/// `AnvilMenu.createResult` on the client reads the synced registry, so a
/// book of a data pack's enchantment on a pickaxe shows its result, as the
/// server computes it, instead of the red cross (#8688).
#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn a_servers_custom_enchantment_book_gives_an_anvil_result() {
    let mut game = client_on_a_server_with_a_custom_enchantment();
    let mut book = ItemStack::new("minecraft:enchanted_book", 1);
    book.set_crafting_enchantments(vec![("fixture:drill".into(), 2)]);
    let mut station = Station::new(Kind::Anvil, [0, 64, 0]);
    station.inputs[0] = Some(ItemStack::new("minecraft:diamond_pickaxe", 1));
    station.inputs[1] = Some(book);
    game.station = Some(station);

    game.station_changed();

    let station = game.station.as_ref().unwrap();
    let result = station.result.as_ref().expect("an anvil result");
    assert_eq!(&*result.item, "minecraft:diamond_pickaxe");
    assert_eq!(
        result.enchantments(),
        &[(Arc::<str>::from("fixture:drill"), 2)]
    );
    assert!(station.anvil.cost > 0);
}
