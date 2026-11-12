//! `Options.mouseWheelSensitivity`'s logarithmic slider (`logMouse`/`unlogMouse`)
//! and the delayed-apply test the slider buttons share.
use super::*;
use std::time::Instant;

/// `SliderableValueSet.applyValueImmediately`: these apply 600 ms after
/// the last change (`OptionInstanceSliderButton.delayedApplyAt`).
fn delays_apply(opt: Opt) -> bool {
    matches!(
        opt,
        Opt::RenderDistance | Opt::SimulationDistance | Opt::BiomeBlend | Opt::WheelSensitivity
    )
}
/// Whether a slider still shows its own position: while held, or for a
/// delayed one until its value applies (both then `resetValue`).
pub(crate) fn slider_unsnapped(opt: Opt, held: bool, moved: Instant) -> bool {
    if delays_apply(opt) {
        moved.elapsed().as_millis() < 600
    } else {
        held
    }
}
/// `Options.unlogMouse`: the stored sensitivity as its hundredth-log step,
/// -200..=100, which the `IntRange(-200, 100)` slider runs over (0.01 to 10.0).
pub(crate) fn step(value: f32) -> f32 {
    (value.max(1.0e-6).log10() * 100.0)
        .floor()
        .clamp(-200.0, 100.0)
}
/// `IntRange(-200, 100).toSliderValue`: the step over the slider's span.
pub(crate) fn kind(value: f32) -> Kind {
    Kind::Slider(((step(value) + 200.0) / 300.0).clamp(0.0, 1.0))
}
/// `IntRange(-200, 100).fromSliderValue` then `Options.logMouse`:
/// floor(map(v, 0, 1, -200, 101)), the last step clamped, as 10^(i/100).
pub(crate) fn from_slider(v: f32) -> f32 {
    let i = (v.clamp(0.0, 1.0) * 301.0).floor() - 200.0;
    10.0_f32.powf(i.clamp(-200.0, 100.0) / 100.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The slider position `Opt::WheelSensitivity`'s kind() gives a value.
    fn wheel_pos(value: f32) -> f32 {
        match kind(value) {
            Kind::Slider(p) => p,
            _ => panic!("the wheel sensitivity is a slider"),
        }
    }
    /// `Options.mouseWheelSensitivity` runs over `IntRange(-200, 100)` through
    /// `logMouse`/`unlogMouse`: 10^(i/100), 0.01 to 10.0, not a straight line.
    #[test]
    fn the_mouse_wheel_sensitivity_slider_is_logarithmic() {
        // 1.0 is step 0: 200 of the 300 slider steps in, not at 0.099.
        assert!((wheel_pos(1.0) - 2.0 / 3.0).abs() < 1e-6);
        assert!((wheel_pos(0.01) - 0.0).abs() < 1e-6);
        assert!((wheel_pos(10.0) - 1.0).abs() < 1e-6);
        assert!((from_slider(2.0 / 3.0) - 1.0).abs() < 1e-5);
        assert!((from_slider(0.0) - 0.01).abs() < 1e-6);
        assert!((from_slider(1.0) - 10.0).abs() < 1e-4);
        assert!((from_slider(1.0 / 3.0) - 0.1).abs() < 1e-5);
        // The value setter goes through the same mapping.
        let mut s = Settings::default();
        set_slider(Opt::WheelSensitivity, &mut s, 2.0 / 3.0);
        assert!((s.mouse_wheel_sensitivity - 1.0).abs() < 1e-5);
        set_slider(Opt::WheelSensitivity, &mut s, 0.0);
        assert!((s.mouse_wheel_sensitivity - 0.01).abs() < 1e-6);
    }
}
