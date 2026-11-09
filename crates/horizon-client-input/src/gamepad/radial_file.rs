//! The radial menu's settings in the controller file (`horizon-controller.txt`): how its opener
//! holds it open, the stick that selects, which buttons the opener may be with that stick, and the
//! opener each stick had this session. `PadSettings::parse` and `serialize` read and write their
//! lines (`radialMode`, `radialStick`, `bind_pad.radial`).
use super::{Button, PadSettings, RADIAL};

/// The buttons under the right thumb, which steers the right stick.
const RIGHT_THUMB: [Button; 4] = [
    Button::South,
    Button::West,
    Button::North,
    Button::RightStick,
];
/// The buttons under the left thumb, which steers the left stick.
const LEFT_THUMB: [Button; 5] = [
    Button::DPadUp,
    Button::DPadDown,
    Button::DPadLeft,
    Button::DPadRight,
    Button::LeftStick,
];

/// Whether the radial menu's opener can't be `button` while `stick` selects. One rule: the opener
/// is never under a thumb that selects (holding it and steering are two jobs for one thumb), nor
/// B, which cancels the open menu. So with the right stick the face buttons and R3 go, with the
/// left the D-pad and L3, with either both; a bumper, a trigger and View are always free, and
/// with a bumper for the opener the other bumper turns the pages.
pub fn opener_refused(stick: RadialStick, button: Button) -> bool {
    button == Button::East
        || (stick != RadialStick::Left && RIGHT_THUMB.contains(&button))
        || (stick != RadialStick::Right && LEFT_THUMB.contains(&button))
}

/// How the radial menu's opener holds it open.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum RadialMode {
    /// Held: letting go runs what is selected.
    #[default]
    Hold,
    /// A press opens it; A or the opener again runs what is selected, B closes it.
    Toggle,
}
impl RadialMode {
    pub const ALL: [Self; 2] = [Self::Hold, Self::Toggle];
    pub fn name(self) -> &'static str {
        match self {
            Self::Hold => "hold",
            Self::Toggle => "toggle",
        }
    }
    pub fn parse(value: &str) -> Self {
        Self::ALL
            .into_iter()
            .find(|mode| mode.name() == value)
            .unwrap_or_default()
    }
}

/// The stick that selects in the radial menu.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum RadialStick {
    #[default]
    Right,
    Left,
    /// The last stick to lean past the select tilt.
    Either,
}
impl RadialStick {
    pub const ALL: [Self; 3] = [Self::Right, Self::Left, Self::Either];
    pub fn name(self) -> &'static str {
        match self {
            Self::Right => "right",
            Self::Left => "left",
            Self::Either => "either",
        }
    }
    pub fn parse(value: &str) -> Self {
        Self::ALL
            .into_iter()
            .find(|stick| stick.name() == value)
            .unwrap_or_default()
    }
}

/// The opener each stick had, for this session: what the player stepped through, not a setting,
/// so it is neither saved nor compared.
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct StickOpeners([Option<Button>; 3]);
impl PartialEq for StickOpeners {
    fn eq(&self, _: &Self) -> bool {
        true
    }
}
impl StickOpeners {
    fn slot(&mut self, stick: RadialStick) -> &mut Option<Button> {
        let at = RadialStick::ALL
            .iter()
            .position(|s| *s == stick)
            .unwrap_or(0);
        &mut self.0[at]
    }
}

impl PadSettings {
    /// Whether `button` can be bound to `action`: anything but the radial menu's opener under
    /// a thumb that selects, or on B (`opener_refused`).
    pub fn can_bind(&self, action: &str, button: Button) -> bool {
        action != RADIAL || !opener_refused(self.radial_stick, button)
    }
    /// The stick the radial menu selects with. An opener under the thumb that now selects is
    /// unbound, and its row in the radial page says so. An unbound opener then takes the one this
    /// stick had before in this session, or else its default where nothing else has it (D-pad
    /// Right, back on the right stick).
    pub fn set_radial_stick(&mut self, stick: RadialStick) {
        let was = self.radial_stick;
        *self.openers.slot(was) = self.binding(RADIAL);
        self.radial_stick = stick;
        if self
            .binding(RADIAL)
            .is_some_and(|b| opener_refused(stick, b))
        {
            self.bindings.insert(RADIAL, None);
        }
        if self.binding(RADIAL).is_some() {
            return;
        }
        // The player's own, shared or not: they put it there.
        if let Some(own) = self
            .openers
            .slot(stick)
            .filter(|b| !opener_refused(stick, *b))
        {
            self.bind(RADIAL, Some(own));
            return;
        }
        let default = Self::default_of(RADIAL);
        let free =
            default.is_some_and(|d| !opener_refused(stick, d) && self.actions_of(d).is_empty());
        if free {
            self.bindings.remove(RADIAL);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::Rumble;
    use super::*;

    #[test]
    fn a_new_player_holds_d_pad_right_for_the_radial_menu() {
        for s in [PadSettings::default(), PadSettings::parse("")] {
            assert_eq!(s.binding(RADIAL), Some(Button::DPadRight));
            assert!(!s.conflicts(RADIAL));
        }
        let s = PadSettings::parse(&PadSettings::default().serialize());
        assert_eq!(s, PadSettings::default());
    }

    #[test]
    fn a_button_bound_by_hand_stays_with_it_over_the_openers_default() {
        let s = PadSettings::parse("bindLayout:1\nbind_key.playerlist:right\n");
        assert_eq!(s.binding("key.playerlist"), Some(Button::DPadRight));
        assert_eq!(s.binding(RADIAL), None);
        assert!(!s.conflicts("key.playerlist"));
        assert_eq!(PadSettings::parse(&s.serialize()), s);
    }

    #[test]
    fn a_saved_opener_stays_where_it_was_saved() {
        let s =
            PadSettings::parse("bindLayout:1\nbind_pad.radial:right\nbind_key.playerlist:right\n");
        assert_eq!(s.binding(RADIAL), Some(Button::DPadRight));
    }

    #[test]
    fn a_conflict_made_since_the_migration_stays_after_a_restart() {
        // Bound in the binds list onto the opener's button: both stay, and the list marks it.
        let mut s = PadSettings::default();
        s.bind("key.playerlist", Some(Button::DPadRight));
        let text = s.serialize();
        assert!(text.contains("bindLayout:2"));
        let read = PadSettings::parse(&text);
        assert_eq!(read.binding(RADIAL), Some(Button::DPadRight));
        assert!(read.conflicts(RADIAL));
        assert_eq!(read, s);
        // The same, hand-written with spaces around the lines.
        let read = PadSettings::parse("  bindLayout:2\n  bind_key.playerlist:right\n");
        assert_eq!(read.binding(RADIAL), Some(Button::DPadRight));
    }

    #[test]
    fn a_layout_one_file_resolves_the_opener_once() {
        let s = PadSettings::parse("bind_key.chat:up\n bindLayout:1\nbind_key.playerlist:right\n");
        assert_eq!(s.binding(RADIAL), None, "the player's own button wins");
        assert_eq!(s.binding("key.playerlist"), Some(Button::DPadRight));
        assert_eq!(s.binding("key.chat"), Some(Button::DPadUp));
        let text = s.serialize();
        assert!(text.contains("bindLayout:2\n") && text.contains("bind_pad.radial:none\n"));
        assert_eq!(PadSettings::parse(&text), s);
        // Without a conflict the opener keeps its default.
        let s = PadSettings::parse("bindLayout:1\nvibration:false\n");
        assert_eq!(s.binding(RADIAL), Some(Button::DPadRight));
        // A line with leading spaces is still a saved opener.
        let s =
            PadSettings::parse("bindLayout:1\n bind_pad.radial:right\nbind_key.playerlist:right\n");
        assert_eq!(s.binding(RADIAL), Some(Button::DPadRight));
    }

    #[test]
    fn the_opener_never_goes_under_the_selecting_thumb_nor_on_b() {
        use Button as B;
        let face = [B::South, B::West, B::North, B::RightStick];
        let dpad = [
            B::DPadUp,
            B::DPadDown,
            B::DPadLeft,
            B::DPadRight,
            B::LeftStick,
        ];
        for stick in RadialStick::ALL {
            let mut s = PadSettings::default();
            s.set_radial_stick(stick);
            for button in Button::ALL {
                let refused = button == B::East
                    || (stick != RadialStick::Left && face.contains(&button))
                    || (stick != RadialStick::Right && dpad.contains(&button));
                assert_eq!(!s.can_bind(RADIAL, button), refused, "{stick:?} {button:?}");
                // Other actions still take any of them.
                assert!(s.can_bind("key.chat", button));
            }
            // A bumper is always free, and stays one through the file.
            s.bind(RADIAL, Some(B::LeftBumper));
            assert_eq!(s.binding(RADIAL), Some(B::LeftBumper), "{stick:?}");
            assert_eq!(PadSettings::parse(&s.serialize()), s, "{stick:?}");
        }
        // A file with a refused opener reads as never saved: the default, unless taken.
        for button in [B::South, B::East, B::West, B::North, B::RightStick] {
            let text = format!("bindLayout:2\nbind_pad.radial:{}\n", button.name());
            let read = PadSettings::parse(&text);
            assert_eq!(read.binding(RADIAL), Some(B::DPadRight), "{button:?}");
            let taken = format!("{text}bind_key.playerlist:right\n");
            assert_eq!(
                PadSettings::parse(&taken).binding(RADIAL),
                None,
                "{button:?}"
            );
        }
        // Read with the stick it was saved for, wherever the stick's line is.
        let left = PadSettings::parse("bind_pad.radial:north\nbindLayout:2\nradialStick:left\n");
        assert_eq!(left.binding(RADIAL), Some(B::North));
    }

    #[test]
    fn a_stick_that_takes_the_openers_thumb_unbinds_it_and_the_right_gives_it_back() {
        let mut s = PadSettings::default();
        s.set_radial_stick(RadialStick::Left);
        assert_eq!(
            s.binding(RADIAL),
            None,
            "D-pad Right is under the left thumb"
        );
        assert_eq!(PadSettings::parse(&s.serialize()).binding(RADIAL), None);
        s.set_radial_stick(RadialStick::Right);
        assert_eq!(s.binding(RADIAL), Some(Button::DPadRight));
        // An opener the player picked that both sticks allow stays.
        s.bind(RADIAL, Some(Button::RightBumper));
        s.set_radial_stick(RadialStick::Either);
        assert_eq!(s.binding(RADIAL), Some(Button::RightBumper));
    }

    #[test]
    fn stepping_the_stick_and_back_gives_each_stick_its_own_opener() {
        // Left with Y, back to the right stick: Y is under the right thumb, D-pad Right opens.
        let mut s = PadSettings::default();
        s.set_radial_stick(RadialStick::Left);
        s.bind(RADIAL, Some(Button::North));
        s.set_radial_stick(RadialStick::Right);
        assert_eq!(s.binding(RADIAL), Some(Button::DPadRight));
        // And Left again has its Y back, shared with Inventory as the player left it.
        s.set_radial_stick(RadialStick::Left);
        assert_eq!(s.binding(RADIAL), Some(Button::North));
        // Right with D-pad Left: the left stick refuses it, and the right gets it back.
        let mut s = PadSettings::default();
        s.bind(RADIAL, Some(Button::DPadLeft));
        s.set_radial_stick(RadialStick::Left);
        assert_eq!(s.binding(RADIAL), None);
        s.set_radial_stick(RadialStick::Either);
        assert_eq!(s.binding(RADIAL), None);
        s.set_radial_stick(RadialStick::Right);
        assert_eq!(s.binding(RADIAL), Some(Button::DPadLeft));
    }

    #[test]
    fn a_file_from_before_the_radial_menu_binds_no_opener() {
        let s = PadSettings::parse("vibration:false\nbind_key.chat:up\n");
        assert_eq!(s.binding(RADIAL), None);
        assert_eq!(PadSettings::parse(&s.serialize()), s);
    }

    #[test]
    fn the_radial_menus_ticks_are_felt_and_tell_apart() {
        let tick = Rumble::RadialSlot.pulse().unwrap();
        assert!((40..=50).contains(&tick.millis), "{tick:?}");
        assert!((0.25..=0.35).contains(&tick.weak), "{tick:?}");
        let page = Rumble::RadialPage.pulse().unwrap();
        assert!(page.millis < tick.millis, "a page turn is the shorter tick");
        assert_eq!(Rumble::RadialWrap.pulse(), Some(page));
        let nothing = Rumble::RadialNothing.pulse().unwrap();
        assert!(
            nothing.strong > 0.0 && nothing.millis > tick.millis,
            "{nothing:?}"
        );
        assert!(
            nothing.strong.max(nothing.weak) < 0.3,
            "a soft buzz: {nothing:?}"
        );
    }

    #[test]
    fn the_radial_menus_mode_and_stick_survive_the_settings_file() {
        let s = PadSettings::default();
        assert_eq!(
            (s.radial_mode, s.radial_stick),
            (RadialMode::Hold, RadialStick::Right)
        );
        for mode in RadialMode::ALL {
            for stick in RadialStick::ALL {
                let mut s = PadSettings {
                    radial_mode: mode,
                    ..PadSettings::default()
                };
                s.set_radial_stick(stick);
                assert_eq!(PadSettings::parse(&s.serialize()), s);
            }
        }
        let read = PadSettings::parse("radialMode:sideways\nradialStick:up\n");
        assert_eq!(
            (read.radial_mode, read.radial_stick),
            (RadialMode::Hold, RadialStick::Right)
        );
    }

    #[test]
    fn radial_pages_survive_the_settings_file() {
        let mut s = PadSettings::default();
        let pages = "radial_0_label:Mine\nradial_0_slots:key.chat,,,,,,,\n";
        s.radial =
            crate::radial::Pages::parse(pages.lines().filter_map(|l| l.split_once(':'))).unwrap();
        let read = PadSettings::parse(&s.serialize());
        assert_eq!(read.radial, s.radial);
        assert_ne!(read.radial, crate::radial::Pages::default());
    }
}
