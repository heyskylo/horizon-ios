//! Linux's codes for X and Y don't say where those buttons sit.
use super::hid_layout;
use gilrs::{Button, Gamepad, MappingSource};
use horizon_client::gamepad::{BITDO_ULTIMATE2_VID_PID, GlyphStyle};

/// Whether this pad's X and Y reach us crossed. The kernel defines `BTN_X` as `BTN_NORTH` and
/// `BTN_Y` as `BTN_WEST`, so an Xbox-style pad's X, on the west, sends "north". gilrs' default
/// mapping takes the names at their word; an SDL row names `x` and `y` itself and is right.
pub(super) fn crossed(pad: &Gamepad) -> bool {
    cfg!(target_os = "linux")
        && crossed_layout(
            pad.mapping_source(),
            [
                Button::South,
                Button::East,
                Button::West,
                Button::North,
                Button::C,
                Button::Z,
            ]
            .map(|button| pad.button_code(button).is_some()),
            hid_layout::numbered_buttons(pad)
                // Its row has `x:b3`, BTN_NORTH, and it lists C and Z with its right stick on RX.
                || (pad.vendor_id(), pad.product_id())
                    == (Some(BITDO_ULTIMATE2_VID_PID.0), Some(BITDO_ULTIMATE2_VID_PID.1)),
            GlyphStyle::of_device(pad.vendor_id(), pad.product_id()),
        )
}

fn crossed_layout(
    source: MappingSource,
    face_buttons: [bool; 6],
    numbered_buttons: bool,
    style: GlyphStyle,
) -> bool {
    source == MappingSource::Driver
        // The Xbox-style layout has no C or Z, unless HID's numbered buttons name them unused.
        && (face_buttons == [true, true, true, true, false, false] || numbered_buttons)
        // These drivers send the same codes for what is really on the north and the west.
        && !matches!(style, GlyphStyle::PlayStation | GlyphStyle::Nintendo)
}

/// What gilrs calls the button we know as `button`, on a pad that is `crossed` or not.
pub(super) fn named(button: Button, crossed: bool) -> Button {
    match button {
        Button::West if crossed => Button::North,
        Button::North if crossed => Button::West,
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const XBOX_BUTTONS: [bool; 6] = [true, true, true, true, false, false];

    #[test]
    fn an_unmapped_xbox_style_pad_is_crossed() {
        // The GameSir Nova Lite 2 of #7323: `3537:1098`, no SDL row, X sends BTN_NORTH.
        let style = GlyphStyle::of_device(Some(0x3537), Some(0x1098));
        assert!(crossed_layout(
            MappingSource::Driver,
            XBOX_BUTTONS,
            false,
            style
        ));
    }

    #[test]
    fn an_sdl_row_already_names_x_and_y() {
        assert!(!crossed_layout(
            MappingSource::SdlMappings,
            XBOX_BUTTONS,
            false,
            GlyphStyle::Xbox
        ));
    }

    #[test]
    fn a_sony_or_nintendo_pad_means_what_its_codes_say() {
        for vendor in [0x054c, 0x057e] {
            let style = GlyphStyle::of_device(Some(vendor), None);
            assert!(!crossed_layout(
                MappingSource::Driver,
                XBOX_BUTTONS,
                false,
                style
            ));
        }
    }

    #[test]
    fn a_pad_with_c_or_z_is_not_xbox_style() {
        for different in [
            [true, true, true, true, true, false],
            [true, true, true, true, false, true],
        ] {
            assert!(!crossed_layout(
                MappingSource::Driver,
                different,
                false,
                GlyphStyle::Xbox
            ));
        }
    }

    #[test]
    fn an_xbox_pad_over_bluetooth_is_crossed_though_it_lists_c_and_z() {
        // `045e:0b13` on hid_microsoft reports BTN_SOUTH to BTN_THUMBR, with no SDL row for
        // firmware 0x0520: X sends BTN_NORTH.
        let style = GlyphStyle::of_device(Some(0x045e), Some(0x0b13));
        assert!(crossed_layout(
            MappingSource::Driver,
            [true; 6],
            true,
            style
        ));
    }

    #[test]
    fn numbered_buttons_on_a_sony_or_nintendo_pad_are_left_alone() {
        for vendor in [0x054c, 0x057e] {
            let style = GlyphStyle::of_device(Some(vendor), None);
            assert!(!crossed_layout(
                MappingSource::Driver,
                [true; 6],
                true,
                style
            ));
        }
    }

    #[test]
    fn six_real_buttons_are_not_crossed() {
        // A pad with C and Z and its stick elsewhere: they may be real buttons, so X is X.
        assert!(!crossed_layout(
            MappingSource::Driver,
            [true; 6],
            false,
            GlyphStyle::Xbox
        ));
    }

    #[test]
    fn only_x_and_y_change_places() {
        assert_eq!(named(Button::West, true), Button::North);
        assert_eq!(named(Button::North, true), Button::West);
        assert_eq!(named(Button::South, true), Button::South);
        assert_eq!(named(Button::East, true), Button::East);
        assert_eq!(named(Button::West, false), Button::West);
        assert_eq!(named(Button::North, false), Button::North);
    }
}
