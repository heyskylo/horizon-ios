//! Which of the overlays over play owns the controller now, for the checks that ask about the
//! on-screen keyboard and the radial menu. The prompts' own checks still call `prompt_open`
//! directly (`pad.rs`, `pad_actions.rs`, `pad_menu.rs`, `pad/guide.rs`); here a prompt only
//! outranks the radial menu.
//!
//! In priority order, highest first ([`PadCapture`]): the on-screen keyboard, then a prompt over
//! the world (a server dialog, the code of conduct, a link, command or resource pack prompt),
//! then the radial menu. Only one owns the pad at a time; the radial menu never opens under the
//! others and closes when one comes up (`pad_radial_can_open`, `pad_radial_frame`), so the order
//! only settles the poll in which both are up. Screens are not overlays: `pad_in_menu` says
//! when one has the pad.
use super::super::Game;

/// An overlay that owns the controller, highest priority first.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::game) enum PadCapture {
    /// The on-screen keyboard: every press types or moves on it.
    Keyboard,
    /// A prompt over the world, answered before anything behind it.
    Prompt,
    /// The radial menu, held open in play: its stick selects, its buttons page and cancel.
    Radial,
}

impl Game<'_> {
    /// The overlay that owns the pad now, if one does.
    pub(in crate::game) fn pad_capture(&self) -> Option<PadCapture> {
        if self.pad.menu.osk.is_some() {
            Some(PadCapture::Keyboard)
        } else if self.prompt_open() {
            Some(PadCapture::Prompt)
        } else if self.pad.radial.is_open() {
            Some(PadCapture::Radial)
        } else {
            None
        }
    }

    /// The on-screen keyboard owns the pad.
    pub(in crate::game) fn pad_keyboard_captures(&self) -> bool {
        self.pad_capture() == Some(PadCapture::Keyboard)
    }

    /// The radial menu owns the pad: its stick doesn't walk or turn the view, its buttons are its
    /// own, and a bumped mouse does nothing. What is drawn gives way only once it shows
    /// (`pad_radial_shown`): a quick tap draws nothing.
    pub(in crate::game) fn pad_radial_captures(&self) -> bool {
        self.pad_capture() == Some(PadCapture::Radial)
    }
}
