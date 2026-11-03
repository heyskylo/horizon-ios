//! Read triggers from mapped buttons, or the conventional unmapped Linux axes.
use gilrs::{Axis, Button, Gamepad, MappingSource};

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
            (pad.vendor_id(), pad.product_id()),
        );
    [
        (Button::LeftTrigger2, Axis::LeftZ),
        (Button::RightTrigger2, Axis::RightZ),
    ]
    .map(|(button, axis)| {
        trigger_value(
            pad.button_code(button).is_some(),
            pad.button_data(button).map(|data| data.value()),
            pad.is_pressed(button),
            axes.then(|| pad.axis_data(axis).map(|data| data.value()))
                .flatten(),
        )
    })
}

fn axis_triggers(
    source: MappingSource,
    six_axes: bool,
    device: (Option<u16>, Option<u16>),
) -> bool {
    source == MappingSource::Driver
        && six_axes
        // This model's default LeftZ is the right stick, not a trigger.
        && device != (Some(0x2dc8), Some(0x6012))
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
    use super::{axis_triggers, trigger_value};
    use gilrs::MappingSource;

    #[test]
    fn only_unmapped_six_axis_layouts_use_axis_triggers() {
        assert!(axis_triggers(MappingSource::Driver, true, (None, None)));
        assert!(!axis_triggers(
            MappingSource::SdlMappings,
            true,
            (None, None)
        ));
        assert!(!axis_triggers(MappingSource::Driver, false, (None, None)));
        assert!(!axis_triggers(
            MappingSource::Driver,
            true,
            (Some(0x2dc8), Some(0x6012))
        ));
    }

    #[test]
    fn unmapped_trigger_axis_pulls_and_releases() {
        assert_eq!(trigger_value(false, None, false, Some(-1.0)), 0.0);
        assert_eq!(trigger_value(false, None, false, Some(0.0)), 0.5);
        assert_eq!(trigger_value(false, None, false, Some(1.0)), 1.0);
        assert_eq!(trigger_value(false, None, false, Some(-1.0)), 0.0);
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
