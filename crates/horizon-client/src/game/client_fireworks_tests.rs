//! Run alone with private prepared resources.
use super::super::*;
use super::{MAX_CLIENT_FIREWORK_EXPLOSIONS, client_fireworks_ok};
use crate::mod_api::{FireworkExplosion, FireworkShape, Host};
use horizon_client::chunk::Chunk;
use horizon_client::particles::ParticleLevel;

fn star(colors: &[u32]) -> FireworkExplosion {
    FireworkExplosion {
        shape: FireworkShape::Star,
        colors: colors.to_vec(),
        fade: vec![0xFFFFFF],
        trail: true,
        twinkle: false,
    }
}

#[test]
fn only_finite_shows_of_up_to_64_explosions_are_drawn() {
    let at = [0.0, 70.0, 0.0];
    assert!(client_fireworks_ok(at, [0.0; 3], &[]));
    assert!(client_fireworks_ok(
        at,
        [0.0, 0.1, 0.0],
        &[star(&[0xFF0000])]
    ));
    let many = vec![star(&[0x00FF00]); MAX_CLIENT_FIREWORK_EXPLOSIONS];
    assert!(client_fireworks_ok(at, [0.0; 3], &many));
    let too_many = vec![star(&[0x00FF00]); MAX_CLIENT_FIREWORK_EXPLOSIONS + 1];
    assert!(!client_fireworks_ok(at, [0.0; 3], &too_many));
    for bad in [f64::NAN, f64::INFINITY] {
        assert!(!client_fireworks_ok([bad, 70.0, 0.0], [0.0; 3], &[]));
        assert!(!client_fireworks_ok(at, [0.0, bad, 0.0], &[]));
    }
}

#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn a_mods_fireworks_burst_and_bang_on_the_client_only() {
    let root = PathBuf::from(std::env::var_os("HORIZON_TEST_RESOURCES").unwrap());
    let settings = Settings::default();
    let loaded = load_resources(
        root.clone(),
        None,
        &root,
        &settings,
        Instant::now(),
        false,
        true,
        &Progress::new(),
        &mut |_, _| unreachable!(),
    )
    .unwrap();
    let mut options = Options::parse(std::iter::empty()).unwrap().unwrap();
    options.resources = root.clone();
    options.execution_role = execution_role::ExecutionRole::Client;
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
    game.world = World::new(game.res.facts().air, 1);
    game.world
        .chunks
        .insert((0, 0), Arc::new(Chunk::empty(0, 0)));
    let at = [0.5, 70.0, 0.5];
    let show = [star(&[0xFF5555, 0xFFFF55]), star(&[0x55FF55])];

    // No client player yet: nothing.
    game.player = None;
    assert!(!(&mut game as &mut dyn Host).client_fireworks(at, [0.0; 3], &show));

    game.player = Some(PlayerMotion::new(&game.motion, Vec3::new(0.5, 64.0, 0.5)).unwrap());
    let entities = game.rockets.len();
    assert!((&mut game as &mut dyn Host).client_fireworks(at, [0.0, 0.05, 0.0], &show));
    assert!(
        !(&mut game as &mut dyn Host).client_fireworks(at, [f64::NAN, 0.0, 0.0], &show),
        "non-finite input"
    );
    // No rocket: only the picture and the sound.
    assert_eq!(game.rockets.len(), entities);

    game.particles.sounds.clear();
    let tick = |game: &mut Game<'_>| {
        let level = ParticleLevel {
            world: &game.world,
            facts: game.res.facts(),
            collision: &game.collision,
            player: None,
        };
        game.particles.tick(&level);
    };
    tick(&mut game);
    assert!(
        game.particles
            .sounds
            .iter()
            .any(|s| s.0 == "minecraft:entity.firework_rocket.blast"),
        "the starter bangs on its first tick: {:?}",
        game.particles.sounds
    );
    // Its first explosion's sparks reached the live queue.
    tick(&mut game);
    assert!(!game.particles.is_empty());

    // A server's game draws nothing.
    game.options.execution_role = execution_role::ExecutionRole::IntegratedServer;
    assert!(!(&mut game as &mut dyn Host).client_fireworks(at, [0.0; 3], &show));
}
