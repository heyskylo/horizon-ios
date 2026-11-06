//! Read triggers from mapped buttons, or the conventional unmapped Linux axes.
use super::hid_layout;
use gilrs::{Axis, Button, Gamepad, MappingSource};
use horizon_client::gamepad::BITDO_ULTIMATE2_VID_PID;

/// Linux's event type for absolute axes, and its codes for an Xbox Bluetooth pad's triggers.
const EV_ABS: u16 = 0x03;
const ABS_GAS: u16 = 0x09;
const ABS_BRAKE: u16 = 0x0a;

pub(super) fn triggers(pad: &Gamepad) -> [f32; 2] {
    let axes = cfg!(target_os = "linux")
        && axis_triggers(
            pad.mapping_source(),
            [
                Axis::LeftStickX,
                Axis::LeftStickY,
                Axis::RightStickX,
                Axis::RightStickY,
                Axis::LeftZ,
                Axis::RightZ,
            ]
            .iter()
            .all(|&axis| pad.axis_code(axis).is_some()),
            [
                Button::South,
                Button::East,
                Button::West,
                Button::North,
                Button::C,
                Button::Z,
            ]
            .map(|button| pad.button_code(button).is_some()),
            (pad.vendor_id(), pad.product_id()),
        );
    let on_gas_and_brake = hid_layout::right_stick_on_z(pad);
    [
        (Button::LeftTrigger2, Axis::LeftZ, ABS_BRAKE),
        (Button::RightTrigger2, Axis::RightZ, ABS_GAS),
    ]
    .map(|(button, axis, absolute)| {
        if on_gas_and_brake && let Some(value) = unnamed_axis(pad, absolute) {
            return trigger_value(false, None, false, Some(value));
        }
        trigger_value(
            pad.button_code(button).is_some(),
            pad.button_data(button).map(|data| data.value()),
            pad.is_pressed(button),
            axes.then(|| pad.axis_data(axis).map(|data| data.value()))
                .flatten(),
        )
    })
}

/// gilrs' default mapping has no name for these, and those pads also list trigger buttons that
/// never fire, so an axis that has moved wins.
fn unnamed_axis(pad: &Gamepad, absolute: u16) -> Option<f32> {
    let code = (u32::from(EV_ABS) << 16) | u32::from(absolute);
    pad.state()
        .axes()
        .find(|(axis, _)| axis.into_u32() == code)
        .map(|(_, data)| data.value())
}

fn axis_triggers(
    source: MappingSource,
    six_axes: bool,
    face_buttons: [bool; 6],
    device: (Option<u16>, Option<u16>),
) -> bool {
    source == MappingSource::Driver
        && six_axes
        // The default Xbox-style button layout skips C and Z. A generic six-axis
        // pad can otherwise put its right stick on LeftZ/RightZ: its centred
        // value would be mistaken for a half-pulled trigger.
        && face_buttons == [true, true, true, true, false, false]
        // This model's default LeftZ is the right stick, not a trigger.
        && device != (Some(BITDO_ULTIMATE2_VID_PID.0), Some(BITDO_ULTIMATE2_VID_PID.1))
}

fn trigger_value(mapped: bool, value: Option<f32>, pressed: bool, axis: Option<f32>) -> f32 {
    if mapped {
        if pressed {
            value.unwrap_or(0.0).max(1.0)
        } else {
            value.unwrap_or(0.0)
        }
    } else {
        axis.map_or(0.0, |value| ((value + 1.0) * 0.5).clamp(0.0, 1.0))
    }
}

#[cfg(test)]
mod tests {
    use super::{BITDO_ULTIMATE2_VID_PID, axis_triggers, trigger_value};
    use gilrs::MappingSource;
    use horizon_client::gamepad::{Button, Edges, PadState};

    const XBOX_BUTTONS: [bool; 6] = [true, true, true, true, false, false];

    #[test]
    fn only_unmapped_xbox_style_six_axis_layouts_use_axis_triggers() {
        assert!(axis_triggers(
            MappingSource::Driver,
            true,
            XBOX_BUTTONS,
            (None, None)
        ));
        assert!(!axis_triggers(
            MappingSource::SdlMappings,
            true,
            XBOX_BUTTONS,
            (None, None)
        ));
        assert!(!axis_triggers(
            MappingSource::Driver,
            false,
            XBOX_BUTTONS,
            (None, None)
        ));
        assert!(!axis_triggers(
            MappingSource::Driver,
            true,
            XBOX_BUTTONS,
            (
                Some(BITDO_ULTIMATE2_VID_PID.0),
                Some(BITDO_ULTIMATE2_VID_PID.1)
            )
        ));
        for button in 0..6 {
            let mut different = XBOX_BUTTONS;
            different[button] = !different[button];
            assert!(!axis_triggers(
                MappingSource::Driver,
                true,
                different,
                (None, None)
            ));
        }
    }

    #[test]
    fn a_centred_stick_on_a_generic_layout_never_holds_attack_or_use() {
        let fallback = axis_triggers(MappingSource::Driver, true, [true; 6], (None, None));
        let mut edges = Edges::default();
        for axis in [None, Some(1.0), Some(0.0), Some(-1.0), Some(0.0)] {
            let value = trigger_value(false, None, false, axis.filter(|_| fallback));
            assert_eq!(value, 0.0);
            let state = PadState {
                triggers: [value; 2],
                ..PadState::default()
            };
            assert!(edges.update(&state, 0.4).is_empty());
            assert!(!edges.is_held(Button::LeftTrigger));
            assert!(!edges.is_held(Button::RightTrigger));
        }
    }

    #[test]
    fn eight_bit_trigger_rest_pull_and_release_generate_button_edges() {
        for (index, button) in [(0, Button::LeftTrigger), (1, Button::RightTrigger)] {
            let mut edges = Edges::default();
            let mut seen = Vec::new();
            for axis in [-0.9921875, 0.0, 1.0, -0.9921875] {
                let value = trigger_value(false, None, false, Some(axis));
                let mut state = PadState::default();
                state.triggers[index] = value;
                seen.extend(edges.update(&state, 0.4));
            }
            assert_eq!(seen, vec![(button, true), (button, false)]);
            assert!(!edges.is_held(button));
        }
    }

    #[test]
    fn absent_axis_is_released() {
        assert_eq!(trigger_value(false, None, false, None), 0.0);
    }

    #[test]
    fn mapped_buttons_take_precedence_even_before_first_event() {
        assert_eq!(trigger_value(true, None, false, Some(1.0)), 0.0);
        assert_eq!(trigger_value(true, Some(0.25), false, Some(1.0)), 0.25);
        assert_eq!(trigger_value(true, None, true, None), 1.0);
    }
}
