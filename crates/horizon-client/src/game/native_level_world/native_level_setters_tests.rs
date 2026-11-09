//! The day time, game rule and weather setters of a native mod's `LevelRef` change what the
//! matching readers and `/time query`, `/gamerule` and `/weather` report.
use super::super::*;
use horizon_client::mod_api::{GameRuleValue, LevelRef, NativeAuthority};

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

fn level(game: &Game<'_>) -> LevelRef {
    game.native_level(game.dim.id()).unwrap()
}

#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn set_day_time_is_time_set() {
    let mut game = game();
    let level = level(&game);
    assert!(game.native_level_set_day_time(level, 18_000));
    assert_eq!(game.native_level_day_time(level), Some(18_000));
    // The same time as `/time set` leaves it.
    assert!(game.run_command("time set 6000"));
    assert_eq!(game.native_level_day_time(level), Some(6_000));
    // A negative time is refused and changes nothing.
    assert!(!game.native_level_set_day_time(level, -1));
    assert_eq!(game.native_level_day_time(level), Some(6_000));
    // The time it already shows is a no-op that succeeds.
    assert!(game.native_level_set_day_time(level, 6_000));
}

#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn set_game_rule_is_gamerule() {
    let mut game = game();
    let level = level(&game);
    for rule in ["advance_time", "minecraft:advance_weather"] {
        assert!(game.native_level_set_game_rule(level, rule, GameRuleValue::Bool(false)));
        assert_eq!(
            game.native_level_game_rule(level, rule),
            Some(GameRuleValue::Bool(false))
        );
    }
    assert!(!game.ui.rules.bool("advance_time"));
    assert!(!game.ui.rules.bool("advance_weather"));
    assert!(game.native_level_set_game_rule(level, "advance_time", GameRuleValue::Bool(true)));
    assert!(game.ui.rules.bool("advance_time"));
    assert!(game.native_level_set_game_rule(level, "random_tick_speed", GameRuleValue::Int(7)));
    assert_eq!(
        game.native_level_game_rule(level, "random_tick_speed"),
        Some(GameRuleValue::Int(7))
    );
    // An unknown rule, the wrong kind of value and a value out of range change nothing.
    assert!(!game.native_level_set_game_rule(level, "no_such_rule", GameRuleValue::Bool(true)));
    assert!(!game.native_level_set_game_rule(level, "pvp", GameRuleValue::Int(1)));
    assert!(!game.native_level_set_game_rule(level, "random_tick_speed", GameRuleValue::Int(-1)));
    assert_eq!(
        game.native_level_game_rule(level, "random_tick_speed"),
        Some(GameRuleValue::Int(7))
    );
}

#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn set_weather_is_weather() {
    let mut game = game();
    let level = level(&game);
    let fade = |game: &mut Game<'_>| (0..120).for_each(|_| game.world.weather.tick_levels());
    assert!(game.native_level_set_weather(level, 0, 5_000, true, true));
    let w = &game.world.weather;
    assert_eq!(
        (w.clear_time, w.rain_time, w.thunder_time),
        (0, 5_000, 5_000)
    );
    assert!(w.raining && w.thundering);
    fade(&mut game);
    assert_eq!(game.native_level_is_raining(level), Some(true));
    assert_eq!(game.native_level_is_thundering(level), Some(true));
    // `/weather clear 700` sets the same parameters.
    assert!(game.native_level_set_weather(level, 700, 0, false, false));
    let w = &game.world.weather;
    assert_eq!((w.clear_time, w.rain_time, w.thunder_time), (700, 0, 0));
    fade(&mut game);
    assert_eq!(game.native_level_is_raining(level), Some(false));
    assert_eq!(game.native_level_weather(level), Some((0.0, 0.0)));
    assert!(game.run_command("weather rain 900"));
    let w = &game.world.weather;
    assert_eq!(
        (w.clear_time, w.rain_time, w.raining, w.thundering),
        (0, 900, true, false)
    );
    assert!(game.native_level_set_weather(level, 0, 900, true, false));
    let w = &game.world.weather;
    assert_eq!(
        (w.clear_time, w.rain_time, w.raining, w.thundering),
        (0, 900, true, false)
    );
    // A negative time changes nothing.
    assert!(!game.native_level_set_weather(level, -1, 0, false, false));
    assert!(game.world.weather.raining);
}
