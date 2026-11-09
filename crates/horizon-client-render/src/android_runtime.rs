// Adapted from Starling (https://github.com/starling-client/starling, `android` branch at
// 150a414b09a1, the input part of `crates/horizon-client-render/src/android_runtime.rs`), written
// by Vasysik and the Starling contributors. Copyright (c) 2026 Ludovic Marie and the Horizon
// contributors, under the same MIT license as Horizon (LICENSE at the repository root).

//! The Android input runtime: which input device is in charge, the connected
//! controllers and the keypad profile, held once for the whole process. The
//! Android event backend, the options screen and the game all reach the same
//! state through these functions, from whichever thread delivers the event.
//!
//! Pure state over [`android_gamepad`](crate::android_gamepad) and
//! [`keypad`](crate::keypad): nothing here talks to the OS, so it builds and is
//! tested on every platform; only the Android build feeds it.
use crate::android_gamepad::{Devices, InputMode, Selection};
use crate::gamepad::PadState;
use crate::keypad::{Keypad, Settings};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{LazyLock, Mutex, MutexGuard, PoisonError};
use winit::event::{DeviceEvent, ElementState};
use winit::keyboard::KeyCode;

static SELECTION: LazyLock<Mutex<Selection>> = LazyLock::new(Mutex::default);
static GAMEPADS: LazyLock<Mutex<Devices>> = LazyLock::new(Mutex::default);
static KEYPAD: LazyLock<Mutex<Keypad>> = LazyLock::new(Mutex::default);
static KEYPAD_ENABLED: AtomicBool = AtomicBool::new(false);

/// A lock that a panic elsewhere never makes unusable: the state stays
/// consistent between calls, so input keeps working.
fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// The input mode picked in the options.
pub fn input_mode() -> InputMode {
    lock(&SELECTION).mode()
}

/// The device in charge now (never `Auto`).
pub fn active_input() -> InputMode {
    lock(&SELECTION).active()
}

/// Pick an input mode. A change lets go of every held controller and keypad
/// input, so nothing stays pressed across the switch.
pub fn configure_input(mode: InputMode) {
    let changed = lock(&SELECTION).configure(mode);
    if changed {
        clear_gamepads();
        clear_keypad();
    }
}

/// A device of `kind` was used: in `Auto` it takes over. Returns whether it
/// may act (false when the player picked another mode). Leaving the
/// controller lets go of its held buttons.
pub fn note_input(kind: InputMode) -> bool {
    let (accepted, left_controller) = {
        let mut selection = lock(&SELECTION);
        let was = selection.active();
        let accepted = selection.note(kind);
        (
            accepted,
            accepted && was == InputMode::Controller && kind != InputMode::Controller,
        )
    };
    if left_controller {
        clear_gamepads();
    }
    accepted
}

/// A raw event of the controller with Android device id `id`: motion axes
/// and buttons by Android code. Returns whether the event was a controller's
/// (the caller then keeps it from other handlers); a meaningful one also
/// notes the controller as used. A removed device forgets every controller,
/// the platform then lists those still there ([`retain_gamepads`]).
pub fn gamepad_event(id: i32, event: &DeviceEvent) -> bool {
    let activity = {
        let mut devices = lock(&GAMEPADS);
        match *event {
            DeviceEvent::Motion { axis, value } => devices.axis(id, axis, value),
            DeviceEvent::Button { button, state } => {
                devices.key(id, button, state == ElementState::Pressed)
            }
            DeviceEvent::Removed => {
                devices.clear();
                false
            }
            _ => return false,
        }
    };
    if activity {
        note_input(InputMode::Controller);
    }
    true
}

/// Forget the controllers that are no longer connected.
pub fn retain_gamepads(ids: &[i32]) {
    lock(&GAMEPADS).retain(ids);
}

/// Let go of every controller's held input.
pub fn clear_gamepads() {
    lock(&GAMEPADS).clear();
}

/// The Android device id of the controller in charge (for its rumble), if a
/// controller is the active input.
pub fn active_gamepad_id() -> Option<i32> {
    if active_input() != InputMode::Controller {
        return None;
    }
    lock(&GAMEPADS).active_id()
}

/// The controller in charge as a pad, once per frame; nothing while another
/// device is the active input.
pub fn gamepad_state() -> PadState {
    if active_input() != InputMode::Controller {
        return PadState::default();
    }
    lock(&GAMEPADS).poll()
}

/// Use this keypad profile, on or off as it says.
pub fn configure_keypad_settings(settings: &Settings) {
    lock(&KEYPAD).configure(settings);
    KEYPAD_ENABLED.store(settings.enabled, Ordering::Relaxed);
}

/// Turn the keypad profile on or off, letting go of its held keys.
pub fn configure_keypad(enabled: bool) {
    KEYPAD_ENABLED.store(enabled, Ordering::Relaxed);
    clear_keypad();
}

pub fn keypad_enabled() -> bool {
    KEYPAD_ENABLED.load(Ordering::Relaxed)
}

/// A key event for the keypad profile. Returns true when the profile is on
/// and the key is one of its own (the caller keeps it from the game's
/// keyboard input).
pub fn keypad_key(key: KeyCode, down: bool) -> bool {
    keypad_enabled() && lock(&KEYPAD).key(key, down)
}

/// Let go of every key the keypad profile holds.
pub fn clear_keypad() {
    lock(&KEYPAD).clear();
}

/// The keypad profile's held actions as a pad, once per frame.
pub fn keypad_state() -> PadState {
    lock(&KEYPAD).poll()
}

/// The keypad profile and the controller in charge together, once per frame:
/// what the game's controller path reads on Android.
pub fn pad_state() -> PadState {
    keypad_state().merge(&gamepad_state())
}

#[cfg(test)]
mod tests;
