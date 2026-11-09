use crate::game::{Audio, Game, Options, Progress, Settings, load_resources};
use horizon_client::effects::EffectInstance;

/// `LivingEntity.addEffect` → `MobEffect.onEffectAdded`: Bad Omen, Trial
/// Omen and Raid Omen sound once when they are new on the player, at its
/// feet; a renewal and any other effect are silent (#11373).
#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn a_new_omen_sounds_once_at_the_feet() {
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
    game.player = Some(
        horizon_client::motion::PlayerMotion::new(
            game.motion,
            horizon_core::geometry::Vec3::new(10.5, 64.0, -3.25),
        )
        .unwrap(),
    );
    game.audio.capture = Some(Vec::new());
    for (id, amplifier) in [
        ("bad_omen", 0),
        ("bad_omen", 1),
        ("speed", 0),
        ("trial_omen", 0),
        ("raid_omen", 0),
    ] {
        game.add_player_effect(EffectInstance::new(id, 2400, amplifier));
    }
    let played: Vec<(String, [f64; 3])> = game
        .audio
        .capture
        .take()
        .unwrap()
        .into_iter()
        .map(|s| (s.event, s.position))
        .collect();
    let feet = [10.5, 64.0, -3.25];
    assert_eq!(
        played,
        [
            ("minecraft:event.mob_effect.bad_omen".to_owned(), feet),
            ("minecraft:event.mob_effect.trial_omen".to_owned(), feet),
            ("minecraft:event.mob_effect.raid_omen".to_owned(), feet),
        ]
    );
}
