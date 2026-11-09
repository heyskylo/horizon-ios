//! Selection and hit testing for the debug game-mode switcher.
use crate::command::args::GameMode;

pub const MODES: [GameMode; 4] = [
    GameMode::Creative,
    GameMode::Survival,
    GameMode::Adventure,
    GameMode::Spectator,
];

pub const ICONS: [&str; 4] = [
    "minecraft:grass_block",
    "minecraft:iron_sword",
    "minecraft:buried_treasure_map",
    "minecraft:ender_eye",
];

pub const SLOT_SIZE: i32 = 26;

pub fn slot_origin(width: i32, height: i32, index: usize) -> (i32, i32) {
    (width / 2 - 59 + index as i32 * 31, height / 2 - 31)
}

/// `GameModeSwitcherScreen.mouseReleased`: `keyDebugModifier.matchesMouse`
/// picks the hovered mode and closes the screen, as the modifier key's own
/// release does in `keyReleased`. `button` and `modifier` are mapping key
/// names (`key.mouse.4`); a modifier bound to a keyboard key matches no mouse
/// release.
pub fn release_applies(button: &str, modifier: &str) -> bool {
    modifier.starts_with("key.mouse.") && button == modifier
}

pub struct GameModeSwitcher {
    pub selected: GameMode,
    first_mouse: Option<(i32, i32)>,
}

impl GameModeSwitcher {
    pub fn new(current: GameMode, previous: Option<GameMode>) -> Self {
        Self {
            selected: previous.unwrap_or(if current == GameMode::Creative {
                GameMode::Survival
            } else {
                GameMode::Creative
            }),
            first_mouse: None,
        }
    }

    pub fn cycle(&mut self) {
        let index = MODES.iter().position(|m| *m == self.selected).unwrap();
        self.selected = MODES[(index + 1) % MODES.len()];
        self.first_mouse = None;
    }

    pub fn hover(&mut self, width: i32, height: i32, cursor: (i32, i32)) {
        if *self.first_mouse.get_or_insert(cursor) == cursor {
            return;
        }
        for (index, mode) in MODES.iter().enumerate() {
            let (x, y) = slot_origin(width, height, index);
            if cursor.0 >= x
                && cursor.0 < x + SLOT_SIZE
                && cursor.1 >= y
                && cursor.1 < y + SLOT_SIZE
            {
                self.selected = *mode;
                return;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initial_selection_and_cycle_follow_debug_order() {
        assert_eq!(
            GameModeSwitcher::new(GameMode::Creative, None).selected,
            GameMode::Survival
        );
        for current in [GameMode::Survival, GameMode::Adventure, GameMode::Spectator] {
            assert_eq!(
                GameModeSwitcher::new(current, None).selected,
                GameMode::Creative
            );
        }
        let mut state = GameModeSwitcher::new(GameMode::Survival, Some(GameMode::Adventure));
        for mode in [
            GameMode::Adventure,
            GameMode::Spectator,
            GameMode::Creative,
            GameMode::Survival,
        ] {
            assert_eq!(state.selected, mode);
            state.cycle();
        }
    }

    #[test]
    fn stationary_mouse_does_not_override_keyboard_selection() {
        let mut state = GameModeSwitcher::new(GameMode::Creative, None);
        let cursor = slot_origin(320, 240, 0);
        state.hover(320, 240, cursor);
        state.hover(320, 240, cursor);
        assert_eq!(state.selected, GameMode::Survival);
        state.hover(320, 240, (cursor.0 + 1, cursor.1));
        assert_eq!(state.selected, GameMode::Creative);
        state.cycle();
        state.hover(320, 240, (cursor.0 + 1, cursor.1));
        assert_eq!(state.selected, GameMode::Survival);
    }

    #[test]
    fn hover_slots_exclude_gaps_and_right_and_bottom_edges() {
        let mut state = GameModeSwitcher::new(GameMode::Creative, None);
        state.hover(320, 240, (0, 0));
        let (x, y) = slot_origin(320, 240, 2);
        for cursor in [(x - 1, y), (x + 26, y), (x, y - 1), (x, y + 26)] {
            state.hover(320, 240, cursor);
            assert_eq!(state.selected, GameMode::Survival);
        }
        state.hover(320, 240, (x + 25, y + 25));
        assert_eq!(state.selected, GameMode::Adventure);
    }

    /// `GameModeSwitcherScreen.mouseReleased`: only the debug modifier's own
    /// button applies the selection (and closes the screen). A modifier on
    /// F3 matches no mouse button, and another button is an ordinary release.
    #[test]
    fn only_the_debug_modifiers_mouse_button_applies_on_release() {
        assert!(release_applies("key.mouse.4", "key.mouse.4"));
        assert!(!release_applies("key.mouse.4", "key.keyboard.f3"));
        assert!(!release_applies("key.mouse.middle", "key.mouse.4"));
        assert!(!release_applies("key.mouse.4", "key.mouse.middle"));
    }
}
