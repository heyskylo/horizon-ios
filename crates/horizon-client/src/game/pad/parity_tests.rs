//! A controller action does what its keyboard key does, on a real `Game` without a window.
//! What needs a renderer (inventory slots, screenshots) is covered by tools/pad_parity_e2e.py.

use super::super::{Audio, Game, Options, Progress, Screen, Settings, load_resources};
use horizon_client::actions::Action;
use horizon_client::command::args::GameMode;
use horizon_client::gamepad::{Button, PadState};
use horizon_client::held::Source;
use horizon_client::motion::PlayerMotion;
use horizon_core::geometry::Vec3;
use winit::event::MouseButton;
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
    // A trigger is held by how far it's pulled, not by a button bit.
    match button {
        Button::LeftTrigger => state.triggers[0] = 1.0,
        Button::RightTrigger => state.triggers[1] = 1.0,
        _ => state.set(button, true),
    }
    game.pad.edges.update(&state, 0.4);
    game.pad_action(action, true).unwrap();
}

#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn a_keyboard_release_keeps_the_sneak_the_pad_went_on_to_hold() {
    playing(|game| {
        // Hold mode: toggling, the pad's press would flip it.
        game.settings.controller.toggle_sneak = false;
        let shift = KeyCode::ShiftLeft;
        game.mapped_key(shift, true, false, shift, Source::Keyboard)
            .unwrap();
        pad_press(game, Button::RightStick, "key.sneak");
        game.mapped_key(shift, false, false, shift, Source::Keyboard)
            .unwrap();
        assert!(game.keys.held(Action::Sneak), "R3 is still down");
    });
}

#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn keyboard_handoff_keeps_what_the_pad_holds() {
    playing(|game| {
        game.mapped_key(KeyCode::KeyW, true, false, KeyCode::KeyW, Source::Keyboard)
            .unwrap();
        pad_press(game, Button::South, "key.jump");
        assert!(game.keys.held(Action::Jump) && game.keys.held(Action::Forward));
        // Splitscreen: keyboard and mouse move to another player.
        game.release_keyboard_mouse();
        assert!(!game.keys.held(Action::Forward), "the keyboard's walk ends");
        assert!(
            game.keys.held(Action::Jump),
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

/// A player at the origin: the wheel's gates ask for one.
fn with_player(game: &mut Game) {
    game.player = Some(PlayerMotion::new(game.motion, Vec3::ZERO).unwrap());
}

#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn bumpers_step_the_hotbar_where_the_wheel_does() {
    playing(|game| {
        game.selected = 8;
        game.pad_action("pad.hotbarNext", true).unwrap();
        assert_eq!(game.selected, 8, "no player, no hotbar");
        with_player(game);
        game.pad_action("pad.hotbarNext", true).unwrap();
        assert_eq!(game.selected, 0, "wraps past the last slot");
        game.pad_action("pad.hotbarPrevious", true).unwrap();
        assert_eq!(game.selected, 8, "and back");
        game.pad_action("pad.hotbarNext", false).unwrap();
        assert_eq!(game.selected, 8, "a release does nothing");
        game.screen = Screen::Paused;
        game.pad_action("pad.hotbarNext", true).unwrap();
        assert_eq!(game.selected, 8, "a screen keeps the wheel from the hotbar");
    });
}

#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn a_spectators_bumpers_set_the_flying_speed_as_the_wheel_does() {
    playing(|game| {
        with_player(game);
        game.mode = GameMode::Spectator;
        game.selected = 3;
        let speed = game.player.as_ref().unwrap().flying_speed;
        game.pad_action("pad.hotbarPrevious", true).unwrap();
        // One wheel notch up: `MouseHandler.onScroll` adds 0.005.
        let after = game.player.as_ref().unwrap().flying_speed;
        assert!((after - speed - 0.005).abs() < 1e-6, "{speed} -> {after}");
        assert_eq!(game.selected, 3, "a spectator's hotbar doesn't step");
    });
}

#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn readers_of_the_movement_keys_see_the_stick() {
    playing(|game| {
        game.focused = true;
        assert!(!game.native_mapping("key.forward").unwrap().0);
        // Up and to the left, on a diagonal: two keys, as a keyboard would hold.
        game.pad.state.left = [-0.7, 0.7];
        assert!(game.native_mapping("key.forward").unwrap().0);
        assert!(game.native_mapping("key.left").unwrap().0);
        assert!(!game.native_mapping("key.back").unwrap().0);
        assert!(!game.native_mapping("key.right").unwrap().0);
        assert!(
            !game.native_mapping("key.jump").unwrap().0,
            "the stick holds only the movement keys"
        );
        // A screen takes the stick away.
        game.screen = Screen::Paused;
        assert!(!game.native_mapping("key.forward").unwrap().0);
    });
}

#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn pad_number_actions_select_the_hotbar_as_their_keys_do() {
    playing(|game| {
        with_player(game);
        game.settings
            .controller
            .bind("key.hotbar.3", Some(Button::DPadUp));
        pad_press(game, Button::DPadUp, "key.hotbar.3");
        assert_eq!(game.hotbar_presses, [2]);
    });
}

#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn a_held_pad_toolbar_action_counts_as_the_toolbar_key() {
    playing(|game| {
        game.settings
            .controller
            .bind("key.saveToolbarActivator", Some(Button::DPadUp));
        assert!(!game.mapped_key_down("key.saveToolbarActivator"));
        pad_press(game, Button::DPadUp, "key.saveToolbarActivator");
        assert!(game.mapped_key_down("key.saveToolbarActivator"));
        assert!(!game.mapped_key_down("key.loadToolbarActivator"));
    });
}

#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn the_pads_attack_and_use_reach_what_the_mouse_tells_mods() {
    playing(|game| {
        assert_eq!(game.native_clicks(), (0, 0));
        pad_press(game, Button::RightTrigger, "key.attack");
        assert!(game.native_mapping("key.attack").unwrap().0);
        assert_eq!(game.native_clicks(), (1, 0));
        game.pad_action("key.attack", false).unwrap();
        assert!(!game.native_mapping("key.attack").unwrap().0);
        pad_press(game, Button::LeftTrigger, "key.use");
        assert!(game.native_mapping("key.use").unwrap().0);
        assert_eq!(game.native_clicks(), (1, 1));
    });
}

#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn the_mouse_letting_go_leaves_what_the_pad_holds_and_back() {
    playing(|game| {
        game.mouse_mapping(MouseButton::Left, true, Source::Keyboard)
            .unwrap();
        pad_press(game, Button::RightTrigger, "key.attack");
        game.mouse_mapping(MouseButton::Left, false, Source::Keyboard)
            .unwrap();
        assert!(game.left, "the pad still holds the attack");
        // The pad let go, so its edges are no longer held either.
        game.pad.edges.update(&PadState::default(), 0.4);
        game.pad_action("key.attack", false).unwrap();
        assert!(!game.left);

        // The other way round: the mouse button is physically down.
        game.physical_buttons.insert(MouseButton::Right);
        game.mouse_mapping(MouseButton::Right, true, Source::Keyboard)
            .unwrap();
        pad_press(game, Button::LeftTrigger, "key.use");
        game.pad.edges.update(&PadState::default(), 0.4);
        game.pad_action("key.use", false).unwrap();
        assert!(game.right, "the mouse still holds the use");
        game.physical_buttons.remove(&MouseButton::Right);
        game.mouse_mapping(MouseButton::Right, false, Source::Keyboard)
            .unwrap();
        assert!(!game.right);
    });
}

#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn a_release_from_a_screen_can_not_strand_the_pads_attack() {
    playing(|game| {
        // The mouse went down in play, and came up where the mappings didn't hear it.
        game.mouse_mapping(MouseButton::Left, true, Source::Keyboard)
            .unwrap();
        game.left = false;
        pad_press(game, Button::RightTrigger, "key.attack");
        assert!(game.left);
        game.pad.edges.update(&PadState::default(), 0.4);
        game.pad_action("key.attack", false).unwrap();
        assert!(!game.left, "nothing is physically held");
    });
}

#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn keyboard_handoff_keeps_the_attack_the_pad_holds() {
    playing(|game| {
        pad_press(game, Button::RightTrigger, "key.attack");
        game.release_keyboard_mouse();
        assert!(game.left);
    });
}
