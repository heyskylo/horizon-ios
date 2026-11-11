//! The graphics screen as the options host it: the output of the renderer they are drawn for,
//! and its pages.
use super::tests::atlas;
use super::*;
use crate::gfx::EffectiveOutput;

/// What a window offering HDR10 shows the options when its system reports `system` HDR, Auto
/// saved.
fn auto_window(system: Option<bool>) -> EffectiveOutput {
    use crate::gfx::output::tests::{snapshot, window};
    let (caps, reading) = window(Some(crate::gfx::HdrOutput::Pq), system);
    let auto = crate::graphics::Graphics {
        hdr: crate::graphics::HdrMode::Auto,
        ..Default::default()
    };
    snapshot(&auto, Default::default(), caps, reading)
}
/// Calibrate on the graphics screen's HDR page, clicked: whether calibration opened (its
/// row is blocked without an offer, or with Auto and no positive report).
fn calibrates(ui: &mut OptionsUi, s: &mut Settings, atlas: &HudAtlas) -> bool {
    use crate::graphics_ui::{H, Item};
    ui.graphics.show(crate::graphics_ui::Page::Hdr);
    let (x, y) = ui
        .graphics
        .point_at(s, ui.size, Item::Horizon(H::HdrCalibrate));
    ui.click(s, atlas, x, y, ui.size, ui.framebuffer, false, false);
    ui.page() == Page::HdrCalibration
}

/// Opening the graphics screen keeps the offer and report of the renderer the options are
/// drawn for: the screen opens afresh, the output doesn't go with it.
#[test]
fn opening_the_graphics_screen_keeps_the_offer_and_report() {
    let atlas = atlas();
    let mut s = Settings::default();
    s.horizon.hdr = crate::graphics::HdrMode::Auto;
    let mut ui = OptionsUi {
        size: (427.0, 240.0),
        ..Default::default()
    };
    let batch = || HudBatch::new([427, 240]);
    ui.draw(
        &s,
        auto_window(Some(true)),
        &mut batch(),
        &atlas,
        (-1.0, -1.0),
    );
    ui.show(Page::Graphics);
    assert!(calibrates(&mut ui, &mut s, &atlas));
    // And again, from another page and back.
    ui.back(&mut s);
    ui.back(&mut s);
    ui.show(Page::Graphics);
    assert!(calibrates(&mut ui, &mut s, &atlas));
}

/// A report that changes while the graphics screen is open (Auto's system HDR turned on)
/// shows on the next draw, with no setting applied.
#[test]
fn a_changed_report_shows_without_an_apply() {
    let atlas = atlas();
    let mut s = Settings::default();
    s.horizon.hdr = crate::graphics::HdrMode::Auto;
    let mut ui = OptionsUi {
        size: (427.0, 240.0),
        ..Default::default()
    };
    let batch = || HudBatch::new([427, 240]);
    ui.show(Page::Graphics);
    ui.draw(
        &s,
        auto_window(Some(false)),
        &mut batch(),
        &atlas,
        (-1.0, -1.0),
    );
    assert!(!calibrates(&mut ui, &mut s, &atlas));
    ui.draw(
        &s,
        auto_window(Some(true)),
        &mut batch(),
        &atlas,
        (-1.0, -1.0),
    );
    assert!(calibrates(&mut ui, &mut s, &atlas));
}

/// Shader packs, both kinds, are on the graphics screen's Shaders page: its Shader
/// Packs... opens the pack list, listed afresh, and Done comes back to that page.
#[test]
fn the_graphics_shaders_page_opens_the_shader_pack_list() {
    use crate::graphics_ui::{H, Item};
    use winit::keyboard::KeyCode;
    let atlas = atlas();
    let mut s = Settings::default();
    let mut ui = OptionsUi {
        size: (427.0, 240.0),
        ..Default::default()
    };
    ui.show(Page::Graphics);
    ui.graphics.show(crate::graphics_ui::Page::Shaders);
    let packs = Item::Horizon(H::ShaderPacks);
    assert!(
        (0..40).any(|_| {
            ui.key(&mut s, &atlas, KeyCode::Tab, None);
            ui.graphics.focused_row() == Some(packs)
        }),
        "{:?}",
        ui.graphics.focused()
    );
    let events = ui.key(&mut s, &atlas, KeyCode::Enter, None);
    assert_eq!(ui.page(), Page::ShaderPacks);
    assert!(events.contains(&Event::ListShaderPacks), "{events:?}");
    // The picked pack's own settings are read with the list.
    assert!(events.contains(&Event::OpenShaderOptions), "{events:?}");
    ui.back(&mut s);
    assert_eq!(ui.page(), Page::Graphics);
    assert_eq!(
        ui.graphics.open_page(),
        Some(crate::graphics_ui::Page::Shaders)
    );
}

#[test]
fn the_graphics_screens_wheel_scrolls_even_over_a_slider_being_changed() {
    use crate::graphics_ui::{H, Item};
    use winit::keyboard::KeyCode as K;
    let mut s = Settings::default();
    let mut ui = OptionsUi::default();
    ui.show(Page::Graphics);
    ui.graphics.show(crate::graphics_ui::Page::Performance);
    let size = (640.0, 360.0);
    s.horizon.chunk_updates = 16;
    let chunk_updates = Item::Horizon(H::ChunkUpdates);
    // Tab onto it: the arrows change it.
    while ui.graphics.focused_row() != Some(chunk_updates) {
        shell::Hosted::key(&mut ui.graphics, &mut s, K::Tab, None, false, size);
    }
    assert!(ui.graphics.changes_value());
    let before = ui.graphics.scroll_amount();
    ui.wheel(&mut s, size, (320.0, 150.0), [1920, 1080], -1.0);
    assert_eq!(s.horizon.chunk_updates, 16);
    assert!(ui.graphics.scroll_amount() > before);
}

/// A window holding HDR output back (a start failed on its driver) shows it on the HDR page
/// as soon as the graphics screen opens, with no setting applied: the row's line, and
/// Calibrate blocked, where the same window not holding it back calibrates.
#[test]
fn a_held_hdr_start_shows_as_the_graphics_screen_opens() {
    use crate::gfx::output::tests::{snapshot, window};
    let atlas = atlas();
    let mut s = Settings::default();
    s.horizon.hdr = crate::graphics::HdrMode::Auto;
    s.horizon.hdr_failed_start = Some("Vulkan 10de:2684 GPU; driver; 1.0; os".into());
    let (mut caps, reading) = window(Some(crate::gfx::HdrOutput::Pq), Some(true));
    for held in [true, false] {
        caps.held = held;
        let mut ui = OptionsUi {
            size: (427.0, 240.0),
            ..Default::default()
        };
        let output = snapshot(&s.horizon, Default::default(), caps, reading);
        ui.draw(
            &s,
            output,
            &mut HudBatch::new([427, 240]),
            &atlas,
            (-1.0, -1.0),
        );
        ui.show(Page::Graphics);
        assert!(calibrates(&mut ui, &mut s, &atlas) != held, "held {held}");
        assert_eq!(ui.graphics.screen.output.held, held);
    }
}

/// The slider position `Opt::ChatDelay`'s kind() gives the stored seconds.
fn delay_pos(seconds: f64) -> f32 {
    match values::kind(seconds) {
        Kind::Slider(p) => p,
        _ => panic!("the chat delay is a slider"),
    }
}
/// `Options.chatDelay`: `IntRange(0, 60)` xmap(v / 10) — a slider in tenths of
/// a second from 0 to 6, not a button with no value behind it.
#[test]
fn the_chat_delay_slider_steps_in_tenths() {
    assert_eq!(delay_pos(0.0), 0.0);
    assert!((delay_pos(3.0) - 0.5).abs() < 1e-6);
    assert_eq!(delay_pos(6.0), 1.0);
    // A value past the option's range clamps to its last step.
    assert_eq!(delay_pos(6.5), 1.0);
    assert_eq!(Opt::ChatDelay.slider_steps(), Some(60));
    let mut s = Settings::default();
    set_slider(Opt::ChatDelay, &mut s, 0.0);
    assert_eq!(s.file.double("chatDelay"), 0.0);
    set_slider(Opt::ChatDelay, &mut s, 0.5);
    assert!((s.file.double("chatDelay") - 3.0).abs() < 1e-9);
    set_slider(Opt::ChatDelay, &mut s, 1.0);
    assert!((s.file.double("chatDelay") - 6.0).abs() < 1e-9);
}

/// `Options.chatDelay`'s message: None at zero, else the delay to one decimal.
#[test]
fn the_chat_delay_slider_says_its_delay() {
    let mut s = Settings::default();
    assert_eq!(chat_delay_label(&s).plain(), "options.chat.delay_none");
    s.file.set("chatDelay", crate::options_file::V::Double(2.0));
    assert_eq!(chat_delay_label(&s).plain(), "Chat Delay: 2.0 second(s)");
    s.file.set("chatDelay", crate::options_file::V::Double(6.0));
    assert_eq!(chat_delay_label(&s).plain(), "Chat Delay: 6.0 second(s)");
    s.file.set("chatDelay", crate::options_file::V::Double(0.0));
    assert_eq!(chat_delay_label(&s).plain(), "options.chat.delay_none");
}
