use super::*;
use crate::options_ui::Event;
use winit::keyboard::KeyCode as K;

const SIZE: (f32, f32) = (854.0, 480.0);
const SHORT: (f32, f32) = (480.0, 240.0);
const CHANGED: Output = Output::Event(Event::Changed);

/// A screen with what Extras doesn't have: two pages, a slider, a value with descriptions of
/// its own, and a row a wheel steps.
#[derive(Default)]
struct Probe {
    slider: f32,
    choice: usize,
    pressed: Vec<(u32, bool)>,
    /// How many times its rows were built.
    built: std::cell::Cell<usize>,
}
const CHOICES: [(&str, &str); 2] = [("Red", "Paints it red."), ("Blue", "Paints it blue.")];
const SLIDER: u32 = 0;
const CHOICE: u32 = 1;
const FIRST_FILLER: u32 = 10;
const FILLERS: u32 = 40;
const GREY: u32 = 100;
const WHY: &str = "Not here.";
impl Screen for Probe {
    type Page = u8;
    type Row = u32;
    fn title(&self) -> &'static str {
        "Probe"
    }
    /// Its rows read the field of view, besides its own state.
    fn stamp(&self, s: &Settings) -> u64 {
        stamp_of(&s.fov)
    }
    fn sheets(&self, s: &Settings) -> Vec<Sheet<u8, u32>> {
        self.built.set(self.built.get() + 1);
        let (value, chosen) = CHOICES[self.choice];
        let mut rows = vec![
            Row::new(
                SLIDER,
                "Level".to_owned(),
                Value::Slider {
                    position: self.slider,
                    steps: Some(10),
                    text: s.fov.to_string(),
                },
            ),
            Row {
                chosen: Some(chosen.to_owned()),
                ..Row::new(CHOICE, "Paint".to_owned(), Value::Choice(value.to_owned()))
                    .about("Picks a colour.".to_owned())
            },
            Row::new(GREY, "Greyed".to_owned(), Value::Press)
                .about("Does a thing.".to_owned())
                .blocked(Some(WHY.to_owned())),
        ];
        rows.extend((0..FILLERS).map(|i| {
            Row::new(FIRST_FILLER + i, format!("Filler {i}"), Value::Press)
                .about("Does nothing.".to_owned())
        }));
        let elsewhere = vec![Row::new(99, "Elsewhere".to_owned(), Value::Press)];
        vec![
            Sheet::new(
                0,
                "One".to_owned(),
                "The first.".to_owned(),
                vec![Group { name: "G", rows }],
            ),
            Sheet::new(
                1,
                "Two".to_owned(),
                "The second.".to_owned(),
                vec![Group {
                    name: "H",
                    rows: elsewhere,
                }],
            ),
        ]
    }
    fn press(&mut self, s: &mut Settings, row: u32, back: bool) -> Vec<Output> {
        self.pressed.push((row, back));
        match row {
            CHOICE => self.choice = (self.choice + 1) % CHOICES.len(),
            FIRST_FILLER => s.fov += 1.0,
            _ => {}
        }
        vec![CHANGED]
    }
    fn undoes(&self) -> bool {
        true
    }
    fn undo(&mut self, s: &mut Settings, before: &Settings) -> Vec<Output> {
        s.fov = before.fov;
        Vec::new()
    }
    fn slide(&mut self, _: &mut Settings, _: u32, position: f32) {
        self.slider = position;
    }
    fn wheel_step(&mut self, _: &mut Settings, row: u32, _: bool) -> Option<Vec<Output>> {
        (row == CHOICE).then(|| {
            self.choice = (self.choice + 1) % CHOICES.len();
            vec![CHANGED]
        })
    }
}

fn middle_of(shell: &mut Shell<Probe>, row: u32, size: (f32, f32)) -> (f32, f32) {
    shell.point_at(&Settings::default(), size, row)
}

fn atlas() -> HudAtlas {
    HudAtlas::build(std::path::Path::new("/nonexistent/horizon-test-assets")).unwrap()
}

/// What one frame asks of a screen: a draw, then the stick's scroll, an arrow key and the wheel
/// away from any row, at the size it was drawn at.
fn frame(shell: &mut Shell<Probe>, s: &mut Settings, atlas: &HudAtlas) -> (f32, f32) {
    let mut batch = HudBatch::new([1708, 960]);
    shell.draw(s, &mut batch, atlas, (0.0, 0.0), false, None);
    let size = (batch.width, batch.height);
    shell.scroll(s, size, -1.0);
    shell.key(s, K::ArrowDown, None, false, size);
    shell.wheel(s, size, (0.0, 0.0), 1.0);
    size
}

#[test]
fn rows_are_built_once_and_again_only_when_what_they_read_changes() {
    let atlas = atlas();
    let mut s = Settings::default();
    let mut shell = Shell::<Probe>::default();
    let built = |shell: &Shell<Probe>| shell.screen.built.get();
    let reads = |shell: &mut Shell<Probe>, s: &Settings, size, end: &str| {
        let view = shell.view(s, size);
        let mut labels = view.rows.iter().map(|r| r.label(r.value_text().as_deref()));
        labels.any(|label| label.ends_with(end))
    };
    // Each of the four built them before: now the first frame builds them once, the next none.
    let size = frame(&mut shell, &mut s, &atlas);
    assert_eq!(built(&shell), 1);
    frame(&mut shell, &mut s, &atlas);
    assert_eq!(built(&shell), 1);
    // A press changes the screen's own state: its row reads the new value.
    let at = middle_of(&mut shell, CHOICE, size);
    shell.click(&mut s, at, size, false, false, false);
    assert!(reads(&mut shell, &s, size, "Blue"));
    // A setting changed elsewhere (a key bound, a command) shows on the next frame.
    s.fov = 90.0;
    frame(&mut shell, &mut s, &atlas);
    assert!(reads(&mut shell, &s, size, "90"));
    let before = built(&shell);
    frame(&mut shell, &mut s, &atlas);
    assert_eq!(built(&shell), before);
    // Searching, and another size, build them again.
    shell.type_text("filler");
    shell.view(&s, size);
    assert_eq!(built(&shell), before + 1);
    shell.view(&s, SHORT);
    assert_eq!(built(&shell), before + 2);
    // So does another language, and coming back from it.
    lang::scoped(lang::Language::load(&[], "de_de"), || shell.view(&s, SHORT));
    assert_eq!(built(&shell), before + 3);
    shell.view(&s, SHORT);
    assert_eq!(built(&shell), before + 4);
}

#[test]
fn a_row_the_list_hides_part_of_scrolls_into_view_when_focused_and_then_takes_a_press() {
    let mut s = Settings::default();
    let mut shell = Shell::<Probe>::default();
    let cols = Columns::of(SHORT.0, SHORT.1);
    // A filler far down, scrolled to just over the bottom: its middle is under the list's edge.
    let cut = FIRST_FILLER + 20;
    let view = shell.view(&s, SHORT);
    let (_, y) = shell.placed(&view).find(|(r, _)| r.id == cut).unwrap();
    shell.scroll.amount = y - cols.bottom + style::ROW / 2.0 - 1.0;
    let rect = cols.row(y - shell.scroll.amount);
    assert!(
        rect[1] + rect[3] / 2.0 >= cols.bottom,
        "the row's middle is hidden"
    );
    let moved = Hosted::reveal(&mut shell, &s, SHORT, rect).unwrap();
    let shown = [rect[0], rect[1] + moved, rect[2], rect[3]];
    assert!(moved < 0.0 && shown[1] + shown[3] <= cols.bottom);
    // All in view, it stays put.
    assert_eq!(Hosted::reveal(&mut shell, &s, SHORT, shown), None);
    let middle = (shown[0] + 4.0, shown[1] + shown[3] / 2.0);
    assert_eq!(
        shell.click(&mut s, middle, SHORT, false, false, false),
        vec![CLICK, CHANGED]
    );
    assert_eq!(shell.screen.pressed, [(cut, false)]);
}

#[test]
fn undo_appears_after_a_change_and_puts_back_what_the_screen_opened_with() {
    let mut s = Settings::default();
    let mut shell = Shell::<Probe>::default();
    let undo = Columns::of(SIZE.0, SIZE.1).button(1);
    let on_undo = (undo[0] + 2.0, undo[1] + 2.0);
    // Nothing changed: there is no Undo to press.
    assert!(
        shell
            .click(&mut s, on_undo, SIZE, false, false, false)
            .is_empty()
    );
    let at = middle_of(&mut shell, FIRST_FILLER, SIZE);
    shell.click(&mut s, at, SIZE, false, false, false);
    assert_eq!(s.fov, 71.0);
    // Another option changed meanwhile isn't the screen's to put back.
    s.chat_opacity = 0.25;
    assert_eq!(
        shell.click(&mut s, on_undo, SIZE, false, false, false),
        vec![CLICK, CHANGED]
    );
    assert_eq!((s.fov, s.chat_opacity), (70.0, 0.25));
    assert!(
        shell
            .click(&mut s, on_undo, SIZE, false, false, false)
            .is_empty()
    );
    // From the keyboard: Tab reaches it, Enter undoes and leaves the focus on Done.
    shell.click(&mut s, at, SIZE, false, false, false);
    let reached = (0..100).any(|_| {
        shell.key(&mut s, K::Tab, None, false, SIZE);
        shell.focus == Some(Target::Undo)
    });
    assert!(reached);
    assert_eq!(
        shell.key(&mut s, K::Enter, None, false, SIZE),
        vec![CLICK, CHANGED]
    );
    assert_eq!(s.fov, 70.0);
    assert_eq!(shell.focus, Some(Target::Done));
}

#[test]
fn the_strip_says_why_a_greyed_row_cant_be_used() {
    let atlas = atlas();
    let s = Settings::default();
    let mut shell = Shell::<Probe>::default();
    let mut batch = HudBatch::new([1708, 960]);
    let size = (batch.width, batch.height);
    let strip = |batch: &mut HudBatch, shell: &mut Shell<Probe>, at| {
        shell.draw(&s, batch, &atlas, at, false, None);
        crate::widgets::end_frame(crate::widgets::generation() + 1);
        let dock = Columns::of(size.0, size.1).dock;
        let labelled = crate::widgets::last_labelled();
        let found = labelled.iter().find(|w| [w.x, w.y, w.w, w.h] == dock);
        found.map(|w| w.label.clone())
    };
    let grey = middle_of(&mut shell, GREY, size);
    assert_eq!(strip(&mut batch, &mut shell, grey).as_deref(), Some(WHY));
    let choice = middle_of(&mut shell, CHOICE, size);
    assert_eq!(
        strip(&mut batch, &mut shell, choice).as_deref(),
        Some(CHOICES[0].1)
    );
}

#[test]
fn growing_the_window_keeps_the_scroll_within_the_list() {
    let s = Settings::default();
    let mut shell = Shell::<Probe>::default();
    shell.scroll(&s, SIZE, -1000.0);
    assert!(shell.scroll.amount > 0.0);
    let tall = (SIZE.0, 4000.0);
    let view = shell.view(&s, tall);
    assert_eq!(shell.scroll.amount, 0.0);
    let first = shell.placed(&view).next().map(|(_, y)| y).unwrap();
    assert!(first >= view.cols.top);
}

#[test]
fn one_scroll_path_moves_the_list_for_wheel_and_stick_alike() {
    let mut s = Settings::default();
    let mut shell = Shell::<Probe>::default();
    let rate = SCROLL_RATE;
    // The wheel away from any row, and `scroll`, which a stick and a D-pad use.
    shell.wheel(&mut s, SIZE, (0.0, 0.0), -1.0);
    assert_eq!(shell.scroll.amount, rate);
    shell.scroll(&s, SIZE, -2.0);
    assert_eq!(shell.scroll.amount, 3.0 * rate);
    shell.scroll(&s, SIZE, 100.0);
    assert_eq!(shell.scroll.amount, 0.0);
}

#[test]
fn the_wheel_steps_a_row_only_with_the_extra_on_and_only_where_the_screen_says() {
    let mut s = Settings::default();
    let mut shell = Shell::<Probe>::default();
    let over_choice = middle_of(&mut shell, CHOICE, SIZE);
    shell.wheel(&mut s, SIZE, over_choice, -1.0);
    assert_eq!(shell.screen.choice, 0);
    assert_eq!(shell.scroll.amount, SCROLL_RATE);
    s.extras.wheel_cycles_settings = true;
    shell.scroll.amount = 0.0;
    let outputs = shell.wheel(&mut s, SIZE, over_choice, -1.0);
    assert_eq!(outputs, vec![CHANGED]);
    assert_eq!(shell.screen.choice, 1);
    assert_eq!(shell.scroll.amount, 0.0);
    // A row the screen doesn't step scrolls, the greyed one too.
    let over_grey = middle_of(&mut shell, GREY, SIZE);
    assert!(shell.wheel(&mut s, SIZE, over_grey, -1.0).is_empty());
    assert_eq!(shell.scroll.amount, SCROLL_RATE);
}

#[test]
fn a_row_says_what_its_chosen_value_does_and_is_found_by_what_it_does() {
    let mut s = Settings::default();
    let mut shell = Shell::<Probe>::default();
    let about = |shell: &mut Shell<Probe>, s: &Settings| {
        let view = shell.view(s, SIZE);
        let row = view.rows.iter().find(|r| r.id == CHOICE).unwrap();
        row.described().to_owned()
    };
    assert_eq!(about(&mut shell, &s), "Paints it red.");
    let at = middle_of(&mut shell, CHOICE, SIZE);
    shell.click(&mut s, at, SIZE, false, false, false);
    assert_eq!(about(&mut shell, &s), "Paints it blue.");
    // The search reads the row's own description, on every page.
    shell.type_text("colour");
    let view = shell.view(&s, SIZE);
    let found: Vec<u32> = view.rows.iter().map(|r| r.id).collect();
    assert!(found.contains(&CHOICE) && !found.contains(&99));
    shell.search.set_value("elsewhere");
    shell.focus = None;
    let view = shell.view(&s, SIZE);
    assert_eq!(view.rows.iter().map(|r| r.id).collect::<Vec<_>>(), [99]);
}

#[test]
fn a_press_on_the_track_slides_and_on_the_name_only_focuses() {
    let mut s = Settings::default();
    let mut shell = Shell::<Probe>::default();
    let at = middle_of(&mut shell, SLIDER, SIZE);
    assert!(
        shell
            .click(&mut s, at, SIZE, false, false, false)
            .is_empty()
    );
    assert_eq!(shell.screen.slider, 0.0);
    let (track, width) = Columns::of(SIZE.0, SIZE.1).track();
    let mid = (track + width / 2.0, at.1);
    assert_eq!(
        shell.click(&mut s, mid, SIZE, false, false, false),
        vec![DRAGGED]
    );
    assert_eq!(shell.screen.slider, 0.5);
    let end = (track + width, at.1);
    assert_eq!(
        shell.click(&mut s, end, SIZE, true, false, false),
        vec![DRAGGED]
    );
    assert_eq!(shell.screen.slider, 1.0);
    assert!(shell.release().is_some());
    assert!(shell.release().is_none());
}

#[test]
fn the_keyboard_walks_the_live_rows_past_the_view_and_presses_them() {
    let mut s = Settings::default();
    let mut shell = Shell::<Probe>::default();
    let mut key = |shell: &mut Shell<Probe>, k| shell.key(&mut s, k, None, false, SHORT);
    // Tab: the search, the two pages, then the first row.
    for _ in 0..4 {
        key(&mut shell, K::Tab);
    }
    assert_eq!(shell.focus, Some(Target::Row(SLIDER)));
    // A slider Tab reaches changes with the arrows, in its steps.
    key(&mut shell, K::ArrowRight);
    assert!((shell.screen.slider - 0.1).abs() < f32::EPSILON);
    // The arrows go on past the greyed row and rows out of view, which scroll in.
    key(&mut shell, K::ArrowDown);
    assert_eq!(shell.focus, Some(Target::Row(CHOICE)));
    key(&mut shell, K::ArrowDown);
    assert_eq!(shell.focus, Some(Target::Row(FIRST_FILLER)));
    for _ in 0..30 {
        key(&mut shell, K::ArrowDown);
    }
    assert_eq!(shell.focus, Some(Target::Row(FIRST_FILLER + 30)));
    assert!(shell.scroll.amount > 0.0);
    assert_eq!(key(&mut shell, K::Enter), vec![CLICK, CHANGED]);
    assert_eq!(shell.screen.pressed, [(FIRST_FILLER + 30, false)]);
    assert_eq!(key(&mut shell, K::Escape), vec![Output::Done]);
}

#[test]
fn left_and_right_step_a_focused_choice_and_leave_other_rows() {
    let mut s = Settings::default();
    let mut shell = Shell::<Probe>::default();
    let mut key = |shell: &mut Shell<Probe>, k| shell.key(&mut s, k, None, false, SIZE);
    shell.set_focus(Some(Target::Row(CHOICE)));
    // Right is a press, Left a Shift press: the screen's own way back, and the focus stays.
    assert_eq!(key(&mut shell, K::ArrowRight), vec![CLICK, CHANGED]);
    assert_eq!(key(&mut shell, K::ArrowLeft), vec![CLICK, CHANGED]);
    assert_eq!(shell.screen.pressed, [(CHOICE, false), (CHOICE, true)]);
    assert_eq!(shell.focus, Some(Target::Row(CHOICE)));
    assert!(shell.changed);
    // A row that isn't a choice: Left goes to the page list, as before.
    shell.set_focus(Some(Target::Row(FIRST_FILLER)));
    assert!(key(&mut shell, K::ArrowLeft).is_empty());
    assert!(matches!(shell.focus, Some(Target::Page(_))));
    assert_eq!(shell.screen.pressed.len(), 2);
}

#[test]
fn a_drawn_choice_tells_the_pad_its_arrows_step_it() {
    let atlas = atlas();
    let s = Settings::default();
    let mut shell = Shell::<Probe>::default();
    let mut batch = HudBatch::new([1708, 960]);
    shell.draw(&s, &mut batch, &atlas, (0.0, 0.0), false, None);
    crate::widgets::end_frame(u64::MAX);
    let view = shell.view(&s, (batch.width, batch.height));
    let rect_of = |id| {
        let mut placed = shell.placed(&view);
        view.cols.row(placed.find(|(r, _)| r.id == id).unwrap().1)
    };
    assert!(crate::widgets::last_steps_value(rect_of(CHOICE)));
    assert!(!crate::widgets::last_steps_value(rect_of(SLIDER)));
    assert!(!crate::widgets::last_steps_value(rect_of(FIRST_FILLER)));
}

/// A controller presses with a click, then moves its own focus: only keys show this one.
#[test]
fn leaving_the_search_field_gives_the_keys_back() {
    let mut s = Settings::default();
    let mut shell = Shell::<Probe>::default();
    let view = shell.view(&s, SIZE);
    let [x, y, w, h] = view.cols.search();
    shell.click(
        &mut s,
        (x + w / 2.0, y + h / 2.0),
        SIZE,
        false,
        false,
        false,
    );
    assert!(shell.typing());
    Hosted::blur_search(&mut shell);
    assert!(!shell.typing());
    // Another focus isn't dropped.
    let at = middle_of(&mut shell, FIRST_FILLER, SIZE);
    shell.click(&mut s, at, SIZE, false, false, false);
    Hosted::blur_search(&mut shell);
    shell.key(&mut s, K::ArrowDown, None, false, SIZE);
    assert!(shell.shows_focus(Target::Row(FIRST_FILLER + 1)));
}

#[test]
fn a_key_that_moves_nothing_shows_no_focus() {
    let mut s = Settings::default();
    let mut shell = Shell::<Probe>::default();
    let at = middle_of(&mut shell, FIRST_FILLER, SIZE);
    shell.click(&mut s, at, SIZE, false, false, false);
    shell.key(&mut s, K::ShiftLeft, None, false, SIZE);
    assert!(!shell.shows_focus(Target::Row(FIRST_FILLER)));
}

#[test]
fn a_cut_description_says_so() {
    assert_eq!(cut_off("off, it never "), "off, it never…");
}

#[test]
fn a_click_leaves_no_focus_showing_until_a_key_moves_it() {
    let mut s = Settings::default();
    let mut shell = Shell::<Probe>::default();
    let at = middle_of(&mut shell, FIRST_FILLER, SIZE);
    shell.click(&mut s, at, SIZE, false, false, false);
    assert!(!shell.shows_focus(Target::Row(FIRST_FILLER)));
    // The next key goes on from the row clicked, and shows where it went.
    shell.key(&mut s, K::ArrowDown, None, false, SIZE);
    assert!(shell.shows_focus(Target::Row(FIRST_FILLER + 1)));
}

#[test]
fn a_taller_strip_takes_from_the_list_as_far_as_the_list_keeps_its_height() {
    let (wide, base) = (
        Columns::of(480.0, 270.0).with_note_lines(4),
        Columns::of(480.0, 270.0),
    );
    assert_eq!(wide.note_lines, 4);
    assert_eq!(wide.bottom, base.bottom - 2.0 * NOTE_LINE_H);
    assert_eq!(wide.dock[1] + wide.dock[3], base.dock[1] + base.dock[3]);
    assert_eq!(wide.button(0), base.button(0));
    // A 720p window at scale 4 has room for one more line, not two.
    let short = Columns::of(320.0, 180.0).with_note_lines(4);
    assert_eq!(short.note_lines, 3);
    assert!(short.bottom - short.top >= MIN_LIST_H);
}

#[test]
fn shift_presses_a_row_backwards() {
    let mut s = Settings::default();
    let mut shell = Shell::<Probe>::default();
    let at = middle_of(&mut shell, FIRST_FILLER, SIZE);
    shell.click(&mut s, at, SIZE, false, false, true);
    // The right button is not a press.
    shell.click(&mut s, at, SIZE, false, true, false);
    assert_eq!(shell.screen.pressed, [(FIRST_FILLER, true)]);
}

#[test]
fn a_click_on_a_greyed_row_does_nothing_and_done_leaves() {
    let mut s = Settings::default();
    let mut shell = Shell::<Probe>::default();
    let at = middle_of(&mut shell, GREY, SIZE);
    assert!(
        shell
            .click(&mut s, at, SIZE, false, false, false)
            .is_empty()
    );
    assert!(shell.screen.pressed.is_empty());
    let [x, y, w, h] = Columns::of(SIZE.0, SIZE.1).button(0);
    let at = (x + w / 2.0, y + h / 2.0);
    assert_eq!(
        shell.click(&mut s, at, SIZE, false, false, false),
        vec![CLICK, Output::Done]
    );
}

#[test]
fn a_page_picked_shows_alone() {
    let mut s = Settings::default();
    let mut shell = Shell::<Probe>::default();
    let [x, y, w, h] = Columns::of(SIZE.0, SIZE.1).page(1);
    let at = (x + w / 2.0, y + h / 2.0);
    assert_eq!(
        shell.click(&mut s, at, SIZE, false, false, false),
        vec![CLICK]
    );
    let view = shell.view(&s, SIZE);
    assert_eq!(view.rows.iter().map(|r| r.id).collect::<Vec<_>>(), [99]);
}

/// A screen with more pages than a short view's page list has room for: a page of its own,
/// then a section of `PAGES` under a line, one more page out of the list that the section's
/// first page links to, and a change that waits for Apply.
#[derive(Default)]
struct Pages {
    pending: bool,
    applied: usize,
}
const PAGES: u8 = 12;
const HOME: u8 = 0;
const DEEP: u8 = 100;
const LINK: u32 = 1000;
const WAITS: u32 = 1001;
const SECTION: &str = "Section";
impl Screen for Pages {
    type Page = u8;
    type Row = u32;
    fn title(&self) -> &'static str {
        "Pages"
    }
    fn stamp(&self, _: &Settings) -> u64 {
        stamp_of(&self.pending)
    }
    fn sheets(&self, _: &Settings) -> Vec<Sheet<u8, u32>> {
        let page = |id: u8, name: String, rows| {
            Sheet::new(id, name, String::new(), vec![Group { name: "G", rows }])
        };
        let change = Row::new(WAITS, "Change".to_owned(), Value::Press);
        let mut sheets = vec![page(HOME, "Home".to_owned(), vec![change])];
        for i in 1..=PAGES {
            let mut rows = vec![Row::new(u32::from(i), format!("Row {i}"), Value::Press)];
            if i == 1 {
                rows.push(Row::new(LINK, "Deeper...".to_owned(), Value::Press));
            }
            let mut sheet = page(i, format!("Page {i}"), rows);
            sheet.heading = (i == 1).then(|| SECTION.to_owned());
            sheets.push(sheet);
        }
        let deep = Row::new(u32::from(DEEP), "Deep".to_owned(), Value::Press);
        let mut deeper = page(DEEP, "Deeper".to_owned(), vec![deep]);
        deeper.listed = false;
        sheets.push(deeper);
        sheets
    }
    fn press(&mut self, _: &mut Settings, row: u32, _: bool) -> Vec<Output> {
        self.pending |= row == WAITS;
        vec![CHANGED]
    }
    fn link(&self, row: u32) -> Option<u8> {
        (row == LINK).then_some(DEEP)
    }
    fn undoes(&self) -> bool {
        true
    }
    fn pending(&self, _: &Settings) -> bool {
        self.pending
    }
    fn apply(&mut self, _: &mut Settings) -> Vec<Output> {
        self.pending = false;
        self.applied += 1;
        vec![CHANGED]
    }
}

/// A window of 854x960 (427x480 GUI px at its automatic scale) and one of 854x480 (427x240:
/// the page list has room for six places).
const TALL: [u32; 2] = [854, 960];
const SHORT_WINDOW: [u32; 2] = [854, 480];

/// One frame of `shell` in a window of `window`; the GUI size it was drawn at.
fn drawn<S: Screen + Default>(shell: &mut Shell<S>, s: &Settings, window: [u32; 2]) -> (f32, f32) {
    let mut batch = HudBatch::new(window);
    shell.draw(s, &mut batch, &atlas(), (0.0, 0.0), false, None);
    crate::widgets::end_frame(crate::widgets::generation() + 1);
    (batch.width, batch.height)
}

fn middle([x, y, w, h]: [f32; 4]) -> (f32, f32) {
    (x + w / 2.0, y + h / 2.0)
}

#[test]
fn pages_that_fit_are_listed_and_more_collapse_into_one_row_leaving_none_out() {
    let s = Settings::default();
    let mut shell = Shell::<Pages>::default();
    // Tall enough: each page under the last, the deeper page left out as it is in no list.
    drawn(&mut shell, &s, TALL);
    let tall = shell.page_places();
    let pages: Vec<u8> = tall.iter().map(|p| p.0).collect();
    assert_eq!(pages, (HOME..=PAGES).collect::<Vec<_>>());
    assert!(tall.iter().all(|p| p.2 == Part::Tab));
    assert!(tall.windows(2).all(|p| p[1].1[1] > p[0].1[1]));
    assert_eq!(shell.strip_ends(), None);
    // Short: the first page, then the section in one row, its first page between nothing and
    // its next. Every place is inside the column, the row's on one line under the section's.
    let size = drawn(&mut shell, &s, SHORT_WINDOW);
    let short = shell.page_places();
    let parts: Vec<(u8, Part)> = short.iter().map(|p| (p.0, p.2)).collect();
    assert_eq!(
        parts,
        [(HOME, Part::Tab), (1, Part::Shown), (2, Part::Next)]
    );
    assert_eq!(shell.strip_ends(), Some((false, true)));
    let cols = Columns::of(size.0, size.1);
    for (_, [x, _, w, _], _) in &short {
        assert!(*x >= cols.side_x && x + w <= cols.side_x + style::CONTROL_W);
    }
    assert_eq!(short[1].1[1], short[2].1[1]);
    assert_eq!(short[1].1[1], cols.page(2)[1]);
    // The line naming the section stays over it.
    let labelled = crate::widgets::last_labelled();
    let line = labelled.iter().find(|w| w.label == SECTION).unwrap();
    assert!(line.y > cols.page(1)[1] && line.y < cols.page(2)[1]);
}

#[test]
fn the_collapsed_rows_arrows_walk_every_page_and_its_ends_go_no_further() {
    let mut s = Settings::default();
    let mut shell = Shell::<Pages>::default();
    shell.show(1);
    let mut seen = vec![1];
    let size = loop {
        let size = drawn(&mut shell, &s, SHORT_WINDOW);
        let places = shell.page_places();
        let Some(&(next, rect, _)) = places.iter().find(|p| p.2 == Part::Next) else {
            break size;
        };
        assert_eq!(
            shell.click(&mut s, middle(rect), size, false, false, false),
            [CLICK]
        );
        assert_eq!(shell.open_page(), Some(next));
        seen.push(next);
    };
    assert_eq!(seen, (1..=PAGES).collect::<Vec<_>>());
    assert_eq!(shell.strip_ends(), Some((true, false)));
    // At the last page, the right of the row picks nothing: its arrow is dimmed.
    let cols = Columns::of(size.0, size.1);
    let [x, y, w, h] = cols.page(2);
    assert!(
        shell
            .click(
                &mut s,
                (x + w - 2.0, y + h / 2.0),
                size,
                false,
                false,
                false
            )
            .is_empty()
    );
    assert_eq!(shell.open_page(), Some(PAGES));
    // The page before it from the keyboard: Enter on its arrow opens it, and the focus follows
    // it to the middle of the row.
    let before = shell
        .page_places()
        .iter()
        .find(|p| p.0 == PAGES - 1)
        .unwrap()
        .2;
    assert_eq!(before, Part::Prev);
    shell.set_focus(Some(Target::Page(PAGES - 1)));
    assert_eq!(shell.key(&mut s, K::Enter, None, false, size), [CLICK]);
    drawn(&mut shell, &s, SHORT_WINDOW);
    let now = shell
        .page_places()
        .iter()
        .find(|p| p.0 == PAGES - 1)
        .unwrap()
        .2;
    assert_eq!(now, Part::Shown);
}

#[test]
fn while_searching_the_collapsed_rows_arrows_skip_pages_without_matches() {
    let mut s = Settings::default();
    let mut shell = Shell::<Pages>::default();
    // Rows 1, 10, 11 and 12: none before the first. The first page stays, as it has no match.
    shell.set_search("row 1");
    let size = drawn(&mut shell, &s, SHORT_WINDOW);
    let pages = |shell: &Shell<Pages>| shell.page_places().iter().map(|p| p.0).collect::<Vec<_>>();
    assert_eq!(pages(&shell), [HOME, 1, 10]);
    assert_eq!(shell.strip_ends(), Some((false, true)));
    // Its arrow goes to the next match, which has the first one before it and the last after.
    let next = shell.page_places()[2].1;
    shell.click(&mut s, middle(next), size, false, false, false);
    drawn(&mut shell, &s, SHORT_WINDOW);
    assert_eq!(pages(&shell), [HOME, 1, 10, 11]);
    assert_eq!(shell.strip_ends(), Some((true, true)));
}

#[test]
fn a_controllers_bumpers_find_the_collapsed_rows_parts_as_tabs() {
    let s = Settings::default();
    let mut shell = Shell::<Pages>::default();
    shell.show(5);
    drawn(&mut shell, &s, SHORT_WINDOW);
    let tabs: Vec<(f32, bool)> = crate::widgets::last_targets()
        .into_iter()
        .filter_map(|t| match t.kind {
            crate::widgets::Kind::Tab(open) => Some((t.rect[0], open)),
            _ => None,
        })
        .collect();
    // The first page, the page before, the open one (lit) and the one after.
    assert_eq!(tabs.len(), 4, "{tabs:?}");
    assert_eq!(tabs.iter().filter(|t| t.1).count(), 1);
    let (before, open, after) = (tabs[1].0, tabs[2], tabs[3].0);
    assert!(open.1 && before < open.0 && open.0 < after);
    // The arrows are vanilla's Previous Page and Next Page to the narrator.
    let labels = crate::widgets::last_labelled();
    let named = |key| {
        labels
            .iter()
            .any(|w| w.label == lang::get(key) && w.enabled)
    };
    assert!(named(PREVIOUS_PAGE) && named(NEXT_PAGE));
}

#[test]
fn a_link_opens_a_page_out_of_the_list_and_escape_comes_back_to_the_links_page() {
    let mut s = Settings::default();
    let mut shell = Shell::<Pages>::default();
    // Found by the search from another page: back is the page the link is on.
    shell.show(HOME);
    shell.set_search("deeper");
    let at = shell.point_at(&s, SIZE, LINK);
    assert_eq!(shell.click(&mut s, at, SIZE, false, false, false), [CLICK]);
    // The page opens alone, the search cleared; the list lights the page the link was on.
    assert_eq!(shell.open_page(), Some(DEEP));
    assert_eq!(shell.listed(&s, SIZE), [(DEEP, u32::from(DEEP))]);
    drawn(&mut shell, &s, TALL);
    assert_eq!(shell.lit_page(), Some(1));
    assert!(shell.key(&mut s, K::Escape, None, false, SIZE).is_empty());
    assert_eq!(shell.open_page(), Some(1));
    assert_eq!(
        shell.key(&mut s, K::Escape, None, false, SIZE),
        [Output::Done]
    );
}

#[test]
fn apply_shows_beside_undo_while_changes_wait_and_goes_with_them() {
    let mut s = Settings::default();
    let mut shell = Shell::<Pages>::default();
    let done = Columns::of(SIZE.0, SIZE.1).button(0);
    assert_eq!(shell.foot_at(&s, SIZE), [(Target::Done, done)]);
    let at = shell.point_at(&s, SIZE, WAITS);
    shell.click(&mut s, at, SIZE, false, false, false);
    let foot = shell.foot_at(&s, SIZE);
    let targets: Vec<_> = foot.iter().map(|f| f.0).collect();
    assert_eq!(targets, [Target::Undo, Target::Apply, Target::Done]);
    // Side by side over Done, as wide as it together.
    let ([ux, uy, uw, _], [ax, ay, aw, _]) = (foot[0].1, foot[1].1);
    assert_eq!((uy, ay), (foot[2].1[1] - FOOT_GAP - style::BUTTON_H, uy));
    assert!(ux + uw < ax && ax + aw == done[0] + done[2] && ux == done[0]);
    assert_eq!(
        shell.click(&mut s, middle(foot[1].1), SIZE, false, false, false),
        [CLICK, CHANGED]
    );
    assert_eq!(shell.screen.applied, 1);
    let targets: Vec<_> = shell.foot_at(&s, SIZE).iter().map(|f| f.0).collect();
    assert_eq!(targets, [Target::Undo, Target::Done]);
}

#[test]
fn the_right_button_steps_no_choice_as_vanillas_cycle_button() {
    let mut s = Settings::default();
    let mut shell = Shell::<Probe>::default();
    let choice = middle_of(&mut shell, CHOICE, SIZE);
    assert!(
        shell
            .click(&mut s, choice, SIZE, false, true, false)
            .is_empty()
    );
    assert!(shell.screen.pressed.is_empty());
}
