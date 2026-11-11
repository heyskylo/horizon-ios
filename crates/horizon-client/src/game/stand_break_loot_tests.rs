//! A burnt-out armor stand rolls its entity loot table: `ArmorStand.causeDamage`
//! at 0.5 health or below calls `brokenByAnything`, whose `dropAllDeathLoot`
//! rolls `minecraft:entities/armor_stand` (a stand that is not small, with
//! `mob_drops` on) before the equipment pops
//! (tools/test_stand_break_loot_parity.py measures the same on vanilla 26.3).
use super::*;
use horizon_client::chunk::Chunk;
use horizon_core::position::ChunkPos;
use std::sync::Arc;

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
    game.world
        .chunks
        .insert((0, 0), Arc::new(Chunk::empty(0, 0)));
    game.block_ticks
        .add_empty_chunk(ChunkPos::new(0, 0))
        .unwrap();
    assert!(game.run_command("fill 0 99 0 15 99 15 stone"));
    game
}

/// The world pack's table for armor stands: one stick.
fn with_stick_table(game: &mut Game<'_>, name: &str) {
    let dir =
        std::env::temp_dir().join(format!("horizon-stand-loot-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let tables = dir.join("datapacks/stand/data/minecraft/loot_table/entities");
    std::fs::create_dir_all(&tables).unwrap();
    std::fs::write(
        dir.join("datapacks/stand/pack.mcmeta"),
        r#"{"pack":{"description":"fixture"}}"#,
    )
    .unwrap();
    std::fs::write(
        tables.join("armor_stand.json"),
        r#"{"type":"minecraft:entity","pools":[{"rolls":1,"entries":[{"type":"minecraft:item","name":"minecraft:stick"}]}]}"#,
    )
    .unwrap();
    std::fs::write(
        dir.join("datapacks.json"),
        r#"{"enabled":["file/stand"],"disabled":[]}"#,
    )
    .unwrap();
    game.world_root = dir;
    game.world_tables = game.build_world_tables(false, &[]).unwrap().0;
    // World open warms the loot data with the world's packs (game.rs); the
    // break's roll reads that copy, here this test's own, so the stick reaches
    // no other test's stand (#14266).
    let _ = horizon_client::structure_blocks::load_thread_loot_data(
        &game.options.resources,
        &game.pack_files(),
    );
    // The table the pack installed: a direct roll, far from the stand.
    assert!(game.run_command("loot spawn 12.5 100 12.5 loot minecraft:entities/armor_stand"));
    assert!(
        game.run_command(
            "execute if entity @e[type=item,x=12.5,y=100,z=12.5,distance=..1,nbt={Item:{id:\"minecraft:stick\"}}]"
        ),
        "the pack's table is the stand's"
    );
    assert!(game.run_command("kill @e[type=item]"));
}

fn burn(game: &mut Game<'_>, small: bool) {
    let small = if small { ",Small:1b" } else { "" };
    assert!(game.run_command(&format!(
        "summon armor_stand 2.5 100 8.5 {{NoGravity:1b,Fire:200s,Health:0.6f,UUID:[I;23326,0,0,1]{small},\
         equipment:{{head:{{id:\"minecraft:diamond_helmet\",count:1}}}}}}"
    )));
    // Stands tick only in entity-ticking chunks.
    assert!(game.run_command("forceload add 0 0"));
    assert!(game.run_command("tick freeze"));
    assert!(game.run_command("tick step 1"));
    game.tick_server(false).unwrap();
    assert!(
        !game.run_command("execute if entity 00005b1e-0000-0000-0000-000000000001"),
        "the fire breaks the stand"
    );
    assert!(
        game.run_command(
            "execute if entity @e[type=item,nbt={Item:{id:\"minecraft:diamond_helmet\"}}]"
        ),
        "its helmet pops"
    );
}

const STICK: &str = "execute if entity @e[type=item,x=2.5,y=100,z=8.5,distance=..3,nbt={Item:{id:\"minecraft:stick\"}}]";

#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn a_burnt_out_stand_drops_its_loot_table() {
    let mut game = game();
    with_stick_table(&mut game, "normal");
    burn(&mut game, false);
    assert!(game.run_command(STICK), "the table's stick drops");
}

#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn a_small_burnt_out_stand_drops_no_loot() {
    // ArmorStand.isBaby is isSmall: LivingEntity.shouldDropLoot is false.
    let mut game = game();
    with_stick_table(&mut game, "small");
    burn(&mut game, true);
    assert!(!game.run_command(STICK));
}

// Controls added after the baseline: the roll is gated by mob_drops, and one
// break rolls the table once.

fn sticks(game: &mut Game<'_>) -> String {
    game.chat_capture = Some(Vec::new());
    game.run_command(STICK);
    let said = game.chat_capture.take().unwrap_or_default();
    said.iter().map(horizon_client::text::Text::plain).collect()
}

#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn control_mob_drops_off_rolls_nothing() {
    let mut game = game();
    with_stick_table(&mut game, "nodrops");
    assert!(game.run_command("gamerule mob_drops false"));
    burn(&mut game, false);
    assert!(!game.run_command(STICK));
}

#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn control_one_break_rolls_the_table_once() {
    let mut game = game();
    with_stick_table(&mut game, "once");
    burn(&mut game, false);
    assert_eq!(sticks(&mut game), "Test passed. Count: 1");
    for _ in 0..20 {
        assert!(game.run_command("tick step 1"));
        game.tick_server(false).unwrap();
    }
    assert_eq!(sticks(&mut game), "Test passed. Count: 1");
}
