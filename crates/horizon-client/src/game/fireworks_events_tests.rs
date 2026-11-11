//! `FireworkRocketEntity.explode`'s game event and sound gate, and the
//! `ITEM_USED` an `useOn` counts. Prepared resources only.
use super::Game;
use crate::game::{Audio, Options, Progress, Settings, load_resources};
use horizon_client::{
    chunk::Chunk,
    items::ItemStack,
    mobs::Who,
    particles::{FireworkExplosion, FireworkShape, ParticleLevel},
};
use horizon_core::{geometry::Vec3, position::ChunkPos, random::LegacyRandom};
use std::sync::Arc;

fn with_game(test: impl FnOnce(&mut Game<'_>)) {
    let root = std::path::PathBuf::from(std::env::var_os("HORIZON_TEST_RESOURCES").unwrap());
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
    options.execution_role = crate::game::execution_role::ExecutionRole::IntegratedServer;
    let mut game = Game::new(
        &loaded.resources,
        loaded.pack,
        loaded.gui.unwrap(),
        options,
        settings,
        Audio::silent(&root),
        None,
    )
    .unwrap();
    game.world
        .chunks
        .insert((0, 0), Arc::new(Chunk::empty(0, 0)));
    game.block_ticks
        .add_empty_chunk(ChunkPos::new(0, 0))
        .unwrap();
    game.player = Some(
        horizon_client::motion::PlayerMotion::new(game.motion, Vec3::new(2.5, 200.0, 2.5)).unwrap(),
    );
    game.world.random = horizon_core::random::LevelRandom::for_level(LegacyRandom::new(114));
    test(&mut game);
}

/// One `small_ball` that twinkles, as `/summon` reads it: a `Starter` of 16
/// ticks that plays a blast first and a twinkle last.
fn twinkling() -> FireworkExplosion {
    FireworkExplosion {
        shape: FireworkShape::SmallBall,
        colors: vec![0xFF5555],
        fade: Vec::new(),
        trail: true,
        twinkle: true,
    }
}

fn tick_particles(game: &mut Game<'_>) {
    let level = ParticleLevel {
        world: &game.world,
        facts: game.res.facts(),
        collision: game.collision,
        player: None,
        cached_chunk: std::cell::Cell::new(None),
    };
    game.particles.tick(&level);
}

/// `FireworkParticles.Starter.playSound`: a silent rocket launches quietly (as
/// it already did) and bursts quietly too, drawing its sparks and flash only.
#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn a_silent_rocket_bursts_without_blast_or_twinkle() {
    with_game(|game| {
        let stack = ItemStack::new("minecraft:firework_rocket", 1);
        let mut launching = game.new_rocket(
            Vec3::new(6.5, 201.5, 10.5),
            &stack,
            false,
            &mut LegacyRandom::new(0),
        );
        launching.silent = true;
        launching.lifetime = 100;
        game.rockets.push(launching);
        game.audio.capture = Some(Vec::new());
        game.tick_rockets();
        let heard = game.audio.capture.take().unwrap_or_default();
        assert!(
            heard
                .iter()
                .all(|s| s.event != "minecraft:entity.firework_rocket.launch"),
            "a silent rocket launches quietly: {heard:?}"
        );
        game.rockets.clear();

        let (quiet_at, loud_at) = ([6.5, 201.5, 10.5], [12.5, 201.5, 10.5]);
        let mut quiet = game.new_rocket(
            Vec3::new(quiet_at[0], quiet_at[1], quiet_at[2]),
            &stack,
            false,
            &mut LegacyRandom::new(0),
        );
        quiet.silent = true;
        quiet.explosions = vec![twinkling()];
        let mut loud = game.new_rocket(
            Vec3::new(loud_at[0], loud_at[1], loud_at[2]),
            &stack,
            false,
            &mut LegacyRandom::new(0),
        );
        loud.explosions = vec![twinkling()];
        game.explode_rocket(&quiet);
        game.explode_rocket(&loud);
        game.particles.sounds.clear();
        for _ in 0..20 {
            tick_particles(game);
        }
        let bursts: Vec<(&str, [f64; 3])> = game
            .particles
            .sounds
            .iter()
            .filter(|s| s.0.starts_with("minecraft:entity.firework_rocket."))
            .map(|s| (s.0, s.1))
            .collect();
        assert_eq!(
            bursts,
            vec![
                ("minecraft:entity.firework_rocket.blast", loud_at),
                ("minecraft:entity.firework_rocket.twinkle", loud_at),
            ],
            "only the loud rocket bangs and twinkles, both at its own position"
        );
        assert!(
            !game.particles.is_empty(),
            "the silent rocket still drew its sparks and flash"
        );
    });
}

/// Recording of `Game::post_game_event`, as the sculk tests read it.
fn start_events() {
    crate::game::sculk::TEST_POSTED_EVENTS.with(|e| *e.borrow_mut() = Some(Vec::new()));
}

fn take_explosions() -> Vec<(&'static str, [f64; 3], Option<Who>)> {
    crate::game::sculk::TEST_POSTED_EVENTS.with(|e| {
        std::mem::take(e.borrow_mut().as_mut().unwrap())
            .into_iter()
            .filter(|e| e.0 == "minecraft:explode")
            .collect()
    })
}

fn stop_events() {
    crate::game::sculk::TEST_POSTED_EVENTS.with(|e| *e.borrow_mut() = None);
}

/// `FireworkRocketEntity.explode`: `gameEvent(GameEvent.EXPLODE, getOwner())` at
/// the rocket, once, with and without explosions; a rocket nobody owns (a
/// dispenser's) has no source entity.
#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn a_detonating_rocket_posts_one_explode_from_its_owner() {
    with_game(|game| {
        let stack = ItemStack::new("minecraft:firework_rocket", 1);
        let mut burst = game.new_rocket(
            Vec3::new(8.5, 202.0, 8.5),
            &stack,
            false,
            &mut LegacyRandom::new(0),
        );
        burst.explosions = vec![twinkling()];
        burst.owner = Some(Who::Player);
        let mut plain = game.new_rocket(
            Vec3::new(9.5, 202.0, 8.5),
            &stack,
            false,
            &mut LegacyRandom::new(0),
        );
        plain.owner = Some(Who::Player);
        let mut stray = game.new_rocket(
            Vec3::new(10.5, 202.0, 8.5),
            &stack,
            false,
            &mut LegacyRandom::new(0),
        );
        stray.explosions = vec![twinkling()];

        start_events();
        game.explode_rocket(&burst);
        assert_eq!(
            take_explosions(),
            vec![("minecraft:explode", [8.5, 202.0, 8.5], Some(Who::Player))],
            "the burst is heard once, at the rocket, from the player who shot it"
        );
        game.explode_rocket(&plain);
        assert_eq!(
            take_explosions(),
            vec![("minecraft:explode", [9.5, 202.0, 8.5], Some(Who::Player))],
            "a rocket with no explosions still posts it"
        );
        game.explode_rocket(&stray);
        assert_eq!(
            take_explosions(),
            vec![("minecraft:explode", [10.5, 202.0, 8.5], None)],
            "`getOwner()` is null: no source entity"
        );
        stop_events();
    });
}

/// `ItemStack.useOn` awards `ITEM_USED` for `FireworkRocketItem.useOn`'s
/// SUCCESS; `FireworkRocketItem.use` while gliding awards its own.
#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn launching_from_a_block_and_while_gliding_use_the_rocket_up() {
    with_game(|game| {
        game.creative = false;
        let stack = ItemStack::new("minecraft:firework_rocket", 1);
        let hit = super::Hit {
            pos: [2, 199, 2],
            face: [0, 1, 0],
            point: [2.5, 200.0, 2.5],
        };
        for used in 1..=5 {
            game.inventory[game.selected] = Some(stack.clone());
            assert!(game.launch_firework(&hit, &stack), "a rocket goes up");
            assert_eq!(
                game.stats.get("used", "minecraft:firework_rocket"),
                used,
                "each right-click on the block counts one use"
            );
        }
        game.player.as_mut().unwrap().fall_flying = true;
        game.inventory[game.selected] = Some(stack.clone());
        assert!(game.boost_firework(&stack), "the gliding boost launches");
        assert_eq!(game.stats.get("used", "minecraft:firework_rocket"), 6);
    });
}
