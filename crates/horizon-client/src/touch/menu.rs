//! Menu pointer gestures. Buttons commit on a tap; direct manipulation keeps
//! the mouse button held for inventory slots, text selection and sliders.

const SCROLL_LINE: f32 = 12.0;
const DRAG_SLOP: f32 = 4.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MenuMode {
    Tap,
    Drag,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct MenuAction {
    pub button: Option<bool>,
    pub scroll: f32,
    pub finished: bool,
}

#[derive(Debug)]
pub struct MenuGesture {
    mode: MenuMode,
    start: (f32, f32),
    scroll_y: f32,
    frames: u8,
    pressed: bool,
    lifted: bool,
    dragged: bool,
    cancelled: bool,
}

impl MenuGesture {
    pub fn new(at: (f32, f32), mode: MenuMode) -> Self {
        Self {
            mode,
            start: at,
            scroll_y: at.1,
            frames: 0,
            pressed: false,
            lifted: false,
            dragged: false,
            cancelled: false,
        }
    }

    pub fn moved(&mut self, at: (f32, f32)) -> MenuAction {
        if self.cancelled || self.lifted {
            return MenuAction::default();
        }
        if !at.0.is_finite() || !at.1.is_finite() {
            return self.cancel();
        }
        if self.mode == MenuMode::Drag {
            return MenuAction::default();
        }
        let dx = at.0 - self.start.0;
        let dy = at.1 - self.start.1;
        self.dragged |= dx * dx + dy * dy > DRAG_SLOP * DRAG_SLOP;
        let scroll = ((at.1 - self.scroll_y) / SCROLL_LINE).trunc();
        self.scroll_y += scroll * SCROLL_LINE;
        MenuAction {
            scroll,
            ..MenuAction::default()
        }
    }

    pub fn end(&mut self, at: (f32, f32)) -> MenuAction {
        let action = self.moved(at);
        if self.cancelled {
            return action;
        }
        self.lifted = true;
        if self.mode == MenuMode::Tap {
            // Draw the final pointer location before dispatching a click.
            self.frames = 0;
        }
        action
    }

    pub fn cancel(&mut self) -> MenuAction {
        self.cancelled = true;
        MenuAction {
            button: std::mem::take(&mut self.pressed).then_some(false),
            finished: true,
            ..MenuAction::default()
        }
    }

    pub fn frame(&mut self) -> MenuAction {
        if self.cancelled || self.lifted && self.dragged {
            return MenuAction {
                finished: true,
                ..MenuAction::default()
            };
        }
        self.frames = self.frames.saturating_add(1);
        if !self.pressed {
            if self.frames >= 2 && (self.mode == MenuMode::Drag || self.lifted) {
                self.pressed = true;
                return MenuAction {
                    button: Some(true),
                    ..MenuAction::default()
                };
            }
        } else if self.lifted {
            self.pressed = false;
            self.cancelled = true;
            return MenuAction {
                button: Some(false),
                finished: true,
                ..MenuAction::default()
            };
        }
        MenuAction::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tap_waits_for_lift_and_final_pointer_then_releases_once() {
        let mut gesture = MenuGesture::new((20.0, 20.0), MenuMode::Tap);
        for _ in 0..10 {
            assert_eq!(gesture.frame(), MenuAction::default());
        }
        gesture.end((21.0, 21.0));
        assert_eq!(gesture.frame(), MenuAction::default());
        assert_eq!(gesture.frame().button, Some(true));
        let release = gesture.frame();
        assert_eq!(release.button, Some(false));
        assert!(release.finished);
        assert_eq!(gesture.frame().button, None);
    }

    #[test]
    fn scroll_never_generates_a_button_even_after_a_long_hold() {
        let mut gesture = MenuGesture::new((20.0, 20.0), MenuMode::Tap);
        for _ in 0..10 {
            assert_eq!(gesture.frame().button, None);
        }
        assert_eq!(gesture.moved((20.0, 26.0)).scroll, 0.0);
        assert_eq!(gesture.moved((20.0, 44.0)).scroll, 2.0);
        assert_eq!(gesture.moved((20.0, 32.0)).scroll, -1.0);
        gesture.end((20.0, 32.0));
        assert!(gesture.frame().finished);
        assert_eq!(gesture.frame().button, None);
    }

    #[test]
    fn cancellation_never_creates_a_pending_tap() {
        for frames in [0, 1, 2, 20] {
            let mut gesture = MenuGesture::new((20.0, 20.0), MenuMode::Tap);
            for _ in 0..frames {
                gesture.frame();
            }
            assert_eq!(gesture.cancel().button, None);
            for _ in 0..3 {
                assert_eq!(gesture.frame().button, None);
            }
        }
    }

    #[test]
    fn direct_manipulation_holds_and_never_scrolls_then_cancels_once() {
        let mut gesture = MenuGesture::new((20.0, 20.0), MenuMode::Drag);
        assert_eq!(gesture.frame().button, None);
        assert_eq!(gesture.frame().button, Some(true));
        assert_eq!(gesture.moved((70.0, 100.0)).scroll, 0.0);
        assert_eq!(gesture.frame().button, None);
        assert_eq!(gesture.cancel().button, Some(false));
        assert_eq!(gesture.cancel().button, None);
        assert!(gesture.frame().finished);
    }

    #[test]
    fn fast_inventory_tap_has_a_frame_between_press_and_release() {
        let mut gesture = MenuGesture::new((20.0, 20.0), MenuMode::Drag);
        gesture.end((20.0, 20.0));
        assert_eq!(gesture.frame().button, None);
        assert_eq!(gesture.frame().button, Some(true));
        assert_eq!(gesture.frame().button, Some(false));
        assert_eq!(gesture.frame().button, None);
    }

    #[test]
    fn moving_away_and_returning_does_not_turn_a_drag_into_a_tap() {
        let mut gesture = MenuGesture::new((20.0, 20.0), MenuMode::Tap);
        gesture.moved((60.0, 20.0));
        gesture.end((20.0, 20.0));
        for _ in 0..3 {
            let action = gesture.frame();
            assert!(action.finished);
            assert_eq!(action.button, None);
        }
    }
}
