use crate::game::{Audio, Game, Options, Progress, Settings, load_resources};
use horizon_client::chunk::Chunk;
use horizon_core::geometry::Vec3;
use std::sync::Arc;

fn end_game() -> Game<'static> {
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
    options.execution_role = crate::game::execution_role::ExecutionRole::IntegratedServer;
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
    // The End as its own level (`execute in minecraft:the_end`), with a
    // forced chunk so the server ticks it.
    assert_eq!(
        game.enter_command_level(crate::game::Dim::End),
        Some(crate::game::Dim::Overworld)
    );
    assert!(game.dimension_type().has_ender_dragon_fight, "the End");
    for x in -1..2 {
        for z in -1..2 {
            game.world
                .chunks
                .insert((x, z), Arc::new(Chunk::empty(x, z)));
        }
    }
    assert!(game.run_command("forceload add 0 0"));
    game
}

/// The level's dragon fireballs: its projectiles of that kind.
fn dragon_fireballs<'a>(game: &'a Game<'_>) -> Vec<&'a horizon_client::projectiles::Projectile> {
    game.creatures
        .projectiles
        .iter()
        .filter(|p| p.kind == horizon_client::projectiles::ProjectileKind::DragonFireball)
        .collect()
}

/// `(v + normalize(v) × 0.1) × 0.95F`: AbstractHurtingProjectile.tick's new motion.
fn hurting_motion(v: Vec3) -> Vec3 {
    let l = (v.x * v.x + v.y * v.y + v.z * v.z).sqrt();
    let inertia = f64::from(0.95_f32);
    Vec3::new(
        (v.x + v.x / l * 0.1) * inertia,
        (v.y + v.y / l * 0.1) * inertia,
        (v.z + v.z / l * 0.1) * inertia,
    )
}

#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn the_dragons_fireball_starts_at_its_start_with_a_tenth_of_its_direction() {
    let mut game = end_game();
    let start = Vec3::new(0.5, 100.0, 0.5);
    game.spawn_dragon_fireball(start, Vec3::new(3.0, 0.0, 4.0));
    let b = dragon_fireballs(&game)
        .last()
        .copied()
        .expect("one fireball");
    assert_eq!(b.position, start);
    // `Vec3.normalize().scale(0.1)` in doubles: 4 / 5 × 0.1 is 0.08000000000000002.
    assert_eq!(b.velocity, Vec3::new(3.0 / 5.0 * 0.1, 0.0, 4.0 / 5.0 * 0.1));
}

/// The old engine's first End world tick (recorded, then compared with
/// `AbstractHurtingProjectile.tick`'s).
#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn the_dragons_fireball_first_world_tick() {
    let mut game = end_game();
    assert!(game.run_command("tick freeze"));
    let start = Vec3::new(0.5, 100.0, 0.5);
    game.spawn_dragon_fireball(start, Vec3::new(3.0, 0.0, 4.0));
    assert!(game.run_command("tick step 1"));
    game.tick_server(false).unwrap();
    // Back in the End level to read its own list.
    let _ = game.enter_command_level(crate::game::Dim::End);
    let after: Vec<(Vec3, Vec3)> = dragon_fireballs(&game)
        .iter()
        .map(|b| (b.position, b.velocity))
        .collect();
    eprintln!(
        "H127 after one End tick: fireballs {}, breath clouds {}, dim end {}",
        dragon_fireballs(&game).len(),
        game.breath_clouds.len(),
        game.dimension_type().has_ender_dragon_fight
    );
    let v = Vec3::new(0.06, 0.0, 0.08);
    let motion = hurting_motion(v);
    let expected = (
        Vec3::new(start.x + motion.x, start.y + motion.y, start.z + motion.z),
        motion,
    );
    eprintln!(
        "H127 dragon fireball one tick: actual {after:?}, AbstractHurtingProjectile {expected:?}"
    );
    assert_eq!(
        after,
        vec![expected],
        "AbstractHurtingProjectile's first tick"
    );
}

/// The pinned chain normalizes twice (`DragonStrafePlayerPhase` then
/// `assignDirectionalMovement`): for (1, 0, -1) one normalization gives
/// 0.07071067811865475 and two give 0.07071067811865477.
#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn the_dragons_fireball_direction_is_normalized_twice() {
    let mut game = end_game();
    game.spawn_dragon_fireball(Vec3::new(0.5, 100.0, 0.5), Vec3::new(1.0, 0.0, -1.0));
    let b = dragon_fireballs(&game)
        .last()
        .copied()
        .expect("one fireball");
    assert_eq!(
        b.velocity,
        Vec3::new(0.07071067811865477, 0.0, -0.07071067811865477)
    );
}
