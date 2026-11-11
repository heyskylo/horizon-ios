//! Taps and holds on the world, and the dynamic joystick.
use super::*;

/// Backdates a look finger, as if it had been held `ms` milliseconds.
fn age(t: &mut TouchControls, id: u64, ms: u64) {
    if let Some((Role::Look { since, .. }, _)) = t.fingers.get_mut(&id) {
        *since = Instant::now() - Duration::from_millis(ms);
    }
}

#[test]
fn dynamic_area_follows_the_configured_width_and_height() {
    let mut t = TouchControls::default();
    t.settings.dynamic_area_x = 0.25;
    t.settings.dynamic_area_y = 0.5;
    let area = t.dynamic_area();
    assert_eq!([area[0], area[2], area[3]], [0.0, 0.25, 0.5]);
    let move_y = t.placement(Control::Move).center[1];
    assert!((area[1] - (move_y - 0.25).clamp(0.0, 0.5)).abs() < 1e-4);
    // Far above the joystick, a touch stays a camera gesture.
    t.settings.dynamic_joystick = true;
    t.update(1, [0.1, 0.05], true, false);
    assert!(matches!(role(&t, 1), Role::Look { .. }));
    t.update(1, [0.1, 0.05], false, true);
    t.update(2, [0.1, move_y], true, false);
    assert!(matches!(role(&t, 2), Role::Move(_)));
    // The strip stays on the surface and follows a left-handed layout.
    t.settings.dynamic_area_y = 1.0;
    t.settings.left_handed = true;
    let area = t.dynamic_area();
    assert_eq!([area[0], area[1], area[3]], [0.75, 0.0, 1.0]);
}

#[test]
fn dynamic_joystick_spawns_in_its_strip_and_keeps_its_frame() {
    let mut t = TouchControls::default();
    t.settings.dynamic_joystick = true;
    t.update(1, [0.5, 0.5], true, false);
    assert!(matches!(role(&t, 1), Role::Look { .. }));
    t.update(1, [0.5, 0.5], false, true);
    // The frame is where the finger first landed.
    t.update(2, [0.12, 0.6], true, false);
    assert!(matches!(role(&t, 2), Role::Move(a) if a == [0.12, 0.6]));
    t.update(2, [0.20, 0.6], false, false);
    assert!(matches!(role(&t, 2), Role::Move(a) if a == [0.12, 0.6]));
    assert!(t.state().left[0] > 0.0);
    t.update(2, [0.20, 0.6], false, true);
    t.settings.left_handed = true;
    t.update(3, [0.80, 0.6], true, false);
    assert!(matches!(role(&t, 3), Role::Move(_)));
}

#[test]
fn dynamic_joystick_respects_hidden_controls_and_one_owner() {
    let mut t = TouchControls::default();
    t.settings.dynamic_joystick = true;
    t.settings.dynamic_area_x = 0.05;
    // The fixed joystick's home is outside a narrow strip: a camera gesture.
    t.update(1, t.placement(Control::Move).center, true, false);
    assert!(matches!(role(&t, 1), Role::Look { .. }));
    t.clear();
    let edge = [0.025, 0.5];
    assert_eq!(t.control_at(edge), Some(Control::Move));
    t.update(2, edge, true, false);
    assert!(matches!(role(&t, 2), Role::Move(_)));
    // A second finger can't take the joystick.
    t.update(3, [0.03, 0.6], true, false);
    assert!(matches!(role(&t, 3), Role::Look { .. }));
    t.update(2, [0.05, 0.5], false, false);
    assert!(t.state().left[0] > 0.0);
    t.update(2, [0.05, 0.5], false, true);
    assert_eq!(t.state().left, [0.0; 2]);
    t.clear();
    t.settings.button_visibility[Control::Move as usize] = layout::HIDDEN;
    t.update(4, edge, true, false);
    assert!(matches!(role(&t, 4), Role::Look { .. }));
    t.clear();
    t.settings.button_visibility[Control::Move as usize] = layout::FOLLOW_SCHEME;
    t.settings.dynamic_area_x = 0.0;
    t.update(5, [0.0, 0.5], true, false);
    assert!(matches!(role(&t, 5), Role::Look { .. }));
}

#[test]
fn fixed_joystick_dpad_and_buttons_win_over_the_dynamic_strip() {
    let mut t = TouchControls::default();
    let home = t.placement(Control::Move).center;
    t.update(1, [home[0] + 0.01, home[1]], true, false);
    assert!(matches!(role(&t, 1), Role::Move(a) if a == home));
    t.clear();
    t.settings.dynamic_joystick = true;
    t.settings.mode = Mode::Dpad;
    let home = t.placement(Control::Move).center;
    let up = [home[0], home[1] - t.placement(Control::Move).size / 3.0];
    t.update(2, up, true, false);
    assert!(matches!(role(&t, 2), Role::Move(a) if a == home));
    t.clear();
    t.settings.mode = Mode::Crosshair;
    t.settings.dynamic_area_x = 1.0;
    t.update(3, t.placement(Control::Jump).center, true, false);
    assert!(t.state().held(Button::South));
    assert!(!t.moving());
}

#[test]
fn projectiles_aim_short_taps_at_the_crosshair() {
    let mut t = TouchControls::default();
    t.settings.mode = Mode::Tap;
    t.projectile_crosshair = true;
    t.hold_uses_item = true;
    t.update(1, [0.6, 0.4], true, false);
    t.update(1, [0.6, 0.4], false, true);
    t.poll_state();
    assert_eq!(t.aim, Some([0.5, 0.5]));
    assert_eq!(t.take_tap(), Some(true));
}

#[test]
fn world_taps_never_hold_attack_or_use_and_keep_the_landing_item() {
    for (mode, split) in Mode::SCHEMES.map(|(m, s, _)| (m, s)) {
        for held_use in [false, true] {
            let mut t = TouchControls::default();
            t.settings.mode = mode;
            t.settings.split_controls = split;
            t.settings.crosshair_gestures = true;
            t.hold_uses_item = held_use;
            t.update(1, [0.6, 0.4], true, false);
            t.hold_uses_item = !held_use;
            t.update(1, [0.6, 0.4], false, true);
            assert_eq!(t.poll_state().triggers, [0.0; 2]);
            assert_eq!(t.take_tap(), Some(held_use));
            assert_eq!(t.take_tap(), None);
            t.update(2, [0.6, 0.4], true, false);
            t.update(2, [0.6, 0.4], false, true);
            t.clear();
            assert_eq!(t.take_tap(), None);
        }
    }
}

#[test]
fn looking_around_with_food_never_starts_eating() {
    let mut t = TouchControls::default();
    t.settings.mode = Mode::Tap;
    t.hold_uses_item = true;
    t.update(1, [0.6, 0.4], true, false);
    assert_eq!(t.poll_state().triggers, [0.0; 2]);
    t.update(1, [0.7, 0.4], false, false);
    age(&mut t, 1, 1000);
    assert_eq!(t.poll_state().triggers, [0.0; 2]);
    assert_eq!(t.pointer(), Some([0.7, 0.4]));
}

#[test]
fn crosshair_gestures_aim_at_the_centre() {
    for (mode, split) in [(Mode::Crosshair, false), (Mode::Dpad, true)] {
        let mut t = TouchControls::default();
        t.settings.mode = mode;
        t.settings.split_controls = split;
        t.update(1, [0.6, 0.4], true, false);
        t.update(1, [0.6, 0.4], false, true);
        assert_eq!(t.take_tap(), None, "off by default");
        t.settings.crosshair_gestures = true;
        t.update(2, [0.6, 0.4], true, false);
        t.update(2, [0.6, 0.4], false, true);
        assert_eq!(t.poll_state().triggers, [0.0; 2]);
        assert_eq!(t.take_tap(), Some(false));
        assert_eq!(t.aim, Some([0.5, 0.5]));
    }
}

#[test]
fn a_tap_aims_where_it_lands_and_a_drag_only_turns() {
    let mut t = TouchControls::default();
    t.settings.mode = Mode::Tap;
    t.update(1, [0.6, 0.4], true, false);
    t.update(1, [0.6, 0.4], false, true);
    assert_eq!(t.poll_state().triggers, [0.0; 2]);
    assert_eq!(t.take_tap(), Some(false));
    assert_eq!(t.aim, Some([0.6, 0.4]));
    t.poll_state();
    assert_eq!(t.aim, None);
    t.update(2, [0.6, 0.4], true, false);
    assert_eq!(t.update(2, [0.605, 0.4], false, false), [0.0; 2]);
    assert_ne!(t.update(2, [0.7, 0.4], false, false), [0.0; 2]);
    t.update(2, [0.7, 0.4], false, true);
    assert_eq!(t.poll_state().triggers, [0.0; 2]);
    assert_eq!(t.take_tap(), None);
}

#[test]
fn the_attack_button_can_turn_the_camera_while_mining() {
    for enabled in [true, false] {
        let mut t = TouchControls::default();
        t.settings.attack_button_look = enabled;
        let start = t.placement(Control::Attack).center;
        t.update(1, start, true, false);
        assert_eq!(t.poll_state().triggers[1], 1.0);
        let end = [start[0] - 0.2, start[1] - 0.1];
        assert_eq!(t.update(1, end, false, false) != [0.0; 2], enabled);
        assert_eq!(t.poll_state().triggers[1], 1.0);
        t.update(1, end, false, true);
        assert_eq!(t.poll_state().triggers[1], 0.0);
    }
}

#[test]
fn a_long_hold_keeps_mining_while_the_camera_turns() {
    let mut t = TouchControls::default();
    t.settings.mode = Mode::Tap;
    t.update(1, [0.6, 0.4], true, false);
    age(&mut t, 1, 400);
    assert_eq!(t.poll_state().triggers[1], 1.0);
    assert_eq!(t.aim, Some([0.6, 0.4]));
    assert!(t.holding && !t.holding_use);
    assert_ne!(t.update(1, [0.7, 0.4], false, false), [0.0; 2]);
    assert_eq!(t.poll_state().triggers[1], 1.0);
    assert_eq!(t.aim, Some([0.7, 0.4]));
    t.update(1, [0.7, 0.4], false, true);
    assert_eq!(t.poll_state().triggers[1], 0.0);
    assert_eq!(t.take_tap(), None);
}

#[test]
fn eating_survives_drift_and_ends_with_its_finger() {
    let mut t = TouchControls::default();
    t.settings.mode = Mode::Tap;
    t.hold_uses_item = true;
    t.update(1, [0.6, 0.4], true, false);
    age(&mut t, 1, 400);
    assert_eq!(t.poll_state().triggers, [1.0, 0.0]);
    assert!(t.holding_use);
    assert_ne!(t.update(1, [0.66, 0.46], false, false), [0.0; 2]);
    t.hold_uses_item = false; // the last item eaten can't turn into mining
    assert_eq!(t.poll_state().triggers, [1.0, 0.0]);
    assert_eq!(t.aim, Some([0.66, 0.46]));
    // A second finger turning the camera doesn't change the hold.
    t.update(2, [0.4, 0.3], true, false);
    assert_ne!(t.update(2, [0.45, 0.3], false, false), [0.0; 2]);
    assert_eq!(t.poll_state().triggers, [1.0, 0.0]);
    t.update(1, [0.66, 0.46], false, true);
    assert_eq!(t.poll_state().triggers, [0.0; 2]);
    assert!(!t.holding_use);
}

#[test]
fn latch_use_turns_a_hold_into_use_but_not_a_tap() {
    let mut t = TouchControls::default();
    t.settings.mode = Mode::Tap;
    t.update(1, [0.6, 0.4], true, false);
    t.poll_state();
    assert!(!t.holding);
    age(&mut t, 1, 400);
    assert_eq!(t.poll_state().triggers, [0.0, 1.0]);
    assert!(t.holding);
    t.latch_use();
    assert_eq!(t.poll_state().triggers, [1.0, 0.0]);
    t.update(1, [0.66, 0.4], false, true);
    assert_eq!(t.poll_state().triggers, [0.0; 2]);
    assert!(!t.holding);
    t.update(2, [0.6, 0.4], true, false);
    t.update(2, [0.6, 0.4], false, true);
    assert_eq!(t.poll_state().triggers, [0.0; 2]);
    assert_eq!(t.take_tap(), Some(false));
}

#[test]
fn the_marker_follows_a_look_finger_only_in_tap_schemes() {
    let mut t = TouchControls::default();
    t.settings.mode = Mode::Tap;
    assert_eq!(t.pointer(), None);
    t.update(1, [0.6, 0.4], true, false);
    assert_eq!(t.pointer(), Some([0.6, 0.4]));
    assert_eq!(t.poll_state().triggers, [0.0; 2]);
    assert_eq!(t.aim, Some([0.6, 0.4]));
    t.update(1, [0.7, 0.4], false, false);
    t.poll_state();
    assert_eq!(t.pointer(), Some([0.7, 0.4]));
    assert_eq!(t.aim, None);
    t.update(1, [0.7, 0.4], false, true);
    t.clear();
    t.settings.mode = Mode::Crosshair;
    t.update(1, [0.6, 0.4], true, false);
    assert_eq!(t.pointer(), None);
}
