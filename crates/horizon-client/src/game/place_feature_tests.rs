//! `PlaceCommand.placeFeature` with an inline feature value
//! (`ResourceOrHolderArgument`): the same `Feature.place` as a named one,
//! reported by `commands.place.feature.success.inline`. The expected results
//! and messages are vanilla 26.3's (tools/test_place_feature_inline_parity.py).
use super::*;
use horizon_client::chunk::Chunk;
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
    growth::install_live_features(&game.res, &game.pack, &game.options);
    assert!(
        game.res.behaviours().live_features().is_some(),
        "live features"
    );
    // `PlaceCommand.checkLoaded`: the 3x3 chunks around the position.
    for cx in -1..=1 {
        for cz in -1..=1 {
            game.world
                .chunks
                .insert((cx, cz), Arc::new(Chunk::empty(cx, cz)));
            game.block_ticks
                .add_empty_chunk(horizon_core::position::ChunkPos::new(cx, cz))
                .unwrap();
        }
    }
    assert!(game.run_command("fill 0 99 0 15 99 15 stone"));
    game.chat_capture = Some(Vec::new());
    game
}

/// Runs `command` and returns whether it succeeded and the text of the one
/// message it sent, as vanilla 26.3 shows it.
fn run(game: &mut Game<'_>, command: &str) -> (bool, Option<String>) {
    game.chat_capture = Some(Vec::new());
    let ok = game.run_command(command);
    let messages = game.chat_capture.take().unwrap_or_default();
    let text = messages.first().map(horizon_client::text::Text::plain);
    (ok, text)
}

fn block_is(game: &mut Game<'_>, at: &str, block: &str) -> bool {
    run(game, &format!("execute if block {at} {block}")).0
}

#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn an_inline_simple_block_feature_places_like_a_named_one() {
    let mut game = game();
    for (feature, at, placed) in [
        (
            r#"{type:"minecraft:simple_block",to_place:{id:"minecraft:stone"}}"#,
            "1 100 1",
            "minecraft:stone",
        ),
        (
            r#"{type:"minecraft:simple_block",to_place:{id:"minecraft:oak_log",properties:{axis:"x"}}}"#,
            "9 100 1",
            "minecraft:oak_log[axis=x]",
        ),
        (
            r#"{type:"minecraft:simple_block",to_place:{type:"minecraft:weighted",entries:[{data:"minecraft:diamond_block",weight:1}]}}"#,
            "13 100 1",
            "minecraft:diamond_block",
        ),
    ] {
        assert!(
            block_is(&mut game, at, "minecraft:air"),
            "{at} starts empty"
        );
        assert_eq!(
            run(&mut game, &format!("place feature {feature} {at}")),
            (
                true,
                Some(format!("Placed feature at {}", at.replace(' ', ", ")))
            ),
            "{feature}"
        );
        assert!(
            block_is(&mut game, at, placed),
            "{feature}: {placed} at {at}"
        );
    }
    // Air is a block the feature can place too.
    assert!(game.run_command("setblock 5 100 1 glass"));
    let air = r#"{type:"minecraft:simple_block",to_place:{id:"minecraft:air"}}"#;
    assert_eq!(
        run(&mut game, &format!("place feature {air} 5 100 1")),
        (true, Some("Placed feature at 5, 100, 1".to_owned()))
    );
    assert!(block_is(&mut game, "5 100 1", "minecraft:air"));
}

/// A feature that does not place fails the command the same way, inline or
/// named; a named feature that places keeps its named message.
#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn inline_and_named_features_fail_and_succeed_alike() {
    let mut game = game();
    let failed = Some("Failed to place feature".to_owned());
    let dandelion = r#"{type:"minecraft:simple_block",to_place:{id:"minecraft:dandelion"}}"#;
    assert_eq!(
        run(&mut game, &format!("place feature {dandelion} 1 100 6")),
        (false, failed.clone())
    );
    assert!(
        block_is(&mut game, "1 100 6", "minecraft:air"),
        "a dandelion cannot stand on stone"
    );
    assert_eq!(
        run(&mut game, "place feature minecraft:sunflower 9 100 6"),
        (false, failed)
    );
    assert_eq!(
        run(&mut game, "place feature minecraft:patch_fire 5 100 6"),
        (
            true,
            Some("Placed \"minecraft:patch_fire\" at 5, 100, 6".to_owned())
        )
    );
    assert!(block_is(&mut game, "5 100 6", "minecraft:fire"));
}

/// `placeFeature` passes the level random to `Feature.place` for an inline
/// value as for a named one: the vanilla `minecraft:flower_default` (a
/// weighted poppy or dandelion) and the same JSON inline pick the same flower
/// and leave the level random at the same point, seed by seed.
#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn an_inline_feature_draws_the_level_random_as_its_named_twin() {
    use horizon_core::random::RandomSource;
    let mut game = game();
    let root = std::path::PathBuf::from(std::env::var_os("HORIZON_TEST_RESOURCES").unwrap());
    let json =
        std::fs::read_to_string(root.join("data/minecraft/worldgen/feature/flower_default.json"))
            .expect("the vanilla feature");
    let inline: String = json.split_whitespace().collect::<Vec<_>>().join(" ");
    let flower = |game: &mut Game<'_>, at: &str| {
        ["minecraft:poppy", "minecraft:dandelion"]
            .into_iter()
            .find(|f| block_is(game, at, f))
    };
    let mut seen = Vec::new();
    for (i, seed) in [1i64, 2, 3, 99, -7, 12345].into_iter().enumerate() {
        let x = i as i32 * 2;
        for z in [10, 12] {
            assert!(game.run_command(&format!("setblock {x} 99 {z} grass_block")));
        }
        game.world.random.set_seed(seed);
        let named_at = format!("{x} 100 10");
        assert!(
            run(
                &mut game,
                &format!("place feature minecraft:flower_default {named_at}")
            )
            .0
        );
        let named = flower(&mut game, &named_at);
        let named_next = game.world.random.clone().next_long();
        game.world.random.set_seed(seed);
        let inline_at = format!("{x} 100 12");
        assert!(run(&mut game, &format!("place feature {inline} {inline_at}")).0);
        assert_eq!(flower(&mut game, &inline_at), named, "seed {seed}");
        assert_eq!(
            game.world.random.clone().next_long(),
            named_next,
            "seed {seed}"
        );
        seen.push(named);
    }
    assert!(
        seen.iter().all(Option::is_some),
        "a flower each time: {seen:?}"
    );
}
