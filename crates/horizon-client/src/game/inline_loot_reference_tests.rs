//! `ResourceOrIdArgument.lootTable` decodes an inline `/loot` table while the
//! command is parsed: a `minecraft:loot_table` entry's `value` is a
//! `HolderSet<LootTable>`, so an id the registry does not hold fails the
//! parse. The command never runs, an `execute store` around it stores
//! nothing and no item is given
//! (tools/test_inline_loot_reference_parity.py measures the same on vanilla 26.3).
use super::*;
use horizon_client::chunk::Chunk;
use horizon_core::position::ChunkPos;
use std::sync::Arc;

const MISSING_MESSAGE: &str = "Failed to parse structure: Failed to parse either. First: Not a tag id; \
     Second: Failed to parse either. First: Not a list: \"parity:il_none\"; \
     Second: Failed to get element parity:il_none from registry minecraft:loot_table";

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
    game.set_allow_commands(true);
    game.player = Some(PlayerMotion::new(&game.motion, Vec3::new(8.5, 100.0, 8.5)).unwrap());
    game.world
        .chunks
        .insert((0, 0), Arc::new(Chunk::empty(0, 0)));
    game.block_ticks
        .add_empty_chunk(ChunkPos::new(0, 0))
        .unwrap();
    // The world's pack: one diamond, and no pools.
    // One folder per fixture: the tests run in parallel.
    static FIXTURES: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let n = FIXTURES.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "horizon-inline-loot-ref-{}-{n}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    let tables = dir.join("datapacks/il/data/parity/loot_table");
    std::fs::create_dir_all(&tables).unwrap();
    std::fs::write(
        dir.join("datapacks/il/pack.mcmeta"),
        r#"{"pack":{"description":"fixture"}}"#,
    )
    .unwrap();
    std::fs::write(
        tables.join("il_one.json"),
        r#"{"pools":[{"rolls":1,"entries":[{"type":"minecraft:item","name":"minecraft:diamond"}]}]}"#,
    )
    .unwrap();
    std::fs::write(tables.join("il_empty.json"), r#"{"pools":[]}"#).unwrap();
    std::fs::write(
        dir.join("datapacks.json"),
        r#"{"enabled":["file/il"],"disabled":[]}"#,
    )
    .unwrap();
    game.world_root = dir;
    game.world_tables = game.build_world_tables(false, &[]).unwrap().0;
    let _ = horizon_client::structure_blocks::world_loot_data(
        &game.options.resources,
        &game.pack_files(),
    );
    // As the world opens: its registries, with the packs' tables.
    game.build_registries();
    assert!(game.run_command("gamemode survival"));
    assert!(game.run_command("scoreboard objectives add il dummy"));
    game
}

fn reference(value: &str) -> String {
    format!("{{pools:[{{rolls:1,entries:[{{type:\"minecraft:loot_table\",value:{value}}}]}}]}}")
}

fn score(game: &Game<'_>, name: &str) -> i32 {
    game.score
        .board
        .score(name, "il")
        .map_or(i32::MIN, |s| s.value)
}

/// `/loot give @s loot <table>`: whether it ran and what it said first; then,
/// under `execute store` from 31, the stored result and success; then the
/// diamonds the player holds (cleared).
fn give(game: &mut Game<'_>, table: &str) -> (bool, Option<String>, i32, i32, i32) {
    game.chat_capture = Some(Vec::new());
    let ok = game.run_command(&format!("loot give @s loot {table}"));
    let said = game.chat_capture.take().unwrap_or_default();
    assert!(game.run_command("scoreboard players set r il 31"));
    assert!(game.run_command("scoreboard players set s il 31"));
    let _ = game.run_command(&format!(
        "execute store result score r il store success score s il run loot give @s loot {table}"
    ));
    let _ = game.run_command("execute store result score d il run clear @s minecraft:diamond");
    (
        ok,
        said.first().map(horizon_client::text::Text::plain),
        score(game, "r"),
        score(game, "s"),
        score(game, "d"),
    )
}

#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn an_inline_reference_to_a_missing_table_fails_the_parse() {
    let mut game = game();
    // The valid references run: one diamond each time, and nothing for the empty table.
    let (ok, said, r, s, d) = give(&mut game, &reference("\"parity:il_one\""));
    assert_eq!(
        (ok, said.as_deref(), r, s, d),
        (true, Some("Dropped 1 [Diamond]"), 1, 1, 2)
    );
    let (ok, said, r, s, d) = give(&mut game, &reference("\"parity:il_empty\""));
    assert_eq!(
        (ok, said.as_deref(), r, s, d),
        (true, Some("Dropped 0 items"), 0, 1, 0)
    );
    // A missing table, at the top, in a composite's child and in a nested
    // inline table: the parse fails, nothing runs, nothing is stored.
    for table in [
        reference("\"parity:il_none\""),
        "{pools:[{rolls:1,entries:[{type:\"minecraft:alternatives\",children:[{type:\"minecraft:loot_table\",value:\"parity:il_none\"}]}]}]}".to_owned(),
        reference(&reference("\"parity:il_none\"")),
    ] {
        let (ok, said, r, s, d) = give(&mut game, &table);
        assert_eq!(
            (ok, said.as_deref(), r, s, d),
            (false, Some(MISSING_MESSAGE), 31, 31, 0),
            "{table}"
        );
    }
    // The named table is unchanged.
    let (ok, said, r, s, d) = give(&mut game, "parity:il_one");
    assert_eq!(
        (ok, said.as_deref(), r, s, d),
        (true, Some("Dropped 1 [Diamond]"), 1, 1, 2)
    );
}

/// Added controls (after the frozen baseline above). Valid references in a
/// composite's child and in a nested inline table run. The check reads the
/// registry `/reload` refreshes: a table the pack adds is missing before and
/// found after, and the reloaded pack's function that names a missing table
/// inline is refused while the pack loads, as `CommandFunction.parseCommand`.
/// `[fail-before]` marks the assertions the pristine source fails.
#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn inline_references_follow_the_codec_and_the_reloaded_registry() {
    let mut game = game();
    let child_one = "{pools:[{rolls:1,entries:[{type:\"minecraft:alternatives\",children:[{type:\"minecraft:loot_table\",value:\"parity:il_one\"}]}]}]}";
    let (ok, said, r, s, d) = give(&mut game, child_one);
    assert_eq!(
        (ok, said.as_deref(), r, s, d),
        (true, Some("Dropped 1 [Diamond]"), 1, 1, 2)
    );
    let nested_empty = reference(&reference("\"parity:il_empty\""));
    let (ok, said, r, s, d) = give(&mut game, &nested_empty);
    assert_eq!(
        (ok, said.as_deref(), r, s, d),
        (true, Some("Dropped 0 items"), 0, 1, 0)
    );
    // [fail-before] Not in the registry yet.
    let late = reference("\"parity:il_late\"");
    let (ok, _, r, s, d) = give(&mut game, &late);
    assert_eq!((ok, r, s, d), (false, 31, 31, 0), "before /reload");
    // The pack gains the table and two functions; /reload refreshes both.
    let pack = game.world_root.join("datapacks/il/data/parity");
    std::fs::write(
        pack.join("loot_table/il_late.json"),
        r#"{"pools":[{"rolls":1,"entries":[{"type":"minecraft:item","name":"minecraft:diamond"}]}]}"#,
    )
    .unwrap();
    std::fs::create_dir_all(pack.join("function")).unwrap();
    std::fs::write(
        pack.join("function/il_bad.mcfunction"),
        format!("loot give @s loot {}\n", reference("\"parity:il_none\"")),
    )
    .unwrap();
    std::fs::write(
        pack.join("function/il_good.mcfunction"),
        format!("loot give @s loot {late}\n"),
    )
    .unwrap();
    assert!(game.run_command("reload"));
    let (ok, said, r, s, d) = give(&mut game, &late);
    assert_eq!(
        (ok, said.as_deref(), r, s, d),
        (true, Some("Dropped 1 [Diamond]"), 1, 1, 2),
        "after /reload"
    );
    assert!(game.run_command("function parity:il_good"));
    assert!(game.run_command("execute store result score d il run clear @s minecraft:diamond"));
    assert_eq!(score(&game, "d"), 1);
    // [fail-before] The function with the missing reference is not loaded.
    game.chat_capture = Some(Vec::new());
    assert!(!game.run_command("function parity:il_bad"));
    let said = game.chat_capture.take().unwrap_or_default();
    assert_eq!(
        said.first()
            .map(horizon_client::text::Text::plain)
            .as_deref(),
        Some("Unknown function parity:il_bad")
    );
}
