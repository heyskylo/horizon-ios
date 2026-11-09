//! HDR calibration (the graphics screen's HDR page, Calibrate...): full screen, one step at a time, the way
//! display calibration tools do it. HDR output draws each step's test picture straight in
//! nits above a band of controls (`hdr.wgsl`, `Hdr::calibration`), past everything that
//! shapes the game's picture, so what the player sees is the display alone:
//!
//! - Black Level: Horizon's logo at the slider's brightness on 0 nits; lowered until it
//!   disappears, it sits on the darkest the display shows.
//! - Peak Brightness: a patch of a tenth of the screen at 10 000 nits (the display clips
//!   it at its brightest), the logo inside at the slider; raised until it disappears, it
//!   sits on that brightest. Small, so the display doesn't dim a bright full screen; the
//!   HDR metadata declares PQ's 10 000 nits meanwhile, so a compositor that clips at the
//!   declared peak (KWin) lets the patch through.
//! - Paper White: a white card at the slider, matched to a sheet of white paper held
//!   beside the screen in the light the player plays in: white is where the eyes have
//!   adapted to, lower in a dark room than by day (as ITU-R BT.2408's reference white).
//!
//! Nothing changes until Save, which writes the three into Horizon's settings; Cancel
//! or Escape leaves them as they were.
use crate::{
    font::Font,
    hud::{HudAtlas, HudBatch, rgba},
    lang,
    settings::Settings,
    text::Text,
    ui,
};
use winit::keyboard::KeyCode;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    Black,
    Peak,
    PaperWhite,
}
const STEPS: [Step; 3] = [Step::Black, Step::Peak, Step::PaperWhite];

/// A step's test picture for HDR output: the step, its nits and where the controls start (a
/// share of the height).
pub type Picture = (Step, f32, f32);

/// What the options screen does after an input here.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    Nothing,
    Click,
    Changed,
    Save,
    Cancel,
}

/// The window as calibration found it: calibration turns it fullscreen for its test pictures,
/// and gives it back as it was.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WindowState {
    Fullscreen,
    Maximized,
    Windowed,
}

impl WindowState {
    pub fn of(fullscreen: bool, maximized: bool) -> Self {
        match (fullscreen, maximized) {
            (true, _) => Self::Fullscreen,
            (false, true) => Self::Maximized,
            (false, false) => Self::Windowed,
        }
    }
    /// On leaving calibration: whether the window leaves fullscreen, and whether it is
    /// maximized again after (leaving fullscreen gives back its restored size on Windows).
    pub fn leaving(self) -> (bool, bool) {
        (self != Self::Fullscreen, self == Self::Maximized)
    }
}

pub struct Calibration {
    step: usize,
    /// Nits, in the order of `STEPS`.
    values: [f32; 3],
    /// Which values were Auto (peak, paper white; black has none): saved as Auto again
    /// unless the player moved them.
    auto: [Option<f32>; 3],
    dragging: bool,
}

/// Height of the band of controls at the bottom with a hint of one line, in GUI pixels.
const BAND: f32 = 84.0;
/// Each further line of the hint, which wraps within the window less this at each side.
const HINT_LINE: f32 = 10.0;
const HINT_MARGIN: f32 = 10.0;

impl Step {
    /// The slider's steps past its first: the settings' own.
    fn steps(self) -> u32 {
        match self {
            Step::Black => 100,
            Step::Peak => crate::graphics::PEAK_STEPS,
            Step::PaperWhite => 47,
        }
    }
    /// The nits at `step` of the slider: black 0..1 by 0.01, the peak's scale, paper
    /// white 10..480 by 10.
    fn nits_at(self, step: u32) -> f32 {
        match self {
            Step::Black => step as f32 / 100.0,
            Step::Peak => crate::graphics::peak_nits_at(step) as f32,
            Step::PaperWhite => 10.0 + step as f32 * 10.0,
        }
    }
    /// Where `nits` sits on the slider, in steps.
    fn step_of(self, nits: f32) -> f32 {
        match self {
            Step::Black => nits * 100.0,
            Step::Peak => crate::graphics::peak_step_of(nits),
            Step::PaperWhite => (nits - 10.0) / 10.0,
        }
    }
    /// The English name, which is also its key in the language table.
    fn name_key(self) -> &'static str {
        match self {
            Step::Black => "Black Level",
            Step::Peak => "Peak Brightness",
            Step::PaperWhite => "Paper White",
        }
    }
    fn name(self) -> String {
        lang::get(self.name_key())
    }
    /// The English hint, which is also its key in the language table.
    fn hint_key(self) -> &'static str {
        match self {
            Step::Black => "Lower it until the logo just disappears into the black.",
            Step::Peak => "Raise it until the logo just disappears into the bright square.",
            Step::PaperWhite => {
                "Make the card a white that's comfortable to look at for a long time, in the light you play in."
            }
        }
    }
    /// `hdr::Calibration::step`'s number.
    pub fn number(self) -> u32 {
        match self {
            Step::Black => 1,
            Step::Peak => 2,
            Step::PaperWhite => 3,
        }
    }
}

struct Layout {
    top: f32,
    hint: Vec<Text>,
    slider: (f32, f32, f32),
    cancel: (f32, f32, f32),
    back: (f32, f32, f32),
    next: (f32, f32, f32),
}
/// The band grows up by the lines `step`'s hint takes past its first.
fn layout(font: &Font, step: Step, (w, h): (f32, f32)) -> Layout {
    let hint = Text::literal(lang::get(step.hint_key()));
    let hint = font.split(&hint, w - 2.0 * HINT_MARGIN);
    let more = hint.len().saturating_sub(1) as f32 * HINT_LINE;
    let top = (h - BAND - more).floor();
    let cx = (w / 2.0).floor();
    let by = top + more + 58.0;
    Layout {
        top,
        hint,
        slider: (cx - 100.0, top + more + 32.0, 200.0),
        cancel: (cx - 152.0, by, 98.0),
        back: (cx - 49.0, by, 98.0),
        next: (cx + 54.0, by, 98.0),
    }
}
fn over(cursor: (f32, f32), (x, y, w): (f32, f32, f32)) -> bool {
    ui::hovered(cursor, x, y, w, 20.0)
}

impl Calibration {
    /// Starts from the settings, their Auto values from what the display of the window it
    /// runs in reported (`output`, its renderer's).
    pub fn new(s: &Settings, output: &crate::gfx::EffectiveOutput) -> Self {
        let g = &s.horizon;
        let (peak, paper_white) = (
            g.peak_nits(output.reported) as f32,
            g.paper_white_nits(output.reported) as f32,
        );
        Self {
            step: 0,
            values: [g.black_level_nits(), peak, paper_white],
            auto: [
                None,
                (g.hdr_peak == 0).then_some(peak),
                (g.hdr_paper_white == 0).then_some(paper_white),
            ],
            dragging: false,
        }
    }
    fn current(&self) -> Step {
        STEPS[self.step]
    }
    fn layout(&self, font: &Font, size: (f32, f32)) -> Layout {
        layout(font, self.current(), size)
    }
    /// The slider's position, 0..=1.
    fn position(&self) -> f32 {
        let step = self.current();
        step.step_of(self.values[self.step]) / step.steps() as f32
    }
    /// Puts the slider on its step nearest `position`.
    fn set(&mut self, position: f32) {
        let step = self.current();
        let steps = step.steps() as f32;
        self.values[self.step] = step.nits_at((position * steps).round().clamp(0.0, steps) as u32);
    }
    /// Moves the slider `by` steps from the one nearest its value.
    fn step_by(&mut self, by: f32) {
        let steps = self.current().steps() as f32;
        self.set(((self.position() * steps).round() + by) / steps);
    }
    fn label(&self) -> Text {
        let v = self.values[self.step];
        let step = self.current();
        let value = match step {
            Step::Black => format!("{v:.2}"),
            _ => format!("{v:.0}"),
        };
        Text::literal(format!("{}: {}", step.name(), value))
    }
    /// The current step's test picture in a GUI of `size`.
    pub fn picture(&self, font: &Font, size: (f32, f32)) -> Picture {
        (
            self.current(),
            self.values[self.step],
            self.layout(font, size).top / size.1.max(1.0),
        )
    }
    pub fn draw(&self, batch: &mut HudBatch, atlas: &HudAtlas, cursor: (f32, f32)) {
        let (w, h) = (batch.width, batch.height);
        let l = self.layout(&atlas.font, (w, h));
        // Above the band HDR output draws the test picture over whatever is here.
        batch.fill(atlas, 0.0, 0.0, w, h, rgba(0, 0, 0, 255));
        batch.fill(atlas, 0.0, l.top, w, h - l.top, rgba(24, 24, 24, 255));
        let step = self.current();
        let title = Text::literal(lang::tr(
            "HDR Calibration %s/%s: %s",
            &[&(self.step + 1), &STEPS.len(), &step.name()],
        ));
        ui::centered_text(batch, atlas, &title, 0.0, l.top + 6.0, w, u32::MAX);
        for (i, line) in l.hint.iter().enumerate() {
            let y = l.top + 18.0 + i as f32 * HINT_LINE;
            ui::centered_text(batch, atlas, line, 0.0, y, w, rgba(160, 160, 160, 255));
        }
        let (x, y, sw) = l.slider;
        ui::slider(
            batch,
            atlas,
            x,
            y,
            sw,
            &self.label(),
            self.position(),
            over(cursor, l.slider) || self.dragging,
            true,
            self.dragging,
        );
        let last = self.step + 1 == STEPS.len();
        let save = lang::get_or("selectWorld.edit.save", "Save");
        let next = lang::get("Next");
        for (rect, label, active) in [
            (l.cancel, lang::get_or("gui.cancel", "Cancel"), true),
            (l.back, lang::get_or("gui.back", "Back"), self.step > 0),
            (l.next, if last { save } else { next }, true),
        ] {
            let hover = over(cursor, rect) && active;
            let action = if rect == l.cancel {
                Some(crate::widgets::Action::Cancel)
            } else if rect == l.back {
                Some(crate::widgets::Action::Back)
            } else {
                None
            };
            if let Some(action) = action {
                crate::widgets::declare_action(action, [rect.0, rect.1, rect.2, 20.0], active);
            }
            ui::button(
                batch,
                atlas,
                rect.0,
                rect.1,
                rect.2,
                20.0,
                &Text::literal(label),
                active,
                hover,
            );
        }
    }
    /// A press, or a drag with `drag`, at `cursor`.
    pub fn click(
        &mut self,
        font: &Font,
        cursor: (f32, f32),
        size: (f32, f32),
        drag: bool,
    ) -> Outcome {
        let l = self.layout(font, size);
        let (x, _, sw) = l.slider;
        if drag {
            if !self.dragging {
                return Outcome::Nothing;
            }
            self.set(ui::slider_value(x, sw, cursor.0));
            return Outcome::Changed;
        }
        if over(cursor, l.slider) {
            self.dragging = true;
            self.set(ui::slider_value(x, sw, cursor.0));
            return Outcome::Changed;
        }
        if over(cursor, l.cancel) {
            return Outcome::Cancel;
        }
        if over(cursor, l.back) && self.step > 0 {
            self.step -= 1;
            return Outcome::Click;
        }
        if over(cursor, l.next) {
            return self.next();
        }
        Outcome::Nothing
    }
    fn next(&mut self) -> Outcome {
        if self.step + 1 == STEPS.len() {
            return Outcome::Save;
        }
        self.step += 1;
        Outcome::Click
    }
    /// The slider let go: true if it was held (it clicks on release, as sliders do).
    pub fn release(&mut self) -> bool {
        std::mem::take(&mut self.dragging)
    }
    /// Left and Right step the slider, Enter goes on, Escape cancels.
    pub fn key(&mut self, key: KeyCode) -> Outcome {
        match key {
            KeyCode::ArrowLeft | KeyCode::ArrowDown => {
                self.step_by(-1.0);
                Outcome::Changed
            }
            KeyCode::ArrowRight | KeyCode::ArrowUp => {
                self.step_by(1.0);
                Outcome::Changed
            }
            KeyCode::Enter | KeyCode::NumpadEnter => self.next(),
            KeyCode::Escape => Outcome::Cancel,
            _ => Outcome::Nothing,
        }
    }
    /// Writes what the player landed on into Horizon's settings.
    pub fn save(&self, s: &mut Settings) {
        let g = &mut s.horizon;
        let scale = [100.0, 1.0, 1.0];
        let [black, peak, paper_white] = [0, 1, 2].map(|i| match self.auto[i] {
            Some(auto) if auto == self.values[i] => 0,
            _ => (self.values[i] * scale[i]).round() as u32,
        });
        g.hdr_black_level = black;
        g.hdr_peak = peak;
        g.hdr_paper_white = paper_white;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gfx::EffectiveOutput;

    const NONE: EffectiveOutput = EffectiveOutput::NONE;

    fn font() -> Font {
        Font::metrics(std::path::Path::new("/nonexistent/horizon-test-assets"))
    }
    /// At the default GUI scale of the game's first window (854 x 480 at 2).
    fn picture(c: &Calibration) -> Picture {
        c.picture(&font(), (427.0, 240.0))
    }

    #[test]
    fn calibration_saves_only_on_save() {
        let mut s = Settings::default();
        s.horizon.hdr_black_level = 5;
        let before = s.horizon.clone();
        let mut c = Calibration::new(&s, &NONE);
        assert_eq!(c.key(KeyCode::ArrowLeft), Outcome::Changed);
        assert_eq!(c.key(KeyCode::ArrowLeft), Outcome::Changed);
        assert!((picture(&c).1 - 0.03).abs() < 1e-6);
        assert_eq!(c.key(KeyCode::Escape), Outcome::Cancel);
        assert_eq!(s.horizon, before);
        // Black, then Peak (one step up), then Paper White, then Save.
        assert_eq!(c.key(KeyCode::Enter), Outcome::Click);
        assert_eq!(c.key(KeyCode::ArrowRight), Outcome::Changed);
        assert_eq!(c.key(KeyCode::Enter), Outcome::Click);
        assert_eq!(picture(&c).0, Step::PaperWhite);
        assert_eq!(c.key(KeyCode::Enter), Outcome::Save);
        c.save(&mut s);
        assert_eq!(s.horizon.hdr_black_level, 3);
        assert_eq!(s.horizon.hdr_peak, before.peak_nits(None) + 50);
        assert_eq!(s.horizon.hdr_paper_white, before.hdr_paper_white);
    }

    /// Auto's peak and paper white start from what the display of the window calibration
    /// runs in reported; with no report, from 1000 and 203 nits.
    #[test]
    fn auto_starts_from_the_report_of_the_window_it_runs_in() {
        use crate::gfx::{HdrOutput, output::tests};
        let s = Settings::default();
        let (caps, reading) = tests::window(Some(HdrOutput::Pq), Some(true));
        let reporting = tests::snapshot(&s.horizon, Default::default(), caps, reading);
        for (output, peak, paper_white) in [(&reporting, 1000.0, 400.0), (&NONE, 1000.0, 203.0)] {
            let mut c = Calibration::new(&s, output);
            c.key(KeyCode::Enter);
            assert_eq!(picture(&c).0, Step::Peak);
            assert_eq!(picture(&c).1, peak);
            c.key(KeyCode::Enter);
            assert_eq!(picture(&c).1, paper_white);
        }
    }

    #[test]
    fn each_step_reaches_every_value_of_its_setting() {
        // Black Level 0..1 nit by 0.01; Peak Brightness 200..2000 by 50, then by 500 to PQ's
        // 10 000; Paper White 10..480 by 10.
        let expected: [Vec<f32>; 3] = [
            (0..=100).map(|n| n as f32 / 100.0).collect(),
            (200..=2000)
                .step_by(50)
                .chain((2500..=10_000).step_by(500))
                .map(|n| n as f32)
                .collect(),
            (10..=480).step_by(10).map(|n| n as f32).collect(),
        ];
        let mut c = Calibration::new(&Settings::default(), &NONE);
        for (i, expected) in expected.iter().enumerate() {
            assert_eq!(picture(&c).0, STEPS[i]);
            for _ in 0..200 {
                c.key(KeyCode::ArrowLeft);
            }
            let mut seen = vec![picture(&c).1];
            for _ in 0..200 {
                c.key(KeyCode::ArrowRight);
                let v = picture(&c).1;
                if seen.last() != Some(&v) {
                    seen.push(v);
                }
            }
            assert_eq!(&seen, expected, "{:?}", STEPS[i]);
            c.key(KeyCode::Enter);
        }
    }

    #[test]
    fn black_level_saves_the_lift_it_shows_and_peak_reaches_10000() {
        let mut s = Settings::default();
        let mut c = Calibration::new(&s, &NONE);
        // Untouched, no lift stays no lift.
        c.save(&mut s);
        assert_eq!(s.horizon.hdr_black_level, 0);
        c.key(KeyCode::ArrowRight);
        c.key(KeyCode::ArrowRight);
        c.save(&mut s);
        assert_eq!(s.horizon.hdr_black_level, 2);
        // Calibrated back down, 0 is no lift: what the step showed.
        let mut c = Calibration::new(&s, &NONE);
        assert!((picture(&c).1 - 0.02).abs() < 1e-6);
        c.key(KeyCode::ArrowLeft);
        c.key(KeyCode::ArrowLeft);
        c.key(KeyCode::Enter);
        for _ in 0..60 {
            c.key(KeyCode::ArrowRight);
        }
        c.save(&mut s);
        assert_eq!(s.horizon.hdr_black_level, 0);
        assert_eq!(s.horizon.black_level_nits(), 0.0);
        assert_eq!(s.horizon.hdr_peak, 10_000);
        let mut c = Calibration::new(&s, &NONE);
        c.key(KeyCode::Enter);
        assert_eq!(picture(&c).1, 10_000.0);
        // One step down from the top is 500 nits; from 2000, 50.
        c.key(KeyCode::ArrowLeft);
        assert_eq!(picture(&c).1, 9500.0);
        s.horizon.hdr_peak = 2000;
        let mut c = Calibration::new(&s, &NONE);
        c.key(KeyCode::Enter);
        c.key(KeyCode::ArrowLeft);
        assert_eq!(picture(&c).1, 1950.0);
    }

    /// A hint wider than the window less its margins wraps, the band grows up by the lines
    /// it adds, and the test picture ends where the band drawn starts.
    #[test]
    fn the_hint_wraps_and_the_band_grows_by_its_lines() {
        let font = font();
        let mut c = Calibration::new(&Settings::default(), &NONE);
        c.key(KeyCode::Enter);
        c.key(KeyCode::Enter);
        let hint = Text::literal(lang::get(Step::PaperWhite.hint_key()));
        let wide = font.width_text(&hint);
        let size = (wide + 2.0 * HINT_MARGIN, 240.0);
        let one = c.layout(&font, size);
        assert_eq!(one.hint.len(), 1);
        assert_eq!(one.top, 240.0 - BAND);
        let narrow = (wide * 0.6, 240.0);
        let wrapped = c.layout(&font, narrow);
        assert_eq!(wrapped.hint.len(), 2);
        assert!(
            wrapped
                .hint
                .iter()
                .all(|l| font.width_text(l) <= narrow.0 - 2.0 * HINT_MARGIN)
        );
        assert_eq!(wrapped.top, 240.0 - BAND - HINT_LINE);
        // Under the hint's lines, the controls keep their place in the band.
        assert_eq!(
            wrapped.slider.1 - wrapped.top,
            one.slider.1 - one.top + HINT_LINE
        );
        assert_eq!(
            wrapped.next.1 - wrapped.top,
            one.next.1 - one.top + HINT_LINE
        );
        for size in [size, narrow] {
            let (_, _, band) = c.picture(&font, size);
            assert_eq!(band * size.1, c.layout(&font, size).top);
        }
    }

    /// The Peak step's patch (a square of a tenth of the screen, centred above the band,
    /// `hdr.wgsl`) stays above the band even on the narrowest GUI with every hint wrapped:
    /// its side is no more than the band's top.
    #[test]
    fn the_test_pictures_stay_above_a_grown_band() {
        let font = font();
        let mut c = Calibration::new(&Settings::default(), &NONE);
        for step in STEPS {
            for (w, h) in [(320.0, 240.0), (200.0, 240.0), (427.0, 240.0)] {
                let (shown, _, band) = c.picture(&font, (w, h));
                assert_eq!(shown, step);
                let side = (0.1 * w * h).sqrt();
                assert!(
                    side <= band * h,
                    "{step:?} at {w}x{h}: {side} > {}",
                    band * h
                );
            }
            c.key(KeyCode::Enter);
        }
    }

    /// Leaving calibration gives the window back as it found it.
    #[test]
    fn leaving_gives_the_window_back_as_it_was() {
        use WindowState as W;
        assert_eq!(W::of(true, false), W::Fullscreen);
        assert_eq!(W::of(true, true), W::Fullscreen);
        assert_eq!(W::of(false, true), W::Maximized);
        assert_eq!(W::of(false, false), W::Windowed);
        // (leave fullscreen, maximize again)
        assert_eq!(W::Fullscreen.leaving(), (false, false));
        assert_eq!(W::Maximized.leaving(), (true, true));
        assert_eq!(W::Windowed.leaving(), (true, false));
    }
}
