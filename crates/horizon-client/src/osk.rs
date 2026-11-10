//! The on-screen keyboard a controller types with in focus mode (Horizon's
//! own; docs/controllers.md, "Text fields"): a QWERTY grid and a symbols
//! grid in Horizon's design language, moved over with the D-pad or stick.
//! A types the key, X deletes, Y is a space, Start is the Done key, B closes it.
//! A field of many lines (a book's page, a sign) gets a New Line key, so Done
//! never makes a line. Over chat, RT is Tab (fill in the suggestion, again for
//! the next) and the right stick moves through the suggestions.
//! It only produces [`Output`]s; the game feeds them to the focused field
//! the way typed keys reach it.
use crate::gamepad::{Button, MenuJob};
use crate::hud_kit::hint::HintRow;
use crate::pad_glyph::{Glyph, Glyphs};
use crate::ui_theme::{self, Face, Ui, color};

/// A label of the keyboard: the language table's, or the English text.
fn tr(key: &str, english: &str) -> String {
    crate::lang::get_or(key, english)
}

/// What a key press does to the focused field.
#[derive(Clone, Debug, PartialEq)]
pub enum Output {
    Text(String),
    Backspace,
    /// The Enter key: a new line on a field of many lines.
    Enter,
    /// The typing is done: the field's own Done (a book's, a sign's), or Enter on a field of
    /// one line.
    Done,
    /// Tab: fill in the selected suggestion (vanilla's `CommandSuggestions`), again for the next.
    Complete,
    /// Up and Down: the suggestion before or after (or chat's history, as the arrow keys).
    Previous,
    Next,
    /// The keyboard closes; the field keeps its text.
    Close,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Key {
    Char(char),
    Shift,
    Layer,
    Space,
    Backspace,
    NewLine,
    Done,
}

/// A row of keys and their widths in key units (every row is 10 wide).
type Row = &'static [(Key, f32)];

const fn c(ch: char) -> (Key, f32) {
    (Key::Char(ch), 1.0)
}
const BOTTOM: Row = &[
    (Key::Shift, 1.5),
    (Key::Layer, 1.5),
    (Key::Space, 4.0),
    (Key::Backspace, 1.5),
    (Key::Done, 1.5),
];
/// The bottom row on a field of many lines: New Line beside Done.
const BOTTOM_LINES: Row = &[
    (Key::Shift, 1.5),
    (Key::Layer, 1.5),
    (Key::Space, 2.5),
    (Key::NewLine, 1.5),
    (Key::Backspace, 1.5),
    (Key::Done, 1.5),
];
const LETTERS: [Row; 5] = [
    &[
        c('1'),
        c('2'),
        c('3'),
        c('4'),
        c('5'),
        c('6'),
        c('7'),
        c('8'),
        c('9'),
        c('0'),
    ],
    &[
        c('q'),
        c('w'),
        c('e'),
        c('r'),
        c('t'),
        c('y'),
        c('u'),
        c('i'),
        c('o'),
        c('p'),
    ],
    &[
        c('a'),
        c('s'),
        c('d'),
        c('f'),
        c('g'),
        c('h'),
        c('j'),
        c('k'),
        c('l'),
        c('\''),
    ],
    &[
        c('z'),
        c('x'),
        c('c'),
        c('v'),
        c('b'),
        c('n'),
        c('m'),
        c(','),
        c('.'),
        c('-'),
    ],
    BOTTOM,
];
const SYMBOLS: [Row; 5] = [
    &[
        c('!'),
        c('@'),
        c('#'),
        c('$'),
        c('%'),
        c('^'),
        c('&'),
        c('*'),
        c('('),
        c(')'),
    ],
    &[
        c('/'),
        c('\\'),
        c('_'),
        c('='),
        c('+'),
        c('['),
        c(']'),
        c('{'),
        c('}'),
        c('|'),
    ],
    &[
        c(';'),
        c(':'),
        c('"'),
        c('<'),
        c('>'),
        c('?'),
        c('~'),
        c('`'),
        c('§'),
        c('°'),
    ],
    &[
        c('é'),
        c('è'),
        c('à'),
        c('ç'),
        c('ù'),
        c('ü'),
        c('ö'),
        c('ä'),
        c('ß'),
        c('ñ'),
    ],
    BOTTOM,
];
const LETTERS_LINES: [Row; 5] = [LETTERS[0], LETTERS[1], LETTERS[2], LETTERS[3], BOTTOM_LINES];
const SYMBOLS_LINES: [Row; 5] = [SYMBOLS[0], SYMBOLS[1], SYMBOLS[2], SYMBOLS[3], BOTTOM_LINES];

/// The panel's padding around the keys and the hints, the gap between them, and a hint line's
/// height in its text size, all in dp (a line box is 1.45 sizes; the pack's font can be a
/// little taller).
const PANEL_PAD: f32 = 12.0;
const HINT_GAP: f32 = 8.0;
const HINT_LINE: f32 = 1.6;
/// Between two hints on a line.
const HINT_SEP: &str = "   ";

/// `items` on as few lines as fit `width` (as `measure` measures a line), in order. An item wider
/// than `width` breaks at its spaces onto lines of its own; a word wider than `width` alone keeps
/// a line, which the drawing shrinks to fit.
fn wrap_hints(
    items: impl IntoIterator<Item = String>,
    width: f32,
    measure: impl Fn(&str) -> f32,
) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    for item in items {
        if measure(&item) > width {
            let mut line = String::new();
            for word in item.split(' ').filter(|w| !w.is_empty()) {
                if !line.is_empty() && measure(&format!("{line} {word}")) > width {
                    lines.push(std::mem::take(&mut line));
                }
                if !line.is_empty() {
                    line.push(' ');
                }
                line.push_str(word);
            }
            lines.push(line);
            continue;
        }
        match lines.last_mut() {
            Some(line) if measure(&format!("{line}{HINT_SEP}{item}")) <= width => {
                line.push_str(HINT_SEP);
                line.push_str(&item);
            }
            _ => lines.push(item),
        }
    }
    lines
}

/// The right stick, up (+) or down (-), past this picks the suggestion before or after; it picks
/// again once back under [`STICK_REST`].
const STICK_PICK: f32 = 0.6;
const STICK_REST: f32 = 0.3;

/// The keyboard's state while open.
#[derive(Clone, Debug, PartialEq)]
pub struct Keyboard {
    row: usize,
    col: usize,
    symbols: bool,
    /// The next letter is upper case (Shift), or every letter (double Shift).
    shift: bool,
    caps: bool,
    /// The field takes many lines: a New Line key, and Done finishes.
    lines: bool,
    /// The field has suggestions (chat): RT fills them in, the right stick picks one.
    suggests: bool,
    /// The right stick has picked and not come back to rest.
    stick_out: bool,
}
impl Default for Keyboard {
    fn default() -> Self {
        // Focus starts on `q`.
        Self {
            row: 1,
            col: 0,
            symbols: false,
            shift: false,
            caps: false,
            lines: false,
            suggests: false,
            stick_out: false,
        }
    }
}

impl Keyboard {
    /// A keyboard over a field of many `lines` (New Line beside Done), and one with
    /// suggestions (`suggests`: RT and the right stick reach them).
    pub fn new(lines: bool, suggests: bool) -> Self {
        Self {
            lines,
            suggests,
            ..Self::default()
        }
    }
    fn rows(&self) -> &'static [Row; 5] {
        match (self.symbols, self.lines) {
            (false, false) => &LETTERS,
            (true, false) => &SYMBOLS,
            (false, true) => &LETTERS_LINES,
            (true, true) => &SYMBOLS_LINES,
        }
    }
    /// The right stick's height (up is +): the suggestion before or after, once per push.
    pub fn stick(&mut self, y: f32) -> Option<Output> {
        if !self.suggests {
            return None;
        }
        if self.stick_out {
            self.stick_out = y.abs() >= STICK_REST;
            return None;
        }
        if y.abs() < STICK_PICK {
            return None;
        }
        self.stick_out = true;
        Some(if y > 0.0 {
            Output::Previous
        } else {
            Output::Next
        })
    }
    /// A key's left edge and width in key units.
    fn span(row: Row, col: usize) -> (f32, f32) {
        let left: f32 = row[..col].iter().map(|k| k.1).sum();
        (left, row[col].1)
    }
    fn key(&self) -> Key {
        self.rows()[self.row][self.col].0
    }
    /// A D-pad step: across a row with wrap-around; up and down to the key
    /// under the focused key's centre (wrapping top and bottom).
    pub fn step(&mut self, button: Button) {
        let rows = self.rows();
        let row = rows[self.row];
        match button {
            Button::DPadLeft => self.col = (self.col + row.len() - 1) % row.len(),
            Button::DPadRight => self.col = (self.col + 1) % row.len(),
            Button::DPadUp | Button::DPadDown => {
                let (left, width) = Self::span(row, self.col);
                let centre = left + width / 2.0;
                self.row = if button == Button::DPadUp {
                    (self.row + rows.len() - 1) % rows.len()
                } else {
                    (self.row + 1) % rows.len()
                };
                let next = rows[self.row];
                self.col = (0..next.len())
                    .find(|&i| {
                        let (l, w) = Self::span(next, i);
                        centre < l + w
                    })
                    .unwrap_or(next.len() - 1);
            }
            _ => {}
        }
    }
    fn letter(&self, ch: char) -> String {
        if self.shift || self.caps {
            ch.to_uppercase().collect()
        } else {
            ch.to_string()
        }
    }
    /// A pressed: the focused key's output (Shift and the symbols switch only
    /// change the keyboard).
    pub fn press(&mut self) -> Option<Output> {
        match self.key() {
            Key::Char(ch) => {
                let text = self.letter(ch);
                self.shift = false;
                Some(Output::Text(text))
            }
            Key::Shift => {
                // Shift, then Shift again locks capitals; a third press lets go.
                (self.shift, self.caps) = match (self.shift, self.caps) {
                    (false, false) => (true, false),
                    (true, false) => (false, true),
                    _ => (false, false),
                };
                None
            }
            Key::Layer => {
                self.symbols = !self.symbols;
                self.col = self.col.min(self.rows()[self.row].len() - 1);
                None
            }
            Key::Space => Some(Output::Text(" ".into())),
            Key::Backspace => Some(Output::Backspace),
            Key::NewLine => Some(Output::Enter),
            Key::Done => Some(Output::Done),
        }
    }
    /// A pad button while the keyboard is open; `None` when it does nothing
    /// to the field (moving, Shift).
    /// Its jobs are menu jobs (`MenuJob`), the map its hint line's glyphs come from.
    pub fn button(&mut self, button: Button) -> Option<Output> {
        match MenuJob::of(button) {
            Some(MenuJob::Confirm) => self.press(),
            Some(MenuJob::Alternate) => Some(Output::Backspace),
            Some(MenuJob::Extra) => Some(Output::Text(" ".into())),
            Some(MenuJob::Start) => Some(Output::Done),
            Some(MenuJob::GroupNext) if self.suggests => Some(Output::Complete),
            Some(MenuJob::Back) => Some(Output::Close),
            Some(MenuJob::LeftStick | MenuJob::GroupPrev) => {
                self.shift = !self.shift;
                None
            }
            Some(MenuJob::PageNext | MenuJob::PagePrev) => {
                self.symbols = !self.symbols;
                self.col = self.col.min(self.rows()[self.row].len() - 1);
                None
            }
            _ if matches!(
                button,
                Button::DPadUp | Button::DPadDown | Button::DPadLeft | Button::DPadRight
            ) =>
            {
                self.step(button);
                None
            }
            _ => None,
        }
    }

    /// The hint line under the keys ("A Type   X Delete ..."), wrapped onto as many lines as it
    /// needs to stay within `width`: the panel grows to hold them.
    fn hint_lines(&self, ui: &Ui, glyphs: Glyphs, width: f32) -> Vec<String> {
        let complete = self
            .suggests
            .then(|| (MenuJob::GroupNext, tr("horizon.osk.complete", "Complete")));
        let hints = [
            Some((MenuJob::Confirm, tr("horizon.osk.type", "Type"))),
            Some((MenuJob::Alternate, tr("horizon.osk.delete", "Delete"))),
            Some((MenuJob::Extra, tr("horizon.osk.space", "Space"))),
            Some((MenuJob::GroupPrev, tr("horizon.osk.shift", "Shift"))),
            complete,
            Some((MenuJob::PageNext, tr("horizon.osk.symbols", "Symbols"))),
            Some((MenuJob::Start, tr("gui.done", "Done"))),
            Some((MenuJob::Back, tr("horizon.osk.close", "Close"))),
        ];
        // This font draws no glyph art: each button by its print, "A Type".
        let items = hints
            .into_iter()
            .flatten()
            .map(|(job, what)| HintRow::new([Glyph::menu(job, glyphs)], what).text());
        wrap_hints(items, width, |line| {
            ui.measure(Face::Regular, ui_theme::small(), line)
        })
    }

    /// Draw the keyboard across the bottom of the screen, or the top when
    /// the field it types into is in the lower half.
    pub fn draw(&self, ui: &mut Ui, at_top: bool, glyphs: Glyphs) {
        const UNIT: f32 = 50.0;
        const GAP: f32 = 6.0;
        const KEY_H: f32 = 38.0;
        const LABEL_PAD: f32 = 4.0;
        let width = 10.0 * UNIT + 9.0 * GAP;
        let hints = self.hint_lines(ui, glyphs, width);
        let line_h = HINT_LINE * ui_theme::small();
        let keys_h = 5.0 * KEY_H + 4.0 * GAP;
        let height = PANEL_PAD + keys_h + HINT_GAP + hints.len() as f32 * line_h + PANEL_PAD;
        let x0 = ((ui.w - width) / 2.0).round();
        let y0 = if at_top { 16.0 } else { ui.h - height - 16.0 };
        ui.rounded(
            x0 - PANEL_PAD,
            y0 - PANEL_PAD,
            width + 2.0 * PANEL_PAD,
            height,
            ui_theme::radius_plate(),
            color::with_alpha(color::bg(), 0.94),
        );
        ui.border(
            x0 - PANEL_PAD,
            y0 - PANEL_PAD,
            width + 2.0 * PANEL_PAD,
            height,
            ui_theme::radius_plate(),
            color::border(),
        );
        for (r, row) in self.rows().iter().enumerate() {
            let y = y0 + r as f32 * (KEY_H + GAP);
            for (i, (key, units)) in row.iter().enumerate() {
                let (left, _) = Self::span(row, i);
                let x = x0 + left * (UNIT + GAP);
                let w = units * UNIT + (units - 1.0) * GAP;
                let focused = r == self.row && i == self.col;
                let lit = matches!(key, Key::Shift) && (self.shift || self.caps)
                    || matches!(key, Key::Layer) && self.symbols;
                let (plate, ink) = if focused {
                    (color::accent(), color::on_accent())
                } else if lit {
                    (color::hover(), color::text())
                } else {
                    (color::raised(), color::text())
                };
                ui.rounded(x, y, w, KEY_H, ui_theme::radius(), plate);
                let label = match key {
                    Key::Char(ch) => self.letter(*ch),
                    Key::Shift if self.caps => "CAPS".into(),
                    Key::Shift => tr("horizon.osk.shift", "Shift"),
                    Key::Layer if self.symbols => "abc".into(),
                    Key::Layer => "&123".into(),
                    Key::Space => tr("horizon.osk.space", "Space"),
                    Key::Backspace => tr("horizon.osk.delete", "Delete"),
                    Key::NewLine => tr("horizon.osk.new_line", "New Line"),
                    Key::Done => tr("gui.done", "Done"),
                };
                // Smaller when the label is wider than its key (a long translation).
                let room = w - 2.0 * LABEL_PAD;
                let wide = ui.measure(Face::Medium, ui_theme::body(), &label);
                let size = ui_theme::body() * (room / wide).min(1.0);
                let lw = ui.measure(Face::Medium, size, &label);
                ui.text_line(
                    Face::Medium,
                    size,
                    KEY_H,
                    x + ((w - lw) / 2.0).round(),
                    y,
                    ink,
                    &label,
                );
            }
        }
        let top = y0 + keys_h + HINT_GAP;
        for (i, line) in hints.iter().enumerate() {
            // Only a line of one word wider than the panel is still too wide: smaller, to fit.
            let wide = ui.measure(Face::Regular, ui_theme::small(), line);
            let size = ui_theme::small() * (width / wide).min(1.0);
            ui.text_centered(
                Face::Regular,
                size,
                x0 + width / 2.0,
                top + i as f32 * line_h,
                color::muted(),
                line,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rows_are_ten_units_wide() {
        for rows in [&LETTERS, &SYMBOLS] {
            for row in rows {
                let units: f32 = row.iter().map(|k| k.1).sum();
                assert_eq!(units, 10.0);
            }
        }
    }

    #[test]
    fn types_shifts_and_moves() {
        let mut k = Keyboard::default();
        assert_eq!(k.press(), Some(Output::Text("q".into())));
        k.step(Button::DPadRight);
        k.button(Button::LeftTrigger);
        assert_eq!(k.press(), Some(Output::Text("W".into())));
        assert_eq!(k.press(), Some(Output::Text("w".into())), "one-shot shift");
        // Left wraps to the end of the row.
        k.col = 0;
        k.step(Button::DPadLeft);
        assert_eq!(k.press(), Some(Output::Text("p".into())));
        // Down from `p` to the row of `'`, then the bottom row: `Done` sits
        // under the right edge; up from the wide Space bar lands mid-row.
        k.step(Button::DPadDown);
        k.step(Button::DPadDown);
        k.step(Button::DPadDown);
        assert_eq!(k.press(), Some(Output::Done));
        k.col = 2;
        assert_eq!(k.press(), Some(Output::Text(" ".into())));
        k.step(Button::DPadUp);
        assert_eq!(k.press(), Some(Output::Text("n".into())));
        // Up from the top row wraps to the bottom row.
        k.row = 0;
        k.col = 0;
        k.step(Button::DPadUp);
        assert_eq!(k.key(), Key::Shift);
        // Shift twice locks capitals; the symbols grid has no case.
        k.press();
        k.press();
        k.row = 1;
        k.col = 0;
        assert_eq!(k.press(), Some(Output::Text("Q".into())));
        assert_eq!(k.press(), Some(Output::Text("Q".into())));
        k.button(Button::RightBumper);
        assert_eq!(k.press(), Some(Output::Text("/".into())));
        assert_eq!(k.button(Button::West), Some(Output::Backspace));
        assert_eq!(k.button(Button::East), Some(Output::Close));
        assert_eq!(k.button(Button::Start), Some(Output::Done));
        // No suggestions here: RT and the right stick do nothing.
        assert_eq!(k.button(Button::RightTrigger), None);
        assert_eq!(k.stick(1.0), None);
    }

    #[test]
    fn many_lines_have_a_new_line_key_and_done_still_finishes() {
        let mut k = Keyboard::new(true, false);
        // From `q` down to the bottom row's right end: Done; New Line two keys before it.
        for _ in 0..3 {
            k.step(Button::DPadDown);
        }
        k.col = k.rows()[4].len() - 1;
        assert_eq!(k.press(), Some(Output::Done));
        k.step(Button::DPadLeft);
        k.step(Button::DPadLeft);
        assert_eq!(k.press(), Some(Output::Enter));
        assert_eq!(k.button(Button::Start), Some(Output::Done));
        // The symbols grid keeps it.
        k.button(Button::RightBumper);
        assert_eq!(k.press(), Some(Output::Enter));
    }

    #[test]
    fn hints_wrap_to_the_panel_in_order() {
        // One unit per character: "A Type" is 6 wide, the separator 3.
        let items = || ["A Type", "X Delete", "Y Space", "B Close"].map(String::from);
        let measure = |s: &str| s.chars().count() as f32;
        assert_eq!(
            wrap_hints(items(), 100.0, measure),
            ["A Type   X Delete   Y Space   B Close"]
        );
        assert_eq!(
            wrap_hints(items(), 20.0, measure),
            ["A Type   X Delete", "Y Space   B Close"]
        );
        // Narrower than every hint: each breaks at its space, a word a line, none lost.
        assert_eq!(
            wrap_hints(items(), 3.0, measure),
            ["A", "Type", "X", "Delete", "Y", "Space", "B", "Close"]
        );
    }

    #[test]
    fn a_hint_wider_than_the_panel_breaks_at_its_spaces() {
        let measure = |s: &str| s.chars().count() as f32;
        let items = ["A Type", "RT Fill in the next one", "B Close"].map(String::from);
        assert_eq!(
            wrap_hints(items, 10.0, measure),
            ["A Type", "RT Fill in", "the next", "one", "B Close"]
        );
        // One word wider than the panel keeps a line of its own (drawn smaller), nothing lost.
        let items = ["A Type", "X Unaussprechlich"].map(String::from);
        assert_eq!(
            wrap_hints(items, 10.0, measure),
            ["A Type", "X", "Unaussprechlich"]
        );
    }

    #[test]
    fn suggestions_take_rt_and_one_pick_per_push_of_the_right_stick() {
        let mut k = Keyboard::new(false, true);
        assert_eq!(k.button(Button::RightTrigger), Some(Output::Complete));
        assert_eq!(k.stick(0.3), None, "a lean isn't a push");
        assert_eq!(k.stick(0.9), Some(Output::Previous));
        assert_eq!(k.stick(1.0), None, "held, it picks once");
        assert_eq!(k.stick(0.4), None, "not yet back at rest");
        assert_eq!(k.stick(0.1), None);
        assert_eq!(k.stick(-0.9), Some(Output::Next));
    }
}
