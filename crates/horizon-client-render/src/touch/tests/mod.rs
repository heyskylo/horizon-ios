// Adapted from Starling's touch.rs tests (see the header of touch.rs).
use super::*;

mod gestures;

fn role(t: &TouchControls, id: u64) -> Role {
    t.fingers[&id].0
}

#[test]
fn factory_swap_stays_beside_inventory_and_moved_swap_stays_put() {
    for (w, h) in [(2560, 1600), (2400, 1080), (1080, 2400), (240, 320)] {
        for left_handed in [false, true] {
            for mode in Mode::ALL {
                let mut t = TouchControls::default();
                t.viewport(w, h);
                t.settings.left_handed = left_handed;
                t.settings.mode = mode;
                let swap = t.placement(Control::Offhand).rect(t.aspect());
                for control in [
                    Control::Inventory,
                    Control::Sprint,
                    Control::Sneak,
                    Control::Jump,
                ] {
                    let other = t.placement(control).rect(t.aspect());
                    let overlaps = swap[0] < other[0] + other[2]
                        && other[0] < swap[0] + swap[2]
                        && swap[1] < other[1] + other[3]
                        && other[1] < swap[1] + swap[3];
                    assert!(!overlaps, "{w}x{h}, {left_handed}, {mode:?}: {control:?}");
                }
                t.settings.layouts[mode.index()][Control::Offhand as usize].center = [0.55, 0.43];
                let moved = t.placement(Control::Offhand).center;
                assert!((moved[0] - if left_handed { 0.45 } else { 0.55 }).abs() < 0.0001);
                assert_eq!(moved[1], 0.43);
            }
        }
    }
}

#[test]
fn swap_hands_sends_one_press_and_shield_holds_until_release() {
    let mut t = TouchControls::default();
    assert!(!t.visible(Control::Offhand));
    t.settings.offhand_button = true;
    t.update(1, t.placement(Control::Offhand).center, true, false);
    assert!(t.clicked);
    assert_eq!(
        t.key_events(),
        [(SWAP_HANDS.into(), true), (SWAP_HANDS.into(), false)]
    );
    t.clear();
    t.settings.mode = Mode::Tap;
    t.shield_available = true;
    t.update(2, t.placement(Control::Shield).center, true, false);
    assert!(t.shield_held());
    // Sliding off the button keeps the shield up.
    t.update(2, [0.5, 0.4], false, false);
    assert!(t.shield_held());
    assert!(t.poll_state().triggers[0] > 0.0);
    t.update(2, [0.5, 0.4], false, true);
    assert!(!t.shield_held());
    assert_eq!(t.poll_state().triggers[0], 0.0);
}

#[test]
fn shield_uses_the_use_button_in_crosshair_layouts() {
    let mut t = TouchControls {
        shield_available: true,
        ..TouchControls::default()
    };
    assert!(!t.visible(Control::Shield));
    let p = t.placement(Control::Use).center;
    t.update(1, p, true, false);
    assert!(t.poll_state().triggers[0] > 0.0);
    t.update(1, p, false, true);
    assert_eq!(t.poll_state().triggers[0], 0.0);
    for mode in [Mode::Tap, Mode::Dpad] {
        t.clear();
        t.settings.mode = mode;
        assert!(t.visible(Control::Shield));
        let p = t.placement(Control::Shield).center;
        t.update(2, p, true, false);
        assert!(t.shield_held() && t.poll_state().triggers[0] > 0.0);
        t.update(2, p, false, true);
        assert!(!t.shield_held());
        t.force_crosshair = true;
        assert!(!t.visible(Control::Shield));
        t.force_crosshair = false;
        t.settings.shield_button = false;
        assert!(!t.visible(Control::Shield));
        t.settings.shield_button = true;
    }
    // Hiding the use button brings the shield button back.
    t.settings.mode = Mode::Crosshair;
    t.settings.button_visibility[Control::Use as usize] = layout::HIDDEN;
    assert!(t.visible(Control::Shield));
    t.shield_available = false;
    assert!(!t.visible(Control::Shield));
}

#[test]
fn sprint_toggle_survives_release_and_clears_with_focus() {
    let mut t = TouchControls::default();
    t.settings.sprint_toggle = true;
    let point = t.placement(Control::Sprint).center;
    t.update(1, point, true, false);
    t.update(1, point, false, true);
    assert!(t.poll_state().held(Button::LeftStick));
    t.update(2, point, true, false);
    t.update(2, point, false, true);
    assert!(!t.poll_state().held(Button::LeftStick));
    t.update(3, point, true, false);
    t.clear();
    assert!(!t.poll_state().held(Button::LeftStick));
    t.settings.sprint_toggle = false;
    t.update(4, point, true, false);
    assert!(t.poll_state().held(Button::LeftStick));
    t.update(4, point, false, true);
    assert!(!t.poll_state().held(Button::LeftStick));
}

#[test]
fn button_scale_changes_hit_areas_not_saved_layouts() {
    let mut t = TouchControls::default();
    t.viewport(1920, 1080);
    let before = t.placement(Control::Jump);
    let stored = t.settings.placement(Control::Jump).size;
    t.settings.button_scale = 1.5;
    assert!((t.placement(Control::Jump).size - before.size * 1.5).abs() < 0.00001);
    assert_eq!(t.settings.placement(Control::Jump).size, stored);
}

#[test]
fn big_controls_stay_on_narrow_and_wide_surfaces() {
    let mut t = TouchControls::default();
    for (w, h) in [(400, 1000), (1920, 1080)] {
        t.viewport(w, h);
        t.settings.layouts[0][Control::Jump as usize].center = [0.99, 0.99];
        t.settings.layouts[0][Control::Jump as usize].size = 0.45;
        t.settings.button_scale = 2.0;
        let r = t.placement(Control::Jump).rect(t.aspect());
        assert!(r[0] >= 0.0 && r[1] >= 0.0 && r[0] + r[2] <= 1.0001 && r[1] + r[3] <= 1.0001);
    }
}

#[test]
fn portrait_spreads_the_factory_top_row_but_not_moved_buttons() {
    let mut t = TouchControls::default();
    t.viewport(480, 854);
    let top = [
        Control::Debug,
        Control::Chat,
        Control::Pause,
        Control::Settings,
        Control::Perspective,
    ];
    let rects: Vec<_> = top
        .iter()
        .map(|c| t.placement(*c).rect(t.aspect()))
        .collect();
    for pair in rects.windows(2) {
        assert!(pair[0][0] + pair[0][2] < pair[1][0]);
    }
    t.settings.layouts[0][Control::Chat as usize].center = [0.25, 0.2];
    assert_eq!(t.placement(Control::Chat).center, [0.25, 0.2]);
}

#[test]
fn shortcut_only_overlay_keeps_menus_without_movement() {
    let mut t = TouchControls {
        auxiliary_only: true,
        ..TouchControls::default()
    };
    assert!(!t.visible(Control::Move) && !t.visible(Control::Jump));
    assert!(t.visible(Control::Debug));
    let at = t.placement(Control::Debug).center;
    assert!(t.auxiliary_at(at));
    assert!(!t.auxiliary_at(t.settings.placement(Control::Jump).center));
    t.update(77, at, true, false);
    assert!(t.debug_requested && t.owns_pointer(77));
    t.update(77, at, false, true);
    assert!(!t.owns_pointer(77));
}

#[test]
fn shortcuts_take_their_finger_and_leave_movement_alone() {
    let mut t = TouchControls::default();
    t.update(1, [0.13, 0.76], true, false);
    t.update(1, [0.13, 0.68], false, false);
    let moving = t.poll_state().left;
    assert_ne!(moving, [0.0; 2]);
    for (id, control) in [
        (2, Control::Debug),
        (3, Control::Perspective),
        (4, Control::Chat),
        (5, Control::Settings),
    ] {
        let point = t.settings.placement(control).center;
        t.update(id, point, true, false);
        assert!(matches!(role(&t, id), Role::Ignore));
        assert_eq!(t.poll_state().left, moving);
        assert_eq!(t.poll_state().triggers, [0.0; 2]);
        t.update(id, point, false, true);
    }
    assert!(t.debug_requested && t.perspective_requested);
    assert!(t.chat_requested && t.settings_requested);
    t.clear();
    assert!(!t.debug_requested && !t.perspective_requested && !t.chat_requested);
}

#[test]
fn contextual_use_button_is_opt_in_and_stays_until_release() {
    let mut t = TouchControls::default();
    t.settings.mode = Mode::Tap;
    t.context_use = true;
    assert!(!t.visible(Control::Use));
    t.settings.contextual_interact = true;
    assert!(t.visible(Control::Use));
    t.update(1, [0.92, 0.52], true, false);
    t.context_use = false;
    assert!(t.visible(Control::Use));
    assert_eq!(t.poll_state().triggers[0], 1.0);
    t.update(1, [0.92, 0.52], false, true);
    assert!(!t.visible(Control::Use));
    t.settings.mode = Mode::Crosshair;
    assert!(t.visible(Control::Use));
}

#[test]
fn quick_taps_survive_one_poll_and_shared_buttons_release_together() {
    let mut t = TouchControls::default();
    t.update(1, [0.92, 0.78], true, false);
    t.update(1, [0.92, 0.78], false, true);
    assert!(t.poll_state().held(Button::South));
    assert!(!t.poll_state().held(Button::South));
    t.update(2, [0.80, 0.64], true, false);
    t.update(3, [0.80, 0.64], true, false);
    t.update(2, [0.80, 0.64], false, true);
    assert_eq!(t.state().triggers[1], 1.0);
    t.update(3, [0.80, 0.64], false, true);
    assert_eq!(t.state().triggers[1], 0.0);
}

#[test]
fn joystick_camera_and_jump_have_their_own_fingers() {
    let mut t = TouchControls::default();
    t.update(1, [0.13, 0.76], true, false);
    t.update(1, [0.20, 0.64], false, false);
    t.update(2, [0.6, 0.4], true, false);
    let d = t.update(2, [0.65, 0.45], false, false);
    assert!((d[0] - 0.05).abs() < 0.0001);
    t.update(3, [0.92, 0.78], true, false);
    assert!(t.state().held(Button::South));
    assert!(t.state().left[0] > 0.6);
    // A second finger on the joystick neither moves nor steals it.
    t.update(4, [0.09, 0.76], true, false);
    t.update(4, [0.02, 0.70], false, false);
    assert!(t.state().left[0] > 0.6);
    t.clear();
    assert_eq!(t.poll_state(), PadState::default());
    assert_eq!(t.update(9, [0.5, 0.5], false, false), [0.0; 2]);
}

#[test]
fn joystick_sprint_starts_past_the_rim_and_stops_inside() {
    let mut t = TouchControls::default();
    t.settings.sprint_with_joystick = true;
    t.update(1, [0.13, 0.76], true, false);
    t.update(1, [0.13, 0.62], false, false);
    assert!(!t.state().held(Button::LeftStick));
    t.update(1, [0.13, 0.58], false, false);
    assert!(t.state().held(Button::LeftStick));
    assert!((t.state().left[1] - 1.0).abs() < 1e-6);
    t.update(1, [0.13, 0.62], false, false);
    assert!(!t.state().held(Button::LeftStick));
}

#[test]
fn dpad_slides_between_directions_crouches_on_double_tap_and_mirrors() {
    let mut t = TouchControls::default();
    t.settings.mode = Mode::Dpad;
    t.update(1, [0.13, 0.66], true, false);
    assert_eq!(t.state().left, [0.0, 1.0]);
    t.update(1, [0.20, 0.76], false, false);
    assert_eq!(t.state().left, [1.0, 0.0]);
    t.clear();
    t.update(10, [0.13, 0.76], true, false);
    assert!(!t.poll_state().held(Button::RightStick));
    t.update(10, [0.13, 0.76], false, true);
    t.update(12, [0.13, 0.76], true, false);
    assert!(t.poll_state().held(Button::RightStick));
    t.clear();
    // Flying, the centre holds sneak (fly down) at once.
    t.vertical_movement = true;
    t.update(13, [0.13, 0.76], true, false);
    assert!(t.state().held(Button::RightStick));
    t.clear();
    t.vertical_movement = false;
    t.settings.swap_jump_sneak = true;
    t.update(11, [0.13, 0.76], true, false);
    assert!(t.state().held(Button::South));
    t.clear();
    t.settings.left_handed = true;
    t.update(2, [0.87, 0.66], true, false);
    assert_eq!(t.state().left, [0.0, 1.0]);
}

#[test]
fn hidden_sprint_button_sends_nothing() {
    let mut t = TouchControls::default();
    t.settings.sprint_button = false;
    let center = t.settings.placement(Control::Sprint).center;
    t.update(1, center, true, false);
    assert!(!t.poll_state().held(Button::LeftStick));
    t.update(1, center, false, true);
    t.settings.sprint_button = true;
    t.update(2, center, true, false);
    assert!(t.poll_state().held(Button::LeftStick));
    t.update(2, center, false, true);
    assert!(!t.poll_state().held(Button::LeftStick));
}

#[test]
fn forced_crosshair_brings_back_attack_and_use_and_ignored_fingers_do_nothing() {
    let mut t = TouchControls::default();
    t.settings.mode = Mode::Tap;
    assert!(!t.crosshair() && !t.visible(Control::Attack));
    t.force_crosshair = true;
    assert!(t.crosshair() && t.visible(Control::Attack) && t.visible(Control::Use));
    t.settings.button_visibility[Control::Attack as usize] = layout::HIDDEN;
    assert!(!t.visible(Control::Attack));
    t.ignore(5);
    assert!(t.owns_pointer(5));
    assert_eq!(t.update(5, [0.9, 0.9], false, false), [0.0; 2]);
    assert_eq!(t.state(), PadState::default());
    t.tap(Button::East);
    assert!(t.poll_state().held(Button::East));
    assert!(!t.poll_state().held(Button::East));
}

#[test]
fn a_world_touch_turns_the_camera_only_once_it_drags() {
    let mut t = TouchControls::default();
    t.update(1, [0.6, 0.4], true, false);
    assert!(matches!(role(&t, 1), Role::Look { dragged: false, .. }));
    // A small wobble is not a drag.
    assert_eq!(t.update(1, [0.605, 0.4], false, false), [0.0; 2]);
    let d = t.update(1, [0.7, 0.45], false, false);
    assert!((d[0] - 0.095).abs() < 1e-5 && (d[1] - 0.05).abs() < 1e-5);
    assert_eq!(t.poll_state(), PadState::default());
    t.update(1, [0.7, 0.45], false, true);
    assert!(!t.owns_pointer(1));
    // Buttons never turn the camera.
    let jump = t.placement(Control::Jump).center;
    t.update(2, jump, true, false);
    assert_eq!(
        t.update(2, [jump[0] - 0.1, jump[1]], false, false),
        [0.0; 2]
    );
}
