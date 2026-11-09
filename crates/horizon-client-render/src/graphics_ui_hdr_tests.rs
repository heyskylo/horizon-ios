//! The graphics screen's HDR rows against what the renderer's output offers and its display
//! reports.
use super::tests::{CHANGED, CLICK, DRAGGED, FB, NONE, SIZE, Ui, offering, press, row, ui};
use super::*;
use crate::settings_shell::{Capture, Columns, Hosted};
use winit::keyboard::KeyCode as K;

#[test]
fn smooth_gradients_wait_for_a_device_that_draws_the_16_bit_frame() {
    let mut s = Settings::default();
    s.horizon.hdr = HdrMode::On;
    let item = Item::Horizon(H::HdrSmoothGradients);
    // Where the device can't, the saved On reads Off.
    let out = EffectiveOutput {
        wide_frame_supported: false,
        ..offering(true, None)
    };
    assert_eq!(
        item.blocked(&s, &out),
        Some("Your GPU or graphics API doesn't support it.")
    );
    assert_eq!(item.label(&s, &out).plain(), "Smooth Gradients: Off");
    assert_eq!(item.switch(&s, &out), Some(false));
    assert!(s.horizon.hdr_smooth_gradients);
    // Where it can, it waits only for HDR, like the rest of the page.
    let out = offering(true, None);
    assert_eq!(item.blocked(&s, &out), None);
    assert_eq!(item.label(&s, &out).plain(), "Smooth Gradients: On");
    assert_eq!(item.switch(&s, &out), Some(true));
    s.horizon.hdr = HdrMode::Off;
    assert_eq!(item.blocked(&s, &out), Some("Turn HDR on to use it."));
}

#[test]
fn hdr_auto_can_be_selected_before_querying_and_brightness_waits_for_output() {
    let mut s = Settings::default();
    let mut ui = ui();
    let at = |[x, y, ..]: [f32; 4]| (x + 8.0, y + 4.0);
    // Off has not asked the system yet: Auto must still be reachable to start the watch.
    let short = (427.0, 240.0);
    let mut at_hdr = Ui::default();
    at_hdr.show(Page::Hdr);
    at_hdr.key(&mut s, K::ArrowDown, None, false, short);
    assert_eq!(at_hdr.focused_row(), Some(Item::Horizon(H::Hdr)));
    let hdr = row(&mut ui, &s, Item::Horizon(H::Hdr));
    assert_eq!(press(&mut ui, &mut s, at(hdr)), [CLICK, CHANGED]);
    assert_eq!(s.horizon.hdr, HdrMode::Auto);
    assert_eq!(press(&mut ui, &mut s, at(hdr)), [CLICK, CHANGED]);
    assert_eq!(s.horizon.hdr, HdrMode::Off);
    // Brightness still needs an output, even with On saved from another computer.
    s.horizon.hdr = HdrMode::On;
    for h in [H::HdrCalibrate, H::HdrPeak, H::HdrPaperWhite] {
        assert!(Item::Horizon(h).blocked(&s, &NONE).is_some(), "{h:?}");
    }
    s.horizon.hdr = HdrMode::Off;
    ui.screen.output = offering(true, None);
    let (tx, _) = Columns::of(SIZE.0, SIZE.1).track();
    let peak = row(&mut ui, &s, Item::Horizon(H::HdrPeak));
    assert!(press(&mut ui, &mut s, (tx + 2.0, peak[1] + 4.0)).is_empty());
    // The choice turns it on, and the sliders below it work.
    let hdr = row(&mut ui, &s, Item::Horizon(H::Hdr));
    assert_eq!(press(&mut ui, &mut s, at(hdr)), [CLICK, CHANGED]);
    assert!(ui.screen.output.requests_hdr(s.horizon.hdr));
    let peak = row(&mut ui, &s, Item::Horizon(H::HdrPeak));
    assert_eq!(press(&mut ui, &mut s, (tx + 2.0, peak[1] + 4.0)), [DRAGGED]);
    ui.release();
    // Calibrate starts the calibration; the shortcut row waits for a key.
    let calibrate = row(&mut ui, &s, Item::Horizon(H::HdrCalibrate));
    assert_eq!(
        press(&mut ui, &mut s, at(calibrate)),
        [CLICK, Output::Push(OptionsPage::HdrCalibration)]
    );
    let key = row(&mut ui, &s, Item::Horizon(H::HdrKey));
    assert_eq!(
        press(&mut ui, &mut s, at(key)),
        [CLICK, Output::Bind(Capture::Key(crate::graphics::HDR_KEY))]
    );
    assert!(ui.changed());
}

#[test]
fn hdr_auto_is_a_choice_on_a_reporting_sdr_display() {
    let out = offering(false, Some(false));
    let mut s = Settings::default();
    let item = Item::Horizon(H::Hdr);
    assert_eq!(item.switch(&s, &out), None);
    assert_eq!(item.blocked(&s, &out), None);
    item.cycle(&mut s, false, FB, &out);
    assert_eq!(s.horizon.hdr, HdrMode::Auto); // On requires an offered output.
    assert!(!out.requests_hdr(s.horizon.hdr));
    assert!(Item::Horizon(H::HdrPeak).blocked(&s, &out).is_some());
    item.cycle(&mut s, true, FB, &out);
    assert_eq!(s.horizon.hdr, HdrMode::Off);
    let out = offering(true, Some(false));
    for expected in [HdrMode::On, HdrMode::Auto, HdrMode::Off] {
        item.cycle(&mut s, false, FB, &out);
        assert_eq!(s.horizon.hdr, expected);
    }
    s.horizon.hdr = HdrMode::Auto;
    let out = offering(true, Some(true));
    assert!(out.requests_hdr(s.horizon.hdr));
    assert_eq!(Item::Horizon(H::HdrPeak).blocked(&s, &out), None);
}

/// The labels and the calibration row read the report of the renderer they are shown
/// for: two windows' screens, one whose display reports 400/1000 nits and one that
/// reports nothing, side by side.
#[test]
fn auto_levels_and_setup_read_their_own_renderers_report() {
    let mut s = Settings::default();
    s.horizon.hdr = HdrMode::Auto;
    let reporting = offering(true, Some(true));
    let silent = NONE;
    let label = |h, out| Item::Horizon(h).label(&s, out).plain();
    assert_eq!(
        label(H::HdrPaperWhite, &reporting),
        "Paper White: Auto (400)"
    );
    assert_eq!(label(H::HdrPaperWhite, &silent), "Paper White: Auto (203)");
    assert_eq!(
        label(H::HdrPeak, &reporting),
        "Peak Brightness: Auto (1000)"
    );
    assert_eq!(label(H::HdrCalibrate, &reporting), "Calibrate Anyway...");
    assert_eq!(label(H::HdrCalibrate, &silent), "Calibrate...");
}

#[test]
fn hdr_setup_follows_what_the_system_reported() {
    let mut g = crate::graphics::Graphics::default();
    assert_eq!(setup_of(&g, false), Setup::Unknown);
    assert_eq!(setup_of(&g, true), Setup::Automatic);
    g.hdr_paper_white = 80;
    assert_eq!(setup_of(&g, true), Setup::Changed);
    assert_eq!(setup_of(&g, false), Setup::Unknown);
}
