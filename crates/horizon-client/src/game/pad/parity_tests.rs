//! A controller action does what its keyboard key does, on a real `Game` without a window.
//! What needs a renderer (inventory slots, screenshots) is covered by tools/pad_parity_e2e.py.

use super::super::{Audio, Game, Options, Progress, Screen, Settings, load_resources};
use horizon_client::gamepad::{Button, PadState};
use horizon_client::held::Source;
use winit::keyboard::KeyCode;

/// A game at play with no world: input reaches the mappings, as in `Screen::Playing`.
fn playing(test: impl FnOnce(&mut Game)) {
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
        &mut |_, _| unreachable!("no menu handoff"),
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
    game.title = None;
    game.screen = Screen::Playing;
    test(&mut game);
}

/// Hold `button` on the pad, as a poll that saw it go down would.
fn pad_press(game: &mut Game, button: Button, action: &str) {
    let mut state = PadState::default();
    state.set(button, true);
    game.pad.edges.update(&state, 0.4);
    game.pad_action(action, true).unwrap();
}

#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn keyboard_handoff_keeps_what_the_pad_holds() {
    playing(|game| {
        game.mapped_key(KeyCode::KeyW, true, false, KeyCode::KeyW, Source::Keyboard)
            .unwrap();
        pad_press(game, Button::South, "key.jump");
        assert!(game.keys.contains(&KeyCode::Space) && game.keys.contains(&KeyCode::KeyW));
        // Splitscreen: keyboard and mouse move to another player.
        game.release_keyboard_mouse();
        assert!(
            !game.keys.contains(&KeyCode::KeyW),
            "the keyboard's walk ends"
        );
        assert!(
            game.keys.contains(&KeyCode::Space),
            "the pad still holds jump; only the keyboard and mouse left"
        );
    });
}

#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn pad_smooth_camera_binding_toggles_the_cinematic_camera() {
    playing(|game| {
        game.settings
            .controller
            .bind("key.smoothCamera", Some(Button::Select));
        assert!(!game.smooth_camera);
        pad_press(game, Button::Select, "key.smoothCamera");
        assert!(game.smooth_camera, "as its key does in play");
    });
}
