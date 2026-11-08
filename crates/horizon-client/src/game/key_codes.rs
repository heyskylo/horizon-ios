//! Key and button codes as GLFW and vanilla's `InputConstants` number them, and the small rules
//! that decide which device or screen a key press goes to.
use super::*;

pub(super) fn follow_named_keymap(
    logical_key: &winit::keyboard::Key,
    physical_key: &mut PhysicalKey,
) {
    use winit::keyboard::{Key, NamedKey};
    if let Key::Named(named) = logical_key {
        let code = match named {
            NamedKey::Escape => KeyCode::Escape,
            NamedKey::CapsLock => KeyCode::CapsLock,
            NamedKey::F1 => KeyCode::F1,
            NamedKey::F2 => KeyCode::F2,
            NamedKey::F3 => KeyCode::F3,
            NamedKey::F4 => KeyCode::F4,
            NamedKey::F5 => KeyCode::F5,
            NamedKey::F6 => KeyCode::F6,
            NamedKey::F7 => KeyCode::F7,
            NamedKey::F8 => KeyCode::F8,
            NamedKey::F9 => KeyCode::F9,
            NamedKey::F10 => KeyCode::F10,
            NamedKey::F11 => KeyCode::F11,
            NamedKey::F12 => KeyCode::F12,
            NamedKey::F13 => KeyCode::F13,
            NamedKey::F14 => KeyCode::F14,
            NamedKey::F15 => KeyCode::F15,
            NamedKey::F16 => KeyCode::F16,
            NamedKey::F17 => KeyCode::F17,
            NamedKey::F18 => KeyCode::F18,
            NamedKey::F19 => KeyCode::F19,
            NamedKey::F20 => KeyCode::F20,
            NamedKey::F21 => KeyCode::F21,
            NamedKey::F22 => KeyCode::F22,
            NamedKey::F23 => KeyCode::F23,
            NamedKey::F24 => KeyCode::F24,
            NamedKey::F25 => KeyCode::F25,
            NamedKey::F26 => KeyCode::F26,
            NamedKey::F27 => KeyCode::F27,
            NamedKey::F28 => KeyCode::F28,
            NamedKey::F29 => KeyCode::F29,
            NamedKey::F30 => KeyCode::F30,
            NamedKey::F31 => KeyCode::F31,
            NamedKey::F32 => KeyCode::F32,
            NamedKey::F33 => KeyCode::F33,
            NamedKey::F34 => KeyCode::F34,
            NamedKey::F35 => KeyCode::F35,
            _ => return,
        };
        *physical_key = PhysicalKey::Code(code);
    }
}

/// The modifier `modifiers` stands for, as GLFW words it in the native mods'
/// input hooks: `GLFW_MOD_SHIFT` 1, `GLFW_MOD_CONTROL` 2, `GLFW_MOD_ALT` 4,
/// `GLFW_MOD_SUPER` 8 — the word a NeoForge listener reads off its event
/// (`getModifiers`). The lock keys are not modifiers there.
pub(super) fn glfw_modifiers(modifiers: winit::keyboard::ModifiersState) -> u32 {
    let mut bits = 0;
    if modifiers.shift_key() {
        bits |= 1;
    }
    if modifiers.control_key() {
        bits |= 2;
    }
    if modifiers.alt_key() {
        bits |= 4;
    }
    if modifiers.super_key() {
        bits |= 8;
    }
    bits
}

/// The GLFW mouse button index `button` is, as the native mods' mouse hooks
/// hand it over: 0 left, 1 right, 2 middle, 3 back, 4 forward. `Other(n)` is
/// winit's n, the platform's own number (X11 or evdev on Linux), so a button
/// past 4 is not GLFW's index everywhere. Buttons 0–4 are the numbering
/// `keybinds::mouse_name` names (`key.mouse.<index + 1>`) and vanilla's
/// `InputConstants` reads.
pub(super) fn glfw_button(button: MouseButton) -> i32 {
    match button {
        MouseButton::Left => 0,
        MouseButton::Right => 1,
        MouseButton::Middle => 2,
        MouseButton::Back => 3,
        MouseButton::Forward => 4,
        MouseButton::Other(n) => i32::from(n),
    }
}

/// `KeyboardHandler.keyPress` for a key bound to a mouse mapping (key.attack,
/// key.use, key.pickItem): in game, a press and every repeat of the held key
/// reach `KeyMapping.set(key, true)` and `KeyMapping.click` alike, so a held
/// key keeps attacking; a release lets go. `None` behind a screen, which takes
/// the key.
pub(super) fn keyboard_button(playing: bool, action: KeyAction) -> Option<bool> {
    playing.then_some(action != KeyAction::Release)
}

/// Gameplay owns the keyboard: `Screen::Playing`, no title, and nothing open
/// over the world (`screen_over_playing`: a server dialog, a book or lectern,
/// a sign editor, a prompt). Each is a `Screen` in vanilla, so `Gui.setScreen`
/// runs `KeyMapping.releaseAll` and `Minecraft.tick` skips `handleKeybinds`
/// while it is up: a mouse button held into it is let go of (#9729).
pub(super) fn gameplay_has_keyboard(screen: Screen, title: bool, screen_over: bool) -> bool {
    screen == Screen::Playing && !title && !screen_over
}

/// A dialog drops held keys and mouse buttons it does not see (#4974).
pub(super) fn drop_held_behind_dialog(
    dialog: bool,
    keys: &mut horizon_client::held::HeldKeys,
    left: &mut bool,
    right: &mut bool,
) {
    if dialog {
        keys.clear();
        *left = false;
        *right = false;
    }
}

/// `AbstractContainerScreen.keyPressed` for a key bound to a mouse mapping: a press or repeat of
/// key.pickItem's key clones the hovered slot; keys bound to key.attack and key.use do nothing
/// there (#5378).
pub(super) fn container_key_clones(button: MouseButton, action: KeyAction) -> bool {
    button == MouseButton::Middle && action != KeyAction::Release
}
