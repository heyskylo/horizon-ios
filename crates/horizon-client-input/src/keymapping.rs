//! The down/click state of `KeyMapping` and `ToggleKeyMapping` (sneak,
//! sprint, attack and use can be toggled instead of held).

/// One mapping's runtime state. `toggle` is `Some(restore)` for a
/// `ToggleKeyMapping` (with its `shouldRestore` flag), `None` for a plain one.
#[derive(Clone, Debug)]
pub struct KeyState {
    pub is_down: bool,
    click_count: i32,
    released_by_screen_when_down: bool,
    toggle: Option<bool>,
    keyboard: bool,
}

impl KeyState {
    pub fn plain() -> Self {
        Self {
            is_down: false,
            click_count: 0,
            released_by_screen_when_down: false,
            toggle: None,
            keyboard: true,
        }
    }
    /// A `ToggleKeyMapping`; `keyboard` is whether the bound key is a keyboard key.
    pub fn toggle(should_restore: bool, keyboard: bool) -> Self {
        Self {
            toggle: Some(should_restore),
            keyboard,
            ..Self::plain()
        }
    }
    /// `KeyMapping.click`: one more pending click.
    pub fn click(&mut self) {
        self.click_count += 1;
    }
    /// `consumeClick`.
    pub fn consume_click(&mut self) -> bool {
        if self.click_count == 0 {
            false
        } else {
            self.click_count -= 1;
            true
        }
    }
    /// `setDown`; `needs_toggle` is the mapping's option value.
    pub fn set_down(&mut self, down: bool, needs_toggle: bool) {
        if self.toggle.is_some() && needs_toggle {
            if down {
                self.is_down = !self.is_down;
            }
        } else {
            self.is_down = down;
        }
    }
    /// `shouldSetOnIngameFocus` for a keyboard-bound, bound key.
    pub fn should_set_on_ingame_focus(&self, needs_toggle: bool) -> bool {
        self.keyboard && (self.toggle.is_none() || !needs_toggle)
    }
    /// `release` (a screen opened): drops pending clicks and the held state.
    pub fn release(&mut self, needs_toggle: bool) {
        if self.toggle.is_some() {
            // ToggleKeyMapping.release does not clear pending clicks.
            if needs_toggle && self.is_down || self.released_by_screen_when_down {
                self.released_by_screen_when_down = true;
            }
            self.reset();
        } else {
            self.click_count = 0;
            self.set_down(false, needs_toggle);
        }
    }
    /// `ToggleKeyMapping.reset`.
    pub fn reset(&mut self) {
        self.is_down = false;
    }
    /// Clicks not yet consumed.
    pub fn pending_clicks(&self) -> i32 {
        self.click_count
    }
    /// `shouldRestoreStateOnScreenClosed` (clears the remembered release).
    pub fn should_restore_state_on_screen_closed(&mut self, needs_toggle: bool) -> bool {
        let Some(restore) = self.toggle else {
            return false;
        };
        let result = restore && needs_toggle && self.keyboard && self.released_by_screen_when_down;
        self.released_by_screen_when_down = false;
        result
    }
}

/// A keyboard event (`KeyEvent.Action`): SDL reports a held key's repeats
/// as their own action.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyAction {
    Release,
    Press,
    Repeat,
}

/// Sneak.
pub const SNEAK: usize = 0;
/// Sprint.
pub const SPRINT: usize = 1;
/// Attack (mouse).
pub const ATTACK: usize = 2;

/// The `ToggleKeyMapping`s `Options` builds: sneak and sprint (keyboard,
/// restored after a screen), attack (mouse, restored) and use (mouse, not
/// restored), and the handlers that drive them. `needs_toggle` is each
/// mapping's toggle option, read at every event as vanilla does.
#[derive(Clone, Debug)]
pub struct ToggleKeys(pub [KeyState; 4]);
impl Default for ToggleKeys {
    fn default() -> Self {
        Self([
            KeyState::toggle(true, true),
            KeyState::toggle(true, true),
            KeyState::toggle(true, false),
            KeyState::toggle(false, false),
        ])
    }
}
impl ToggleKeys {
    pub fn is_down(&self, slot: usize) -> bool {
        self.0[slot].is_down
    }
    /// `KeyboardHandler.keyPress` for a key bound to `slot`: a release sets
    /// the mapping up (`KeyMapping.set(key, false)`); a press or a repeat
    /// with no screen open sets it down and clicks it.
    pub fn key(&mut self, slot: usize, action: KeyAction, screen_open: bool, needs_toggle: bool) {
        match action {
            KeyAction::Release => self.0[slot].set_down(false, needs_toggle),
            KeyAction::Press | KeyAction::Repeat if !screen_open => {
                self.0[slot].set_down(true, needs_toggle);
                self.0[slot].click();
            }
            _ => {}
        }
    }
    /// `MouseHandler.onButton` for a button bound to `slot`: only without a
    /// screen or overlay.
    pub fn button(&mut self, slot: usize, pressed: bool, screen_open: bool, needs_toggle: bool) {
        if !screen_open {
            self.0[slot].set_down(pressed, needs_toggle);
            if pressed {
                self.0[slot].click();
            }
        }
    }
    /// `Gui.setScreen` with a screen: `KeyMapping.releaseAll`.
    pub fn screen_opened(&mut self, needs_toggle: [bool; 4]) {
        for (key, option) in self.0.iter_mut().zip(needs_toggle) {
            key.release(option);
        }
    }
    /// `Gui.setScreen(null)` in a level:
    /// `KeyMapping.restoreToggleStatesOnScreenClosed`.
    pub fn screen_closed(&mut self, needs_toggle: [bool; 4]) {
        for (key, option) in self.0.iter_mut().zip(needs_toggle) {
            if key.should_restore_state_on_screen_closed(option) {
                key.set_down(true, option);
            }
        }
    }
    /// `LocalPlayer.respawn`: `KeyMapping.resetToggleKeys`.
    pub fn respawn(&mut self) {
        for key in &mut self.0 {
            key.reset();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn toggle_flips_on_press_and_ignores_release() {
        let mut k = KeyState::toggle(true, true);
        k.set_down(true, true);
        assert!(k.is_down);
        k.set_down(false, true);
        assert!(k.is_down);
        k.set_down(true, true);
        assert!(!k.is_down);
    }
    #[test]
    fn screens_block_presses_but_not_releases() {
        let mut keys = ToggleKeys::default();
        keys.key(SNEAK, KeyAction::Press, true, false);
        assert!(!keys.is_down(SNEAK));
        keys.key(SNEAK, KeyAction::Press, false, false);
        assert!(keys.is_down(SNEAK));
        keys.key(SNEAK, KeyAction::Release, true, false);
        assert!(!keys.is_down(SNEAK));
        keys.key(SPRINT, KeyAction::Press, false, true);
        keys.respawn();
        assert!(!keys.is_down(SPRINT));
    }
    #[test]
    fn screen_release_restores_toggled_state() {
        let mut k = KeyState::toggle(true, true);
        k.set_down(true, true);
        k.release(true);
        assert!(!k.is_down);
        assert!(k.should_restore_state_on_screen_closed(true));
        assert!(!k.should_restore_state_on_screen_closed(true));
    }
}
