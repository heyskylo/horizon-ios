//! `PhantomSpawner.tick` tests the player's position with `Level.canSeeSky`,
//! "sky light 15 here" (`BlockAndLightGetter.canSeeSky`), not a scan for a
//! full collision cube above. Glass and stained glass are full collision cubes
//! the sky light passes, so phantoms spawn under them; stone, water (which
//! damps the light) and a bottom slab (whose up face occludes the sky-light
//! column) do not. The world is lit by the vanilla light engine, not by hand.
use super::super::*;
use horizon_client::chunk::Chunk;
use horizon_client::light::{LightTables, apply_lighting, light_chunk};
use std::sync::Arc;

/// The prepared resources, or `None` where CI has none (the test then skips).
fn resources() -> Option<std::path::PathBuf> {
    let root = std::env::var_os("HORIZON_TEST_RESOURCES")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../.local/horizon-client")
                .join(horizon_core::VANILLA_VERSION)
        });
    (root.join("occlusion.nbt").is_file() && root.join("entity-types.json").is_file())
        .then_some(root)
}

/// A server's game on `root`: hard difficulty, a long-inhabited chunk, the
/// player at (2.5, 99, 2.5), midnight (sky darken 11) and a fixed level seed.
fn game(root: &std::path::Path) -> Game<'static> {
    let settings = Settings::default();
    let loaded = load_resources(
        root.to_path_buf(),
        None,
        root,
        &settings,
        std::time::Instant::now(),
        false,
        true,
        &Progress::new(),
        &mut |_, _| unreachable!(),
    )
    .unwrap();
    let mut options = Options::parse(std::iter::empty()).unwrap().unwrap();
    options.resources = root.to_path_buf();
    options.execution_role = execution_role::ExecutionRole::IntegratedServer;
    let res: &'static _ = Box::leak(Box::new(loaded.resources));
    let mut game = Game::new(
        res,
        loaded.pack,
        loaded.gui.unwrap(),
        options,
        settings,
        Audio::silent(root),
        None,
    )
    .unwrap();
    game.build_registries();
    game.set_allow_commands(true);
    assert!(game.run_command("difficulty hard"));
    for x in -1..=0 {
        for z in -1..=0 {
            game.world
                .chunks
                .insert((x, z), Arc::new(Chunk::empty(x, z)));
        }
    }
    // `getCurrentDifficultyAt`: an old chunk is harder, so every attempt
    // passes `isHarderThan(random * 3)` and the seed decides only the spawn.
    game.world.inhabited.insert((0, 0), 3_600_000);
    game.world.day_time = 18000;
    game.player = Some(PlayerMotion::new(game.motion, Vec3::new(2.5, 99.0, 2.5)).unwrap());
    game.stats.custom("time_since_rest", i32::MAX);
    game.world.random.set_seed(0x5EED);
    game
}

/// The sky light under a 5×5 roof of `roof` at y=100 (`None`: open sky), lit by
/// `tables`, and the phantoms 64 spawn attempts make for the player under it.
fn attempt(game: &mut Game<'_>, tables: &LightTables, roof: Option<&str>) -> (i32, Vec<Vec3>) {
    let mut chunk = Chunk::empty(0, 0);
    if let Some(roof) = roof {
        let state = game.pack.assets.registry.default_state(roof).unwrap().0;
        for x in 0..5 {
            for z in 0..5 {
                chunk.set(x, 100, z, state);
            }
        }
    }
    let region = [
        [None, None, None],
        [None, Some(&chunk), None],
        [None, None, None],
    ];
    let lights = light_chunk(&region, tables, true);
    apply_lighting(&mut chunk, lights);
    game.world.chunks.insert((0, 0), Arc::new(chunk));
    game.mobs.clear();
    let mut spawned = Vec::new();
    // `random.nextInt(timeSinceRest) >= 72000` almost always passes at
    // `i32::MAX`; 64 attempts leave no room for the draw to decide the test.
    for _ in 0..64 {
        game.stats.phantom_tick = 0;
        game.spawn_phantoms();
        spawned = game
            .mobs
            .iter()
            .filter(|m| m.species() == horizon_client::mobs::Species::Phantom)
            .map(|m| m.position)
            .collect();
        if !spawned.is_empty() {
            break;
        }
    }
    (i32::from(game.world.light(2, 99, 2) >> 4), spawned)
}

#[test]
fn phantoms_spawn_under_a_glass_roof() {
    let Some(root) = resources() else { return };
    let mut game = game(&root);
    let tables = LightTables::load(&root, game.pack.assets.registry, game.res.facts()).unwrap();

    // Open night sky.
    let (sky, phantoms) = attempt(&mut game, &tables, None);
    assert_eq!(sky, 15, "open sky light");
    assert!(!phantoms.is_empty(), "open sky spawns phantoms");

    // Full collision cubes the sky light still passes.
    for roof in ["minecraft:glass", "minecraft:red_stained_glass"] {
        let (sky, phantoms) = attempt(&mut game, &tables, Some(roof));
        assert_eq!(sky, 15, "{roof} passes the sky light down");
        assert!(!phantoms.is_empty(), "phantoms spawn under {roof}");
        // `playerPos.above(20 + nextInt(15)).east(-10 + nextInt(21))...`
        for at in phantoms {
            assert!(
                (119.0..=133.0).contains(&at.y) && (at.x - 2.0).abs() <= 10.5,
                "{at:?} is not in the spawn box above the player"
            );
        }
    }
}

#[test]
fn no_phantoms_under_a_roof_the_sky_light_does_not_reach() {
    let Some(root) = resources() else { return };
    let mut game = game(&root);
    let tables = LightTables::load(&root, game.pack.assets.registry, game.res.facts()).unwrap();

    // Stone stops the light; water and ice damp it by a level; a bottom slab
    // is no full cube yet its up face occludes the sky-light column.
    for roof in [
        "minecraft:stone",
        "minecraft:water",
        "minecraft:ice",
        "minecraft:stone_slab",
    ] {
        let (sky, phantoms) = attempt(&mut game, &tables, Some(roof));
        assert!(sky < 15, "{roof} keeps the sky light out, got {sky}");
        assert!(phantoms.is_empty(), "no phantom under {roof}");
    }
}
