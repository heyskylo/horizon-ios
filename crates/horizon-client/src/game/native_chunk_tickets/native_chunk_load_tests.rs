//! A native mod asks for a chunk without waiting (`native_chunk_load_request`),
//! polls it, checks it without loading and lets it go.
use super::super::*;
use super::*;
use std::time::{Duration, Instant};

const OVERWORLD: &str = "minecraft:overworld";
const OWNER: &str = "test:loader";
// Far from the spawn the game starts around, so nothing else holds it.
const FAR: [i32; 2] = [200, 200];

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
    game.options.dedicated = Some(Vec::new());
    game.streaming.paused = false;
    game
}

/// Runs the streaming until `done`, for at most a minute.
fn run_until(game: &mut Game<'_>, done: impl Fn(&Game<'_>) -> bool) -> bool {
    let deadline = Instant::now() + Duration::from_secs(60);
    while !done(game) && Instant::now() < deadline {
        game.update_world();
        game.streaming.wait(Duration::from_millis(5));
    }
    done(game)
}

#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn a_requested_chunk_comes_in_after_ticks_and_a_release_lets_it_unload() {
    let mut game = game();
    assert!(!game.native_chunk_is_loaded(OVERWORLD, FAR));
    assert_eq!(
        game.native_chunk_load_state(OWNER, OVERWORLD, FAR),
        NativeChunkLoad::Failed,
        "nothing asked for yet"
    );
    assert!(game.native_chunk_load_request(OWNER, OVERWORLD, FAR));
    assert!(
        !game.native_chunk_load_request(OWNER, OVERWORLD, FAR),
        "a second request of the same owner is not new"
    );
    // The call returned without loading: the chunk is still on its way.
    assert_eq!(
        game.native_chunk_load_state(OWNER, OVERWORLD, FAR),
        NativeChunkLoad::Pending
    );
    assert!(!game.native_chunk_is_loaded(OVERWORLD, FAR));
    assert!(
        run_until(&mut game, |g| g.native_chunk_is_loaded(OVERWORLD, FAR)),
        "the chunk loads over the next ticks"
    );
    assert_eq!(
        game.native_chunk_load_state(OWNER, OVERWORLD, FAR),
        NativeChunkLoad::Loaded
    );
    // Asking whether another chunk is loaded does not load it.
    let other = [FAR[0] + 40, FAR[1]];
    for _ in 0..20 {
        game.update_world();
        game.streaming.wait(Duration::from_millis(5));
    }
    assert!(!game.native_chunk_is_loaded(OVERWORLD, other));
    assert!(!game.world.chunks.contains_key(&(other[0], other[1])));
    // Released, the chunk can unload again.
    assert!(game.native_chunk_load_release(OWNER, OVERWORLD, FAR));
    assert!(!game.native_chunk_load_release(OWNER, OVERWORLD, FAR));
    assert_eq!(
        game.native_chunk_load_state(OWNER, OVERWORLD, FAR),
        NativeChunkLoad::Failed
    );
    assert!(
        run_until(&mut game, |g| !g.native_chunk_is_loaded(OVERWORLD, FAR)),
        "the released chunk unloads"
    );
}

#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn a_refused_request_fails_and_another_owner_keeps_the_chunk_held() {
    let mut game = game();
    assert!(!game.native_chunk_load_request("nonamespace", OVERWORLD, FAR));
    assert!(!game.native_chunk_load_request(OWNER, "minecraft:nowhere", FAR));
    assert!(!game.native_chunk_load_request(OWNER, OVERWORLD, [2_000_000, 0]));
    assert_eq!(
        game.native_chunk_load_state(OWNER, OVERWORLD, FAR),
        NativeChunkLoad::Failed
    );
    assert!(game.native_chunk_load_request("test:a", OVERWORLD, FAR));
    assert!(game.native_chunk_load_request("test:b", OVERWORLD, FAR));
    assert!(run_until(&mut game, |g| g.native_chunk_is_loaded(OVERWORLD, FAR)));
    assert!(game.native_chunk_load_release("test:a", OVERWORLD, FAR));
    for _ in 0..40 {
        game.update_world();
        game.streaming.wait(Duration::from_millis(5));
    }
    assert!(
        game.native_chunk_is_loaded(OVERWORLD, FAR),
        "b still holds it"
    );
    assert_eq!(
        game.native_chunk_load_state("test:a", OVERWORLD, FAR),
        NativeChunkLoad::Failed
    );
    assert_eq!(
        game.native_chunk_load_state("test:b", OVERWORLD, FAR),
        NativeChunkLoad::Loaded
    );
}

#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn off_a_server_nothing_is_requested() {
    let mut game = game();
    game.options.dedicated = None;
    assert!(!game.native_chunk_load_request(OWNER, OVERWORLD, FAR));
    assert!(!game.native_chunk_is_loaded(OVERWORLD, FAR));
}
