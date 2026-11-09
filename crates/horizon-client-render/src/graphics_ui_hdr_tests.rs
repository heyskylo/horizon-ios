//! The graphics screen's HDR rows against what the renderer's output offers and its display
//! reports.
use super::tests::{CHANGED, CLICK, DRAGGED, FB, NONE, SIZE, Ui, offering, press, row, ui};
use super::*;
use crate::settings_shell::{Columns, Hosted};
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
fn hdr_lighting_is_greyed_only_where_the_running_pack_lights_the_world() {
    use horizon_shader_api::Api;
    let mut s = Settings::default();
    s.horizon.hdr = HdrMode::On;
    let item = Item::Horizon(H::HdrLighting);
    let lights = Some("The shader pack lights the world itself.");
    let out = offering(true, None);
    // No pack running, as with one picked but refused, missing or its mod off: live.
    assert_eq!(item.blocked(&s, &out.with_pack(false, None)), None);
    // A 0.1 pack on the 16-bit frame: the light sources and the sun still shine.
    assert_eq!(
        item.blocked(&s, &out.with_pack(false, Some(Api::V0_1))),
        None
    );
    assert_eq!(
        item.blocked(&s, &out.with_pack(false, Some(Api::V0_2))),
        lights
    );
    assert_eq!(item.blocked(&s, &out.with_pack(true, None)), lights);
    let float = EffectiveOutput {
        frame: crate::gfx::FrameConfiguration {
            format: wgpu::TextureFormat::Rgba16Float,
            ..out.frame
        },
        ..out
    };
    assert_eq!(
        item.blocked(&s, &float.with_pack(false, Some(Api::V0_1))),
        lights
    );
    // HDR's own reasons come first.
    assert_eq!(
        item.blocked(&s, &NONE.with_pack(true, None)),
        Some("Your display or system doesn't offer HDR output.")
    );
    s.horizon.hdr = HdrMode::Off;
    assert_eq!(
        item.blocked(&s, &out.with_pack(true, None)),
        Some("Turn HDR on to use it.")
    );
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
    // Calibrate starts the calibration.
    let calibrate = row(&mut ui, &s, Item::Horizon(H::HdrCalibrate));
    assert_eq!(
        press(&mut ui, &mut s, at(calibrate)),
        [CLICK, Output::Push(OptionsPage::HdrCalibration)]
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

/// The setup rows' label and description for each kind of report: none, a compositor's
/// (Wayland), and Windows', whose peak is the display's own guess.
#[test]
fn the_setup_rows_ask_to_calibrate_unless_the_system_set_the_display_up() {
    let trusted = offering(true, Some(true));
    let mut guessed = trusted;
    guessed.reported = guessed.reported.map(|r| crate::graphics::Reported {
        peak_guessed: true,
        ..r
    });
    let rows = |s: &Settings, out: &EffectiveOutput| {
        [H::HdrCalibrate, H::HdrPeak, H::HdrPaperWhite].map(|h| {
            let item = Item::Horizon(h);
            (item.label(s, out).plain(), description(item, s, out, "-"))
        })
    };
    let calibrate = "Calibrate...";
    let set_up = "Set up for your display. Leave on Auto unless it looks wrong.";
    let mut s = Settings::default();
    s.horizon.hdr = HdrMode::On;
    let none = (
        calibrate.into(),
        "Your display didn't say how bright it gets: calibrate to set it up.",
    );
    assert_eq!(
        rows(&s, &NONE),
        [
            none.clone(),
            ("Peak Brightness: Auto (1000)".into(), "-"),
            ("Paper White: Auto (203)".into(), "-"),
        ]
    );
    assert_eq!(
        rows(&s, &guessed),
        [
            (
                calibrate.into(),
                "Your display's reported peak is often too high: calibrate to set it up."
            ),
            ("Peak Brightness: Auto (1000)".into(), "-"),
            ("Paper White: Auto (400)".into(), "-"),
        ]
    );
    assert_eq!(
        rows(&s, &trusted),
        [
            (
                "Calibrate Anyway...".into(),
                "Already set up for your display. Only calibrate if HDR looks wrong."
            ),
            ("Peak Brightness: Auto (1000)".into(), set_up),
            ("Paper White: Auto (400)".into(), set_up),
        ]
    );
    // Calibrated or set by hand, either report offers its automatic setup back.
    s.horizon.hdr_peak = 800;
    // Peak's Auto stays a guess on Windows: only Paper White is said to be set up there.
    let changed = |peak| {
        [
            (
                "Use Automatic Setup".into(),
                "Puts Peak Brightness and Paper White back to your display's setup.",
            ),
            ("Peak Brightness: 800".into(), peak),
            ("Paper White: Auto (400)".into(), set_up),
        ]
    };
    assert_eq!(rows(&s, &guessed), changed("-"));
    assert_eq!(rows(&s, &trusted), changed(set_up));
    assert_eq!(rows(&s, &NONE)[0], none);
    let mut ui = ui();
    ui.screen.output = guessed;
    let at = |[x, y, ..]: [f32; 4]| (x + 8.0, y + 4.0);
    let calibrate = row(&mut ui, &s, Item::Horizon(H::HdrCalibrate));
    assert_eq!(press(&mut ui, &mut s, at(calibrate)), [CLICK, CHANGED]);
    assert_eq!((s.horizon.hdr_peak, s.horizon.hdr_paper_white), (0, 0));
    assert_eq!(
        press(&mut ui, &mut s, at(calibrate)),
        [CLICK, Output::Push(OptionsPage::HdrCalibration)]
    );
}

/// UI Brightness reads as Paper White does, in nits apart from it: Auto shows the paper white
/// in use, and a value set by hand its own nits, whatever Paper White is. Its slider has Paper
/// White's steps: Auto, then 10 to 480 nits by 10.
#[test]
fn ui_brightness_reads_in_nits_on_paper_whites_steps() {
    let mut s = Settings::default();
    s.horizon.hdr = HdrMode::On;
    let reporting = offering(true, Some(true));
    let label = |s: &Settings, out| Item::Horizon(H::HdrUiBrightness).label(s, out).plain();
    assert_eq!(label(&s, &reporting), "UI Brightness: Auto (400)");
    assert_eq!(label(&s, &NONE), "UI Brightness: Auto (203)");
    s.horizon.hdr_paper_white = 250;
    assert_eq!(label(&s, &reporting), "UI Brightness: Auto (250)");
    s.horizon.hdr_ui_nits = 160;
    for paper_white in [0, 80, 250] {
        s.horizon.hdr_paper_white = paper_white;
        assert_eq!(label(&s, &reporting), "UI Brightness: 160");
    }
    let item = Item::Horizon(H::HdrUiBrightness);
    assert_eq!(
        H::HdrUiBrightness.slider_steps(),
        H::HdrPaperWhite.slider_steps()
    );
    H::HdrUiBrightness.set_slider(&mut s, 0.0);
    let mut reached = vec![s.horizon.hdr_ui_nits];
    for _ in 0..H::HdrUiBrightness.slider_steps().unwrap() {
        item.step_slider(&mut s, true, 200.0, &reporting);
        reached.push(s.horizon.hdr_ui_nits);
    }
    let paper: Vec<u32> = std::iter::once(0).chain((10..=480).step_by(10)).collect();
    assert_eq!(reached, paper);
    // Each value sits where Paper White's same value does.
    for nits in paper {
        s.horizon.hdr_ui_nits = nits;
        s.horizon.hdr_paper_white = nits;
        assert_eq!(item.slider(&s), Item::Horizon(H::HdrPaperWhite).slider(&s));
    }
}

/// While HDR output is held back on this window's driver, the HDR row says so and keeps
/// the mode; any change of the mode drops the hold, and HDR is asked for again.
#[test]
fn a_held_hdr_start_is_said_on_the_page_and_a_mode_change_tries_again() {
    use crate::gfx::{HdrOutput, output::tests};
    let mut s = Settings::default();
    s.horizon.hdr = HdrMode::Auto;
    s.horizon.hdr_failed_start = Some("Vulkan 10de:2684 GPU; driver; 1.0".into());
    let (mut caps, reading) = tests::window(Some(HdrOutput::Pq), Some(true));
    caps.held = true;
    let out = tests::snapshot(&s.horizon, Default::default(), caps, reading);
    let hdr = Item::Horizon(H::Hdr);
    let setting = h(
        H::Hdr,
        Cost::None,
        "Lava, fire and sunlight shine brighter than white.",
    );
    let held = super::row(setting, &s, &out);
    assert_eq!(held.chosen.as_deref(), Some(HDR_HELD));
    assert_eq!(held.why, None);
    assert_eq!(hdr.label(&s, &out).plain(), "HDR: Auto");
    assert_eq!(Item::Horizon(H::HdrPeak).blocked(&s, &out), Some(HDR_HELD));
    // Not held, the row says what HDR does.
    caps.held = false;
    let free = tests::snapshot(&s.horizon, Default::default(), caps, reading);
    assert_eq!(super::row(setting, &s, &free).chosen, None);
    // Any step of the mode, either way, tries again.
    for back in [false, true] {
        let mut s = s.clone();
        hdr.cycle(&mut s, back, FB, &out);
        assert_ne!(s.horizon.hdr, HdrMode::Auto);
        assert_eq!(s.horizon.hdr_failed_start, None);
    }
}

/// Undo never brings the hold back: held Auto, changed to On (HDR tried again), then
/// undone, is Auto without the hold. With the mode left alone, it keeps the hold as it is.
#[test]
fn undo_never_restores_a_held_hdr_start() {
    let mut screen = GraphicsScreen {
        framebuffer: FB,
        output: NONE,
    };
    let mut opened = Settings::default();
    opened.horizon.hdr = HdrMode::Auto;
    opened.horizon.hdr_failed_start = Some("Vulkan 10de:2684 GPU; driver; 1.0; os".into());
    let mut s = opened.clone();
    let out = offering(true, Some(true));
    Item::Horizon(H::Hdr).cycle(&mut s, true, FB, &out);
    assert_eq!(
        (s.horizon.hdr, &s.horizon.hdr_failed_start),
        (HdrMode::On, &None)
    );
    screen.undo(&mut s, &opened);
    assert_eq!(
        (s.horizon.hdr, &s.horizon.hdr_failed_start),
        (HdrMode::Auto, &None)
    );
    // Another row changed and undone: the hold stays.
    let mut s = opened.clone();
    s.horizon.fxaa = !s.horizon.fxaa;
    screen.undo(&mut s, &opened);
    assert_eq!(s.horizon, opened.horizon);
}
