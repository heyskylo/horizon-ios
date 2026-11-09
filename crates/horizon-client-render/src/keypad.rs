// Adapted from Starling (https://github.com/starling-client/starling, `android` branch at
// 150a414b09a1, `crates/horizon-client-render/src/keypad.rs`), written by Vasysik and the
// Starling contributors. Copyright (c) 2026 Ludovic Marie and the Horizon contributors, under the
// same MIT license as Horizon (LICENSE at the repository root).

//! The keypad profile: play with a D-pad, a TV remote or a phone's number keys
//! through the controller path. Each profile action is a set of keys; held
//! actions become a `gamepad::PadState` (movement on the left stick, looking on
//! the right stick, attack and use on the triggers), so the controller bindings,
//! menus and HUD hints all apply unchanged. The player can rebind every action;
//! the profile is saved apart from `options.txt`. Pure logic: the runtime routes
//! keys here only while the profile is turned on.
use crate::gamepad::{Button, PadState};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use winit::keyboard::{Key, KeyCode, NamedKey, NativeKeyCode, PhysicalKey};

/// The profile's actions: a stable id (the saved name, not a key code), the
/// label shown in the options, and the default keys by their `options.txt`
/// names.
pub const ACTIONS: &[(&str, &str, &[&str])] = &[
    (
        "key.keypad.forward",
        "Forward",
        &["key.keyboard.up", "key.keyboard.keypad.8"],
    ),
    (
        "key.keypad.back",
        "Backward",
        &["key.keyboard.down", "key.keyboard.keypad.2"],
    ),
    (
        "key.keypad.left",
        "Left",
        &["key.keyboard.left", "key.keyboard.keypad.4"],
    ),
    (
        "key.keypad.right",
        "Right",
        &["key.keyboard.right", "key.keyboard.keypad.6"],
    ),
    ("key.keypad.look_up", "Look up", &["key.keyboard.2"]),
    ("key.keypad.look_down", "Look down", &["key.keyboard.8"]),
    ("key.keypad.look_left", "Look left", &["key.keyboard.4"]),
    ("key.keypad.look_right", "Look right", &["key.keyboard.6"]),
    ("key.keypad.attack", "Attack", &["key.keyboard.1"]),
    ("key.keypad.use", "Use", &["key.keyboard.3"]),
    (
        "key.keypad.jump",
        "Jump",
        &[
            "key.keyboard.5",
            "key.keyboard.keypad.5",
            "key.keyboard.enter",
            "key.keyboard.keypad.enter",
        ],
    ),
    (
        "key.keypad.sneak",
        "Sneak",
        &["key.keyboard.0", "key.keyboard.keypad.0"],
    ),
    ("key.keypad.inventory", "Inventory", &["key.keyboard.7"]),
    (
        "key.keypad.next_slot",
        "Next hotbar slot",
        &["key.keyboard.9"],
    ),
    (
        "key.keypad.forward_left",
        "Forward + left",
        &["key.keyboard.keypad.7"],
    ),
    (
        "key.keypad.forward_right",
        "Forward + right",
        &["key.keyboard.keypad.9"],
    ),
    (
        "key.keypad.back_left",
        "Backward + left",
        &["key.keyboard.keypad.1"],
    ),
    (
        "key.keypad.back_right",
        "Backward + right",
        &["key.keyboard.keypad.3"],
    ),
];

// Positions in `ACTIONS`, which are also the bits of `Keypad`'s held set.
const FORWARD: usize = 0;
const BACK: usize = 1;
const LEFT: usize = 2;
const RIGHT: usize = 3;
const LOOK_UP: usize = 4;
const LOOK_DOWN: usize = 5;
const LOOK_LEFT: usize = 6;
const LOOK_RIGHT: usize = 7;
const ATTACK: usize = 8;
const USE: usize = 9;
const JUMP: usize = 10;
const SNEAK: usize = 11;
const INVENTORY: usize = 12;
const NEXT_SLOT: usize = 13;
const FORWARD_LEFT: usize = 14;
const FORWARD_RIGHT: usize = 15;
const BACK_LEFT: usize = 16;
const BACK_RIGHT: usize = 17;

/// The saved profile (`horizon-keypad.json`): on or off, and the actions the
/// player rebound (the others keep their default keys).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub enabled: bool,
    pub bindings: BTreeMap<String, Vec<String>>,
}
impl Settings {
    /// The keys of an action: the player's, or the defaults.
    pub fn keys(&self, action: &str) -> Vec<String> {
        self.bindings.get(action).cloned().unwrap_or_else(|| {
            ACTIONS
                .iter()
                .find(|a| a.0 == action)
                .map_or_else(Vec::new, |a| a.2.iter().map(|s| (*s).into()).collect())
        })
    }
    /// Bind an action to one key, replacing its defaults.
    pub fn set(&mut self, action: &str, key: String) {
        self.bindings.insert(action.into(), vec![key]);
    }
    /// Give an action its default keys back.
    pub fn reset(&mut self, action: &str) {
        self.bindings.remove(action);
    }
    /// The controller button a key presses in menus: the arrow keys, Enter,
    /// Space and Escape as on any keyboard, and the profile's own keys for the
    /// matching directions, confirm and back.
    pub fn menu_button(&self, key: KeyCode) -> Option<Button> {
        match key {
            KeyCode::ArrowUp => return Some(Button::DPadUp),
            KeyCode::ArrowDown => return Some(Button::DPadDown),
            KeyCode::ArrowLeft => return Some(Button::DPadLeft),
            KeyCode::ArrowRight => return Some(Button::DPadRight),
            KeyCode::Enter | KeyCode::NumpadEnter | KeyCode::Space => return Some(Button::South),
            KeyCode::Escape => return Some(Button::East),
            _ => {}
        }
        let name = crate::keybinds::key_name(key)?;
        [
            (FORWARD, Button::DPadUp),
            (LOOK_UP, Button::DPadUp),
            (BACK, Button::DPadDown),
            (LOOK_DOWN, Button::DPadDown),
            (LEFT, Button::DPadLeft),
            (LOOK_LEFT, Button::DPadLeft),
            (RIGHT, Button::DPadRight),
            (LOOK_RIGHT, Button::DPadRight),
            (JUMP, Button::South),
            (USE, Button::South),
            (INVENTORY, Button::East),
            (SNEAK, Button::East),
            (NEXT_SLOT, Button::RightBumper),
        ]
        .into_iter()
        .find_map(|(action, button)| {
            self.keys(ACTIONS[action].0)
                .iter()
                .any(|k| k == name)
                .then_some(button)
        })
    }
    pub fn load(path: &Path) -> Self {
        std::fs::read(path)
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default()
    }
    /// Save the profile; an untouched profile writes no file.
    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        if *self == Self::default() && !path.exists() {
            return Ok(());
        }
        let temp = path.with_extension("json.tmp");
        std::fs::write(&temp, serde_json::to_vec_pretty(self)?)?;
        std::fs::rename(temp, path)
    }
}

/// The key code of a key event, filling in what winit leaves unnamed on Android:
/// the D-pad centre (KEYCODE_DPAD_CENTER, 23) is Enter, and the diagonal D-pad
/// keys (268-271) are the numpad diagonals, so the defaults cover them.
pub fn resolve_key(physical: PhysicalKey, logical: &Key) -> Option<KeyCode> {
    match (physical, logical) {
        (PhysicalKey::Code(code), _) => Some(code),
        (PhysicalKey::Unidentified(NativeKeyCode::Android(code)), _) => match code {
            23 => Some(KeyCode::Enter),
            268 => Some(KeyCode::Numpad7),
            269 => Some(KeyCode::Numpad1),
            270 => Some(KeyCode::Numpad9),
            271 => Some(KeyCode::Numpad3),
            _ => None,
        },
        (_, Key::Named(NamedKey::Enter)) => Some(KeyCode::Enter),
        _ => None,
    }
}

/// The keypad as a controller: the held profile actions, and presses too short
/// to be seen by a poll.
#[derive(Default)]
pub struct Keypad {
    settings: Settings,
    held: BTreeSet<KeyCode>,
    actions: u32,
    pending: u32,
}
impl Keypad {
    /// Use this profile; a changed profile releases everything held.
    pub fn configure(&mut self, settings: &Settings) {
        if self.settings != *settings {
            self.clear();
            self.settings = settings.clone();
        }
    }
    pub fn clear(&mut self) {
        self.held.clear();
        self.actions = 0;
        self.pending = 0;
    }
    /// A key event. Returns true when the key belongs to the profile (the
    /// caller then keeps it from the game's keyboard input).
    pub fn key(&mut self, key: KeyCode, down: bool) -> bool {
        let Some(name) = crate::keybinds::key_name(key) else {
            return false;
        };
        if !ACTIONS
            .iter()
            .any(|a| self.settings.keys(a.0).iter().any(|k| k == name))
        {
            return false;
        }
        if down {
            self.held.insert(key);
        } else {
            self.held.remove(&key);
        }
        // Recomputed from the held keys, so two keys bound to one action (or one
        // key in two actions) release only when the last of them goes up.
        let mut actions = 0;
        for (i, a) in ACTIONS.iter().enumerate() {
            if self
                .settings
                .keys(a.0)
                .iter()
                .filter_map(|k| crate::keybinds::key_code(k))
                .any(|k| self.held.contains(&k))
            {
                actions |= 1 << i;
            }
        }
        self.pending |= actions & !self.actions;
        self.actions = actions;
        true
    }
    /// The held actions as a pad, without consuming short presses.
    pub fn state(&self) -> PadState {
        Self::state_for(self.actions)
    }
    /// The held actions as a pad, once per frame: a press released since the
    /// last poll still shows this once.
    pub fn poll(&mut self) -> PadState {
        Self::state_for(self.actions | std::mem::take(&mut self.pending))
    }
    fn state_for(actions: u32) -> PadState {
        let held = |i: usize| actions & (1 << i) != 0;
        let one = |on: bool| f32::from(u8::from(on));
        let axis = |positive, negative| one(positive) - one(negative);
        let left = [
            axis(
                held(RIGHT) || held(FORWARD_RIGHT) || held(BACK_RIGHT),
                held(LEFT) || held(FORWARD_LEFT) || held(BACK_LEFT),
            ),
            axis(
                held(FORWARD) || held(FORWARD_LEFT) || held(FORWARD_RIGHT),
                held(BACK) || held(BACK_LEFT) || held(BACK_RIGHT),
            ),
        ];
        // Diagonals walk at the same speed as straight lines.
        let len = left[0].hypot(left[1]).max(1.0);
        let mut state = PadState {
            left: left.map(|v| v / len),
            right: [
                axis(held(LOOK_RIGHT), held(LOOK_LEFT)),
                axis(held(LOOK_UP), held(LOOK_DOWN)),
            ],
            // The default controller bindings: use on the left trigger, attack on the right.
            triggers: [one(held(USE)), one(held(ATTACK))],
            ..Default::default()
        };
        for (action, button) in [
            (JUMP, Button::South),
            (SNEAK, Button::RightStick),
            (INVENTORY, Button::North),
            (NEXT_SLOT, Button::RightBumper),
        ] {
            state.set(button, held(action));
        }
        state
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn action_positions_match_their_ids() {
        for (i, id) in [
            (FORWARD, "forward"),
            (BACK, "back"),
            (LEFT, "left"),
            (RIGHT, "right"),
            (LOOK_UP, "look_up"),
            (LOOK_DOWN, "look_down"),
            (LOOK_LEFT, "look_left"),
            (LOOK_RIGHT, "look_right"),
            (ATTACK, "attack"),
            (USE, "use"),
            (JUMP, "jump"),
            (SNEAK, "sneak"),
            (INVENTORY, "inventory"),
            (NEXT_SLOT, "next_slot"),
            (FORWARD_LEFT, "forward_left"),
            (FORWARD_RIGHT, "forward_right"),
            (BACK_LEFT, "back_left"),
            (BACK_RIGHT, "back_right"),
        ] {
            assert_eq!(ACTIONS[i].0, format!("key.keypad.{id}"));
        }
        assert_eq!(ACTIONS.len(), 18);
    }
    #[test]
    fn menu_navigation_uses_profile_rebindings_and_standard_ui_keys() {
        let mut s = Settings::default();
        assert_eq!(s.menu_button(KeyCode::Digit5), Some(Button::South));
        assert_eq!(s.menu_button(KeyCode::Digit2), Some(Button::DPadUp));
        s.set("key.keypad.forward", "key.keyboard.z".into());
        assert_eq!(s.menu_button(KeyCode::KeyZ), Some(Button::DPadUp));
        assert_eq!(s.menu_button(KeyCode::ArrowUp), Some(Button::DPadUp));
        assert_eq!(s.menu_button(KeyCode::Escape), Some(Button::East));
        assert_eq!(s.menu_button(KeyCode::F3), None);
        s.reset("key.keypad.forward");
        assert_eq!(s, Settings::default());
    }
    #[test]
    fn rebinding_and_overlapping_aliases_keep_normalized_eight_way_motion() {
        let mut k = Keypad::default();
        let mut settings = Settings::default();
        settings.set("key.keypad.forward", "key.keyboard.z".into());
        k.configure(&settings);
        assert!(!k.key(KeyCode::ArrowUp, true));
        k.key(KeyCode::KeyZ, true);
        k.key(KeyCode::Numpad9, true);
        let state = k.poll();
        assert!((state.left[0].hypot(state.left[1]) - 1.0).abs() < 1e-6);
        k.key(KeyCode::Numpad9, false);
        assert_eq!(k.poll().left, [0.0, 1.0]);
        k.key(KeyCode::Digit5, true);
        k.key(KeyCode::Enter, true);
        k.poll();
        k.key(KeyCode::Digit5, false);
        assert!(k.poll().held(Button::South));
        k.key(KeyCode::Enter, false);
        assert!(!k.poll().held(Button::South));
        let saved = serde_json::to_vec(&settings).unwrap();
        assert_eq!(
            serde_json::from_slice::<Settings>(&saved).unwrap(),
            settings
        );
    }
    #[test]
    fn all_default_profile_keys_resolve_and_diagonals_are_bindable() {
        for action in ACTIONS {
            for key in action.2 {
                assert!(crate::keybinds::key_code(key).is_some(), "{key}");
            }
        }
        for (code, key) in [
            (268, KeyCode::Numpad7),
            (269, KeyCode::Numpad1),
            (270, KeyCode::Numpad9),
            (271, KeyCode::Numpad3),
        ] {
            assert_eq!(
                resolve_key(
                    PhysicalKey::Unidentified(NativeKeyCode::Android(code)),
                    &Key::Unidentified(winit::keyboard::NativeKey::Android(code))
                ),
                Some(key)
            );
        }
        assert_eq!(
            resolve_key(
                PhysicalKey::Unidentified(NativeKeyCode::Android(24)),
                &Key::Named(NamedKey::AudioVolumeUp)
            ),
            None
        );
    }
    #[test]
    fn android_dpad_centre_and_numpad_enter_jump() {
        let code = resolve_key(
            PhysicalKey::Unidentified(NativeKeyCode::Android(23)),
            &Key::Named(NamedKey::Enter),
        )
        .unwrap();
        let mut keypad = Keypad::default();
        keypad.key(code, true);
        assert!(keypad.poll().held(Button::South));
        keypad.clear();
        keypad.key(KeyCode::NumpadEnter, true);
        assert!(keypad.poll().held(Button::South));
    }
    #[test]
    fn short_taps_survive_one_poll_and_clear_discards_them() {
        let mut keypad = Keypad::default();
        keypad.key(KeyCode::Digit1, true);
        keypad.key(KeyCode::Digit1, false);
        assert_eq!(keypad.poll().triggers, [0.0, 1.0]);
        assert_eq!(keypad.poll(), PadState::default());
        keypad.key(KeyCode::Digit5, true);
        keypad.clear();
        assert_eq!(keypad.poll(), PadState::default());
    }
    #[test]
    fn diagonal_release_and_camera_are_independent() {
        let mut keypad = Keypad::default();
        keypad.key(KeyCode::ArrowUp, true);
        keypad.key(KeyCode::ArrowRight, true);
        keypad.key(KeyCode::Digit6, true);
        let s = keypad.state();
        assert!((s.left[0].hypot(s.left[1]) - 1.0).abs() < 0.0001);
        assert_eq!(s.right, [1.0, 0.0]);
        keypad.key(KeyCode::Digit6, false);
        assert_eq!(keypad.state().right, [0.0, 0.0]);
        keypad.clear();
        assert_eq!(keypad.state(), PadState::default());
    }
    #[test]
    fn attack_and_use_are_analog_triggers_with_independent_releases() {
        let mut keypad = Keypad::default();
        let mut edges = crate::gamepad::Edges::default();
        keypad.key(KeyCode::Digit1, true);
        keypad.key(KeyCode::Digit3, true);
        assert_eq!(keypad.state().triggers, [1.0, 1.0]);
        let pressed = edges.update(&keypad.state(), 0.4);
        assert!(pressed.contains(&(Button::RightTrigger, true)));
        assert!(pressed.contains(&(Button::LeftTrigger, true)));
        keypad.key(KeyCode::Digit1, false);
        assert_eq!(
            edges.update(&keypad.state(), 0.4),
            [(Button::RightTrigger, false)]
        );
        keypad.clear();
        assert_eq!(
            edges.update(&keypad.state(), 0.4),
            [(Button::LeftTrigger, false)]
        );
    }
    #[test]
    fn a_changed_profile_releases_held_keys_and_an_untouched_one_writes_no_file() {
        let mut keypad = Keypad::default();
        keypad.key(KeyCode::Digit5, true);
        keypad.configure(&Settings::default());
        assert!(keypad.state().held(Button::South));
        let settings = Settings {
            enabled: true,
            ..Default::default()
        };
        keypad.configure(&settings);
        assert_eq!(keypad.state(), PadState::default());

        let dir = std::env::temp_dir().join(format!("horizon-keypad-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("horizon-keypad.json");
        Settings::default().save(&path).unwrap();
        assert!(!path.exists());
        settings.save(&path).unwrap();
        assert_eq!(Settings::load(&path), settings);
        std::fs::remove_dir_all(&dir).unwrap();
        assert_eq!(Settings::load(&path), Settings::default());
    }
}
