//! Finding entities across levels for native mods: an entity by UUID in another level (gone once
//! removed) and the markers of a level with their data.
use super::super::*;
use horizon_client::mod_api::NativeAuthority;
use horizon_world::nbt::Tag;

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
    game.native_lifecycle
        .begin_open(NativeAuthority::IntegratedServer);
    game
}

const UUID: &str = "[I;7,0,0,1]";
const UUID_NUMBER: u128 = (7u128 << 96) | 1;

#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn an_entity_in_another_level_is_found_by_uuid_until_removed() {
    let mut game = game();
    let nether = "minecraft:the_nether";
    assert!(game.run_command(&format!(
        "execute in {nether} run summon cow 8.5 70 8.5 {{UUID:{UUID},NoAI:1b}}"
    )));
    // The level in place is still the overworld.
    assert_eq!(game.dim.id(), "minecraft:overworld");
    let (dimension, _, kind) = game.native_entity_by_uuid(UUID_NUMBER).expect("found");
    assert_eq!(
        (dimension.as_str(), kind.as_str()),
        (nether, "minecraft:cow")
    );
    assert_eq!(game.dim.id(), "minecraft:overworld");
    assert_eq!(game.native_entity_by_uuid(UUID_NUMBER + 5), None);
    // A marker is discarded the moment it is killed (a mob dies over a second of ticks).
    assert!(game.run_command(&format!(
        "execute in {nether} run summon marker 9.5 70 9.5 {{UUID:[I;7,0,0,2]}}"
    )));
    let marker = UUID_NUMBER + 1;
    assert_eq!(
        game.native_entity_by_uuid(marker).map(|f| f.2),
        Some("minecraft:marker".to_owned())
    );
    assert!(game.run_command(&format!("execute in {nether} run kill @e[type=marker]")));
    assert_eq!(game.native_entity_by_uuid(marker), None);
}

#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn markers_are_listed_with_their_data() {
    let mut game = game();
    let nether = "minecraft:the_nether";
    let overworld = game.dim.id();
    assert!(game.native_level_markers(overworld).is_empty());
    assert!(game.run_command("summon marker 1.5 80 2.5 {data:{zone:\"spawn\",size:4}}"));
    assert!(game.run_command("summon marker 5.5 80 6.5"));
    assert!(game.run_command("summon cow 3.5 80 3.5"));
    assert!(game.run_command(&format!(
        "execute in {nether} run summon marker 9.5 70 9.5 {{data:{{zone:\"hell\"}}}}"
    )));
    let mut here = game.native_level_markers(overworld);
    here.sort_by(|a, b| a.1[0].total_cmp(&b.1[0]));
    assert_eq!(here.len(), 2, "the cow is no marker");
    assert_eq!(here[0].1, [1.5, 80.0, 2.5]);
    assert_eq!(
        here[0].2.get(&"zone".into()),
        Some(&Tag::String("spawn".into()))
    );
    assert!(here[1].2.is_empty(), "no data reads as an empty compound");
    let there = game.native_level_markers(nether);
    assert_eq!(there.len(), 1);
    assert_eq!(
        there[0].2.get(&"zone".into()),
        Some(&Tag::String("hell".into()))
    );
    assert_eq!(game.dim.id(), overworld);
    assert!(game.native_level_markers("minecraft:nowhere").is_empty());
}
