//! The Horizon Extras page, on the settings shell (`settings_shell`): which rows it has and what
//! pressing them does. Each one's value and what it does stay in `extras::Item`; the sections crates
//! add (`horizon_hooks::pages::Place::Extras`) keep theirs.
use crate::{
    extras::{self, Item},
    lang,
    options_ui::Event,
    settings::Settings,
    settings_shell::{self as shell, Binding, Group, Output, Row, Screen, Sheet, Value},
};
use horizon_hooks::pages::{self as added, Act, Label};

#[cfg(not(target_os = "ios"))]
use crate::options_ui::Page;

const TITLE: &str = "horizon.pages.extras";
const ABOUT: &str = "horizon.pages.extras.tooltip";
/// The group of the mod that replaced an extra.
#[cfg(not(target_os = "ios"))]
const MODS: &str = "horizon.pages.mods";
#[cfg(not(target_os = "ios"))]
const ZOOM: &str = "horizon.extras.zoom";
#[cfg(not(target_os = "ios"))]
const ZOOM_INSTALL: &str = "horizon.extras.zoom_mod_install";
#[cfg(not(target_os = "ios"))]
const MOD_SETTINGS: &str = "horizon.mods.settings";
/// Its descriptions run long, as tooltips they were: most need more than two lines.
const NOTE_LINES: usize = 5;
/// The official native mod that took over from the Zoom extra (#8713).
#[cfg(not(target_os = "ios"))]
const ZOOM_ID: &str = "zoom";

/// A row of the page.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ExtraRow {
    Item(Item),
    /// The Zoom mod, at this place among the installed ones, on or off.
    #[cfg(not(target_os = "ios"))]
    Zoom(usize),
    /// Its own settings page.
    #[cfg(not(target_os = "ios"))]
    ZoomSettings(usize),
    /// It isn't installed: where to get it. A note, not a button: Options > Mods already has the
    /// one way into the Mods page (`every_setting_and_page_is_reached_from_one_place`).
    #[cfg(not(target_os = "ios"))]
    ZoomInstall,
    /// Row `.1` of the section registered page `.0` adds (`Place::Extras`).
    Added(usize, usize),
}

#[cfg(test)]
thread_local! {
    /// A test's own pages, instead of the game's registered ones: each test runs on its thread.
    static TEST_PAGES: std::cell::Cell<Option<&'static [&'static dyn added::Page]>> =
        const { std::cell::Cell::new(None) };
}

/// The pages crates registered, all places.
fn pages() -> &'static [&'static dyn added::Page] {
    #[cfg(test)]
    if let Some(pages) = TEST_PAGES.get() {
        return pages;
    }
    added::engine::all()
}

/// The sections of this page, by their index among the registered pages.
fn sections() -> impl Iterator<Item = (usize, &'static dyn added::Page)> {
    let pages = pages().iter().copied().enumerate();
    pages.filter(|(_, page)| page.place() == added::Place::Extras)
}

fn label_text(label: Label) -> String {
    match label {
        Label::Key(key) => lang::get(key),
        Label::Value(text) => text,
    }
}

/// The page's group named `name`, made at the end if it has none.
fn group_named<'a>(
    groups: &'a mut Vec<Group<ExtraRow>>,
    name: &'static str,
) -> &'a mut Group<ExtraRow> {
    let at = groups.iter().position(|group| group.name == name);
    let at = at.unwrap_or_else(|| {
        groups.push(Group {
            name,
            rows: Vec::new(),
        });
        groups.len() - 1
    });
    &mut groups[at]
}

/// The sections' rows, in the groups their titles and headings name.
fn add_sections(groups: &mut Vec<Group<ExtraRow>>) {
    for (page, section) in sections() {
        let mut rows = Vec::new();
        section.rows(&mut rows);
        let mut name = section.title();
        for (r, row) in rows.into_iter().enumerate() {
            let id = ExtraRow::Added(page, r);
            let row = match row {
                added::Row::Heading(key) => {
                    name = key;
                    continue;
                }
                added::Row::Note(text) => Row::new(id, text, Value::Note).live(false),
                added::Row::Toggle { label, on } => {
                    Row::new(id, lang::get(label), Value::Switch(on))
                }
                added::Row::Cycle { label, value } => {
                    Row::new(id, lang::get(label), Value::Choice(label_text(value)))
                }
                added::Row::Slider { label, value, text } => {
                    let position = value.clamp(0.0, 1.0);
                    let text = label_text(text);
                    let value = Value::Slider {
                        position,
                        steps: None,
                        text,
                    };
                    Row::new(id, lang::get(label), value)
                }
                added::Row::Button {
                    label,
                    active: true,
                    ..
                } => Row::new(id, lang::get(label), Value::Press),
                added::Row::Key { label, key } => {
                    let key = crate::keybinds::display_name(&key).plain();
                    Row::new(id, lang::get(label), Value::Read(key))
                }
                // A row kind newer than this engine.
                _ => continue,
            };
            let row = match section.tooltip(r) {
                Some(about) => row.about(label_text(about)),
                None => row,
            };
            group_named(groups, name).rows.push(row);
        }
    }
}

/// Row `r` of registered page `page` was acted on: the options screen saves when it closes.
fn act(page: usize, r: usize, act: Act) -> Vec<Output> {
    let Some(section) = pages().get(page) else {
        return Vec::new();
    };
    section.act(r, act);
    vec![Output::Event(Event::Changed)]
}

/// Whether row `r` of registered page `page` steps (a switch or a cycle).
fn steps(page: usize, r: usize) -> bool {
    let mut rows = Vec::new();
    if let Some(section) = pages().get(page) {
        section.rows(&mut rows);
    }
    matches!(
        rows.get(r),
        Some(added::Row::Toggle { .. } | added::Row::Cycle { .. })
    )
}

fn item_row(item: Item, s: &Settings) -> Row<ExtraRow> {
    let e = &s.extras;
    let value = match (item.key_name(), item.switch(e)) {
        (Some(key), _) => Value::Binding(Binding::key(key, item.value(e))),
        (None, Some(on)) => Value::Switch(on),
        (None, None) => Value::Choice(item.value(e)),
    };
    // A setting of a switch that is off does nothing, so it is greyed out.
    let live = item
        .parent()
        .is_none_or(|parent| parent.switch(e) != Some(false));
    Row::new(ExtraRow::Item(item), item.caption_text(), value)
        .about(item.description_text())
        .live(live)
}

/// Zoom, the FPS counter, Keystrokes and the Minimap are official mods now; Extras keeps a way to
/// the one players look for here.
#[cfg(not(target_os = "ios"))]
fn zoom_rows(s: &Settings) -> Vec<Row<ExtraRow>> {
    let installed = &s.mods.installed;
    let found = installed
        .iter()
        .position(|m| m.id == ZOOM_ID && m.native.is_some());
    let Some(i) = found else {
        let note = Row::new(ExtraRow::ZoomInstall, lang::get(ZOOM_INSTALL), Value::Note);
        return vec![note.live(false)];
    };
    let about = installed[i].problem.as_ref();
    let about = about.unwrap_or(&installed[i].description).clone();
    let on = Value::Switch(s.mods.is_on(i));
    let mut rows = vec![Row::new(ExtraRow::Zoom(i), lang::get(ZOOM), on).about(about)];
    let page = installed[i].native.as_ref();
    if page.is_some_and(|page| !page.keys.is_empty() || !page.buttons.is_empty()) {
        let name = lang::get(MOD_SETTINGS);
        rows.push(Row::new(ExtraRow::ZoomSettings(i), name, Value::Press));
    }
    rows
}

/// The page itself holds nothing: everything it shows is in the settings.
#[derive(Default)]
pub(crate) struct ExtrasScreen;

impl Screen for ExtrasScreen {
    type Page = ();
    type Row = ExtraRow;
    fn title(&self) -> &'static str {
        TITLE
    }
    fn note_lines(&self) -> usize {
        NOTE_LINES
    }
    /// The rows read the extras, the mods and the sections' rows.
    fn stamp(&self, s: &Settings) -> u64 {
        let mut added = Vec::new();
        for (_, section) in sections() {
            section.rows(&mut added);
        }
        shell::stamp_of(&(&s.extras, &s.mods, added))
    }
    fn sheets(&self, s: &Settings) -> Vec<Sheet<(), ExtraRow>> {
        #[cfg(not(target_os = "ios"))]
        let mut groups = vec![Group {
            name: MODS,
            rows: zoom_rows(s),
        }];
        // Mobile keeps the built-in zoom and its key/level settings in
        // mobile_ui::rows; native mod compilation is a desktop launcher flow.
        #[cfg(target_os = "ios")]
        let mut groups = Vec::new();
        for row in extras::rows() {
            match row {
                Err(name) => groups.push(Group {
                    name,
                    rows: Vec::new(),
                }),
                Ok(item) => {
                    if let Some(group) = groups.last_mut() {
                        group.rows.push(item_row(item, s));
                    }
                }
            }
        }
        add_sections(&mut groups);
        vec![Sheet::new((), lang::get(TITLE), lang::get(ABOUT), groups)]
    }
    fn press(&mut self, s: &mut Settings, row: ExtraRow, back: bool) -> Vec<Output> {
        match row {
            ExtraRow::Item(item) => {
                item.cycle(&mut s.extras, back);
                vec![Output::Event(Event::Changed)]
            }
            #[cfg(not(target_os = "ios"))]
            ExtraRow::Zoom(i) => {
                s.mods.toggle(i);
                vec![Output::Event(Event::Changed)]
            }
            #[cfg(not(target_os = "ios"))]
            ExtraRow::ZoomSettings(i) => vec![
                Output::Event(Event::ButtonClick),
                Output::Push(Page::NativeMod(i)),
                Output::Event(Event::NativeSettings(i)),
            ],
            #[cfg(not(target_os = "ios"))]
            ExtraRow::ZoomInstall => Vec::new(),
            ExtraRow::Added(page, r) => {
                act(page, r, if back { Act::PressBack } else { Act::Press })
            }
        }
    }
    fn slide(&mut self, _s: &mut Settings, row: ExtraRow, position: f32) {
        if let ExtraRow::Added(page, r) = row {
            act(page, r, Act::Slide(position));
        }
    }
    /// With the wheel extra on, the wheel over an item changes it, without a click; the mod's rows
    /// let the list scroll.
    fn wheel_step(&mut self, s: &mut Settings, row: ExtraRow, back: bool) -> Option<Vec<Output>> {
        match row {
            ExtraRow::Item(item) => {
                item.cycle(&mut s.extras, back);
                Some(vec![Output::Event(Event::Changed)])
            }
            ExtraRow::Added(page, r) if steps(page, r) => {
                Some(act(page, r, if back { Act::PressBack } else { Act::Press }))
            }
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings_shell::{Hosted, Shell};

    const SIZE: (f32, f32) = (854.0, 480.0);

    fn rows_of(s: &Settings) -> Vec<Row<ExtraRow>> {
        let sheets = ExtrasScreen.sheets(s);
        let groups = sheets.into_iter().flat_map(|sheet| sheet.groups);
        groups.flat_map(|group| group.rows).collect()
    }

    /// Greyed exactly while its own switch is off, so it takes no press then.
    #[test]
    fn a_setting_of_a_switch_that_is_off_is_greyed_and_takes_no_press() {
        let mut s = Settings::default();
        let live = |s: &Settings| {
            let wheel = ExtraRow::Item(Item::MouseTweaksWheel);
            rows_of(s).iter().find(|r| r.id == wheel).unwrap().live
        };
        s.extras.mouse_tweaks = false;
        assert!(!live(&s));
        let mut shell = Shell::<ExtrasScreen>::default();
        let row = ExtraRow::Item(Item::MouseTweaksWheel);
        let at = shell.point_at(&s, SIZE, row);
        let before = s.extras.mouse_tweaks_wheel;
        assert!(
            shell
                .click(&mut s, at, SIZE, false, false, false)
                .is_empty()
        );
        assert_eq!(s.extras.mouse_tweaks_wheel, before);
        s.extras.mouse_tweaks = true;
        assert!(live(&s));
        assert!(
            !shell
                .click(&mut s, at, SIZE, false, false, false)
                .is_empty()
        );
        assert_ne!(s.extras.mouse_tweaks_wheel, before);
    }

    /// The sort key binds instead of cycling; the wheel leaves it be.
    #[test]
    fn the_key_row_waits_for_a_key() {
        use crate::settings_shell::Capture;
        let mut s = Settings::default();
        s.extras.sort = true;
        let mut shell = Shell::<ExtrasScreen>::default();
        let row = ExtraRow::Item(Item::SortKey);
        let key = s.extras.sort_key.clone();
        let at = shell.point_at(&s, SIZE, row);
        assert_eq!(
            shell.click(&mut s, at, SIZE, false, false, false),
            vec![
                Output::Event(Event::ButtonClick),
                Output::Bind(Capture::Key(extras::SORT_KEY))
            ]
        );
        ExtrasScreen.wheel_step(&mut s, row, true);
        assert_eq!(s.extras.sort_key, key);
    }

    #[test]
    fn the_wheel_flips_a_switch_and_leaves_the_mod_to_scroll() {
        let mut s = Settings::default();
        let mut screen = ExtrasScreen;
        let row = ExtraRow::Item(Item::Sort);
        assert_eq!(
            screen.wheel_step(&mut s, row, true),
            Some(vec![Output::Event(Event::Changed)])
        );
        assert!(s.extras.sort);
        #[cfg(not(target_os = "ios"))]
        assert_eq!(screen.wheel_step(&mut s, ExtraRow::Zoom(0), true), None);
    }

    /// Extras has no Undo: a change leaves none to press or reach.
    #[test]
    fn a_change_on_extras_leaves_no_undo() {
        use crate::settings_shell::{Columns, Target};
        use winit::keyboard::KeyCode as K;
        let mut s = Settings::default();
        let mut shell = Shell::<ExtrasScreen>::default();
        let at = shell.point_at(&s, SIZE, ExtraRow::Item(Item::Sort));
        assert!(
            !shell
                .click(&mut s, at, SIZE, false, false, false)
                .is_empty()
        );
        assert!(s.extras.sort && !shell.changed());
        let cols = Columns::of(SIZE.0, SIZE.1).with_note_lines(NOTE_LINES);
        let [x, y, ..] = cols.button(1);
        assert!(
            shell
                .click(&mut s, (x + 2.0, y + 2.0), SIZE, false, false, false)
                .is_empty()
        );
        assert!(s.extras.sort);
        let reached = (0..100).any(|_| {
            shell.key(&mut s, K::Tab, None, false, SIZE);
            shell.focused() == Some(Target::Undo)
        });
        assert!(!reached);
    }

    /// A page with no page list has Done under the search field: Down goes to the first row
    /// instead, from the keys and, as the frame declares it, from a controller's focus.
    #[test]
    fn down_from_the_search_field_lands_on_the_first_row() {
        use crate::{hud::HudBatch, settings_shell::Columns, ui};
        use winit::keyboard::KeyCode as K;
        let atlas = crate::hud::HudAtlas::build(std::path::Path::new("/nonexistent")).unwrap();
        let mut s = Settings::default();
        let mut shell = Shell::<ExtrasScreen>::default();
        let mut batch = HudBatch::new([1708, 960]);
        shell.draw(&s, &mut batch, &atlas, (0.0, 0.0), false, None);
        crate::widgets::end_frame(1);
        let size = (batch.width, batch.height);
        let cols = Columns::of(size.0, size.1).with_note_lines(NOTE_LINES);
        // The Mods group's note can't take the focus: the first item can.
        let first = ExtraRow::Item(Item::OwnNameTag);
        let at = shell.point_at(&s, size, first);
        let [x, y, w, h] = crate::widgets::linked_step(cols.search(), true).unwrap();
        assert!(ui::hovered(at, x, y, w, h));
        assert_eq!(crate::widgets::linked_step(cols.search(), false), None);
        let [x, y, w, h] = cols.search();
        shell.click(
            &mut s,
            (x + w / 2.0, y + h / 2.0),
            size,
            false,
            false,
            false,
        );
        shell.key(&mut s, K::ArrowDown, None, false, size);
        assert_eq!(shell.focused_row(), Some(first));
    }

    /// A section with a switch in the page's Multiplayer group, then a heading of its own with a
    /// number that steps either way, a note and a volume.
    struct Section {
        on: std::sync::Mutex<bool>,
        step: std::sync::Mutex<i32>,
        volume: std::sync::Mutex<f32>,
    }
    impl added::Page for Section {
        fn id(&self) -> &'static str {
            "test.section"
        }
        fn title(&self) -> &'static str {
            "horizon.extras.header.multiplayer"
        }
        fn place(&self) -> added::Place {
            added::Place::Extras
        }
        fn rows(&self, rows: &mut Vec<added::Row>) {
            rows.push(added::Row::Toggle {
                label: "test.section.switch",
                on: *self.on.lock().unwrap(),
            });
            rows.push(added::Row::Heading("test.section.heading"));
            rows.push(added::Row::Cycle {
                label: "test.section.step",
                value: Label::Value(self.step.lock().unwrap().to_string()),
            });
            rows.push(added::Row::Note("a note".into()));
            rows.push(added::Row::Slider {
                label: "test.section.volume",
                value: *self.volume.lock().unwrap(),
                text: Label::Value(String::new()),
            });
        }
        fn act(&self, row: usize, act: Act) {
            match (row, act) {
                (0, Act::Press | Act::PressBack) => {
                    let mut on = self.on.lock().unwrap();
                    *on = !*on;
                }
                (2, Act::Press) => *self.step.lock().unwrap() += 1,
                (2, Act::PressBack) => *self.step.lock().unwrap() -= 1,
                (4, Act::Slide(v)) => *self.volume.lock().unwrap() = v,
                _ => {}
            }
        }
    }

    /// A page for Horizon Settings: never on Extras.
    struct Elsewhere;
    impl added::Page for Elsewhere {
        fn id(&self) -> &'static str {
            "test.elsewhere"
        }
        fn title(&self) -> &'static str {
            "test.elsewhere"
        }
        fn place(&self) -> added::Place {
            added::Place::HorizonSettings
        }
        fn rows(&self, rows: &mut Vec<added::Row>) {
            rows.push(added::Row::button("test.elsewhere.button"));
        }
        fn act(&self, _: usize, _: Act) {}
    }

    fn with_section() -> &'static Section {
        let section: &'static Section = Box::leak(Box::new(Section {
            on: false.into(),
            step: 0.into(),
            volume: 0.5.into(),
        }));
        let pages: &'static [&'static dyn added::Page] = Box::leak(Box::new([
            &Elsewhere as &dyn added::Page,
            section as &dyn added::Page,
        ]));
        TEST_PAGES.set(Some(pages));
        section
    }

    /// The section's first row joins the page's own Multiplayer group, after its rows; its heading
    /// starts a group after the page's; a page for elsewhere adds nothing.
    #[test]
    fn a_section_joins_the_group_its_title_names_and_heads_its_own() {
        with_section();
        let s = Settings::default();
        let sheets = ExtrasScreen.sheets(&s);
        let groups = &sheets[0].groups;
        let ids = |name: &str| -> Vec<ExtraRow> {
            let group = groups.iter().find(|g| g.name == name).unwrap();
            group.rows.iter().map(|r| r.id).collect()
        };
        let multiplayer = ids("horizon.extras.header.multiplayer");
        assert_eq!(multiplayer.last(), Some(&ExtraRow::Added(1, 0)));
        assert_eq!(groups.last().unwrap().name, "test.section.heading");
        let own = [
            ExtraRow::Added(1, 2),
            ExtraRow::Added(1, 3),
            ExtraRow::Added(1, 4),
        ];
        assert_eq!(ids("test.section.heading"), own);
        let all = rows_of(&s);
        assert!(!all.iter().any(|r| matches!(r.id, ExtraRow::Added(0, _))));
    }

    /// A click flips the switch, a Shift click steps the number back, the wheel steps it, and the
    /// note takes no click; the rows show what the section holds after each.
    #[test]
    fn a_sections_rows_act_on_it() {
        let section = with_section();
        let mut s = Settings::default();
        let mut shell = Shell::<ExtrasScreen>::default();
        let switch = ExtraRow::Added(1, 0);
        let at = shell.point_at(&s, SIZE, switch);
        assert!(
            !shell
                .click(&mut s, at, SIZE, false, false, false)
                .is_empty()
        );
        assert!(*section.on.lock().unwrap());
        let value = |s: &Settings, id| rows_of(s).into_iter().find(|r| r.id == id).unwrap().value;
        assert_eq!(value(&s, switch), Value::Switch(true));
        let step = ExtraRow::Added(1, 2);
        let mut screen = ExtrasScreen;
        screen.press(&mut s, step, true);
        assert_eq!(value(&s, step), Value::Choice("-1".into()));
        assert!(screen.wheel_step(&mut s, step, false).is_some());
        assert_eq!(*section.step.lock().unwrap(), 0);
        assert_eq!(
            screen.wheel_step(&mut s, ExtraRow::Added(1, 3), false),
            None
        );
        let note = shell.point_at(&s, SIZE, ExtraRow::Added(1, 3));
        assert!(
            shell
                .click(&mut s, note, SIZE, false, false, false)
                .is_empty()
        );
        screen.slide(&mut s, ExtraRow::Added(1, 4), 0.25);
        assert_eq!(*section.volume.lock().unwrap(), 0.25);
    }
}
