//! The radial menu's pages in the controller settings (Horizon's own; `radial.rs` has their
//! rules, `radial_overlay.rs` draws them in play), on the settings shell (`settings_shell`): a
//! sheet per page with its eight slots, what changes the page, and what changes them all. A
//! page's own name is typed on a small page of its own, [`NameEntry`].
use crate::{
    gamepad::{self, GlyphOf, MenuJob},
    graphics_style::{self as style, BUTTON_H},
    hud::{HudAtlas, HudBatch},
    lang,
    options_ui::{Event, Page},
    pad_glyph::{Glyph, Glyphs},
    radial::{self, Pages, RadialAction, SLOTS, Slot},
    radial_overlay::{page_label, slot_caption},
    settings::Settings,
    settings_shell::{
        self as shell, Binding, Capture, Group, Output, Reset, Row, Screen, Sheet, Shown, Value,
    },
    text::Text,
    ui,
};

const TITLE: &str = "horizon.pages.controller_radial";
const SLOTS_GROUP: &str = "horizon.radial.group.slots";
const PAGE_GROUP: &str = "horizon.radial.group.page";
const ALL_GROUP: &str = "horizon.radial.group.all";
/// The opener's row binds the menu's own pad action, in place.
const OPENER: &str = gamepad::RADIAL;

/// A row of a page; `usize`s are the page and the slot.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RadialRow {
    Slot(usize, usize),
    Rename(usize),
    NewPage,
    Delete(usize),
    Reset,
    Opener,
}

/// A slot's action, that it holds none, or that its action is missing.
fn slot_label(slot: &Slot) -> String {
    slot_caption(slot).unwrap_or_else(|| lang::get("horizon.radial.empty"))
}

/// What the slot after `slot` (before, `back`) holds: Empty between the last action and the first.
/// A missing action steps as Empty does.
fn step(slot: &Slot, back: bool) -> Slot {
    let order: Vec<_> = std::iter::once(None)
        .chain(RadialAction::ALL.map(Some))
        .collect();
    let at = order
        .iter()
        .position(|entry| *entry == slot.action())
        .unwrap_or(0);
    let next = if back { at + order.len() - 1 } else { at + 1 };
    order[next % order.len()].into()
}

fn page_of(page: usize, pages: usize) -> String {
    lang::tr("horizon.radial.page_of", &[&(page + 1), &pages])
}

/// The opener's row: a binding of the menu's own pad action, in place, on its button's art. Its
/// strip says what stands in the way (no button, or one another action shares); while it waits,
/// the shell says how to bind it.
fn opener_row(s: &Settings, glyphs: Glyphs) -> Row<RadialRow> {
    let pad = &s.controller;
    // Through the player's glyphs, so a Nintendo pad going by its letters names its own button.
    let glyph = Glyph::of(GlyphOf::Bound(OPENER), pad, glyphs);
    let warning = match pad.binding(OPENER) {
        None => Some(lang::get("horizon.radial.unbound")),
        Some(_) => pad
            .conflicts(OPENER)
            .then(|| lang::get("horizon.radial.conflict")),
    };
    let binding = Binding {
        capture: Capture::Pad(OPENER),
        shown: Shown::Pad {
            glyph,
            cancel: Glyph::menu(MenuJob::Start, glyphs),
        },
        shared: Vec::new(),
        // The default (D-pad Right) is refused with some sticks: no reset, only a new button.
        reset: Reset::None,
    };
    let name = lang::get("horizon.radial.opener");
    Row {
        chosen: warning,
        ..Row::new(RadialRow::Opener, name, Value::Binding(binding))
            .about(lang::get(opener_about(pad.radial_stick)))
    }
}

/// What the opener's row says it can be, with the stick that selects: never under that thumb.
fn opener_about(stick: gamepad::RadialStick) -> &'static str {
    match stick {
        gamepad::RadialStick::Right => "horizon.radial.opener.about",
        gamepad::RadialStick::Left => "horizon.radial.opener.about.left",
        gamepad::RadialStick::Either => "horizon.radial.opener.about.either",
    }
}

/// The page's rows: its slots, then what changes it.
fn page_groups(pages: &Pages, page: usize) -> [Group<RadialRow>; 2] {
    let slots = (0..SLOTS).map(|slot| {
        let name = lang::get(&format!("horizon.radial.slot.{slot}"));
        let cell = pages.page(page).slot(slot);
        let value = Value::Choice(slot_label(cell));
        let missing = match cell {
            Slot::Missing(id) => Some(lang::tr("horizon.radial.missing.about", &[id])),
            _ => None,
        };
        Row {
            chosen: missing,
            ..Row::new(RadialRow::Slot(page, slot), name, value)
                .about(lang::get("horizon.radial.slot.about"))
        }
    });
    let full = (pages.len() >= radial::MAX_PAGES)
        .then(|| lang::tr("horizon.radial.new_page.full", &[&radial::MAX_PAGES]));
    let last = (pages.len() <= 1).then(|| lang::get("horizon.radial.delete_page.last"));
    let changes = vec![
        Row::new(
            RadialRow::Rename(page),
            lang::get("horizon.radial.rename"),
            Value::Press,
        )
        .about(lang::get("horizon.radial.rename.about")),
        Row::new(
            RadialRow::NewPage,
            lang::get(radial::NEW_PAGE_NAME),
            Value::Press,
        )
        .about(lang::get("horizon.radial.new_page.about"))
        .blocked(full),
        Row::new(
            RadialRow::Delete(page),
            lang::get("horizon.radial.delete_page"),
            Value::Press,
        )
        .about(lang::get("horizon.radial.delete_page.about"))
        .blocked(last),
    ];
    [
        Group {
            name: SLOTS_GROUP,
            rows: slots.collect(),
        },
        Group {
            name: PAGE_GROUP,
            rows: changes,
        },
    ]
}

/// The screen's own state; the pages are the player's settings.
#[derive(Default)]
pub(crate) struct RadialScreen {
    /// The buttons' names follow the controller in use: the options screen says which.
    pub(crate) glyphs: Glyphs,
    /// The page to show next: one just added, or where a deleted one was.
    show: Option<usize>,
    /// The page whose name is being typed, on its own page (`Page::ControllerRadialName`).
    pub(crate) naming: Option<NameEntry>,
    /// The slot (page, slot) whose list of actions opens next (`Page::ControllerRadialPick`).
    pub(crate) picking: Option<(usize, usize)>,
}

impl RadialScreen {
    /// The page the shell should show now, once.
    pub(crate) fn take_shown(&mut self) -> Option<usize> {
        self.show.take()
    }
}

impl Screen for RadialScreen {
    type Page = usize;
    type Row = RadialRow;
    fn title(&self) -> &'static str {
        TITLE
    }
    /// The rows read the pages, the opener's button, the stick that selects and the glyphs it is
    /// named with.
    fn stamp(&self, s: &Settings) -> u64 {
        let pad = &s.controller;
        let opener = (pad.binding(OPENER), pad.conflicts(OPENER));
        shell::stamp_of(&(&pad.radial, opener, pad.radial_stick, self.glyphs))
    }
    fn sheets(&self, s: &Settings) -> Vec<Sheet<usize, RadialRow>> {
        let pages = &s.controller.radial;
        let default = *pages == Pages::default();
        let all = Group {
            name: ALL_GROUP,
            rows: vec![
                Row::new(
                    RadialRow::Reset,
                    lang::get("horizon.radial.reset"),
                    Value::Press,
                )
                .about(lang::get("horizon.radial.reset.about"))
                .blocked(default.then(|| lang::get("horizon.radial.reset.default"))),
                opener_row(s, self.glyphs),
            ],
        };
        (0..pages.len())
            .map(|page| {
                let [slots, changes] = page_groups(pages, page);
                let all = Group {
                    name: all.name,
                    rows: all.rows.clone(),
                };
                Sheet {
                    id: page,
                    name: page_label(pages.page(page)),
                    about: page_of(page, pages.len()),
                    groups: vec![slots, changes, all],
                }
            })
            .collect()
    }
    fn press(&mut self, s: &mut Settings, row: RadialRow, back: bool) -> Vec<Output> {
        let pages = &mut s.controller.radial;
        match row {
            RadialRow::Slot(page, slot) => {
                let next = step(pages.page(page).slot(slot), back);
                pages.set_slot(page, slot, next);
            }
            RadialRow::Rename(page) => {
                self.naming = Some(NameEntry::new(page, &page_label(pages.page(page))));
                return vec![Output::Push(Page::ControllerRadialName)];
            }
            RadialRow::NewPage => self.show = pages.add(),
            RadialRow::Delete(page) => {
                pages.remove(page);
                self.show = Some(pages.clamp(page));
            }
            RadialRow::Reset => {
                *pages = Pages::default();
                self.show = Some(0);
            }
            // The shell starts its wait (`gamepad::opener_refused` refused, Start cancels).
            RadialRow::Opener => return Vec::new(),
        }
        vec![Output::Event(Event::Changed)]
    }
    /// A press on a slot opens the list of every action for it; Left and Right still step it.
    fn choose(&mut self, _s: &mut Settings, row: RadialRow) -> Option<Vec<Output>> {
        let RadialRow::Slot(page, slot) = row else {
            return None;
        };
        self.picking = Some((page, slot));
        Some(vec![Output::Push(Page::ControllerRadialPick)])
    }
    /// The wheel over a slot steps it, as a press does.
    fn wheel_step(&mut self, s: &mut Settings, row: RadialRow, back: bool) -> Option<Vec<Output>> {
        matches!(row, RadialRow::Slot(..)).then(|| self.press(s, row, back))
    }
    /// Undo is how a delete or a reset is taken back: they act at once.
    fn undoes(&self) -> bool {
        true
    }
    /// The pages, and the opener's button, which its row binds.
    fn undo(&mut self, s: &mut Settings, before: &Settings) {
        s.controller.radial = before.controller.radial.clone();
        s.controller.bind(OPENER, before.controller.binding(OPENER));
    }
}

const PICK_TITLE: &str = "horizon.radial.pick_title";
const PICK_GROUP: &str = "horizon.radial.group.actions";

/// A row of a slot's list: the line naming the slot, then Empty and every action.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PickRow {
    Slot,
    Pick(Option<RadialAction>),
}

/// One slot's list of every action and Empty, on the settings shell: it names the slot, marks
/// what the slot holds and opens on it; a press puts that in the slot and goes back.
#[derive(Default)]
pub(crate) struct PickScreen {
    /// The page and the slot.
    at: (usize, usize),
    picked: bool,
}

impl PickScreen {
    /// For `at`; the row the focus opens on, what the slot holds.
    pub(crate) fn open_on(&mut self, at: (usize, usize), pages: &Pages) -> PickRow {
        *self = Self { at, picked: false };
        self.current(pages).unwrap_or(PickRow::Pick(None))
    }
    /// The page and the slot it is for.
    pub(crate) fn at(&self) -> (usize, usize) {
        self.at
    }
    /// A pick changed the pages since the last call, which the editor's Undo takes back.
    pub(crate) fn take_picked(&mut self) -> bool {
        std::mem::take(&mut self.picked)
    }
    /// The row of what the slot holds; none for an action this game doesn't know.
    fn current(&self, pages: &Pages) -> Option<PickRow> {
        match pages.page(self.at.0).slot(self.at.1) {
            Slot::Empty => Some(PickRow::Pick(None)),
            Slot::Action(action) => Some(PickRow::Pick(Some(*action))),
            Slot::Missing(_) => None,
        }
    }
}

impl Screen for PickScreen {
    type Page = ();
    type Row = PickRow;
    fn title(&self) -> &'static str {
        PICK_TITLE
    }
    fn stamp(&self, s: &Settings) -> u64 {
        let (page, slot) = self.at;
        shell::stamp_of(&(self.at, s.controller.radial.page(page).slot(slot)))
    }
    fn sheets(&self, s: &Settings) -> Vec<Sheet<(), PickRow>> {
        let pages = &s.controller.radial;
        let (page, slot) = self.at;
        let direction = lang::get(&format!("horizon.radial.slot.{slot}"));
        let named = lang::tr(
            "horizon.radial.pick.slot",
            &[&direction, &page_label(pages.page(page))],
        );
        let current = self.current(pages);
        let about = lang::get("horizon.radial.pick.about");
        let mut rows =
            vec![Row::new(PickRow::Slot, named.clone(), Value::Note).about(about.clone())];
        for action in std::iter::once(None).chain(RadialAction::ALL.map(Some)) {
            let id = PickRow::Pick(action);
            let value = if current == Some(id) {
                Value::Choice(lang::get("horizon.radial.pick.current"))
            } else {
                Value::Press
            };
            let name = slot_label(&action.into());
            rows.push(Row::new(id, name, value).about(about.clone()));
        }
        vec![Sheet {
            id: (),
            name: named,
            about,
            groups: vec![Group {
                name: PICK_GROUP,
                rows,
            }],
        }]
    }
    fn press(&mut self, s: &mut Settings, row: PickRow, _back: bool) -> Vec<Output> {
        let PickRow::Pick(action) = row else {
            return Vec::new();
        };
        let (page, slot) = self.at;
        s.controller.radial.set_slot(page, slot, action.into());
        self.picked = true;
        vec![Output::Event(Event::Changed), Output::Done]
    }
}

/// The name field and its buttons, centred in the screen but under the title, one row apart.
const NAME_W: f32 = 200.0;
const NAME_BLOCK_H: f32 = 100.0;
const NAME_TOP_MIN: f32 = 30.0;
const NAME_STEP: f32 = 25.0;

/// What a press on the name's page did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Named {
    Nothing,
    /// Done: the page takes the name.
    Done,
    /// Cancel, keeping the old name.
    Back,
}

/// A page's name being typed.
pub(crate) struct NameEntry {
    page: usize,
    field: ui::EditBox,
}

impl NameEntry {
    fn new(page: usize, name: &str) -> Self {
        let mut field = ui::EditBox::new(0.0, 0.0, NAME_W, BUTTON_H);
        field.max_length = radial::NAME_MAX;
        field.set_value(name);
        field.focused = true;
        Self { page, field }
    }

    /// Done and Cancel, under the field.
    fn buttons(w: f32, h: f32) -> [([f32; 4], Named); 2] {
        let (x, top) = ((w / 2.0).floor() - NAME_W / 2.0, name_top(h));
        let at = |row: f32| [x, top + row * NAME_STEP, NAME_W, BUTTON_H];
        [(at(1.0), Named::Done), (at(2.0), Named::Back)]
    }

    pub(crate) fn draw(
        &mut self,
        batch: &mut HudBatch,
        atlas: &HudAtlas,
        cursor: (f32, f32),
        in_world: bool,
    ) {
        let (w, h) = (batch.width.ceil(), batch.height.ceil());
        ui::menu_background(batch, atlas, 0.0, 0.0, w, h, in_world);
        let title = Text::literal(lang::get("horizon.radial.name_title"));
        style::title(batch, atlas, &title);
        let field = &mut self.field;
        let (x, top) = ((w / 2.0).floor() - NAME_W / 2.0, name_top(h));
        (field.x, field.y, field.width, field.height) = (x, top, NAME_W, BUTTON_H);
        field.draw(batch, atlas, None);
        for ([x, y, bw, bh], named) in Self::buttons(w, h) {
            let (label, action) = match named {
                Named::Done => ("gui.done", crate::widgets::Action::Done),
                _ => ("gui.cancel", crate::widgets::Action::Cancel),
            };
            let hover = ui::hovered(cursor, x, y, bw, bh);
            let text = Text::literal(lang::get(label));
            ui::action_button(batch, atlas, action, x, y, bw, bh, &text, true, hover);
        }
    }

    /// A press at `(x, y)` on a screen `w` x `h`.
    pub(crate) fn click(
        &mut self,
        atlas: &HudAtlas,
        (x, y): (f32, f32),
        (w, h): (f32, f32),
    ) -> Named {
        let hit = Self::buttons(w, h)
            .into_iter()
            .find(|([bx, by, bw, bh], _)| ui::hovered((x, y), *bx, *by, *bw, *bh));
        if let Some((_, named)) = hit {
            return named;
        }
        let field = &mut self.field;
        field.focused = ui::hovered((x, y), field.x, field.y, field.width, field.height);
        if field.focused {
            field.click(atlas, x, false, false);
        }
        Named::Nothing
    }

    /// The name field takes the keys.
    pub(crate) fn typing(&self) -> bool {
        self.field.focused
    }

    pub(crate) fn type_text(&mut self, text: &str) {
        if self.field.focused {
            self.field.insert_text(text);
        }
    }

    /// A key: Enter is Done, Escape is Cancel, the rest the field's.
    pub(crate) fn key(&mut self, key: winit::keyboard::KeyCode, text: Option<&str>) -> Named {
        use winit::keyboard::KeyCode as K;
        match key {
            K::Escape => return Named::Back,
            K::Enter | K::NumpadEnter => return Named::Done,
            _ if self.field.focused => {
                if !self.field.key(key, false, false)
                    && let Some(t) = text
                {
                    let typed: String = t.chars().filter(|c| !c.is_control()).collect();
                    self.field.insert_text(&typed);
                }
            }
            _ => {}
        }
        Named::Nothing
    }

    /// Ctrl+A/C/V/X in the name field; false when it hasn't the focus.
    pub(crate) fn shortcut(
        &mut self,
        shortcut: crate::clipboard::EditShortcut,
        clipboard: &mut dyn crate::clipboard::Clipboard,
    ) -> bool {
        if self.field.focused {
            self.field.shortcut(shortcut, clipboard);
        }
        self.field.focused
    }

    /// The name into the pages. Left as it was, a default page keeps following the language.
    pub(crate) fn apply(&self, pages: &mut Pages) -> bool {
        let changed = self.field.value != page_label(pages.page(self.page));
        if changed {
            pages.rename(self.page, &self.field.value);
        }
        changed
    }
}

/// The name field's top.
fn name_top(h: f32) -> f32 {
    ((h - NAME_BLOCK_H) / 2.0).floor().max(NAME_TOP_MIN)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gamepad::Button as PadButton;
    use crate::radial::PageName;
    use crate::settings_shell::{Hosted, Shell};
    use winit::keyboard::KeyCode as K;

    const SIZE: (f32, f32) = (854.0, 480.0);

    fn rows_of(s: &Settings, page: usize) -> Vec<Row<RadialRow>> {
        let sheets = RadialScreen::default().sheets(s);
        let groups = sheets.into_iter().nth(page).unwrap().groups;
        groups.into_iter().flat_map(|group| group.rows).collect()
    }
    fn row(s: &Settings, id: RadialRow) -> Row<RadialRow> {
        let page = match id {
            RadialRow::Slot(page, _) | RadialRow::Rename(page) | RadialRow::Delete(page) => page,
            _ => 0,
        };
        rows_of(s, page).into_iter().find(|r| r.id == id).unwrap()
    }
    /// A click on `id` through the shell, as the mouse or a controller's A gives it.
    fn click(shell: &mut Shell<RadialScreen>, s: &mut Settings, id: RadialRow) -> Vec<Output> {
        let at = shell.point_at(s, SIZE, id);
        shell.click(s, at, SIZE, false, false, false)
    }

    /// Undo, at the foot over Done.
    fn undo(shell: &mut Shell<RadialScreen>, s: &mut Settings) {
        let [x, y, ..] = shell::Columns::of(SIZE.0, SIZE.1).button(1);
        shell.click(s, (x + 2.0, y + 2.0), SIZE, false, false, false);
    }

    #[test]
    fn a_slot_steps_through_empty_and_every_action_both_ways() {
        let mut seen = vec![Slot::Empty];
        let mut slot = Slot::Empty;
        for _ in RadialAction::ALL {
            slot = step(&slot, false);
            seen.push(slot.clone());
        }
        assert_eq!(step(&slot, false), Slot::Empty, "the last wraps to Empty");
        let actions: Vec<_> = seen.iter().filter_map(Slot::action).collect();
        assert_eq!(actions, RadialAction::ALL.to_vec());
        assert_eq!(
            step(&Slot::Empty, true).action(),
            RadialAction::ALL.last().copied()
        );
        let chat = Slot::Action(RadialAction::Chat);
        assert_eq!(step(&step(&chat, true), false), chat);
        // A missing action steps on as Empty would.
        let missing = Slot::Missing("mymod:dash".to_owned());
        assert_eq!(step(&missing, false), step(&Slot::Empty, false));
    }

    #[test]
    fn a_press_on_a_slot_opens_its_list_and_left_and_right_still_step_it() {
        let mut s = Settings::default();
        let mut shell = Shell::<RadialScreen>::default();
        let before = s.controller.radial.clone();
        let outputs = click(&mut shell, &mut s, RadialRow::Slot(0, 2));
        assert!(outputs.contains(&Output::Push(Page::ControllerRadialPick)));
        assert_eq!(shell.screen.picking, Some((0, 2)));
        assert_eq!(
            s.controller.radial, before,
            "the press itself changes nothing"
        );
        // Right on the focused row steps it.
        shell.set_focus(Some(shell::Target::Row(RadialRow::Slot(0, 2))));
        shell.key(&mut s, K::ArrowRight, None, false, SIZE);
        let stepped = step(before.page(0).slot(2), false);
        assert_eq!(*s.controller.radial.page(0).slot(2), stepped);
    }

    #[test]
    fn the_list_names_the_slot_marks_its_action_and_a_pick_fills_it() {
        let mut s = Settings::default();
        let mut picker = PickScreen::default();
        // General's right slot holds Commands.
        let opens_on = picker.open_on((0, 2), &s.controller.radial);
        assert_eq!(opens_on, PickRow::Pick(Some(RadialAction::Command)));
        let rows: Vec<_> = picker
            .sheets(&s)
            .into_iter()
            .flat_map(|sheet| sheet.groups)
            .flat_map(|group| group.rows)
            .collect();
        assert_eq!(rows[0].id, PickRow::Slot);
        assert_eq!(rows[0].value, Value::Note);
        // Empty, then every action, each once; only the slot's own marked.
        let picks: Vec<_> = rows[1..].iter().map(|row| row.id).collect();
        let every: Vec<_> = std::iter::once(None)
            .chain(RadialAction::ALL.map(Some))
            .map(PickRow::Pick)
            .collect();
        assert_eq!(picks, every);
        let marked: Vec<_> = rows
            .iter()
            .filter(|row| matches!(row.value, Value::Choice(_)))
            .map(|row| row.id)
            .collect();
        assert_eq!(marked, [opens_on]);
        let outputs = picker.press(&mut s, PickRow::Pick(Some(RadialAction::Chat)), false);
        assert_eq!(outputs, [Output::Event(Event::Changed), Output::Done]);
        assert_eq!(
            *s.controller.radial.page(0).slot(2),
            Slot::Action(RadialAction::Chat)
        );
        assert!(picker.take_picked() && !picker.take_picked());
        // Empty empties it.
        picker.press(&mut s, PickRow::Pick(None), false);
        assert!(s.controller.radial.page(0).slot(2).is_empty());
    }

    #[test]
    fn a_missing_action_says_which_it_is() {
        let mut s = Settings::default();
        s.controller
            .radial
            .set_slot(0, 1, Slot::Missing("mymod:dash".to_owned()));
        let english = lang::Language::load(&[], "en_us");
        let (chosen, _) = lang::scoped(english, || row(&s, RadialRow::Slot(0, 1)).chosen);
        assert!(chosen.is_some_and(|text| text.contains("mymod:dash")));
    }

    #[test]
    fn a_slot_changes_alone_and_undo_puts_back_what_the_screen_found() {
        let mut s = Settings::default();
        let mut shell = Shell::<RadialScreen>::default();
        let before = s.controller.radial.page(0).clone();
        shell.set_focus(Some(shell::Target::Row(RadialRow::Slot(0, 0))));
        let outputs = shell.key(&mut s, K::ArrowRight, None, false, SIZE);
        assert!(outputs.contains(&Output::Event(Event::Changed)));
        let after = s.controller.radial.page(0);
        assert_eq!(*after.slot(0), step(before.slot(0), false));
        assert_eq!(after.slots()[1..], before.slots()[1..]);
        assert!(shell.changed());
        undo(&mut shell, &mut s);
        assert_eq!(s.controller.radial, Pages::default());
        assert!(!shell.changed());
    }

    #[test]
    fn delete_and_reset_act_at_once_and_undo_takes_them_back() {
        let mut s = Settings::default();
        s.controller.radial.rename(1, "Mine");
        let found = s.controller.radial.clone();
        let mut shell = Shell::<RadialScreen>::default();
        click(&mut shell, &mut s, RadialRow::Delete(0));
        assert_eq!(s.controller.radial.len(), 1);
        assert_eq!(page_label(s.controller.radial.page(0)), "Mine");
        // One page left: it can't go, and the strip says why.
        let delete = row(&s, RadialRow::Delete(0));
        assert!(!delete.live && delete.why.is_some());
        let reset = row(&s, RadialRow::Reset);
        assert!(reset.live);
        shell.screen.take_shown();
        click(&mut shell, &mut s, RadialRow::Reset);
        assert_eq!(s.controller.radial, Pages::default());
        assert!(!row(&s, RadialRow::Reset).live, "already the defaults");
        assert_eq!(shell.screen.take_shown(), Some(0));
        undo(&mut shell, &mut s);
        assert_eq!(s.controller.radial, found);
    }

    #[test]
    fn new_pages_stop_at_the_most_and_the_new_one_shows() {
        let mut s = Settings::default();
        let mut screen = RadialScreen::default();
        while row(&s, RadialRow::NewPage).live {
            screen.press(&mut s, RadialRow::NewPage, false);
            assert_eq!(screen.take_shown(), Some(s.controller.radial.len() - 1));
        }
        assert_eq!(s.controller.radial.len(), radial::MAX_PAGES);
        assert!(row(&s, RadialRow::NewPage).why.is_some());
        // Every page is a sheet, named as the overlay names it.
        let sheets = screen.sheets(&s);
        assert_eq!(sheets.len(), radial::MAX_PAGES);
        assert_eq!(sheets[0].name, page_label(s.controller.radial.page(0)));
    }

    #[test]
    fn a_deleted_last_page_leaves_the_one_before_it_showing() {
        let mut s = Settings::default();
        let mut screen = RadialScreen::default();
        screen.press(&mut s, RadialRow::Delete(1), false);
        assert_eq!(screen.take_shown(), Some(0));
    }

    #[test]
    fn rename_opens_the_name_page_and_only_done_with_a_new_name_renames() {
        let general = Pages::default().page(0).name.clone();
        let mut s = Settings::default();
        let mut screen = RadialScreen::default();
        let outputs = screen.press(&mut s, RadialRow::Rename(0), false);
        assert_eq!(outputs, vec![Output::Push(Page::ControllerRadialName)]);
        let entry = screen.naming.as_mut().unwrap();
        // Done with the name as it was: a default page keeps following the language.
        assert_eq!(entry.key(K::Enter, None), Named::Done);
        assert!(!entry.apply(&mut s.controller.radial));
        assert_eq!(s.controller.radial.page(0).name, general);
        entry.type_text("!");
        assert_eq!(entry.key(K::Escape, None), Named::Back);
        entry.field.set_value("Mine");
        entry.type_text("!");
        assert!(entry.apply(&mut s.controller.radial));
        assert_eq!(
            s.controller.radial.page(0).name,
            PageName::Custom("Mine!".to_owned())
        );
    }

    #[test]
    fn undo_puts_back_a_name_given_on_the_name_page() {
        let mut s = Settings::default();
        s.controller.radial.rename(1, "Mine");
        let mut shell = Shell::<RadialScreen>::default();
        shell.show(1);
        click(&mut shell, &mut s, RadialRow::Rename(1));
        let entry = shell.screen.naming.as_mut().unwrap();
        entry.field.set_value("Building");
        // As the options screen does on Done: the name page changed what Undo puts back.
        let entry = shell.screen.naming.take().unwrap();
        assert!(entry.apply(&mut s.controller.radial));
        shell.changed_elsewhere();
        assert_eq!(page_label(s.controller.radial.page(1)), "Building");
        assert!(shell.changed());
        undo(&mut shell, &mut s);
        assert_eq!(page_label(s.controller.radial.page(1)), "Mine");
        assert!(!shell.changed());
    }

    #[test]
    fn a_nintendo_pad_going_by_its_letters_names_its_own_opener() {
        let mut s = Settings::default();
        // Selecting with the left stick, the top face button (North) can open it.
        s.controller.radial_stick = gamepad::RadialStick::Left;
        s.controller.bind(OPENER, Some(gamepad::Button::North));
        s.controller.glyphs = gamepad::GlyphStyle::Nintendo;
        let label = |s: &Settings| {
            let glyphs = Glyphs::of(&s.controller, gamepad::GlyphStyle::Nintendo);
            match opener_row(s, glyphs).value {
                Value::Binding(Binding {
                    shown: Shown::Pad { glyph, .. },
                    ..
                }) => glyph.expect("bound").label().to_owned(),
                other => panic!("{other:?}"),
            }
        };
        // By places, the top button: printed X on a Nintendo pad.
        s.controller.nintendo_letters = false;
        assert_eq!(label(&s), "X");
        // By letters, the part is played by the button printed Y (on the left).
        s.controller.nintendo_letters = true;
        assert_eq!(label(&s), "Y");
    }

    #[test]
    fn the_opener_row_names_its_button_and_says_what_gets_in_the_way() {
        let mut s = Settings::default();
        let glyphs = Glyphs::of(&s.controller, gamepad::GlyphStyle::Xbox);
        let shown = |s: &Settings| match opener_row(s, glyphs).value {
            Value::Binding(Binding {
                capture,
                shown: Shown::Pad { glyph, .. },
                ..
            }) => {
                assert_eq!(capture, Capture::Pad(OPENER));
                glyph
            }
            other => panic!("{other:?}"),
        };
        let bound = s.controller.binding(OPENER).unwrap();
        // The button on its glyph.
        let glyph = shown(&s).expect("the opener's glyph");
        assert_eq!(glyph.part(), bound);
        assert_eq!(glyph.label(), "→");
        assert_eq!(opener_row(&s, glyphs).chosen, None);
        s.controller.bind("key.chat", Some(bound));
        let shared = opener_row(&s, glyphs).chosen;
        assert!(shared.is_some(), "a shared button");
        s.controller.bind(OPENER, None);
        let unbound = opener_row(&s, glyphs).chosen;
        assert!(unbound.is_some() && unbound != shared && shown(&s).is_none());
        // A press waits for a button on the row, and Undo takes a new one back.
        let mut shell = Shell::<RadialScreen>::default();
        s.controller.bind(OPENER, Some(bound));
        let outputs = click(&mut shell, &mut s, RadialRow::Opener);
        assert!(outputs.contains(&Output::Bind(Capture::Pad(OPENER))));
        assert!(shell.changed());
        s.controller.bind(OPENER, Some(PadButton::North));
        undo(&mut shell, &mut s);
        assert_eq!(s.controller.binding(OPENER), Some(bound));
    }

    #[test]
    fn every_string_has_a_horizon_key() {
        let atlas =
            HudAtlas::build(std::path::Path::new("/nonexistent/horizon-test-assets")).unwrap();
        let english = lang::Language::load(&[], "en_us");
        let ((), missed) = lang::scoped(english, || {
            let mut s = Settings::default();
            s.controller.bind("key.chat", Some(PadButton::DPadRight));
            let mut shell = Shell::<RadialScreen>::default();
            let draw = |shell: &mut Shell<RadialScreen>, s: &Settings| {
                let mut batch = HudBatch::new([854, 480]);
                shell.draw(s, &mut batch, &atlas, (0.0, 0.0), false, None);
            };
            // Every page's rows are built, with what the strip says of each.
            draw(&mut shell, &s);
            s.controller.bind(OPENER, None);
            s.controller.radial.remove(1);
            while s.controller.radial.add().is_some() {}
            draw(&mut shell, &s);
            s.controller.radial = Pages::default();
            s.controller.radial.remove(1);
            draw(&mut shell, &s);
            let mut batch = HudBatch::new([854, 480]);
            let waiting = Some(Capture::Pad(OPENER));
            shell.draw(&s, &mut batch, &atlas, (0.0, 0.0), false, waiting);
            let mut entry = NameEntry::new(0, "Mine");
            let mut batch = HudBatch::new([854, 480]);
            entry.draw(&mut batch, &atlas, (0.0, 0.0), false);
            let mut picker = Shell::<PickScreen>::default();
            picker.screen.open_on((0, 1), &s.controller.radial);
            let mut batch = HudBatch::new([854, 480]);
            picker.draw(&s, &mut batch, &atlas, (0.0, 0.0), false, None);
        });
        // Vanilla's own (gui.done...) come from the game's resources, not bundled ones.
        let missed: Vec<_> = missed
            .iter()
            .filter(|key| key.starts_with("horizon.") || !key.contains('.'))
            .collect();
        assert!(missed.is_empty(), "no text for {missed:?}");
    }
}
