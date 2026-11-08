//! Which of the overlays over play owns the controller now, for the checks that ask about the
//! on-screen keyboard. The prompts' own checks still call `prompt_open` directly (`pad.rs`,
//! `pad_actions.rs`, `pad_menu.rs`, `pad/guide.rs`).
//!
//! In priority order, highest first ([`PadCapture`]): the on-screen keyboard, then a prompt over
//! the world (a server dialog, the code of conduct, a link, command or resource pack prompt).
//! Only one owns the pad at a time. Screens are not overlays: `pad_in_menu` says when one has the
//! pad.
use super::Game;

/// An overlay that owns the controller, highest priority first.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum PadCapture {
    /// The on-screen keyboard: every press types or moves on it.
    Keyboard,
    /// A prompt over the world, answered before anything behind it.
    Prompt,
}

impl Game<'_> {
    /// The overlay that owns the pad now, if one does.
    pub(super) fn pad_capture(&self) -> Option<PadCapture> {
        if self.pad.menu.osk.is_some() {
            Some(PadCapture::Keyboard)
        } else if self.prompt_open() {
            Some(PadCapture::Prompt)
        } else {
            None
        }
    }

    /// The on-screen keyboard owns the pad.
    pub(super) fn pad_keyboard_captures(&self) -> bool {
        self.pad_capture() == Some(PadCapture::Keyboard)
    }
}
