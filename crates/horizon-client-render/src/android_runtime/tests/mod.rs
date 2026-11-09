// Adapted from Starling (https://github.com/starling-client/starling, `android` branch at
// 150a414b09a1, `crates/horizon-client-render/src/android_runtime.rs`), written by Vasysik and the
// Starling contributors. Copyright (c) 2026 Ludovic Marie and the Horizon contributors, under the
// same MIT license as Horizon (LICENSE at the repository root).

//! Tests of the Android input runtime. Its state is process-wide, so each
//! test holds [`serial`] and starts from [`reset`].
use super::*;
use crate::gamepad::Button;

/// Android's `KEYCODE_BUTTON_A`.
const BUTTON_A: u32 = 96;

static SERIAL: Mutex<()> = Mutex::new(());

fn serial() -> MutexGuard<'static, ()> {
    let guard = lock(&SERIAL);
    reset();
    guard
}

fn reset() {
    configure_input(InputMode::Touch);
    configure_input(InputMode::Auto);
    clear_gamepads();
    configure_keypad_settings(&Settings::default());
    configure_keypad(false);
}

fn button(id: i32, down: bool) -> bool {
    let state = if down {
        ElementState::Pressed
    } else {
        ElementState::Released
    };
    gamepad_event(
        id,
        &DeviceEvent::Button {
            button: BUTTON_A,
            state,
        },
    )
}

fn motion(id: i32, value: f64) -> bool {
    gamepad_event(id, &DeviceEvent::Motion { axis: 0, value })
}

#[test]
fn explicit_modes_reject_other_devices_and_auto_follows_activity() {
    let _serial = serial();
    configure_input(InputMode::Touch);
    assert_eq!(input_mode(), InputMode::Touch);
    assert!(!note_input(InputMode::Controller));
    assert_eq!(active_input(), InputMode::Touch);
    // A controller pressed while touch is picked is read but never acts.
    assert!(button(2, true));
    assert_eq!(gamepad_state(), PadState::default());
    configure_input(InputMode::Controller);
    assert!(!note_input(InputMode::Touch));
    configure_input(InputMode::Auto);
    assert_eq!(active_input(), InputMode::Touch);
    assert!(note_input(InputMode::KeyboardMouse));
    assert_eq!(active_input(), InputMode::KeyboardMouse);
    // A resting stick does not take over; a pushed one does.
    motion(2, 0.05);
    assert_eq!(active_input(), InputMode::KeyboardMouse);
    motion(2, 1.0);
    assert_eq!(active_input(), InputMode::Controller);
    assert!(note_input(InputMode::Touch));
    assert_eq!(gamepad_state(), PadState::default());
}

#[test]
fn the_controller_in_charge_is_read_once_per_frame() {
    let _serial = serial();
    assert!(button(4, true));
    assert_eq!(active_input(), InputMode::Controller);
    assert_eq!(active_gamepad_id(), Some(4));
    // A press released before the frame still shows this once.
    button(4, false);
    assert!(gamepad_state().held(Button::South));
    assert!(!gamepad_state().held(Button::South));
    button(4, true);
    assert!(pad_state().held(Button::South));
}

#[test]
fn leaving_the_controller_lets_go_of_its_buttons() {
    let _serial = serial();
    button(4, true);
    assert!(note_input(InputMode::Touch));
    assert_eq!(active_gamepad_id(), None);
    // Back on the controller, nothing is still held from before.
    assert!(note_input(InputMode::Controller));
    assert_eq!(gamepad_state(), PadState::default());
    assert_eq!(active_gamepad_id(), None);
}

#[test]
fn removed_and_disconnected_controllers_are_forgotten() {
    let _serial = serial();
    button(4, true);
    retain_gamepads(&[4, 5]);
    assert_eq!(active_gamepad_id(), Some(4));
    retain_gamepads(&[5]);
    assert_eq!(active_gamepad_id(), None);
    button(6, true);
    assert!(gamepad_event(6, &DeviceEvent::Removed));
    assert_eq!(active_gamepad_id(), None);
    // Not a controller's event: left to the other handlers.
    assert!(!gamepad_event(
        6,
        &DeviceEvent::MouseMotion { delta: (1.0, 1.0) }
    ));
}

#[test]
fn the_keypad_profile_takes_only_its_keys_and_only_while_on() {
    let _serial = serial();
    assert!(!keypad_key(KeyCode::ArrowUp, true));
    assert_eq!(keypad_state(), PadState::default());
    configure_keypad_settings(&Settings {
        enabled: true,
        ..Settings::default()
    });
    assert!(keypad_enabled());
    assert!(keypad_key(KeyCode::ArrowUp, true));
    assert!(!keypad_key(KeyCode::KeyQ, true));
    assert_eq!(pad_state().left[1], 1.0);
    // A tap shorter than a frame still moves once.
    assert!(keypad_key(KeyCode::ArrowUp, false));
    assert!(keypad_key(KeyCode::ArrowUp, true));
    assert!(keypad_key(KeyCode::ArrowUp, false));
    assert_eq!(keypad_state().left[1], 1.0);
    assert_eq!(keypad_state(), PadState::default());
    // Turning it off lets go of held keys and ignores new ones.
    keypad_key(KeyCode::ArrowUp, true);
    configure_keypad(false);
    assert!(!keypad_key(KeyCode::ArrowDown, true));
    assert_eq!(keypad_state(), PadState::default());
}

#[test]
fn switching_modes_lets_go_of_held_input() {
    let _serial = serial();
    configure_keypad(true);
    keypad_key(KeyCode::ArrowUp, true);
    button(4, true);
    configure_input(InputMode::Controller);
    assert_eq!(keypad_state(), PadState::default());
    assert_eq!(gamepad_state(), PadState::default());
    // Picking the same mode again changes nothing.
    button(4, true);
    configure_input(InputMode::Controller);
    assert_eq!(active_gamepad_id(), Some(4));
}
