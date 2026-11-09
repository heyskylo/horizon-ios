//! The Horizon Extras page, on the settings shell (`settings_shell`): which rows it has and what
//! pressing them does. Each one's value and what it does stay in `extras::Item`.
use crate::{
    extras::{self, Item},
    lang,
    options_ui::{Event, Page},
    settings::Settings,
    settings_shell::{self as shell, Binding, Group, Output, Row, Screen, Sheet, Value},
};

const TITLE: &str = "horizon.pages.extras";
const ABOUT: &str = "horizon.pages.extras.tooltip";
/// The group of the mod that replaced an extra.
const MODS: &str = "horizon.pages.mods";
const ZOOM: &str = "horizon.extras.zoom";
const ZOOM_INSTALL: &str = "horizon.extras.zoom_mod_install";
const MOD_SETTINGS: &str = "horizon.mods.settings";
/// Its descriptions run long, as tooltips they were: most need more than two lines.
const NOTE_LINES: usize = 5;
/// The official native mod that took over from the Zoom extra (#8713).
const ZOOM_ID: &str = "zoom";

/// A row of the page.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ExtraRow {
    Item(Item),
    /// The Zoom mod, at this place among the installed ones, on or off.
    Zoom(usize),
    /// Its own settings page.
    ZoomSettings(usize),
    /// It isn't installed: where to get it. A note, not a button: Options > Mods already has the
    /// one way into the Mods page (`every_setting_and_page_is_reached_from_one_place`).
    ZoomInstall,
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
    /// The rows read the extras, the mods and whether cosmetics are offered.
    fn stamp(&self, s: &Settings) -> u64 {
        shell::stamp_of(&(&s.extras, &s.mods, horizon_cosmetics::enabled()))
    }
    fn sheets(&self, s: &Settings) -> Vec<Sheet<(), ExtraRow>> {
        let mut groups = vec![Group {
            name: MODS,
            rows: zoom_rows(s),
        }];
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
        vec![Sheet {
            id: (),
            name: lang::get(TITLE),
            about: lang::get(ABOUT),
            groups,
        }]
    }
    fn press(&mut self, s: &mut Settings, row: ExtraRow, back: bool) -> Vec<Output> {
        match row {
            ExtraRow::Item(item) => {
                item.cycle(&mut s.extras, back);
                vec![Output::Event(Event::Changed)]
            }
            ExtraRow::Zoom(i) => {
                s.mods.toggle(i);
                vec![Output::Event(Event::Changed)]
            }
            ExtraRow::ZoomSettings(i) => vec![
                Output::Event(Event::ButtonClick),
                Output::Push(Page::NativeMod(i)),
                Output::Event(Event::NativeSettings(i)),
            ],
            ExtraRow::ZoomInstall => Vec::new(),
        }
    }
    /// With the wheel extra on, the wheel over an item changes it, without a click; the mod's rows
    /// let the list scroll.
    fn wheel_step(&mut self, s: &mut Settings, row: ExtraRow, back: bool) -> Option<Vec<Output>> {
        let ExtraRow::Item(item) = row else {
            return None;
        };
        item.cycle(&mut s.extras, back);
        Some(vec![Output::Event(Event::Changed)])
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

    #[test]
    fn the_three_way_cosmetics_row_turns_both_ways() {
        use horizon_cosmetics::prefs::Others;
        let mut s = Settings::default();
        let mut screen = ExtrasScreen;
        let row = ExtraRow::Item(Item::CosmeticsOthers);
        // Not chosen, nothing owned: it shows Hidden, and forward wraps to Shown.
        assert_eq!(s.extras.effective_others(), Others::Hidden);
        screen.press(&mut s, row, false);
        assert_eq!(s.extras.cosmetics_others, Some(Others::Shown));
        screen.press(&mut s, row, true);
        assert_eq!(s.extras.cosmetics_others, Some(Others::Hidden));
        screen.press(&mut s, row, true);
        assert_eq!(s.extras.cosmetics_others, Some(Others::NameTags));
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
}
