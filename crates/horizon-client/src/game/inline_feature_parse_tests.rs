//! `ResourceOrIdArgument` decodes an inline `/place feature` value with the
//! feature codec while the command is parsed: a malformed one fails the
//! parse, so the command never runs and an `execute store` around it stores
//! nothing (tools/test_inline_feature_parse_parity.py measures the same on
//! vanilla 26.3).
use super::*;
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
    // As a world opens: the live features are installed while it is
    // prepared, then its registries are built (`load_world_state`).
    growth::install_live_features(&game.res, &game.pack, &game.options);
    assert!(
        game.res.behaviours().live_features().is_some(),
        "live features"
    );
    game.build_registries();
    game.set_allow_commands(true);
    // PlaceCommand.checkLoaded: the chunks around the target.
    for cx in -1..=1 {
        for cz in -1..=1 {
            game.world.chunks.insert(
                (cx, cz),
                Arc::new(horizon_client::chunk::Chunk::empty(cx, cz)),
            );
            game.block_ticks
                .add_empty_chunk(horizon_core::position::ChunkPos::new(cx, cz))
                .unwrap();
        }
    }
    game
}

/// Runs `command`: whether it succeeded and what it said first.
fn run(game: &mut Game<'_>, command: &str) -> (bool, Option<String>) {
    game.chat_capture = Some(Vec::new());
    let ok = game.run_command(command);
    let said = game.chat_capture.take().unwrap_or_default();
    (ok, said.first().map(horizon_client::text::Text::plain))
}

/// The stored result and success of `execute store … run place feature
/// <feature> 7 100 8`, from -1, and whether the target is still air.
fn stored(game: &mut Game<'_>, feature: &str) -> (i32, i32, bool) {
    assert!(game.run_command("scoreboard players set r h171 -1"));
    assert!(game.run_command("scoreboard players set s h171 -1"));
    let _ = game.run_command("setblock 7 100 8 air");
    let _ = run(
        game,
        &format!(
            "execute store result score r h171 store success score s h171 run place feature {feature} 7 100 8"
        ),
    );
    let score = |g: &Game<'_>, name: &str| {
        g.score
            .board
            .score(name, "h171")
            .map_or(i32::MIN, |s| s.value)
    };
    let air = run(game, "execute if block 7 100 8 air").0;
    (score(game, "r"), score(game, "s"), air)
}

#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn a_malformed_inline_feature_fails_before_the_command_runs() {
    let mut game = game();
    assert!(game.run_command("fill 0 99 0 15 99 15 stone"));
    assert!(game.run_command("scoreboard objectives add h171 dummy"));
    // A valid inline feature places; one that decodes but does not place fails when it runs.
    assert_eq!(
        stored(
            &mut game,
            "{type:\"minecraft:simple_block\",to_place:{id:\"minecraft:stone\"}}"
        ),
        (1, 1, false)
    );
    assert_eq!(
        stored(
            &mut game,
            "{type:\"minecraft:simple_block\",to_place:{id:\"minecraft:dandelion\"}}"
        ),
        (0, 0, true)
    );
    // Malformed: the parse fails, so nothing runs and nothing is stored.
    for feature in [
        "{type:\"minecraft:simple_block\"}",
        "{type:\"minecraft:h171_unknown\"}",
    ] {
        assert_eq!(stored(&mut game, feature), (-1, -1, true), "{feature}");
        let (ok, said) = run(&mut game, &format!("place feature {feature} 7 100 8"));
        assert!(!ok);
        assert!(
            said.as_deref()
                .is_some_and(|s| s.starts_with("Failed to parse structure: ")),
            "{feature}: {said:?}"
        );
    }
}

/// Added controls (after the baseline): with the server's live features, a
/// malformed inline feature is refused while the command is parsed and never
/// reaches placement (no block is written); with no live features the parse
/// leaves the decode to the run, as before (preservation, not parity).
#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn the_parse_check_is_the_live_feature_codec_and_only_where_it_exists() {
    let mut game = game();
    assert!(game.run_command("fill 0 99 0 15 99 15 stone"));
    assert!(game.run_command("scoreboard objectives add h171 dummy"));
    assert!(
        game.ui.registries.inline_feature.is_some(),
        "the server has it"
    );
    let malformed = "{type:\"minecraft:simple_block\"}";
    let before = game.world.revision;
    let (ok, said) = run(&mut game, &format!("place feature {malformed} 7 100 8"));
    assert!(!ok);
    assert!(
        said.as_deref()
            .is_some_and(|s| s.starts_with("Failed to parse structure: ")),
        "{said:?}"
    );
    assert_eq!(game.world.revision, before, "nothing was placed or written");
    // The valid controls still place, and still fail to place, when they run.
    assert_eq!(
        stored(
            &mut game,
            "{type:\"minecraft:simple_block\",to_place:{id:\"minecraft:stone\"}}"
        ),
        (1, 1, false)
    );
    assert_eq!(
        stored(
            &mut game,
            "{type:\"minecraft:simple_block\",to_place:{id:\"minecraft:dandelion\"}}"
        ),
        (0, 0, true)
    );
    // No live features: the old run-time failure.
    game.ui.registries.inline_feature = None;
    assert_eq!(stored(&mut game, malformed), (0, 0, true));
    let (ok, said) = run(&mut game, &format!("place feature {malformed} 7 100 8"));
    assert!(!ok);
    assert_eq!(
        said.as_deref(),
        Some("An unexpected error occurred while trying to execute that command")
    );
}
