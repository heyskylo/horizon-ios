//! `/clone from <sourceDimension>` / `to <targetDimension>`: each name is
//! resolved (`DimensionArgument.getDimension`) as its position is read, so an
//! unknown one fails the command with `argument.dimension.invalid` before
//! anything is cloned (tools/test_clone_dimensions_parity.py measures the
//! same on vanilla 26.3).
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
    assert!(game.run_command("setblock 0 100 0 chest{CustomName:'\"h236\"'}"));
    game
}

/// Runs `command`: whether it succeeded and the key of the error it raised.
fn run(game: &mut Game<'_>, command: &str) -> (bool, Option<String>) {
    game.chat_capture = Some(Vec::new());
    let ok = game.run_command(command);
    let said = game.chat_capture.take().unwrap_or_default();
    (ok, said.first().map(horizon_client::text::Text::plain))
}

#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn an_unknown_clone_dimension_fails_and_clones_nothing() {
    let mut game = game();
    for (command, x, unknown) in [
        (
            "clone from h236:none 0 100 0 0 100 0 4 100 0",
            4,
            "h236:none",
        ),
        ("clone 0 100 0 0 100 0 to h236:none 6 100 0", 6, "h236:none"),
        (
            "clone from h236:first 0 100 0 0 100 0 to h236:second 8 100 0",
            8,
            "h236:first",
        ),
    ] {
        assert!(game.run_command(&format!("setblock {x} 100 0 gold_block")));
        assert_eq!(
            run(&mut game, command),
            (false, Some(format!("Unknown dimension '{unknown}'"))),
            "{command}"
        );
        assert!(
            run(&mut game, &format!("execute if block {x} 100 0 gold_block")).0,
            "{command}: the destination stays"
        );
    }
    // The same level, named or not, still clones.
    assert!(run(&mut game, "clone 0 100 0 0 100 0 10 100 0").0);
    assert!(
        run(
            &mut game,
            "clone from minecraft:overworld 0 100 0 0 100 0 to minecraft:overworld 12 100 0"
        )
        .0
    );
    assert!(run(&mut game, "execute if block 12 100 0 chest").0);
}

/// Opens `dim`'s level through the same adapter the clone uses and gives it
/// the empty chunk at the origin, then returns to `back`.
fn load_chunk_in(game: &mut Game<'_>, dim: Dim, back: Dim) {
    game.enter_command_level(dim);
    assert_eq!(game.dim, dim, "the level is in place");
    game.world
        .chunks
        .insert((0, 0), Arc::new(Chunk::empty(0, 0)));
    let _ = game.block_ticks.add_empty_chunk(ChunkPos::new(0, 0));
    game.enter_command_level(back);
    assert_eq!(game.dim, back, "back in the first level");
}

fn block_in(game: &mut Game<'_>, dim: &str, at: &str, block: &str) -> bool {
    run(game, &format!("execute in {dim} if block {at} {block}")).0
}

/// Added controls (after the baseline): a valid other dimension clones into
/// that level's own world, not into the command's, and the command's level
/// is back in place after every success and every failure.
#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn a_clone_to_another_dimension_lands_in_that_world_and_comes_back() {
    let mut game = game();
    let here = game.dim;
    assert_eq!(here, Dim::Overworld);
    load_chunk_in(&mut game, Dim::Nether, here);
    assert!(game.run_command("execute in minecraft:the_nether run setblock 14 100 0 gold_block"));
    assert!(block_in(
        &mut game,
        "minecraft:the_nether",
        "14 100 0",
        "gold_block"
    ));
    // Into the nether.
    assert_eq!(
        run(
            &mut game,
            "clone from minecraft:overworld 0 100 0 0 100 0 to minecraft:the_nether 14 100 0"
        ),
        (true, Some("Successfully cloned 1 block(s)".to_owned()))
    );
    assert_eq!(game.dim, here, "the command's level is back");
    assert!(block_in(
        &mut game,
        "minecraft:the_nether",
        "14 100 0",
        "chest"
    ));
    assert!(
        run(
            &mut game,
            "execute in minecraft:the_nether if data block 14 100 0 {CustomName:'\"h236\"'}"
        )
        .0,
        "the chest's name came along"
    );
    assert!(
        block_in(&mut game, "minecraft:overworld", "14 100 0", "air"),
        "not in the overworld"
    );
    assert!(
        block_in(&mut game, "minecraft:overworld", "0 100 0", "chest"),
        "the source stays"
    );
    // The same coordinates in two levels do not overlap.
    assert!(
        run(
            &mut game,
            "clone from minecraft:overworld 0 100 0 0 100 0 to minecraft:the_nether 0 100 0"
        )
        .0
    );
    assert_eq!(game.dim, here);
    assert!(block_in(
        &mut game,
        "minecraft:the_nether",
        "0 100 0",
        "chest"
    ));
    // Failures leave both worlds and the level as they were.
    assert!(game.run_command("execute in minecraft:the_nether run setblock 2 100 0 gold_block"));
    for (command, said) in [
        (
            "clone 0 100 0 0 100 0 to h236:none 2 100 0",
            "Unknown dimension 'h236:none'",
        ),
        (
            "clone 0 100 0 0 100 0 to minecraft:the_nether 40 100 40",
            "That position is not loaded",
        ),
    ] {
        assert_eq!(
            run(&mut game, command),
            (false, Some(said.to_owned())),
            "{command}"
        );
        assert_eq!(game.dim, here, "{command}: the level is back");
        assert!(block_in(
            &mut game,
            "minecraft:the_nether",
            "2 100 0",
            "gold_block"
        ));
        assert!(block_in(
            &mut game,
            "minecraft:overworld",
            "0 100 0",
            "chest"
        ));
    }
}
