//! Test instances come from the data packs the world enables
//! (`Registries.TEST_INSTANCE` is loaded from them), not only from the
//! game's own data: a definition only a world pack has is found and runs
//! (tools/test_world_test_def_parity.py measures the same on vanilla 26.3).
use super::*;
use horizon_client::chunk::Chunk;
use horizon_core::geometry::Vec3;
use std::sync::Arc;

/// A world folder with one data pack holding `h275:broken`.
fn world_with_pack(tag: &str) -> std::path::PathBuf {
    let world = std::env::temp_dir().join(format!("h275-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&world);
    let pack = world.join("datapacks/fixture");
    let tests = pack.join("data/h275/test_instance");
    std::fs::create_dir_all(&tests).unwrap();
    std::fs::write(
        pack.join("pack.mcmeta"),
        r#"{"pack":{"description":"H275 fixture","pack_format":100}}"#,
    )
    .unwrap();
    std::fs::write(
        tests.join("broken.json"),
        r#"{"type":"minecraft:block_based","structure":"minecraft:empty","environment":"minecraft:default","max_ticks":20,"required":true}"#,
    )
    .unwrap();
    world
}

fn game(world: &std::path::Path) -> Game<'static> {
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
    game.world_root = world.to_path_buf();
    game.build_registries();
    game.set_allow_commands(true);
    game
}

#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn a_test_instance_from_an_enabled_world_pack_is_found_and_runs() {
    let world = world_with_pack("found");
    let mut game = game(&world);
    // The world opens: its state, registries and test instances.
    game.load_world_state();
    game.ensure_datapacks();
    let enabled = &game.ui.registries.datapacks_enabled;
    assert!(
        enabled.iter().any(|id| id == "file/fixture"),
        "the world's pack is enabled: {enabled:?}"
    );
    let found = game.test_def("h275:broken").expect("the world pack's test");
    assert!(found.block_based && found.required, "its own settings");
    assert!(
        game.test_def("minecraft:always_pass").is_some(),
        "the built-in one"
    );
    assert!(game.test_def("h275:missing").is_none(), "no pack has it");
    // `/test run` places and starts it.
    for cx in -1..=2 {
        for cz in -1..=2 {
            game.world
                .chunks
                .insert((cx, cz), Arc::new(Chunk::empty(cx, cz)));
        }
    }
    game.player = Some(
        horizon_client::motion::PlayerMotion::new(game.motion, Vec3::new(24.5, 100.0, 8.5))
            .unwrap(),
    );
    game.chat_capture = Some(Vec::new());
    assert!(game.run_command("test run h275:broken"));
    let said: Vec<String> = game
        .chat_capture
        .take()
        .unwrap()
        .iter()
        .map(|t| t.plain())
        .collect();
    assert_eq!(
        said.first().map(String::as_str),
        Some("Running 1 test(s)..."),
        "{said:?}"
    );
    // TestBatchSummaryDisplayer tells the command's source as the batch starts.
    assert_eq!(
        said.get(1).map(String::as_str),
        Some("Starting environment minecraft:default batch 0"),
        "{said:?}"
    );
    let _ = std::fs::remove_dir_all(&world);
}

/// A world folder with one pack holding these `test_instance` files.
fn world_with(tag: &str, files: &[(&str, &str)]) -> std::path::PathBuf {
    let world = std::env::temp_dir().join(format!("h275-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&world);
    let pack = world.join("datapacks/fixture");
    std::fs::create_dir_all(&pack).unwrap();
    std::fs::write(
        pack.join("pack.mcmeta"),
        r#"{"pack":{"description":"H275 fixture","pack_format":100}}"#,
    )
    .unwrap();
    for (id, body) in files {
        let (ns, path) = id.split_once(':').unwrap();
        let file = pack.join(format!("data/{ns}/test_instance/{path}.json"));
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(file, body).unwrap();
    }
    world
}

const FAILING: &str = r#"{"type":"minecraft:block_based","structure":"minecraft:empty","environment":"minecraft:default","max_ticks":20,"required":true}"#;

/// Added controls (after the baseline): the world opens with its test
/// instances and keeps them. Files changed after it opened, and packs
/// disabled and reloaded, change nothing. The next world replaces them,
/// cached misses and selector ids included. A pack overrides a built-in id.
/// A game with no world opened reads the game's own data as before.
#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn a_world_keeps_the_test_instances_it_opened_with() {
    let a = world_with(
        "a",
        &[("h275:broken", FAILING), ("minecraft:always_pass", FAILING)],
    );
    let b = world_with("b", &[("h275:other", FAILING)]);
    let mut game = game(&a);
    // No world opened yet: the game's own data, as before.
    let builtin = game.test_def("minecraft:always_pass").expect("built in");
    assert!(!builtin.block_based, "the built-in function test");
    game.load_world_state();
    let ids = |game: &Game<'_>| game.ui.registries.ids("minecraft:test_instance").to_vec();
    assert_eq!(ids(&game), ["h275:broken", "minecraft:always_pass"]);
    assert!(
        game.test_def("minecraft:always_pass").unwrap().block_based,
        "the pack's override"
    );
    assert!(game.test_def("h275:other").is_none(), "a miss in world A");
    // A file changed after the world opened.
    let file = a.join("datapacks/fixture/data/h275/test_instance/broken.json");
    std::fs::write(
        &file,
        FAILING.replace("\"required\":true", "\"required\":false"),
    )
    .unwrap();
    assert!(
        game.test_def("h275:broken").unwrap().required,
        "the opened definition"
    );
    // The pack disabled and the data reloaded.
    assert!(game.run_command("datapack disable \"file/fixture\""));
    assert!(game.run_command("reload"));
    assert!(
        game.test_def("h275:broken").is_some(),
        "kept over /datapack and /reload"
    );
    assert_eq!(ids(&game), ["h275:broken", "minecraft:always_pass"]);
    // World B.
    game.world_root = b.clone();
    game.load_world_state();
    assert_eq!(ids(&game), ["h275:other", "minecraft:always_pass"]);
    assert!(
        game.test_def("h275:broken").is_none(),
        "nothing from world A"
    );
    assert!(
        game.test_def("h275:other").is_some(),
        "world A's miss is gone"
    );
    assert!(
        !game.test_def("minecraft:always_pass").unwrap().block_based,
        "built in again"
    );
    let _ = std::fs::remove_dir_all(&a);
    let _ = std::fs::remove_dir_all(&b);
}
