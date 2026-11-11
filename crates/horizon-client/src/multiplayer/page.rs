//! A page a crate registers at `horizon_hooks::pages::Place::Title`, which the title screen's
//! Friends icon opens: a form drawn as Join a Friend and `ManageServerScreen` are. The title at
//! the top, then the rows from y 53 (a field's grey label, its 200-wide `EditBox` 11 below, the
//! next 41 lower; text 10 a line), and the page's buttons in a column under them from a quarter
//! of the height + 114, 24 apart, the one that leaves it last.
//!
//! The engine keeps each field's text while the form is open (the title screen's caret edits
//! it, as every field of its own) and tells the page each change before anything else reaches
//! it ([`Act::Edit`]), then Enter in a field ([`Act::Submit`]) and the presses. A page joins a
//! server with `horizon_hooks::pages::connect`.
use super::{ButtonSpec, Outcome, centered_x, half, quarter};
use crate::hud::{HudAtlas, HudBatch, rgba};
use horizon_hooks::pages::{self, Act, Page, Place, Row};
use std::cell::RefCell;
use std::time::Instant;

/// The first button's id; button `k` of the page is `FIRST + k`.
const FIRST: u8 = 100;
/// The text a line of the form wraps at.
const WIDTH: f32 = 300.0;

/// The page the title screen's Friends icon opens: the first registered there.
pub(super) fn title_page() -> Option<&'static dyn Page> {
    pages::engine::all()
        .iter()
        .copied()
        .find(|page| page.place() == Place::Title)
}

/// An open form: the page, and what the engine keeps while it is open.
#[derive(Clone)]
pub struct Form {
    page: &'static dyn Page,
    /// Each field's text, in the order the rows have them.
    texts: Vec<String>,
    /// The text the page last heard of each field.
    sent: RefCell<Vec<String>>,
    /// The focused field; `texts.len()` (or more) while a button has the focus.
    pub(super) focus: usize,
    /// Each field's box where the last draw put it.
    boxes: RefCell<Vec<[f32; 4]>>,
}

impl std::fmt::Debug for Form {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Form")
            .field("page", &self.page.id())
            .field("texts", &self.texts)
            .field("focus", &self.focus)
            .finish()
    }
}

/// A button of the form: its row (None: the one that leaves), label key, and whether it acts.
struct Button {
    row: Option<usize>,
    label: &'static str,
    active: bool,
    half: bool,
}

impl Form {
    /// The form of `page`, its fields holding the text its rows start with, the first focused.
    pub fn open(page: &'static dyn Page) -> Self {
        let mut rows = Vec::new();
        page.rows(&mut rows);
        let texts: Vec<String> = rows
            .iter()
            .filter_map(|row| match row {
                Row::Field { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect();
        Self {
            page,
            sent: RefCell::new(texts.clone()),
            boxes: RefCell::new(vec![[0.0; 4]; texts.len()]),
            texts,
            focus: 0,
        }
    }

    pub fn id(&self) -> &'static str {
        self.page.id()
    }

    /// The page's rows, after it heard every change to its fields.
    fn rows(&self) -> Vec<Row> {
        let mut rows = Vec::new();
        self.page.rows(&mut rows);
        let fields = field_rows(&rows);
        let mut sent = self.sent.borrow_mut();
        let mut told = false;
        for ((text, sent), row) in self.texts.iter().zip(sent.iter_mut()).zip(fields) {
            if text != sent {
                sent.clone_from(text);
                self.page.act(row, Act::Edit(text.clone()));
                told = true;
            }
        }
        if told {
            rows.clear();
            self.page.rows(&mut rows);
        }
        rows
    }

    fn buttons_of(rows: &[Row], back: &'static str) -> Vec<Button> {
        let mut buttons: Vec<Button> = rows
            .iter()
            .enumerate()
            .filter_map(|(r, row)| match row {
                Row::Button {
                    label,
                    active,
                    half,
                    ..
                } => Some(Button {
                    row: Some(r),
                    label,
                    active: *active,
                    half: *half,
                }),
                _ => None,
            })
            .collect();
        buttons.push(Button {
            row: None,
            label: back,
            active: true,
            half: false,
        });
        buttons
    }

    /// The id of the button that leaves: Cancel's or Done's, for the controller.
    fn back_id(&self) -> u8 {
        if self.page.back() == "gui.cancel" {
            57
        } else {
            50
        }
    }

    pub fn buttons(&self, w: f32, h: f32) -> Vec<ButtonSpec> {
        let rows = self.rows();
        let cx = half(w);
        let mut y = quarter(h) + 114.0;
        let mut beside = false;
        let mut out = Vec::new();
        for (k, b) in Self::buttons_of(&rows, self.page.back())
            .into_iter()
            .enumerate()
        {
            let id = match b.row {
                Some(_) => FIRST + k as u8,
                None => self.back_id(),
            };
            let label = crate::lang::get(b.label);
            let (x, bw) = match (b.half, beside) {
                (true, true) => {
                    y -= 24.0;
                    beside = false;
                    (cx + 2.0, 98.0)
                }
                (true, false) => {
                    beside = true;
                    (cx - 100.0, 98.0)
                }
                (false, _) => {
                    beside = false;
                    (cx - 100.0, 200.0)
                }
            };
            out.push((x, y, bw, label, b.active, id));
            y += 24.0;
        }
        out
    }

    pub fn draw(
        &self,
        atlas: &HudAtlas,
        batch: &mut HudBatch,
        caret: &mut crate::ui::Caret,
        focused_at: Instant,
    ) {
        let w = batch.width;
        let rows = self.rows();
        let title = crate::lang::get(self.page.title());
        let tw = HudBatch::text_width(atlas, &title);
        batch.text(atlas, &title, centered_x(w, tw), 20.0, u32::MAX, true);
        let mut y = 53.0;
        let mut field = 0;
        let mut boxes = self.boxes.borrow_mut();
        for (r, row) in rows.iter().enumerate() {
            let (text, color) = match row {
                Row::Field { label, max, .. } => {
                    let Some(value) = self.texts.get(field) else {
                        break;
                    };
                    super::Screen::field(
                        atlas,
                        batch,
                        caret,
                        focused_at,
                        &crate::lang::get(label),
                        value,
                        ("page.field", r),
                        *max,
                        (y, y + 11.0),
                        self.focus == field,
                    );
                    boxes[field] = [half(w) - 100.0, y + 11.0, 200.0, 20.0];
                    field += 1;
                    y += 41.0;
                    continue;
                }
                Row::Space(px) => {
                    y += f32::from(*px);
                    continue;
                }
                Row::Heading(key) => (crate::lang::get(key), u32::MAX),
                Row::Note(text) => (text.clone(), rgba(160, 160, 160, 255)),
                Row::Text { text, quiet, .. } => {
                    let grey = rgba(160, 160, 160, 255);
                    (text.clone(), if *quiet { grey } else { u32::MAX })
                }
                _ => continue,
            };
            for line in super::wrap(&text, WIDTH, atlas) {
                let lw = HudBatch::text_width(atlas, &line);
                batch.text(atlas, &line, centered_x(w, lw), y, color, true);
                y += 10.0;
            }
        }
    }

    pub fn narration(&self) -> crate::widgets::ScreenInfo {
        crate::widgets::ScreenInfo::new(self.page.id(), crate::lang::get(self.page.title()))
    }

    /// A press on button `id`, or on a field (`id` None).
    pub fn click(&mut self, id: Option<u8>, x: f32, y: f32) -> Outcome {
        match id {
            Some(id) if id == self.back_id() => Outcome::Back,
            Some(id) if id >= FIRST => {
                let rows = self.rows();
                let buttons = Self::buttons_of(&rows, self.page.back());
                match buttons.get(usize::from(id - FIRST)) {
                    Some(Button {
                        row: Some(r),
                        active: true,
                        ..
                    }) => self.act(*r, Act::Press),
                    _ => Outcome::None,
                }
            }
            Some(_) => Outcome::None,
            None => {
                let hit = self
                    .boxes
                    .borrow()
                    .iter()
                    .position(|&[bx, by, bw, bh]| crate::ui::hovered((x, y), bx, by, bw, bh));
                if let Some(field) = hit {
                    self.focus = field;
                }
                Outcome::None
            }
        }
    }

    /// Enter: the focused field submits.
    pub fn confirm(&self) -> Outcome {
        let rows = self.rows();
        match field_rows(&rows).nth(self.focus) {
            Some(r) => self.act(r, Act::Submit),
            None => Outcome::None,
        }
    }

    /// The page acts; a server it asked to join is joined.
    fn act(&self, row: usize, act: Act) -> Outcome {
        self.page.act(row, act);
        match pages::engine::take_connect() {
            Some(address) => Outcome::Connect {
                address,
                lan: false,
            },
            None => Outcome::None,
        }
    }

    pub fn focused_box(&self) -> Option<[f32; 4]> {
        self.boxes.borrow().get(self.focus).copied()
    }

    /// The focused field, its id and maximum length.
    pub fn focused_text(&mut self) -> Option<(crate::clipboard::FieldId, &mut String, usize)> {
        let mut rows = Vec::new();
        self.page.rows(&mut rows);
        let (r, max) = rows
            .iter()
            .enumerate()
            .filter_map(|(r, row)| match row {
                Row::Field { max, .. } => Some((r, *max)),
                _ => None,
            })
            .nth(self.focus)?;
        let text = self.texts.get_mut(self.focus)?;
        Some((("page.field", r), text, max))
    }

    /// Tab through the fields, then the enabled `buttons`, then round to the first field.
    pub fn tab_dir(&mut self, reverse: bool, buttons: &[u8], button: &mut Option<u8>) {
        let fields = self.texts.len();
        let stops = fields + buttons.len();
        if stops == 0 {
            return;
        }
        let at = match *button {
            Some(id) => buttons.iter().position(|&b| b == id).map(|i| fields + i),
            None => Some(self.focus.min(fields.saturating_sub(1))),
        }
        .unwrap_or(0);
        let next = if reverse {
            (at + stops - 1) % stops
        } else {
            (at + 1) % stops
        };
        if next < fields {
            self.focus = next;
            *button = None;
        } else {
            self.focus = fields;
            *button = Some(buttons[next - fields]);
        }
    }
}

/// The rows that are fields, in order.
fn field_rows(rows: &[Row]) -> impl Iterator<Item = usize> + '_ {
    rows.iter()
        .enumerate()
        .filter(|(_, row)| matches!(row, Row::Field { .. }))
        .map(|(r, _)| r)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// A join form: a code and a relay, Join while the code has 4 characters or more.
    struct Join {
        heard: Mutex<Vec<(usize, Act)>>,
        code: Mutex<String>,
    }

    impl Page for Join {
        fn id(&self) -> &'static str {
            "test.join"
        }
        fn title(&self) -> &'static str {
            "horizon.multiplayer.join_friend"
        }
        fn place(&self) -> Place {
            Place::Title
        }
        fn rows(&self, rows: &mut Vec<Row>) {
            let code = self.code.lock().unwrap().clone();
            rows.push(Row::field("horizon.multiplayer.invite_code", code.clone()).max(8));
            rows.push(Row::field(
                "horizon.multiplayer.relay",
                "relay.example:25599",
            ));
            rows.push(Row::button("horizon.multiplayer.join").active(code.len() >= 4));
        }
        fn act(&self, row: usize, act: Act) {
            if let (0, Act::Edit(text)) = (row, &act) {
                text.clone_into(&mut self.code.lock().unwrap());
            }
            if matches!((row, &act), (2, Act::Press) | (0, Act::Submit)) {
                pages::connect(format!("friend:{}", self.code.lock().unwrap()));
            }
            self.heard.lock().unwrap().push((row, act));
        }
        fn back(&self) -> &'static str {
            "gui.cancel"
        }
    }

    fn join() -> &'static Join {
        Box::leak(Box::new(Join {
            heard: Mutex::new(Vec::new()),
            code: Mutex::new(String::new()),
        }))
    }

    #[test]
    fn a_form_lays_out_as_join_a_friend() {
        let form = Form::open(join());
        let buttons = form.buttons(427.0, 240.0);
        // Join at a quarter of the height + 114, Cancel 24 below, both 200 wide and centred.
        assert_eq!(
            buttons
                .iter()
                .map(|(x, y, w, _, on, id)| (*x, *y, *w, *on, *id))
                .collect::<Vec<_>>(),
            [
                (113.0, 174.0, 200.0, false, FIRST),
                (113.0, 198.0, 200.0, true, 57)
            ]
        );
        assert_eq!(form.texts, ["", "relay.example:25599"]);
    }

    #[test]
    fn typing_reaches_the_page_before_a_press_and_enter_submits() {
        let page = join();
        let mut form = Form::open(page);
        let (id, text, max) = form.focused_text().unwrap();
        assert_eq!((id, max), (("page.field", 0), 8));
        text.push_str("AB");
        // Join is greyed: the page heard "AB" before the buttons were laid out.
        assert!(!form.buttons(427.0, 240.0)[0].4);
        form.focused_text().unwrap().1.push_str("CD");
        assert!(form.buttons(427.0, 240.0)[0].4);
        match form.click(Some(FIRST), 0.0, 0.0) {
            Outcome::Connect { address, lan } => {
                assert_eq!((address.as_str(), lan), ("friend:ABCD", false))
            }
            _ => panic!("Join did not connect"),
        }
        match form.confirm() {
            Outcome::Connect { address, .. } => assert_eq!(address, "friend:ABCD"),
            _ => panic!("Enter did not submit"),
        }
        let heard = page.heard.lock().unwrap();
        assert_eq!(
            *heard,
            [
                (0, Act::Edit("AB".into())),
                (0, Act::Edit("ABCD".into())),
                (2, Act::Press),
                (0, Act::Submit)
            ]
        );
        assert!(matches!(form.click(Some(57), 0.0, 0.0), Outcome::Back));
    }

    #[test]
    fn tab_goes_through_the_fields_then_the_buttons() {
        let mut form = Form::open(join());
        let mut button = None;
        form.tab_dir(false, &[57], &mut button);
        assert_eq!((form.focus, button), (1, None));
        form.tab_dir(false, &[57], &mut button);
        assert_eq!((form.focus, button), (2, Some(57)));
        form.tab_dir(false, &[57], &mut button);
        assert_eq!((form.focus, button), (0, None));
        form.tab_dir(true, &[57], &mut button);
        assert_eq!(button, Some(57));
        assert!(form.focused_text().is_none());
    }
}
