//! A pad the kernel's generic HID driver describes, which an SDL row would otherwise explain.
//! SDL rows match one firmware version exactly, so a new firmware of a known pad lands here too.
use gilrs::{Axis, Button, Gamepad, MappingSource};

/// Whether this pad has its right stick on `ABS_Z`/`ABS_RZ`, as an Xbox pad over Bluetooth does.
pub(super) fn right_stick_on_z(pad: &Gamepad) -> bool {
    cfg!(target_os = "linux")
        && stick_on_z(
            pad.mapping_source(),
            [
                Axis::RightStickX,
                Axis::RightStickY,
                Axis::LeftZ,
                Axis::RightZ,
            ]
            .map(|axis| pad.axis_code(axis).is_some()),
        )
}

/// Whether HID's numbered buttons reach us as A, B, C, X, Y, Z: this layout's X and Y are the
/// fourth and fifth buttons, and C and Z are the third and sixth, which nothing presses.
pub(super) fn numbered_buttons(pad: &Gamepad) -> bool {
    right_stick_on_z(pad)
        && [Button::C, Button::Z]
            .iter()
            .all(|&button| pad.button_code(button).is_some())
}

/// `[RightStickX, RightStickY, LeftZ, RightZ]` present. With `RightStickX` there, Z is a trigger
/// or belongs to a model `gamepad::stick_axes` knows.
fn stick_on_z(source: MappingSource, axes: [bool; 4]) -> bool {
    source == MappingSource::Driver && axes == [false, false, true, true]
}

/// The right stick from those axes. gilrs reverses Y only on the axes it calls Y, so RZ, which
/// grows downward like every Linux Y axis, is turned over here to match them.
pub(super) fn right_stick([z, rz]: [f32; 2]) -> [f32; 2] {
    [z, -rz]
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `045e:0b13` firmware 0x0520 over Bluetooth: X, Y, Z, RZ, GAS, BRAKE and hats.
    const XBOX_BLUETOOTH: [bool; 4] = [false, false, true, true];

    #[test]
    fn a_pad_without_rx_or_ry_has_its_right_stick_on_z() {
        assert!(stick_on_z(MappingSource::Driver, XBOX_BLUETOOTH));
    }

    #[test]
    fn an_sdl_row_already_says_where_the_stick_is() {
        assert!(!stick_on_z(MappingSource::SdlMappings, XBOX_BLUETOOTH));
    }

    #[test]
    fn a_pad_with_rx_and_ry_keeps_them() {
        // xpad: Z and RZ are its triggers.
        assert!(!stick_on_z(MappingSource::Driver, [true, true, true, true]));
        // The 8BitDo Ultimate 2 has RX too, and stick_axes reads it.
        assert!(!stick_on_z(
            MappingSource::Driver,
            [true, false, true, true]
        ));
    }

    #[test]
    fn a_pad_missing_either_axis_has_no_stick_there() {
        assert!(!stick_on_z(
            MappingSource::Driver,
            [false, false, true, false]
        ));
        assert!(!stick_on_z(
            MappingSource::Driver,
            [false, false, false, true]
        ));
    }

    #[test]
    fn pushing_the_stick_up_reads_up() {
        // Up is the low end of ABS_RZ, and up is positive on every other stick axis.
        assert_eq!(right_stick([0.0, -1.0]), [0.0, 1.0]);
        assert_eq!(right_stick([-0.5, 0.25]), [-0.5, -0.25]);
    }
}
