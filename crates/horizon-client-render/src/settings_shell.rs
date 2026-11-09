//! What Horizon's searchable settings screens share: the frame's geometry, its list of pages,
//! groups and rows, and how the keyboard moves through them. Which settings a screen has, and what
//! they do, stay in that screen; the look comes from `graphics_style`.
//!
//! A new screen implements [`Screen`], describing its rows from the settings, and [`Shell`] does
//! everything else: drawing, clicks, one scroll path, a controller's focus and its declared
//! buttons, keys and each row's description (`extras_ui` is the smallest; `graphics_ui` has
//! pages, sliders, costs and Undo; `controller_ui` has bindings, [`Value::Binding`], whose wait
//! for a button or a key the shell runs; `shader_ui` has pages read from a shader pack, rows that
//! link to pages out of the page list, and Apply). A page list with more pages than the column
//! has room for collapses into one `‹ Name ›` row (`hud_kit::tabs`), leaving none out.
use crate::{
    graphics_style as style,
    hud::{HudAtlas, HudBatch},
    hud_kit::tabs,
    lang,
    pad_glyph::{self, Glyph},
    settings::Settings,
    text::Text,
    ui,
};
use std::rc::Rc;

/// The screen's parts in GUI pixels. Down the left: the mark, the search
/// field, the page list, and Undo and Done at the foot. On the right: the
/// plate with the open page's name and its list, and the strip under it that
/// describes the hovered setting. `list_x`, `list_w`, `top` and `bottom` are
/// the list's view.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Columns {
    pub(crate) side_x: f32,
    pub(crate) side_y: f32,
    pub(crate) plate: [f32; 4],
    pub(crate) dock: [f32; 4],
    pub(crate) list_x: f32,
    pub(crate) list_w: f32,
    pub(crate) top: f32,
    pub(crate) bottom: f32,
    /// The lines the strip has for a description (`with_note_lines`).
    note_lines: usize,
}
/// The lines of description a strip has unless its screen asks for more.
const NOTE_LINES: usize = 2;
/// The height of a line of description.
const NOTE_LINE_H: f32 = 10.0;
/// The strip's text from its left edge, and its first line (the name) from its top.
const DOCK_INSET: f32 = 12.0;
const DOCK_NAME_Y: f32 = 8.0;
/// A cost's square before the name: its side, how far down it sits, and the name after it.
const COST_SQUARE: f32 = 4.0;
const COST_SQUARE_Y: f32 = 10.0;
const COST_NAME_X: f32 = 8.0;
/// What the strip keeps clear at its right of a name with its cost.
const COST_LINE_SLACK: f32 = 32.0;
/// A taller strip leaves the list at least this high.
const MIN_LIST_H: f32 = 60.0;
/// From one place in the page list to the next.
const PAGE_STEP: f32 = 18.0;
/// Between the foot's buttons, over and beside one another, and over them.
const FOOT_GAP: f32 = 4.0;
impl Columns {
    pub(crate) fn of(w: f32, h: f32) -> Self {
        let side_w = style::CONTROL_W;
        let gap = if w >= 560.0 { 16.0 } else { 8.0 };
        let pw = (w - 16.0 - side_w - gap).min(432.0).floor();
        let side_x = ((w - side_w - gap - pw) / 2.0).floor();
        let px = side_x + side_w + gap;
        // A short screen (a 720p window at scale 3) gives the margin to the list.
        let side_y = if h >= style::TITLED { 24.0 } else { 8.0 };
        let dock = [px, h - 8.0 - style::DOCK_H, pw, style::DOCK_H];
        let plate = [px, side_y, pw, dock[1] - 4.0 - side_y];
        let list_x = px + 8.0;
        Self {
            side_x,
            side_y,
            plate,
            dock,
            list_x,
            list_w: pw - 16.0 - style::SCROLLER_W - 2.0,
            top: side_y + 30.0,
            bottom: plate[1] + plate[3] - 6.0,
            note_lines: NOTE_LINES,
        }
    }
    /// The strip grown up over the plate for `wanted` lines of description, as far as the list
    /// keeps its height.
    pub(crate) fn with_note_lines(mut self, wanted: usize) -> Self {
        let spare = ((self.bottom - self.top - MIN_LIST_H) / NOTE_LINE_H).max(0.0) as usize;
        let grown = wanted.saturating_sub(NOTE_LINES).min(spare);
        let extra = grown as f32 * NOTE_LINE_H;
        self.dock[1] -= extra;
        self.dock[3] += extra;
        self.plate[3] -= extra;
        self.bottom -= extra;
        self.note_lines = NOTE_LINES + grown;
        self
    }
    /// The search field, under the mark.
    pub(crate) fn search(&self) -> [f32; 4] {
        [
            self.side_x,
            self.side_y + 26.0,
            style::CONTROL_W,
            style::SEARCH_H,
        ]
    }
    /// The search field's clear button, at its right end.
    pub(crate) fn clear(&self) -> [f32; 4] {
        let [x, y, w, h] = self.search();
        [x + w - h, y, h, h]
    }
    /// Room for vanilla's scroller, whose thumb is 32 high and 8 short of its track.
    pub(crate) fn scrolls(&self) -> bool {
        self.bottom - self.top >= 40.0
    }
    pub(crate) fn bar_x(&self) -> f32 {
        self.plate[0] + self.plate[2] - 8.0 - style::SCROLLER_W
    }
    /// The `i`th place in the page list, under the search field: a page, or a line naming the
    /// pages under it.
    pub(crate) fn page(&self, i: usize) -> [f32; 4] {
        [
            self.side_x,
            self.side_y + 56.0 + i as f32 * PAGE_STEP,
            style::CONTROL_W,
            16.0,
        ]
    }
    /// How many places the page list has room for, from its first down to above Undo.
    pub(crate) fn page_room(&self) -> usize {
        let room = self.button(1)[1] - FOOT_GAP - self.page(0)[1];
        (room / PAGE_STEP).floor().max(0.0) as usize
    }
    /// Done (`0`), and Undo over it (`1`): under the page list, level with the strip.
    pub(crate) fn button(&self, i: usize) -> [f32; 4] {
        let foot = self.dock[1] + self.dock[3] - style::BUTTON_H;
        let y = if i == 0 {
            foot
        } else {
            foot - FOOT_GAP - style::BUTTON_H
        };
        [self.side_x, y, style::CONTROL_W, style::BUTTON_H]
    }
    /// A row's control column: its button, tick box or slider.
    pub(crate) fn track(&self) -> (f32, f32) {
        let x = self.list_x + self.list_w - style::INSET - style::CONTROL_W;
        (x, style::CONTROL_W)
    }
    /// A slider's value for the pointer at `x`: the handle's middle follows it.
    pub(crate) fn slider_at(&self, x: f32) -> f32 {
        let (tx, tw) = self.track();
        ((x - tx - 5.0) / (tw - 10.0)).clamp(0.0, 1.0)
    }
    /// A row's whole width at `y`.
    pub(crate) fn row(&self, y: f32) -> [f32; 4] {
        [self.list_x, y, self.list_w, style::ROW]
    }
}

/// A line of the list at `y` in its content: a page's name (only while
/// searching, which shows every page), a group's name with the height of the
/// panel its rows sit on, or a setting.
#[derive(Clone, Copy)]
pub(crate) struct Line<P, S> {
    pub(crate) y: f32,
    pub(crate) page: P,
    pub(crate) group: Option<(&'static str, f32)>,
    pub(crate) setting: Option<S>,
}
impl<P, S> Line<P, S> {
    pub(crate) fn h(&self) -> f32 {
        match (&self.setting, self.group) {
            (Some(_), _) => style::ROW,
            (None, Some(_)) => style::GROUP_ROW,
            (None, None) => style::PAGE_ROW,
        }
    }
}

/// Lays lines out from the top.
pub(crate) struct Lines<P, S> {
    lines: Vec<Line<P, S>>,
    y: f32,
}
impl<P: Copy, S: Copy> Lines<P, S> {
    pub(crate) fn new() -> Self {
        Self {
            lines: Vec::new(),
            y: 0.0,
        }
    }
    /// A page's name over its matches, while searching.
    pub(crate) fn page(&mut self, page: P) {
        if !self.lines.is_empty() {
            self.y += style::PAGE_GAP;
        }
        self.lines.push(Line {
            y: self.y,
            page,
            group: None,
            setting: None,
        });
        self.y += style::PAGE_ROW;
    }
    /// A named group of rows.
    pub(crate) fn group(&mut self, page: P, name: &'static str, rows: Vec<S>) {
        self.lines.push(Line {
            y: self.y,
            page,
            group: Some((name, rows.len() as f32 * style::ROW)),
            setting: None,
        });
        self.y += style::GROUP_ROW;
        for setting in rows {
            self.lines.push(Line {
                y: self.y,
                page,
                group: None,
                setting: Some(setting),
            });
            self.y += style::ROW;
        }
        self.y += style::GROUP_GAP;
    }
    /// A group of rows under its page's own name (`list`'s `page_name`), for pages named at run
    /// time, such as a resource pack's config entries (`pack_options_ui`).
    pub(crate) fn titled(&mut self, page: P, rows: Vec<S>) {
        self.group(page, "", rows);
    }
    /// The lines and the content's height.
    pub(crate) fn finish(self) -> (Vec<Line<P, S>>, f32) {
        (self.lines, (self.y - style::GROUP_GAP).max(0.0))
    }
}

/// The world or the menu's background, veiled.
pub(crate) fn backdrop(batch: &mut HudBatch, atlas: &HudAtlas, in_world: bool) {
    let (w, h) = (batch.width, batch.height);
    ui::menu_background(batch, atlas, 0.0, 0.0, w, h, in_world);
    style::fill(batch, atlas, [0.0, 0.0, w, h], style::VEIL);
}

/// The mark and the screen's name over the search field.
pub(crate) fn mark(batch: &mut HudBatch, atlas: &HudAtlas, cols: &Columns, title: &str) {
    let (x, y) = (cols.side_x, cols.side_y);
    style::fill(batch, atlas, [x, y + 2.0, 6.0, 6.0], style::ACCENT);
    let horizon = Text::literal("Horizon");
    batch.text_styled(atlas, &horizon, x + 10.0, y, style::WHITE, true);
    let title = Text::literal(title);
    batch.text_styled(atlas, &title, x, y + 10.0, style::MUTED, true);
}

/// A page in the page list: the open page on a plate with the accent's bar beside it, the
/// others bare text. `has`: something to show under it.
pub(crate) fn page_tab(
    batch: &mut HudBatch,
    atlas: &HudAtlas,
    rect: [f32; 4],
    name: &str,
    (has, open, focused): (bool, bool, bool),
    cursor: (f32, f32),
) {
    let [px, py, pw, ph] = rect;
    crate::widgets::record(name, rect, has);
    let hover = has && ui::hovered(cursor, px, py, pw, ph);
    crate::widgets::narratable(|| {
        crate::widgets::Narratable::button(name.to_owned(), rect, has)
            .hovered(hover)
            .focused(focused)
    });
    crate::widgets::widget_cursor(hover, has);
    if has {
        crate::widgets::target_as(rect, crate::widgets::Kind::Tab(open));
    }
    if open {
        style::fill(batch, atlas, rect, style::OPEN);
        style::fill(batch, atlas, [px, py, 2.0, ph], style::ACCENT);
    }
    let color = match (open || hover, has) {
        (true, _) => style::WHITE,
        (false, true) => style::SOFT,
        (false, false) => style::MUTED,
    };
    let ty = py + ((ph - 8.0) / 2.0).floor();
    let name = Text::literal(style::fit(atlas, name, pw - 12.0));
    batch.text_styled(atlas, &name, px + 8.0, ty, color, true);
    if focused {
        style::ring(batch, atlas, rect, style::WHITE);
    }
}

/// The search field, with its hint or clear button.
pub(crate) fn search_field(
    search: &mut ui::EditBox,
    batch: &mut HudBatch,
    atlas: &HudAtlas,
    cols: &Columns,
    cursor: (f32, f32),
    focused: bool,
) {
    let rect = cols.search();
    style::well(batch, atlas, rect);
    search.focused = focused;
    let [x, y, _, h] = rect;
    let clear = cols.clear();
    (search.x, search.y) = (x + 5.0, y + ((h - 8.0) / 2.0).floor());
    search.width = clear[0] - search.x;
    search.text_color = style::WHITE;
    // The clear button isn't the field's: a click at the field's end must not press it.
    let frame = [x, y, clear[0] - x, h];
    search.frame = Some(frame);
    search.draw(batch, atlas, None);
    if search.value.is_empty() && !focused {
        let hint = Text::literal(search.message.clone());
        batch.text_styled(atlas, &hint, x + 5.0, search.y, style::MUTED, true);
    }
    if !search.value.is_empty() {
        let hover = ui::hovered(cursor, clear[0], clear[1], clear[2], clear[3]);
        crate::widgets::widget_cursor(hover, true);
        let color = if hover { style::WHITE } else { style::MUTED };
        let cy = y + ((h - 8.0) / 2.0).floor();
        ui::centered_text(
            batch,
            atlas,
            &Text::literal("×"),
            clear[0],
            cy,
            clear[2],
            color,
        );
    }
    if focused {
        style::ring(batch, atlas, frame, style::WHITE);
    }
}

/// The plate, with the open page's name (or the search's) over its list.
pub(crate) fn plate(batch: &mut HudBatch, atlas: &HudAtlas, cols: &Columns, heading: &str) {
    let [px, py, pw, _] = cols.plate;
    style::panel(batch, atlas, cols.plate);
    let heading = Text::literal(style::fit(atlas, heading, pw - 24.0));
    batch.text_styled(atlas, &heading, px + 12.0, py + 10.0, style::WHITE, true);
    style::fill(
        batch,
        atlas,
        [px + 8.0, py + 24.0, pw - 16.0, 1.0],
        style::HEAD_RULE,
    );
}

/// The list, clipped to its view: each group's panel, then its names and rows on it. `row`
/// draws a setting at its `y`; `page_name` names a page, and a group without a name of its own
/// (`Lines::titled`), in the player's language.
pub(crate) fn list<P: Copy, S: Copy>(
    batch: &mut HudBatch,
    atlas: &HudAtlas,
    cols: &Columns,
    lines: &[Line<P, S>],
    scroll: f32,
    page_name: impl Fn(P) -> String,
    mut row: impl FnMut(&mut HudBatch, S, f32),
) {
    let start = batch.vertices.len();
    let mark = crate::widgets::targets_mark();
    let row_y = |line: &Line<P, S>| cols.top + line.y - scroll;
    let shows = |y: f32, h: f32| y + h > cols.top && y < cols.bottom;
    for line in lines {
        let y = row_y(line);
        if let Some((_, panel)) = line.group
            && shows(y + style::GROUP_ROW, panel)
        {
            let rect = [cols.list_x, y + style::GROUP_ROW, cols.list_w, panel];
            style::group(batch, atlas, rect);
        }
    }
    // A group's first row, which has no rule over it.
    let mut first = false;
    for line in lines {
        let y = row_y(line);
        let after_name = std::mem::replace(&mut first, line.group.is_some());
        if !shows(y, line.h()) {
            continue;
        }
        let Some(setting) = line.setting else {
            let (name, color, dy) = match line.group {
                Some(("", _)) => (page_name(line.page), style::MUTED, 3.0),
                Some((name, _)) => (lang::get(name), style::MUTED, 3.0),
                None => (page_name(line.page), style::ACCENT, 4.0),
            };
            let name = Text::literal(name);
            batch.text_styled(atlas, &name, cols.list_x + 2.0, y + dy, color, true);
            continue;
        };
        if !after_name {
            let rule = [cols.list_x + 8.0, y, cols.list_w - 16.0, 1.0];
            style::fill(batch, atlas, rule, style::ROW_RULE);
        }
        row(batch, setting, y);
    }
    if lines.is_empty() {
        let none = Text::literal(lang::get("No settings match."));
        batch.text_styled(
            atlas,
            &none,
            cols.list_x + 2.0,
            cols.top + 4.0,
            style::MUTED,
            true,
        );
    }
    batch.clip_since(
        start,
        cols.list_x,
        cols.top,
        cols.list_x + cols.list_w,
        cols.bottom,
    );
    crate::widgets::clip_targets(mark, cols.top, cols.bottom);
}

/// A control's label as its name and its value ("Render Distance" and "12 chunks").
pub(crate) fn name_and_value(label: &str) -> (String, Option<String>) {
    // `options.generic_value` ("%s: %s") joins vanilla's; Horizon's use ": " too.
    let format = lang::get("options.generic_value");
    let sep = format
        .split("%s")
        .nth(1)
        .filter(|sep| !sep.is_empty() && label.contains(*sep))
        .unwrap_or(": ");
    match label.split_once(sep) {
        Some((name, value)) => (name.to_owned(), Some(value.to_owned())),
        None => (label.to_owned(), None),
    }
}

/// A slider's position one step on: a step of its own, or a pixel of its `width` wide track.
pub(crate) fn slider_step(pos: f32, steps: Option<u32>, width: f32, forward: bool) -> f32 {
    if let Some(steps) = steps {
        let steps = steps as f32;
        let current = (pos * steps).round();
        let next = if forward {
            current + 1.0
        } else {
            current - 1.0
        };
        (next / steps).clamp(0.0, 1.0)
    } else {
        let delta = 1.0 / (width - 8.0).max(1.0);
        if forward {
            (pos + delta).min(1.0)
        } else {
            (pos - delta).max(0.0)
        }
    }
}

/// A setting's row as the screen reads it.
pub(crate) struct RowView<'a> {
    /// The whole caption ("Render Distance: 12 chunks"), for the narrator.
    pub(crate) label: &'a str,
    pub(crate) name: &'a str,
    pub(crate) value: Option<&'a str>,
    pub(crate) switch: Option<bool>,
    pub(crate) slider: Option<f32>,
    /// Can be used now.
    pub(crate) live: bool,
    pub(crate) hover: bool,
    pub(crate) focused: bool,
    /// Its slider is being dragged.
    pub(crate) held: bool,
    /// A key binding, drawn as a button rather than a value between arrows: whether it waits for
    /// its key.
    pub(crate) key: Option<bool>,
}

/// A setting's row at `y`, its name on the left and its control on the right.
pub(crate) fn row(batch: &mut HudBatch, atlas: &HudAtlas, cols: &Columns, y: f32, view: &RowView) {
    let &RowView {
        label,
        name,
        value,
        switch,
        slider,
        live,
        hover,
        focused,
        held,
        key,
    } = view;
    let rect = cols.row(y);
    let (tx, tw) = cols.track();
    let right = tx + tw;
    let control = [
        tx,
        y + (style::ROW - style::CONTROL_H) / 2.0,
        tw,
        style::CONTROL_H,
    ];
    let active = live && (hover || focused || held);
    if active {
        let lit = [rect[0] + 2.0, y + 1.0, rect[2] - 4.0, style::ROW - 2.0];
        style::fill(batch, atlas, lit, style::HOVER);
    }
    crate::widgets::widget_cursor(hover, live);
    // The control socket's widget tree, the narrator and a controller's focus.
    let kind = match slider {
        Some(value) => crate::widgets::Kind::Slider {
            value,
            track: [tx + 5.0, tw - 10.0],
        },
        None => crate::widgets::Kind::Button,
    };
    crate::widgets::record_as(label, rect, live, kind);
    crate::widgets::narratable(|| {
        match (switch, slider) {
            (Some(on), _) => crate::widgets::Narratable::checkbox(name.to_owned(), on, rect, live),
            (_, Some(_)) => crate::widgets::Narratable::slider(label.to_owned(), rect, live),
            _ => crate::widgets::Narratable::button(label.to_owned(), rect, live),
        }
        .hovered(hover)
        .focused(focused)
    });
    if live {
        crate::widgets::target_as(rect, kind);
    }
    let color = if live { style::WHITE } else { style::MUTED };
    let soft = if live { style::SOFT } else { style::MUTED };
    let ty = y + ((style::ROW - 8.0) / 2.0).floor();
    let lit = live && (hover || held);
    let left = rect[0] + style::INSET;
    // An action (Calibrate...) is a button across the row, as vanilla's lists have them.
    if switch.is_none() && slider.is_none() && value.is_none() {
        let wide = [left, control[1], right - left, style::CONTROL_H];
        style::button(batch, atlas, wide, &Text::literal(name), (lit, live, false));
    } else {
        let value = value.unwrap_or_default();
        let vw = atlas.font.width(value);
        // Where the control starts, and the value's text for a slider at rest.
        let start = match (switch, slider) {
            (Some(_), _) => right - style::CONTROL_H,
            (None, Some(_)) if active => tx,
            (None, Some(_)) => right - vw,
            (None, None) => right - vw - 18.0,
        };
        match (switch, slider) {
            (Some(on), _) => style::checkbox(batch, atlas, right, y, on, live),
            // A slider shows its track, the value on it, while it is pointed at, focused or held.
            (None, Some(position)) => {
                let value = Text::literal(value);
                if active {
                    style::slider(batch, atlas, control, &value, position, (lit, live));
                } else {
                    batch.text_styled(atlas, &value, start, ty, soft, true);
                }
            }
            (None, None) if key.is_some() => {
                let value = Text::literal(value);
                let value = match key {
                    Some(true) => value.with_color(crate::text::YELLOW),
                    _ => value,
                };
                let chip = [start, y + 3.0, vw + 18.0, 14.0];
                style::button(batch, atlas, chip, &value, (lit, live, false));
            }
            (None, None) => {
                let value = Text::literal(value);
                style::cycler(
                    batch,
                    atlas,
                    [start, y + 3.0, vw + 18.0, 14.0],
                    &value,
                    (lit, live),
                );
            }
        }
        // The name, cut short before the control where a translation runs long.
        let shown = Text::literal(style::fit(atlas, name, start - 6.0 - left));
        batch.text_styled(atlas, &shown, left, ty, color, true);
    }
    if focused {
        style::ring(
            batch,
            atlas,
            [rect[0] + 1.0, y, rect[2] - 2.0, style::ROW],
            style::WHITE,
        );
    }
}

/// A binding's reset button, at the end of its row at `y`.
fn reset_rect(cols: &Columns, y: f32) -> [f32; 4] {
    let right = cols.list_x + cols.list_w - RESET_INSET;
    [right - RESET_W, y + RESET_INSET, RESET_W, style::CONTROL_H]
}

/// What a row takes the focus and a press on: all of it but a binding's reset.
fn row_rect<R>(row: &Row<R>, cols: &Columns, y: f32) -> [f32; 4] {
    let resets = row.binding().is_some_and(|b| b.reset.shown());
    short_of_reset(cols, y, resets)
}
/// The row at `y`, short of its reset button where it has one.
fn short_of_reset(cols: &Columns, y: f32, resets: bool) -> [f32; 4] {
    let [x, y, w, h] = cols.row(y);
    let w = if resets { w - RESET_W - RESET_GAP } else { w };
    [x, y, w, h]
}

/// A row's lit background while it is pointed at or focused.
fn lit(batch: &mut HudBatch, atlas: &HudAtlas, [x, y, w, _]: [f32; 4]) {
    style::fill(
        batch,
        atlas,
        [x + 2.0, y + 1.0, w - 4.0, style::ROW - 2.0],
        style::HOVER,
    );
}

/// A value to read ([`Value::Read`]) at `y`: its name, and the value at the right, cut short to
/// fit. `(label, name, value)`; `(hover, focused)`.
fn read_row(
    batch: &mut HudBatch,
    atlas: &HudAtlas,
    cols: &Columns,
    y: f32,
    (label, name, value): (&str, &str, &str),
    (hover, focused): (bool, bool),
) {
    let rect = cols.row(y);
    crate::widgets::record(label, rect, true);
    crate::widgets::narratable(|| {
        crate::widgets::Narratable::button(label.to_owned(), rect, true)
            .hovered(hover)
            .focused(focused)
    });
    crate::widgets::target(rect);
    if hover || focused {
        lit(batch, atlas, rect);
    }
    let ty = y + ((style::ROW - 8.0) / 2.0).floor();
    let left = rect[0] + style::INSET;
    let name_w = atlas.font.width(name);
    let shown = Text::literal(name.to_owned());
    batch.text_styled(atlas, &shown, left, ty, style::WHITE, true);
    let room = rect[2] - 2.0 * style::INSET - name_w - 8.0;
    let value = style::fit(atlas, value, room);
    let vx = rect[0] + rect[2] - style::INSET - atlas.font.width(&value);
    batch.text_styled(atlas, &Text::literal(value), vx, ty, style::SOFT, true);
    if focused {
        style::ring(
            batch,
            atlas,
            [rect[0] + 1.0, y, rect[2] - 2.0, style::ROW],
            style::WHITE,
        );
    }
}

/// A controller binding's row as the shell reads it ([`Shown::Pad`]).
struct PadRow<'a> {
    /// The whole caption, for the narrator and the widget tree.
    label: &'a str,
    name: &'a str,
    /// What the strip says, for the narrator.
    hint: &'a str,
    glyph: Option<Glyph>,
    waits: bool,
    shared: bool,
    reset: Reset,
    live: bool,
    /// The row and its reset: pointed at, focused.
    hover: (bool, bool),
    focus: (bool, bool),
}

/// A controller binding's row at `y`: its name, a chip with the bound button's art ("Not Bound"
/// while nothing is), marked "> <" in yellow while it waits and "[ ]" while its button does
/// something else too, and its reset at the end.
fn pad_row(batch: &mut HudBatch, atlas: &HudAtlas, cols: &Columns, y: f32, view: &PadRow) {
    let &PadRow {
        label,
        name,
        hint,
        glyph,
        waits,
        shared,
        reset,
        live,
        hover: (hover, reset_hover),
        focus: (focused, reset_focused),
    } = view;
    let rect = short_of_reset(cols, y, reset.shown());
    if live && (hover || focused) {
        lit(batch, atlas, rect);
    }
    crate::widgets::widget_cursor(hover, live);
    crate::widgets::record(label, rect, live);
    crate::widgets::narratable(|| {
        let mut n = crate::widgets::Narratable::button(label.to_owned(), rect, live)
            .hovered(hover)
            .focused(focused);
        n.hint = Some(hint.to_owned());
        n
    });
    if live {
        crate::widgets::target(rect);
    }
    // The chip: the button's art, or its name where there is no art or nothing is bound.
    let art = glyph.filter(|g| pad_glyph::art(atlas, *g).is_some());
    let text = match glyph {
        Some(g) => g.label().to_owned(),
        None => lang::get(NOT_BOUND),
    };
    let inner = match art {
        Some(g) => pad_glyph::width(atlas, g),
        None => atlas.font.width(&text),
    };
    let marks = match (waits, shared) {
        (true, _) => Some((">", "<")),
        (false, true) => Some(("[", "]")),
        (false, false) => None,
    };
    let mark_w = marks.map_or(0.0, |(open, close)| {
        atlas.font.width(open) + atlas.font.width(close) + 2.0 * PAD_MARK_GAP
    });
    let chip_w = (inner + mark_w + PAD_CHIP_PAD).max(PAD_CHIP_W);
    let chip = [
        rect[0] + rect[2] - chip_w - 2.0,
        y + (style::ROW - style::CONTROL_H) / 2.0,
        chip_w,
        style::CONTROL_H,
    ];
    // Text in the chip only where there is no art, yellow with its marks.
    let face = match (art, marks) {
        (Some(_), _) => Text::literal(String::new()),
        (None, Some(_)) => Text::literal(text).with_color(crate::text::YELLOW),
        (None, None) => Text::literal(text),
    };
    style::button(batch, atlas, chip, &face, (live && hover, live, false));
    let ty = y + ((style::ROW - 8.0) / 2.0).floor();
    let middle = chip[0] + chip[2] / 2.0;
    if let Some(g) = art {
        pad_glyph::draw(batch, atlas, g, (middle - inner / 2.0 + 1.0).floor(), ty);
    }
    if let Some((open, close)) = marks {
        let yellow = |mark: &str| Text::literal(mark.to_owned()).with_color(crate::text::YELLOW);
        let left = middle - inner / 2.0 - PAD_MARK_GAP - atlas.font.width(open);
        batch.text_styled(atlas, &yellow(open), left.floor(), ty, style::WHITE, true);
        let right = middle + inner / 2.0 + PAD_MARK_GAP;
        batch.text_styled(atlas, &yellow(close), right.ceil(), ty, style::WHITE, true);
    }
    let left = rect[0] + style::INSET;
    let color = if live { style::WHITE } else { style::MUTED };
    let shown = Text::literal(style::fit(atlas, name, chip[0] - 6.0 - left));
    batch.text_styled(atlas, &shown, left, ty, color, true);
    if focused {
        style::ring(batch, atlas, rect, style::WHITE);
    }
    if !reset.shown() {
        return;
    }
    // The reset: a target only where it would do something.
    let resets = live && reset == Reset::Changed;
    let reset = reset_rect(cols, y);
    let reset_label = lang::get("controls.reset");
    let reset_hover = resets && reset_hover;
    crate::widgets::widget_cursor(reset_hover, resets);
    crate::widgets::record(&reset_label, reset, resets);
    crate::widgets::narratable(|| {
        let what = lang::tr("narrator.controls.reset", &[&name]);
        crate::widgets::Narratable::button(what, reset, resets)
            .hovered(reset_hover)
            .focused(reset_focused)
    });
    if resets {
        crate::widgets::target(reset);
    }
    let shown = Text::literal(reset_label);
    style::button(batch, atlas, reset, &shown, (reset_hover, resets, false));
    if reset_focused {
        style::ring(batch, atlas, reset, style::WHITE);
    }
}

/// Lines a screen shows under its page list (`Screen::side`), from the place of the `below`th page.
fn side_lines(
    batch: &mut HudBatch,
    atlas: &HudAtlas,
    cols: &Columns,
    below: usize,
    lines: &[String],
) {
    let [x, y, w, _] = cols.page(below);
    for (i, line) in lines.iter().enumerate() {
        let rect = [
            x + SIDE_INSET,
            y + SIDE_INSET + i as f32 * SIDE_LINE_H,
            w - 2.0 * SIDE_INSET,
            8.0,
        ];
        crate::widgets::record(line, rect, false);
        let shown = Text::literal(style::fit(atlas, line, rect[2]));
        batch.text_styled(atlas, &shown, rect[0], rect[1], style::MUTED, true);
    }
}
/// A line under the page list: its inset, and its height.
const SIDE_INSET: f32 = 2.0;
const SIDE_LINE_H: f32 = 10.0;
/// A line naming a section of the page list, from the top of its place.
const HEADING_Y: f32 = 6.0;
/// What the narrator and the widget tree call the collapsed row's arrows: vanilla's words.
const PREVIOUS_PAGE: &str = "spectatorMenu.previous_page";
const NEXT_PAGE: &str = "spectatorMenu.next_page";

/// What a place in the page list is: a page, or, in its collapsed row, the shown page's name or
/// an arrow to the page before or after it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Part {
    Tab,
    Shown,
    Prev,
    Next,
}

/// The page list as laid out (`Shell::page_list`).
#[derive(Clone, Debug)]
pub(crate) struct PageList<P> {
    /// Where each page is picked, and as what.
    places: Vec<(P, [f32; 4], Part)>,
    /// The lines naming a section, and where.
    headings: Vec<(String, [f32; 4])>,
    /// The last section collapsed into one row, when the pages don't all fit, and its page.
    strip: Option<(tabs::Strip, P)>,
    /// The page it lights.
    lit: Option<P>,
    /// How many places it takes down the column.
    used: usize,
}
impl<P> Default for PageList<P> {
    fn default() -> Self {
        Self {
            places: Vec::new(),
            headings: Vec::new(),
            strip: None,
            lit: None,
            used: 0,
        }
    }
}

/// A place to pick in the collapsed row: the shown page's name, or an arrow to the page before
/// or after, for the pointer, the keyboard, a controller's focus and its bumpers (a `Tab` target,
/// lit for the open page), and the widget tree and narrator under `name`. The row draws the text
/// (`tabs::draw`); focused, `ring` is drawn round.
fn strip_place(
    batch: &mut HudBatch,
    atlas: &HudAtlas,
    (rect, ring): ([f32; 4], [f32; 4]),
    name: &str,
    (has, open, focused): (bool, bool, bool),
    cursor: (f32, f32),
) {
    let [x, y, w, h] = rect;
    crate::widgets::record(name, rect, has);
    let hover = has && ui::hovered(cursor, x, y, w, h);
    crate::widgets::narratable(|| {
        crate::widgets::Narratable::button(name.to_owned(), rect, has)
            .hovered(hover)
            .focused(focused)
    });
    crate::widgets::widget_cursor(hover, has);
    if has {
        crate::widgets::target_as(rect, crate::widgets::Kind::Tab(open));
    }
    if focused {
        style::ring(batch, atlas, ring, style::WHITE);
    }
}

/// A line naming the section of the page list under it, at the place `rect`.
fn heading(batch: &mut HudBatch, atlas: &HudAtlas, text: &str, [x, y, w, _]: [f32; 4]) {
    let rect = [x + SIDE_INSET, y + HEADING_Y, w - 2.0 * SIDE_INSET, 8.0];
    crate::widgets::record(text, rect, false);
    let shown = Text::literal(style::fit(atlas, text, rect[2]));
    batch.text_styled(atlas, &shown, rect[0], rect[1], style::MUTED, true);
}

/// Vanilla's scroller, inside the panel's right edge, sized as `ui::Scroll` drags it.
pub(crate) fn scroller(
    batch: &mut HudBatch,
    atlas: &HudAtlas,
    cols: &Columns,
    content: f32,
    scroll: f32,
) {
    if !cols.scrolls() {
        return;
    }
    let Some((y, bar)) = ui::scrollbar_thumb(cols.top, cols.bottom, content, scroll) else {
        return;
    };
    let x = cols.bar_x();
    let height = cols.bottom - cols.top;
    crate::widgets::scroll_area(
        [
            cols.list_x,
            cols.top,
            x + style::SCROLLER_W - cols.list_x,
            height,
        ],
        scroll,
        content - height,
    );
    let track = [x, cols.top, style::SCROLLER_W, height];
    style::scroller(batch, atlas, track, [x, y, style::SCROLLER_W, bar]);
}

/// The last line that fits of a description with more to say: it doesn't just stop.
fn cut_off(line: &str) -> String {
    format!("{}…", line.trim_end())
}

/// The strip's text under its first line.
pub(crate) fn dock_note(
    batch: &mut HudBatch,
    atlas: &HudAtlas,
    cols: &Columns,
    text: String,
    color: u32,
) {
    let [x, y, w, _] = cols.dock;
    let lines = atlas.font.split(&Text::literal(text), w - 24.0);
    let cut = lines.len() > cols.note_lines;
    for (i, line) in lines.iter().take(cols.note_lines).enumerate() {
        let marked =
            (cut && i + 1 == cols.note_lines).then(|| Text::literal(cut_off(&line.plain())));
        batch.text_styled(
            atlas,
            marked.as_ref().unwrap_or(line),
            x + 12.0,
            y + 20.0 + i as f32 * NOTE_LINE_H,
            color,
            true,
        );
    }
}

/// Done or Undo at the foot of the page list.
pub(crate) fn foot_button(
    batch: &mut HudBatch,
    atlas: &HudAtlas,
    rect: [f32; 4],
    label: &Text,
    cursor: (f32, f32),
    flags: (bool, bool),
) {
    foot_button_with_room(batch, atlas, rect, label, cursor, flags, 0.0);
}

/// A foot button whose label leaves `room` at its left for a controller's glyph: a button that
/// declared its job gets one, as vanilla's do (`ui::action_button`), which a half-width Undo or
/// Apply needs.
fn foot_button_with_room(
    batch: &mut HudBatch,
    atlas: &HudAtlas,
    rect: [f32; 4],
    label: &Text,
    cursor: (f32, f32),
    (focused, primary): (bool, bool),
    room: f32,
) {
    let [x, y, w, h] = rect;
    let hovered = ui::hovered(cursor, x, y, w, h);
    let hover = hovered || focused;
    crate::widgets::record(&label.plain(), rect, true);
    crate::widgets::narratable(|| {
        crate::widgets::Narratable::button(label.plain(), rect, true)
            .hovered(hovered)
            .focused(focused)
    });
    crate::widgets::widget_cursor(hovered, true);
    crate::widgets::target(rect);
    style::button_with_room(batch, atlas, rect, label, (hover, true, primary), room);
}

/// Of the rows (their target, `y` and whether they can take the focus), those the keys reach.
/// Tab reaches all. The arrows (`near`) only reach the rows in view and the next live one past
/// either edge, which scrolls in, over any greyed ones: a row farther out sits under the tabs or
/// the buttons and would take the step from them.
pub(crate) fn reach<T>(
    rows: &[(T, f32, bool)],
    cols: &Columns,
    near: bool,
) -> std::ops::Range<usize> {
    let in_view = |y: f32| y + style::ROW > cols.top && y < cols.bottom;
    match rows.iter().position(|r| in_view(r.1)) {
        Some(first) if near => {
            let last = rows.iter().rposition(|r| in_view(r.1)).unwrap_or(first);
            let lo = rows[..first].iter().rposition(|r| r.2).unwrap_or(first);
            let hi = rows[last + 1..]
                .iter()
                .position(|r| r.2)
                .map_or(last, |i| last + 1 + i);
            lo..hi + 1
        }
        None if near => 0..0,
        _ => 0..rows.len(),
    }
}

/// Where Tab or Shift+Tab goes among `n` controls.
pub(crate) fn tab_step(n: usize, current: Option<usize>, shift: bool) -> usize {
    match (current, shift) {
        (Some(i), false) => (i + 1) % n,
        (Some(i), true) => (i + n - 1) % n,
        (None, false) => 0,
        (None, true) => n - 1,
    }
}

/// Where an arrow key goes among the controls. Nothing focused in view: going down, the first
/// setting from the view's top on (the next past its bottom where greyed rows fill it); going
/// up, the last one down to its bottom. `is_row`: a control that is a setting's row.
pub(crate) fn arrow_step<T: Copy>(
    controls: &[(T, [f32; 4])],
    current: Option<usize>,
    key: winit::keyboard::KeyCode,
    cols: &Columns,
    is_row: impl Fn(T) -> bool,
) -> Option<usize> {
    use crate::gamepad::Button as B;
    use winit::keyboard::KeyCode as K;
    let dir = match key {
        K::ArrowUp => B::DPadUp,
        K::ArrowDown => B::DPadDown,
        K::ArrowLeft => B::DPadLeft,
        _ => B::DPadRight,
    };
    match current {
        Some(i) => {
            let rects: Vec<[f32; 4]> = controls.iter().map(|f| f.1).collect();
            crate::gamepad::focus_step_rect(controls[i].1, dir, &rects)
        }
        None => {
            let mut rows = controls.iter().enumerate().filter(|(_, (t, _))| is_row(*t));
            let pick = if key == K::ArrowUp {
                rows.rfind(|(_, (_, [_, y, ..]))| *y + style::ROW <= cols.bottom)
            } else {
                rows.find(|(_, (_, [_, y, ..]))| *y >= cols.top)
            };
            pick.map(|(i, _)| i)
        }
    }
}

/// Scroll just enough to bring a row at `rect` into the list's view.
pub(crate) fn scroll_into_view(
    scroll: &mut ui::Scroll,
    [_, y, _, h]: [f32; 4],
    cols: &Columns,
    content: f32,
) {
    if y < cols.top {
        scroll.amount -= cols.top - y;
    } else if y + h > cols.bottom {
        scroll.amount += y + h - cols.bottom;
    }
    scroll.clamp(cols.top, cols.bottom, content);
}

// ---- A screen on the shell: it describes its rows, the shell does the rest ----

/// Pixels the list moves per wheel step or stick notch (`AbstractScrollArea`'s rate).
const SCROLL_RATE: f32 = 18.0;

/// A row's value, which says how the shell draws and drives it.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Value {
    /// A tick box.
    Switch(bool),
    /// A value stepped between `<` `>`, as it reads now: a press steps it on, a Shift press back,
    /// and focused, Left and Right do the same.
    Choice(String),
    /// A slider: left and right step it (`steps`: its stops, else a pixel of the track), a press
    /// on the track sets it (a controller's focus does the same).
    Slider {
        position: f32,
        steps: Option<u32>,
        text: String,
    },
    /// A binding: a press makes the host wait for a key or a controller button
    /// ([`Output::Bind`]), and the row shows what it is bound to now ([`Binding`]).
    Binding(Binding),
    /// A value to read beside the name, which the focus reaches (unlike `Note`), so the strip and
    /// the narrator say what it is; nothing presses it.
    Read(String),
    /// A button across the row.
    Press,
    /// A line to read, which no focus reaches (unlike `Read`) and nothing presses.
    Note,
}

/// What a binding row waits for once pressed, by the name the host binds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Capture {
    /// A key or a mouse button, as on Key Binds (`OptionsUi::selected_key`): Escape unbinds.
    Key(&'static str),
    /// A controller button for a gameplay action (`OptionsUi::selected_pad_action`): the game binds
    /// the next button pressed, Start cancels, and the keyboard's Escape unbinds, as on Key Binds.
    Pad(&'static str),
}

/// What a binding row is bound to now.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Shown {
    /// The key's name, as Key Binds shows it.
    Key(String),
    /// The controller button's glyph, drawn as its art; `None` while nothing is bound. `cancel`
    /// is the button that cancels the wait (`MenuJob::Start`), named in the strip meanwhile.
    Pad { glyph: Option<Glyph>, cancel: Glyph },
}

/// A binding row's value ([`Value::Binding`]). The screen says what it binds and what it is bound
/// to; the shell starts the wait, shows it ("> A <"), draws the button, says in the strip that
/// it isn't bound or what else its button does, and draws the row's reset.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Binding {
    pub(crate) capture: Capture,
    pub(crate) shown: Shown,
    /// What else the same button does, by name: shown in the strip, and the button in yellow.
    pub(crate) shared: Vec<String>,
    /// Its reset button at the row's end (`Screen::reset`).
    pub(crate) reset: Reset,
}
/// A binding row's reset button.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Reset {
    /// The row has none (a key, as Key Binds lists it).
    None,
    /// Greyed: the binding is its default already.
    Default,
    /// Live: the binding differs from its default.
    Changed,
}
impl Reset {
    /// The row has a reset button, live or not.
    fn shown(self) -> bool {
        self != Reset::None
    }
}
impl Binding {
    /// A key's binding, as Key Binds lists it: no reset of its own.
    pub(crate) fn key(name: &'static str, shown: String) -> Self {
        Self {
            capture: Capture::Key(name),
            shown: Shown::Key(shown),
            shared: Vec::new(),
            reset: Reset::None,
        }
    }
    /// What it reads as, for the narrator and the widget tree: a pad's shared button in brackets,
    /// as the row draws it in yellow.
    fn text(&self) -> String {
        match &self.shown {
            Shown::Key(text) => text.clone(),
            Shown::Pad { glyph, .. } => {
                let name = glyph.map_or_else(|| lang::get(NOT_BOUND), |g| g.label().to_owned());
                if self.shared.is_empty() {
                    name
                } else {
                    format!("[ {name} ]")
                }
            }
        }
    }
    /// What the strip says about it while `waiting`, else when it isn't bound or shares its
    /// button; `None` leaves the row's own description.
    fn note(&self, waiting: bool) -> Option<String> {
        match (&self.shown, waiting) {
            (Shown::Pad { cancel, .. }, true) => Some(lang::tr(WAITING_PAD, &[&cancel.label()])),
            (Shown::Pad { glyph: None, .. }, false) => Some(lang::get(UNBOUND)),
            _ if !self.shared.is_empty() => Some(lang::tr(SHARED, &[&self.shared.join(", ")])),
            _ => None,
        }
    }
}
/// Vanilla's word for a key bound to nothing.
const NOT_BOUND: &str = "key.keyboard.unknown";
/// The strip while a controller button is awaited, naming the one that cancels.
const WAITING_PAD: &str = "Press the button for this. %s cancels.";
/// The strip over a binding with no button.
const UNBOUND: &str = "Not bound: press this row, then the button that should do it.";
/// The strip over a binding whose button does other things too.
const SHARED: &str = "This button also does: %s";
/// A pad binding's chip, at least: wide enough for most labels in vanilla's font.
const PAD_CHIP_W: f32 = 56.0;
/// A pad binding's chip around its art or text, and the room for the marks ("> <", "[ ]").
const PAD_CHIP_PAD: f32 = 8.0;
/// Clear of the art's edge, which its sprite draws past (`pad_glyph::rect`).
const PAD_MARK_GAP: f32 = 5.0;
/// A binding's reset button at the row's end, and the gap left before it.
const RESET_W: f32 = 36.0;
const RESET_GAP: f32 = 4.0;
/// The reset button's inset from the list's right edge.
const RESET_INSET: f32 = 2.0;

/// A row as a screen describes it, built afresh from the settings each time it is read.
#[derive(Clone, Debug)]
pub(crate) struct Row<R> {
    pub(crate) id: R,
    pub(crate) name: String,
    pub(crate) value: Value,
    /// What it does: found by the search, and shown in the strip.
    pub(crate) about: String,
    /// What the chosen value does, for a row whose values each do something different: shown in
    /// the strip instead of `about`.
    pub(crate) chosen: Option<String>,
    /// Can be used now (a setting of a switch that is off can't).
    pub(crate) live: bool,
    /// Why it can't be used now: shown in the strip, greyed, instead of what it does.
    pub(crate) why: Option<String>,
    /// What it costs, beside its name in the strip.
    pub(crate) cost: Option<style::Cost>,
}
impl<R> Row<R> {
    pub(crate) fn new(id: R, name: String, value: Value) -> Self {
        Self {
            id,
            name,
            value,
            about: String::new(),
            chosen: None,
            live: true,
            why: None,
            cost: None,
        }
    }
    pub(crate) fn about(mut self, about: String) -> Self {
        self.about = about;
        self
    }
    pub(crate) fn live(mut self, live: bool) -> Self {
        self.live = live;
        self
    }
    /// Greyed, saying why, while there is a reason it can't be used.
    pub(crate) fn blocked(mut self, why: Option<String>) -> Self {
        self.live = why.is_none();
        self.why = why;
        self
    }
    pub(crate) fn cost(mut self, cost: style::Cost) -> Self {
        self.cost = Some(cost);
        self
    }
    /// Its whole caption ("Name: value"), for the narrator and the widget tree.
    fn label(&self, value: Option<&str>) -> String {
        match value {
            Some(value) => Text::option_name_value(
                Text::literal(self.name.clone()),
                Text::literal(value.to_owned()),
            )
            .plain(),
            None => self.name.clone(),
        }
    }
    /// What the strip says about it.
    fn described(&self) -> &str {
        self.chosen.as_deref().unwrap_or(&self.about)
    }
    /// Its value as text, where it has one.
    fn value_text(&self) -> Option<String> {
        match &self.value {
            Value::Switch(on) => Some(if *on {
                lang::get_or("On", "On")
            } else {
                lang::get_or("Off", "Off")
            }),
            Value::Choice(text) | Value::Slider { text, .. } | Value::Read(text) => {
                Some(text.clone())
            }
            Value::Binding(binding) => Some(binding.text()),
            Value::Press | Value::Note => None,
        }
    }
    /// Its binding, for a binding row.
    fn binding(&self) -> Option<&Binding> {
        match &self.value {
            Value::Binding(binding) => Some(binding),
            _ => None,
        }
    }
}

/// Rows under a group's name (a language key).
pub(crate) struct Group<R> {
    pub(crate) name: &'static str,
    pub(crate) rows: Vec<Row<R>>,
}

/// A page of a screen. A screen of one page shows no page list.
pub(crate) struct Sheet<P, R> {
    pub(crate) id: P,
    pub(crate) name: String,
    /// What the page holds, in the strip while no row is pointed at.
    pub(crate) about: String,
    pub(crate) groups: Vec<Group<R>>,
    /// In the page list. A page out of it opens from a row that links to it (`Screen::link`),
    /// and the list lights the page that link was followed from.
    pub(crate) listed: bool,
    /// A line in the page list over this page, starting a section of it ("BSL settings"). When the
    /// pages don't fit, the last section collapses into one row.
    pub(crate) heading: Option<String>,
}
impl<P, R> Sheet<P, R> {
    /// A page in the page list, with no line over it.
    pub(crate) fn new(id: P, name: String, about: String, groups: Vec<Group<R>>) -> Self {
        Self {
            id,
            name,
            about,
            groups,
            listed: true,
            heading: None,
        }
    }
}

/// What the host of a screen does after an interaction.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Output {
    /// Passed on to the game as it is.
    Event(crate::options_ui::Event),
    /// Wait for what a binding row binds: a key or a mouse button, or a controller button.
    Bind(Capture),
    /// Open another options page over this one.
    Push(crate::options_ui::Page),
    /// Leave the screen.
    Done,
}
const CLICK: Output = Output::Event(crate::options_ui::Event::ButtonClick);
/// The foot's buttons, by their language keys.
const UNDO: &str = "Undo";
const APPLY: &str = "Apply";
const DONE: &str = "Done";
const DRAGGED: Output = Output::Event(crate::options_ui::Event::SliderDrag);

/// What a settings screen says. To add one: describe its pages here, keep a [`Shell`] of it on
/// `OptionsUi`, and name it in `OptionsUi::shell` and `shell_mut`, which every input path goes
/// through: it can't take the wheel and miss the stick.
pub(crate) trait Screen {
    type Page: Copy + PartialEq;
    type Row: Copy + PartialEq;
    /// The language key of the name under the mark.
    fn title(&self) -> &'static str;
    /// The lines of description the strip should hold.
    fn note_lines(&self) -> usize {
        NOTE_LINES
    }
    fn sheets(&self, s: &Settings) -> Vec<Sheet<Self::Page, Self::Row>>;
    /// A fingerprint ([`stamp_of`]) of what `sheets` reads that can change while the screen is
    /// open, besides through `press`, `slide` and `wheel_step` (the shell builds again after
    /// those): the rows are built again only when it changes, or the search, the page, the size or
    /// the language does.
    fn stamp(&self, s: &Settings) -> u64;
    /// A row was pressed (`back`: Shift held); the shell has played the click.
    fn press(&mut self, s: &mut Settings, row: Self::Row, back: bool) -> Vec<Output>;
    /// A press (A, Enter, a click, not Left or Right nor a Shift press) on a choice row: `None`
    /// steps it as Right does; a screen with a list of its values opens it here.
    fn choose(&mut self, _s: &mut Settings, _row: Self::Row) -> Option<Vec<Output>> {
        None
    }
    /// A slider row moved to `position`.
    fn slide(&mut self, _s: &mut Settings, _row: Self::Row, _position: f32) {}
    /// A slider let go of, or stepped by a key: what its change does now that it is done (a
    /// Horizon shader pack saves its options once a drag, not on every move).
    fn slider_released(&mut self) -> Vec<Output> {
        Vec::new()
    }
    /// The page a row opens, for a row that links to one: the shell opens it, and Escape goes
    /// back to the page the link was on.
    fn link(&self, _row: Self::Row) -> Option<Self::Page> {
        None
    }
    /// The wheel over a row, with the "Scroll wheel changes settings" extra on: `None` leaves it
    /// to scroll the list.
    fn wheel_step(
        &mut self,
        _s: &mut Settings,
        _row: Self::Row,
        _back: bool,
    ) -> Option<Vec<Output>> {
        None
    }
    /// A binding row's reset was pressed: its binding goes back to its default.
    fn reset(&mut self, _s: &mut Settings, _row: Self::Row) -> Vec<Output> {
        Vec::new()
    }
    /// Lines under the page list: whose settings these are, say.
    fn side(&self) -> Vec<String> {
        Vec::new()
    }
    /// Whether the screen has Undo, which puts back what it changed (`undo`) since it opened.
    fn undoes(&self) -> bool {
        false
    }
    /// Undo: put back, from the settings as the screen opened (`before`), what it changes, and say
    /// what else that does (besides the click and `Event::Changed`).
    fn undo(&mut self, _s: &mut Settings, _before: &Settings) -> Vec<Output> {
        Vec::new()
    }
    /// Whether changes wait for Apply, which the foot shows while they do.
    fn pending(&self, _s: &Settings) -> bool {
        false
    }
    /// Apply: what the changes that wait for it do now.
    fn apply(&mut self, _s: &mut Settings) -> Vec<Output> {
        Vec::new()
    }
    /// Whether what a row did (its outputs) is a change Undo puts back.
    fn changes(&self, outputs: &[Output]) -> bool {
        changes_settings(outputs)
    }
    /// Whether what the screen changes differs now from `before`, for a screen that can tell:
    /// Undo then shows exactly while it would do something, a binding the host made (the game
    /// binds a controller button) included, and a value stepped back isn't a change. `None`
    /// leaves it to `changes`.
    fn differs(&self, _s: &Settings, _before: &Settings) -> Option<bool> {
        None
    }
}

/// Whether `outputs` say a setting changed, or a key is being bound to one.
pub(crate) fn changes_settings(outputs: &[Output]) -> bool {
    use crate::options_ui::Event;
    let changed = |output: &Output| {
        matches!(
            output,
            Output::Bind(_) | Output::Event(Event::Changed | Event::SliderDrag)
        )
    };
    outputs.iter().any(changed)
}

/// A fingerprint of `parts` for [`Screen::stamp`]: their `Debug` text, hashed as it is written,
/// without keeping it.
pub(crate) fn stamp_of(parts: &impl std::fmt::Debug) -> u64 {
    use std::{fmt::Write, hash::Hasher};
    struct Hashing(std::collections::hash_map::DefaultHasher);
    impl Write for Hashing {
        fn write_str(&mut self, text: &str) -> std::fmt::Result {
            self.0.write(text.as_bytes());
            Ok(())
        }
    }
    let mut hashing = Hashing(Default::default());
    // Writing into a hasher can't fail.
    let _ = write!(hashing, "{parts:?}");
    hashing.0.finish()
}

/// A [`Shell`] of any screen, as `OptionsUi` drives it.
pub(crate) trait Hosted {
    /// The screen opens, afresh.
    fn open(&mut self);
    /// Where the screen asked the focus to open (`Shell::focus_soon`), for a controller's focus
    /// to land there too.
    fn opened_focus(&self) -> Option<[f32; 4]>;
    fn clear_opened_focus(&mut self);
    /// The clicks until it is set back are a controller's Left or Right: on a choice they step
    /// it, as the arrow keys do, rather than press it.
    fn clicks_step(&mut self, on: bool);
    /// `waiting`: the binding the host waits on.
    fn draw(
        &mut self,
        s: &Settings,
        batch: &mut HudBatch,
        atlas: &HudAtlas,
        cursor: (f32, f32),
        in_world: bool,
        waiting: Option<Capture>,
    );
    /// A press (or a drag with `drag`) at `at`.
    fn click(
        &mut self,
        s: &mut Settings,
        at: (f32, f32),
        size: (f32, f32),
        drag: bool,
        right: bool,
        shift: bool,
    ) -> Vec<Output>;
    /// Lets go of a slider or the scrollbar: for a slider (it plays its click), what its change
    /// does now (`Screen::slider_released`).
    fn release(&mut self) -> Option<Vec<Output>>;
    /// A controller's or the keyboard's focus is on the row at `rect`: if the list hides part of
    /// it, it scrolls all into view (`AbstractSelectionList.setSelected`). How far the row moved
    /// down, when it moved.
    fn reveal(&mut self, s: &Settings, size: (f32, f32), rect: [f32; 4]) -> Option<f32>;
    /// The mouse wheel at `at`: it scrolls, unless the row under it takes the step (`wheel_step`).
    fn wheel(
        &mut self,
        s: &mut Settings,
        size: (f32, f32),
        at: (f32, f32),
        amount: f32,
    ) -> Vec<Output>;
    /// The one way a list scrolls: the wheel, a right stick and a D-pad past the view's edge.
    fn scroll(&mut self, s: &Settings, size: (f32, f32), amount: f32);
    /// Typing in the search field, Tab and the arrows between controls, Left and Right on a
    /// slider or a choice, Enter or Space to press, Escape to leave.
    fn key(
        &mut self,
        s: &mut Settings,
        key: winit::keyboard::KeyCode,
        text: Option<&str>,
        shift: bool,
        size: (f32, f32),
    ) -> Vec<Output>;
    /// Whether the search field takes the keys.
    fn typing(&self) -> bool;
    /// A key or the pad moved the focus away: the search field stops taking the keys.
    fn blur_search(&mut self);
    /// Nothing has the focus: a touch moved off it, say.
    #[allow(dead_code)] // For #11846's `OptionsUi::touch_start`, which cleared Graphics' focus.
    fn clear_focus(&mut self);
    /// Text typed into the search field (the keyboard, a controller's on-screen keyboard).
    fn type_text(&mut self, text: &str);
    /// Ctrl+A/C/V/X in the search field.
    fn shortcut(
        &mut self,
        shortcut: crate::clipboard::EditShortcut,
        clipboard: &mut dyn crate::clipboard::Clipboard,
    ) -> bool;
}

/// What the keyboard or a controller can focus.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Target<P, R> {
    Row(R),
    /// A binding row's reset button.
    Reset(R),
    Page(P),
    Search,
    Undo,
    Apply,
    Done,
}

/// A page as the page list and the strip read it.
struct PageOf<P> {
    id: P,
    name: String,
    /// What it holds.
    about: String,
    listed: bool,
    heading: Option<String>,
}

/// A screen as this moment's settings describe it, laid out.
struct View<P, R> {
    cols: Columns,
    /// Every page.
    pages: Vec<PageOf<P>>,
    rows: Vec<Row<R>>,
    /// The list; a line's setting is an index into `rows`.
    lines: Vec<Line<P, usize>>,
    content: f32,
}

/// What a [`View`] was built for: when any of it changes, the rows are built again.
#[derive(PartialEq)]
struct Built<P, R> {
    stamp: u64,
    /// Which language the rows' text is in (`lang::generation`).
    language: u64,
    size: (f32, f32),
    query: String,
    page: Option<P>,
    /// While searching, the rows the focus and a drag keep in the list.
    kept: (Option<R>, Option<R>),
}

/// A screen on the shell, with all that isn't the screen's: where the list is, what has the focus,
/// what is held and what is searched.
pub(crate) struct Shell<S: Screen> {
    pub(crate) screen: S,
    scroll: ui::Scroll,
    dragging: Option<S::Row>,
    focus: Option<Target<S::Page, S::Row>>,
    /// The focus came from this screen's keys, so it shows. A click only leaves it for the next
    /// key to go on from: a controller presses with a click and moves its own focus, which would
    /// leave this one lit on the row it pressed.
    keyed: bool,
    /// Left and Right move the focused slider rather than the focus.
    can_change_value: bool,
    search: ui::EditBox,
    /// The page the plate shows, alone, while nothing is searched; `None` is the first.
    page: Option<S::Page>,
    /// While searching, the page last picked in the page list: current until the list scrolls.
    picked: Option<S::Page>,
    waiting: Option<Capture>,
    /// The rows as last built, and what for.
    built: Option<(Built<S::Page, S::Row>, Rc<View<S::Page, S::Row>>)>,
    /// The settings as the screen found them, for Undo (`Screen::undoes`), and whether it
    /// changed them since.
    before: Option<Box<Settings>>,
    changed: bool,
    /// A row to put the focus on, scrolled into view, at the next draw, and where it went.
    focus_next: Option<S::Row>,
    focus_opened: Option<(S::Row, [f32; 4])>,
    /// Clicks are a controller's Left or Right (`Hosted::clicks_step`).
    clicks_step: bool,
    /// The pages links were followed from, to go back to with Escape (`Screen::link`).
    path: Vec<S::Page>,
    /// Changes wait for Apply (`Screen::pending`).
    pending: bool,
    /// The page list as last drawn: what a press or a key picks is what is on the screen (the
    /// collapsed row's arrows sit either side of a name as wide as the font draws it).
    drawn: Option<PageList<S::Page>>,
}
impl<S: Screen + Default> Default for Shell<S> {
    fn default() -> Self {
        let mut search = ui::EditBox::new(0.0, 0.0, 0.0, 8.0);
        search.bordered = false;
        search.focused = false;
        search.message = lang::get("Search options");
        Self {
            screen: S::default(),
            scroll: ui::Scroll::default(),
            dragging: None,
            focus: None,
            keyed: false,
            can_change_value: false,
            search,
            page: None,
            picked: None,
            waiting: None,
            built: None,
            before: None,
            changed: false,
            focus_next: None,
            focus_opened: None,
            clicks_step: false,
            path: Vec::new(),
            pending: false,
            drawn: None,
        }
    }
}
impl<S: Screen> Shell<S> {
    /// Put the focus on `row` as the keyboard would, scrolled into view, once it is drawn.
    pub(crate) fn focus_soon(&mut self, row: S::Row) {
        self.focus_next = Some(row);
    }
    fn query(&self) -> String {
        self.search.value.trim().to_lowercase()
    }
    fn shows_focus(&self, target: Target<S::Page, S::Row>) -> bool {
        self.keyed && self.focus == Some(target)
    }
    /// The screen's rows now, built again only when what they were built for changed
    /// ([`Built`]). The scroll is kept within them.
    fn view(&mut self, s: &Settings, size: (f32, f32)) -> Rc<View<S::Page, S::Row>> {
        let query = self.query();
        let searching = !query.is_empty();
        let focused = match self.focus {
            Some(Target::Row(row)) if searching => Some(row),
            _ => None,
        };
        let key = Built {
            stamp: self.screen.stamp(s),
            language: lang::generation(),
            size,
            query,
            page: self.page,
            kept: (focused, self.dragging.filter(|_| searching)),
        };
        let view = match &self.built {
            Some((built, view)) if *built == key => view.clone(),
            _ => {
                let view = Rc::new(self.build(s, size));
                self.built = Some((key, view.clone()));
                view
            }
        };
        self.scroll
            .clamp(view.cols.top, view.cols.bottom, view.content);
        // A page gone (another shader pack has fewer screens): the first page opens.
        if let Some(page) = self.page
            && !view.pages.iter().any(|p| p.id == page)
        {
            self.page = None;
            self.path.clear();
        }
        view
    }
    /// A row may read otherwise now: the screen changed something.
    fn rebuild(&mut self) {
        self.built = None;
    }
    /// A row did `outputs`: it reads anew, and Undo has something to put back if they changed it.
    fn did(&mut self, outputs: &[Output]) {
        self.rebuild();
        // Only a screen with Undo has something for it to put back.
        if self.screen.undoes() && self.screen.changes(outputs) {
            self.changed = true;
        }
    }
    /// Remember the settings before the first change, for Undo, and ask a screen that can tell
    /// whether they differ now (`Screen::differs`).
    fn keep(&mut self, s: &Settings) {
        self.pending = self.screen.pending(s);
        if !self.screen.undoes() {
            return;
        }
        let before = self.before.get_or_insert_with(|| Box::new(s.clone()));
        if let Some(differs) = self.screen.differs(s, before) {
            self.changed = differs;
        }
    }
    /// Undo, gone once it has put the settings back.
    fn undo(&mut self, s: &mut Settings) -> Vec<Output> {
        let mut outputs = vec![CLICK, Output::Event(crate::options_ui::Event::Changed)];
        if let Some(before) = self.before.take() {
            outputs.extend(self.screen.undo(s, &before));
            self.before = Some(before);
        }
        self.changed = false;
        self.rebuild();
        outputs
    }
    /// Apply, which stays while the screen says changes wait for it.
    fn apply(&mut self, s: &mut Settings) -> Vec<Output> {
        let mut outputs = vec![CLICK];
        outputs.extend(self.screen.apply(s));
        self.rebuild();
        outputs
    }
    /// A page the screen pushed over itself changed what Undo puts back.
    pub(crate) fn changed_elsewhere(&mut self) {
        self.did(&[Output::Event(crate::options_ui::Event::Changed)]);
    }
    /// Open at `page`.
    pub(crate) fn show(&mut self, page: S::Page) {
        self.page = Some(page);
        self.path.clear();
    }
    /// Follow `row`'s link to `page`: it opens alone, and Escape comes back to the page the row
    /// is on (while searching, the page it was found on).
    fn follow(&mut self, row: S::Row, page: S::Page) -> Vec<Output> {
        let on = self.built.as_ref().and_then(|(_, view)| {
            let mut lines = view.lines.iter();
            let line = lines.find(|l| l.setting.is_some_and(|i| view.rows[i].id == row));
            line.map(|l| l.page)
        });
        if let Some(from) = on.or(self.page) {
            self.path.push(from);
        }
        self.page = Some(page);
        self.scroll.amount = 0.0;
        self.search.set_value("");
        self.picked = None;
        self.focus = None;
        self.can_change_value = false;
        self.rebuild();
        vec![CLICK]
    }
    /// The screen's rows: the open page's or, while searching, every page's whose name or
    /// description has the search in it, under their pages' names. The row being changed stays
    /// whatever it reads now.
    fn build(&self, s: &Settings, (w, h): (f32, f32)) -> View<S::Page, S::Row> {
        let cols = Columns::of(w, h).with_note_lines(self.screen.note_lines());
        let query = self.query();
        let sheets = self.screen.sheets(s);
        let open = self
            .page
            .filter(|&page| sheets.iter().any(|sheet| sheet.id == page));
        let open = open.or(sheets.first().map(|sheet| sheet.id));
        let mut pages = Vec::new();
        let mut rows = Vec::new();
        let mut lines = Lines::new();
        for sheet in sheets {
            pages.push(PageOf {
                id: sheet.id,
                name: sheet.name,
                about: sheet.about,
                listed: sheet.listed,
                heading: sheet.heading,
            });
            if query.is_empty() && open != Some(sheet.id) {
                continue;
            }
            let mut titled = query.is_empty();
            for group in sheet.groups {
                let mut indices = Vec::new();
                for row in group.rows {
                    if query.is_empty()
                        || self.focus == Some(Target::Row(row.id))
                        || self.dragging == Some(row.id)
                        || row.name.to_lowercase().contains(&query)
                        || row.about.to_lowercase().contains(&query)
                    {
                        indices.push(rows.len());
                        rows.push(row);
                    }
                }
                if indices.is_empty() {
                    continue;
                }
                if !std::mem::replace(&mut titled, true) {
                    lines.page(sheet.id);
                }
                lines.group(sheet.id, group.name, indices);
            }
        }
        let (lines, content) = lines.finish();
        View {
            cols,
            pages,
            rows,
            lines,
            content,
        }
    }
    /// Whether the page list's `page` has anything to show.
    fn has(&self, page: S::Page, view: &View<S::Page, S::Row>) -> bool {
        self.query().is_empty() || view.lines.iter().any(|l| l.page == page)
    }
    /// Whether the screen has a page list: two pages or more in it.
    fn has_list(view: &View<S::Page, S::Row>) -> bool {
        view.pages.iter().filter(|p| p.listed).count() > 1
    }
    /// The page the page list lights: the one the plate shows or, for a page out of the list, the
    /// one its link was followed from.
    fn lit(&self, view: &View<S::Page, S::Row>) -> Option<S::Page> {
        let listed = |page| view.pages.iter().any(|p| p.id == page && p.listed);
        let current = self.current(view)?;
        if listed(current) {
            return Some(current);
        }
        self.path.iter().rev().copied().find(|&page| listed(page))
    }
    /// The page list laid out: its pages one under another while they fit, each section under
    /// its line; past that, the last section collapses into one row, `‹ Name ›` with a pip per
    /// page, rather than leaving some out. Without the atlas, the font that lays the row out, it
    /// doesn't collapse.
    fn page_list(
        &self,
        view: &View<S::Page, S::Row>,
        atlas: Option<&HudAtlas>,
    ) -> PageList<S::Page> {
        if !Self::has_list(view) {
            return PageList::default();
        }
        let cols = &view.cols;
        let listed: Vec<&PageOf<S::Page>> = view.pages.iter().filter(|p| p.listed).collect();
        let headings = listed.iter().filter(|p| p.heading.is_some()).count();
        let section = listed
            .iter()
            .rposition(|p| p.heading.is_some())
            .unwrap_or(0);
        let collapse = atlas.filter(|_| listed.len() + headings > cols.page_room());
        let mut list = PageList {
            lit: self.lit(view),
            ..PageList::default()
        };
        for (i, page) in listed.iter().enumerate() {
            if let Some(text) = &page.heading {
                list.headings.push((text.clone(), cols.page(list.used)));
                list.used += 1;
            }
            if collapse.is_some() && i >= section {
                break;
            }
            list.places.push((page.id, cols.page(list.used), Part::Tab));
            list.used += 1;
        }
        let Some(atlas) = collapse else {
            return list;
        };
        let pages: Vec<S::Page> = listed[section..].iter().map(|p| p.id).collect();
        let names: Vec<String> = listed[section..].iter().map(|p| p.name.clone()).collect();
        let shown = pages.iter().position(|&p| Some(p) == list.lit).unwrap_or(0);
        let row = cols.page(list.used);
        list.used += 1;
        let mut strip = tabs::collapsed(atlas, &names, shown, row);
        // While searching, the arrows skip the pages with no matches.
        let prev = pages[..shown].iter().rev().find(|&&p| self.has(p, view));
        let next = pages[shown + 1..].iter().find(|&&p| self.has(p, view));
        (strip.has_prev, strip.has_next) = (prev.is_some(), next.is_some());
        let [x, y, w, h] = row;
        let [nx, _, nw, _] = strip.label_area;
        // Either side of the name, as far as the row's ends, the page before and the one after.
        if let Some(&prev) = prev {
            list.places.push((prev, [x, y, nx - x, h], Part::Prev));
        }
        list.places
            .push((pages[shown], [nx, y, nw, h], Part::Shown));
        if let Some(&next) = next {
            let after = nx + nw;
            list.places
                .push((next, [after, y, x + w - after, h], Part::Next));
        }
        list.strip = Some((strip, pages[shown]));
        list
    }
    /// The page list as drawn, else laid out without collapsing (nothing drawn yet).
    fn places(&self, view: &View<S::Page, S::Row>) -> PageList<S::Page> {
        (self.drawn.clone()).unwrap_or_else(|| self.page_list(view, None))
    }
    /// The page list: each section's line, the collapsed row and each place to pick a page.
    fn draw_pages(
        &self,
        batch: &mut HudBatch,
        atlas: &HudAtlas,
        view: &View<S::Page, S::Row>,
        list: &PageList<S::Page>,
        cursor: (f32, f32),
    ) {
        for (text, rect) in &list.headings {
            heading(batch, atlas, text, *rect);
        }
        if let Some((strip, shown)) = &list.strip {
            // The row's page is open: lit as the list's open page is.
            if list.lit == Some(*shown) {
                let row @ [x, y, _, h] = strip.area;
                style::fill(batch, atlas, row, style::OPEN);
                style::fill(batch, atlas, [x, y, 2.0, h], style::ACCENT);
            }
            tabs::draw(batch, atlas, strip);
        }
        let name_of = |page| {
            view.pages
                .iter()
                .find(|p| p.id == page)
                .map(|p| p.name.clone())
        };
        for &(page, rect, part) in &list.places {
            let flags = (
                self.has(page, view),
                list.lit == Some(page),
                self.shows_focus(Target::Page(page)),
            );
            let name = match part {
                Part::Prev => lang::get(PREVIOUS_PAGE),
                Part::Next => lang::get(NEXT_PAGE),
                Part::Tab | Part::Shown => name_of(page).unwrap_or_default(),
            };
            match (part, &list.strip) {
                (Part::Tab, _) => page_tab(batch, atlas, rect, &name, flags, cursor),
                // The shown page's ring goes round the whole row, as a listed page's does.
                (Part::Shown, Some((strip, _))) => {
                    strip_place(batch, atlas, (rect, strip.area), &name, flags, cursor)
                }
                _ => strip_place(batch, atlas, (rect, rect), &name, flags, cursor),
            }
        }
    }
    /// The foot's buttons: Undo while there is something to undo and Apply while changes wait for
    /// it, side by side over Done.
    fn foot(&self, cols: &Columns) -> Vec<(Target<S::Page, S::Row>, [f32; 4])> {
        let shown = [(self.changed, Target::Undo), (self.pending, Target::Apply)];
        let over: Vec<_> = shown.into_iter().filter(|s| s.0).map(|s| s.1).collect();
        let [x, y, w, h] = cols.button(1);
        let n = over.len().max(1) as f32;
        let each = ((w - FOOT_GAP * (n - 1.0)) / n).floor();
        let mut foot: Vec<_> = (over.into_iter().enumerate())
            .map(|(i, t)| (t, [x + i as f32 * (each + FOOT_GAP), y, each, h]))
            .collect();
        foot.push((Target::Done, cols.button(0)));
        foot
    }
    /// The page the plate shows: the open one or, while searching, the one picked, else the
    /// topmost line's.
    fn current(&self, view: &View<S::Page, S::Row>) -> Option<S::Page> {
        if self.query().is_empty() {
            // A page that went (a screen whose pages are the player's) leaves the first.
            let open = self
                .page
                .filter(|page| view.pages.iter().any(|p| p.id == *page));
            return open.or(view.pages.first().map(|p| p.id));
        }
        self.picked.or_else(|| {
            let line = view.lines.iter().find(|l| l.y + l.h() > self.scroll.amount);
            line.map(|l| l.page)
        })
    }
    /// Pick `page`: the plate shows it alone or, while searching, scrolls to its matches.
    fn pick(&mut self, page: S::Page, view: &View<S::Page, S::Row>) {
        self.page = Some(page);
        self.path.clear();
        self.scroll.amount = 0.0;
        if self.query().is_empty() {
            return;
        }
        if let Some(line) = view.lines.iter().find(|l| l.page == page) {
            self.scroll.amount = line.y;
            let cols = &view.cols;
            self.scroll.clamp(cols.top, cols.bottom, view.content);
            self.picked = Some(page);
        }
    }
    /// Each row with where it is now.
    fn placed<'a>(
        &self,
        view: &'a View<S::Page, S::Row>,
    ) -> impl Iterator<Item = (&'a Row<S::Row>, f32)> + use<'a, S> {
        let (top, scroll) = (view.cols.top, self.scroll.amount);
        let lines = view.lines.iter();
        lines.filter_map(move |l| Some((&view.rows[l.setting?], top + l.y - scroll)))
    }
    /// The row under the pointer, inside the list's view, and where it is.
    fn row_at<'a>(
        &self,
        view: &'a View<S::Page, S::Row>,
        (x, y): (f32, f32),
    ) -> Option<(&'a Row<S::Row>, f32)> {
        let cols = &view.cols;
        if y < cols.top || y >= cols.bottom {
            return None;
        }
        let mut placed = self.placed(view);
        placed.find(|&(_, ry)| {
            let [rx, ry, rw, rh] = cols.row(ry);
            ui::hovered((x, y), rx, ry, rw, rh)
        })
    }
    /// Press `row`, or (`stepped`) Left or Right on it: `track` is where on its slider's track
    /// (`None` from the keyboard, or off it).
    fn activate(
        &mut self,
        s: &mut Settings,
        row: &Row<S::Row>,
        back: bool,
        track: Option<f32>,
        stepped: bool,
    ) -> Vec<Output> {
        if let Some(page) = self.screen.link(row.id) {
            return self.follow(row.id, page);
        }
        match row.value {
            Value::Note | Value::Read(_) => Vec::new(),
            // The shell, not the screen, starts the wait.
            Value::Binding(ref binding) => {
                self.can_change_value = false;
                let outputs = vec![CLICK, Output::Bind(binding.capture)];
                self.did(&outputs);
                outputs
            }
            Value::Slider { .. } => {
                self.can_change_value = true;
                let Some(position) = track else {
                    return Vec::new();
                };
                self.dragging = Some(row.id);
                self.screen.slide(s, row.id, position);
                self.did(&[DRAGGED]);
                vec![DRAGGED]
            }
            _ => {
                self.can_change_value = false;
                let mut outputs = vec![CLICK];
                let chosen = matches!(row.value, Value::Choice(_)) && !stepped && !back;
                match chosen.then(|| self.screen.choose(s, row.id)).flatten() {
                    Some(opened) => outputs.extend(opened),
                    None => outputs.extend(self.screen.press(s, row.id, back)),
                }
                self.did(&outputs);
                outputs
            }
        }
    }
    /// What Tab steps through, in order, with where each is on screen (`reach`).
    fn focusables(
        &self,
        view: &View<S::Page, S::Row>,
        near: bool,
    ) -> Vec<(Target<S::Page, S::Row>, [f32; 4])> {
        let cols = &view.cols;
        let mut out = vec![(Target::Search, cols.search())];
        for (page, rect, _) in self.places(view).places {
            if self.has(page, view) {
                out.push((Target::Page(page), rect));
            }
        }
        let rows: Vec<(&Row<S::Row>, f32, bool)> = self
            .placed(view)
            .map(|(row, y)| (row, y, row.live))
            .collect();
        for &(row, y, live) in &rows[reach(&rows, cols, near)] {
            if !live {
                continue;
            }
            out.push((Target::Row(row.id), row_rect(row, cols, y)));
            // A binding's reset, right of it, only while it would do something.
            if row.binding().is_some_and(|b| b.reset == Reset::Changed) {
                out.push((Target::Reset(row.id), reset_rect(cols, y)));
            }
        }
        out.extend(self.foot(cols));
        out
    }
    /// Where Down from the search field goes on a screen with no page list, whose foot is under the
    /// field instead: the first row to use that is all in view.
    fn first_row(
        &self,
        view: &View<S::Page, S::Row>,
    ) -> Option<(Target<S::Page, S::Row>, [f32; 4])> {
        if Self::has_list(view) {
            return None;
        }
        let cols = &view.cols;
        let mut placed = self.placed(view);
        let first = placed.find(|&(row, y)| {
            row.live && row.value != Value::Note && y >= cols.top && y + style::ROW <= cols.bottom
        });
        first.map(|(row, y)| (Target::Row(row.id), cols.row(y)))
    }
    /// Focus `target`, scrolling its row into the list's view.
    fn focus_on(
        &mut self,
        (target, rect): (Target<S::Page, S::Row>, [f32; 4]),
        view: &View<S::Page, S::Row>,
    ) {
        self.focus = Some(target);
        let row = |id| view.rows.iter().find(|r| r.id == id);
        self.can_change_value = matches!(target, Target::Row(id)
            if row(id).is_some_and(|r| matches!(r.value, Value::Slider { .. })));
        if matches!(target, Target::Row(_) | Target::Reset(_)) {
            scroll_into_view(&mut self.scroll, rect, &view.cols, view.content);
            self.picked = None;
        }
    }
    /// A binding row's reset: the binding goes back to its default and, as the reset goes away
    /// with it, the focus to its row.
    fn reset(&mut self, s: &mut Settings, row: S::Row) -> Vec<Output> {
        let mut outputs = vec![CLICK];
        outputs.extend(self.screen.reset(s, row));
        self.did(&outputs);
        self.focus = Some(Target::Row(row));
        outputs
    }
    /// The search changed: the list starts over at its top.
    fn searched(&mut self) {
        self.scroll.amount = 0.0;
        self.picked = None;
    }
    /// What the strip and the narrator say about `row`: a binding's wait, its missing button or
    /// its shared one, else what the row does. A row's own `chosen` outranks the binding's note
    /// except while it waits, so a binding can say why it has no button (the radial opener).
    fn strip_text(&self, row: &Row<S::Row>) -> String {
        let waits = self.waits(row);
        if let (Some(chosen), false) = (&row.chosen, waits) {
            return chosen.clone();
        }
        let note = row.binding().and_then(|b| b.note(waits));
        note.unwrap_or_else(|| row.described().to_owned())
    }
    /// Whether the host waits on `row`'s binding.
    fn waits(&self, row: &Row<S::Row>) -> bool {
        row.binding()
            .is_some_and(|b| self.waiting == Some(b.capture))
    }
    /// A row of the list at `y`; `pointer`: the cursor, while it is over the list.
    fn draw_row(
        &self,
        batch: &mut HudBatch,
        atlas: &HudAtlas,
        cols: &Columns,
        row: &Row<S::Row>,
        y: f32,
        pointer: Option<(f32, f32)>,
    ) {
        if row.value == Value::Note {
            let rect = cols.row(y);
            crate::widgets::record(&row.name, rect, false);
            crate::widgets::narratable(|| crate::widgets::Narratable::text(row.name.clone(), rect));
            let ty = y + ((style::ROW - 8.0) / 2.0).floor();
            let name = Text::literal(style::fit(atlas, &row.name, rect[2] - 2.0 * style::INSET));
            batch.text_styled(atlas, &name, rect[0] + style::INSET, ty, style::MUTED, true);
            return;
        }
        let over = |[x, y, w, h]: [f32; 4]| pointer.is_some_and(|at| ui::hovered(at, x, y, w, h));
        let hover = over(row_rect(row, cols, y));
        let waits = self.waits(row);
        let value = row
            .value_text()
            .map(|text| if waits { format!("> {text} <") } else { text });
        let label = row.label(value.as_deref());
        let focused = self.shows_focus(Target::Row(row.id));
        let hint = self.strip_text(row);
        if let Value::Read(value) = &row.value {
            let view = (label.as_str(), row.name.as_str(), value.as_str());
            read_row(batch, atlas, cols, y, view, (hover, focused));
            crate::widgets::last_narratable(|n| n.hint = Some(hint.clone()));
            return;
        }
        if let Some(binding) = row.binding()
            && let Shown::Pad { glyph, .. } = binding.shown
        {
            let view = PadRow {
                label: &label,
                name: &row.name,
                hint: &hint,
                glyph,
                waits,
                shared: !binding.shared.is_empty(),
                reset: binding.reset,
                live: row.live,
                hover: (hover, over(reset_rect(cols, y))),
                focus: (focused, self.shows_focus(Target::Reset(row.id))),
            };
            pad_row(batch, atlas, cols, y, &view);
            return;
        }
        let view = RowView {
            label: &label,
            name: &row.name,
            value: value.as_deref(),
            switch: match row.value {
                Value::Switch(on) => Some(on),
                _ => None,
            },
            slider: match row.value {
                Value::Slider { position, .. } => Some(position),
                _ => None,
            },
            live: row.live,
            hover,
            focused,
            held: self.dragging == Some(row.id),
            key: row.binding().map(|_| waits),
        };
        self::row(batch, atlas, cols, y, &view);
        if row.live && matches!(row.value, Value::Choice(_)) {
            crate::widgets::steps_value(cols.row(y));
        }
        crate::widgets::last_narratable(|n| n.hint = Some(hint.clone()));
    }
    /// The strip under the plate: the hovered or focused row's name and what it does; with none,
    /// what the open page holds.
    fn draw_dock(
        &self,
        batch: &mut HudBatch,
        atlas: &HudAtlas,
        view: &View<S::Page, S::Row>,
        described: Option<usize>,
        current: Option<S::Page>,
    ) {
        let cols = &view.cols;
        let [x, y, w, _] = cols.dock;
        let left = x + DOCK_INSET;
        let top = y + DOCK_NAME_Y;
        style::panel(batch, atlas, cols.dock);
        let row = described.map(|i| &view.rows[i]);
        let why = row.and_then(|row| row.why.as_deref().filter(|_| !row.live));
        let said = row.map(|row| self.strip_text(row));
        let (name, about) = match (row, &said) {
            (Some(row), Some(said)) => (row.name.as_str(), why.unwrap_or(said)),
            _ => {
                let Some(page) = view.pages.iter().find(|p| Some(p.id) == current) else {
                    return;
                };
                (page.name.as_str(), page.about.as_str())
            }
        };
        match row.and_then(|row| row.cost) {
            // Its cost in its colour, after a square of it.
            Some(cost) => {
                let line = format!("{name} · {}", lang::get(cost.label()));
                let line = style::fit(atlas, &line, w - COST_LINE_SLACK);
                let line = Text::literal(line).with_color(cost.rgb());
                let square = [left, y + COST_SQUARE_Y, COST_SQUARE, COST_SQUARE];
                style::fill(batch, atlas, square, style::cost_color(cost));
                let after = left + COST_NAME_X;
                batch.text_styled(atlas, &line, after, top, style::WHITE, true);
            }
            None => {
                let name = Text::literal(name.to_owned());
                batch.text_styled(atlas, &name, left, top, style::WHITE, true);
            }
        }
        // Labelled for the widget tree: what the strip says is what a test or an agent reads.
        crate::widgets::record(about, cols.dock, false);
        let color = if why.is_some() {
            style::MUTED
        } else {
            style::GREY
        };
        dock_note(batch, atlas, cols, about.to_owned(), color);
    }
}
impl<S: Screen + Default> Hosted for Shell<S> {
    fn open(&mut self) {
        *self = Self::default();
    }
    fn draw(
        &mut self,
        s: &Settings,
        batch: &mut HudBatch,
        atlas: &HudAtlas,
        cursor: (f32, f32),
        in_world: bool,
        waiting: Option<Capture>,
    ) {
        self.waiting = waiting;
        self.keep(s);
        let size = (batch.width, batch.height);
        let opened = self.focus_next.take();
        if let Some(row) = opened {
            let view = self.view(s, size);
            let target = self
                .focusables(&view, false)
                .into_iter()
                .find(|(target, _)| *target == Target::Row(row));
            if let Some(target) = target {
                self.keyed = true;
                self.focus_on(target, &view);
            }
        }
        let view = self.view(s, size);
        // Where it is now the list has scrolled to it.
        if let Some(row) = opened {
            self.focus_opened = self
                .focusables(&view, false)
                .into_iter()
                .find(|(target, _)| *target == Target::Row(row))
                .map(|(_, rect)| (row, rect));
        }
        let cols = view.cols;
        backdrop(batch, atlas, in_world);
        let current = self.current(&view);
        self::mark(batch, atlas, &cols, &lang::get(self.screen.title()));
        let pages = self.page_list(&view, Some(atlas));
        self.draw_pages(batch, atlas, &view, &pages, cursor);
        side_lines(batch, atlas, &cols, pages.used, &self.screen.side());
        self.drawn = Some(pages);
        let focused = self.focus == Some(Target::Search);
        search_field(&mut self.search, batch, atlas, &cols, cursor, focused);
        let name_of = |page| {
            view.pages
                .iter()
                .find(|p| p.id == page)
                .map(|p| p.name.clone())
        };
        let query = self.search.value.trim();
        let heading = if query.is_empty() {
            current.and_then(name_of).unwrap_or_default()
        } else {
            lang::get("Results for \"%s\"").replace("%s", query)
        };
        self::plate(batch, atlas, &cols, &heading);
        let over_list = cursor.1 >= cols.top && cursor.1 < cols.bottom;
        let mut described = None;
        let mut waiting = None;
        self::list(
            batch,
            atlas,
            &cols,
            &view.lines,
            self.scroll.amount,
            |page| name_of(page).unwrap_or_default(),
            |batch, i, y| {
                let row = &view.rows[i];
                let [rx, ry, rw, rh] = cols.row(y);
                let hover = over_list && ui::hovered(cursor, rx, ry, rw, rh);
                let focused = self.shows_focus(Target::Row(row.id))
                    || self.shows_focus(Target::Reset(row.id));
                // A wait is what the strip says; then the pointer's row wins over the keyboard's.
                if self.waits(row) {
                    waiting = Some(i);
                }
                if hover || self.dragging == Some(row.id) || (focused && described.is_none()) {
                    described = Some(i);
                }
                self.draw_row(batch, atlas, &cols, row, y, over_list.then_some(cursor));
            },
        );
        scroller(batch, atlas, &cols, view.content, self.scroll.amount);
        if let Some((_, row)) = self.first_row(&view) {
            crate::widgets::focus_link(cols.search(), true, row);
        }
        self.draw_dock(batch, atlas, &view, waiting.or(described), current);
        // Undo and Apply, over Done, only once there is something for them to do. A controller's
        // X presses Apply where there is one, else Undo.
        for (target, rect) in self.foot(&cols) {
            use crate::widgets::Action as Declared;
            let (label, declared, primary) = match target {
                Target::Undo => (UNDO, (!self.pending).then_some(Declared::Apply), false),
                Target::Apply => (APPLY, Some(Declared::Apply), true),
                _ => (DONE, Some(Declared::Done), true),
            };
            let label = Text::literal(lang::get(label));
            let flags = (self.shows_focus(target), primary);
            let room = match declared {
                Some(declared) => {
                    crate::widgets::declare_action(declared, rect, true);
                    crate::widgets::action_glyph_room()
                }
                None => 0.0,
            };
            foot_button_with_room(batch, atlas, rect, &label, cursor, flags, room);
        }
    }
    fn click(
        &mut self,
        s: &mut Settings,
        (x, y): (f32, f32),
        size: (f32, f32),
        drag: bool,
        right: bool,
        shift: bool,
    ) -> Vec<Output> {
        self.keep(s);
        // A click of its own: the focus a page opened on is the mouse's now.
        if !drag {
            self.focus_opened = None;
        }
        let view = self.view(s, size);
        let cols = view.cols;
        let at = |[rx, ry, rw, rh]: [f32; 4]| ui::hovered((x, y), rx, ry, rw, rh);
        if !drag {
            self.keyed = false;
        }
        if drag {
            if self.scroll.held() && !cols.scrolls() {
                self.scroll.release();
            }
            if self.scroll.held() {
                self.picked = None;
                self.scroll.drag(y, cols.top, cols.bottom, view.content);
                return Vec::new();
            }
            if let Some(row) = self.dragging {
                self.screen.slide(s, row, cols.slider_at(x));
                self.did(&[DRAGGED]);
                return vec![DRAGGED];
            }
            return Vec::new();
        }
        // The right button presses nothing, as on vanilla's `CycleButton`.
        if right {
            return Vec::new();
        }
        if cols.scrolls()
            && self
                .scroll
                .press((x, y), cols.bar_x(), cols.top, cols.bottom, view.content)
        {
            return Vec::new();
        }
        if at(cols.search()) {
            if !self.search.value.is_empty() && at(cols.clear()) {
                self.search.set_value("");
                self.searched();
                return vec![CLICK];
            }
            self.focus = Some(Target::Search);
            self.search.reset_blink();
            self.search.move_cursor_to_end(false);
            return Vec::new();
        }
        if self.focus == Some(Target::Search) {
            self.focus = None;
        }
        for (target, rect) in self.foot(&cols) {
            if !at(rect) {
                continue;
            }
            return match target {
                // Undo goes away with the change it undid, and Apply with the changes it applied.
                Target::Undo => {
                    self.focus = None;
                    self.undo(s)
                }
                Target::Apply => {
                    self.focus = None;
                    self.apply(s)
                }
                _ => {
                    self.focus = Some(Target::Done);
                    vec![CLICK, Output::Done]
                }
            };
        }
        for (page, rect, _) in self.places(&view).places {
            if at(rect) && self.has(page, &view) {
                self.focus = Some(Target::Page(page));
                self.pick(page, &view);
                return vec![CLICK];
            }
        }
        let Some((row, row_y)) = self.row_at(&view, (x, y)).filter(|(row, _)| row.live) else {
            return Vec::new();
        };
        self.focus = Some(Target::Row(row.id));
        if row.binding().is_some_and(|b| b.reset == Reset::Changed) && at(reset_rect(&cols, row_y))
        {
            return self.reset(s, row.id);
        }
        // A press on the track sets the value there; on the name, only focuses.
        let track = (x >= cols.track().0).then(|| cols.slider_at(x));
        self.activate(s, row, shift, track, self.clicks_step)
    }
    fn opened_focus(&self) -> Option<[f32; 4]> {
        self.focus_opened.map(|(_, rect)| rect)
    }
    fn clear_opened_focus(&mut self) {
        self.focus_opened = None;
    }
    fn clicks_step(&mut self, on: bool) {
        self.clicks_step = on;
    }
    fn release(&mut self) -> Option<Vec<Output>> {
        self.scroll.release();
        self.dragging.take()?;
        Some(self.screen.slider_released())
    }
    fn reveal(&mut self, s: &Settings, size: (f32, f32), [x, y, w, h]: [f32; 4]) -> Option<f32> {
        let view = self.view(s, size);
        let cols = &view.cols;
        let middle = (x + w / 2.0, y + h / 2.0);
        let (_, row_y) = self.placed(&view).find(|&(_, ry)| {
            let [rx, ry, rw, rh] = cols.row(ry);
            ui::hovered(middle, rx, ry, rw, rh)
        })?;
        let before = self.scroll.amount;
        scroll_into_view(&mut self.scroll, cols.row(row_y), cols, view.content);
        let moved = before - self.scroll.amount;
        (moved != 0.0).then(|| {
            self.picked = None;
            moved
        })
    }
    fn wheel(
        &mut self,
        s: &mut Settings,
        size: (f32, f32),
        at: (f32, f32),
        amount: f32,
    ) -> Vec<Output> {
        if amount != 0.0 && s.extras.wheel_cycles_settings {
            let view = self.view(s, size);
            if let Some((row, _)) = self.row_at(&view, at).filter(|(row, _)| row.live)
                && let Some(outputs) = self.screen.wheel_step(s, row.id, amount > 0.0)
            {
                self.did(&outputs);
                return outputs;
            }
        }
        self.scroll(s, size, amount);
        Vec::new()
    }
    fn scroll(&mut self, s: &Settings, size: (f32, f32), amount: f32) {
        let view = self.view(s, size);
        let cols = &view.cols;
        self.picked = None;
        self.scroll
            .wheel(amount, SCROLL_RATE, cols.top, cols.bottom, view.content);
    }
    fn key(
        &mut self,
        s: &mut Settings,
        key: winit::keyboard::KeyCode,
        text: Option<&str>,
        shift: bool,
        size: (f32, f32),
    ) -> Vec<Output> {
        use winit::keyboard::KeyCode as K;
        self.keep(s);
        if key == K::Escape {
            self.can_change_value = false;
            // Back out of a page a link opened, then out of the screen.
            if let Some(from) = self.path.pop() {
                self.page = Some(from);
                self.scroll.amount = 0.0;
                self.focus = None;
                return Vec::new();
            }
            return vec![Output::Done];
        }
        if self.typing() && !matches!(key, K::Tab | K::ArrowDown | K::Enter | K::NumpadEnter) {
            let before = self.search.value.clone();
            if !self.search.key(key, false, shift)
                && let Some(t) = text
            {
                let typed: String = t.chars().filter(|c| !c.is_control()).collect();
                self.type_text(&typed);
            }
            if self.search.value != before {
                self.searched();
            }
            return Vec::new();
        }
        let view = self.view(s, size);
        let focusables = self.focusables(&view, key != K::Tab);
        let current = self
            .focus
            .and_then(|f| focusables.iter().position(|&(t, _)| t == f));
        if key == K::Tab {
            let next = tab_step(focusables.len(), current, shift);
            self.keyed = true;
            self.focus_on(focusables[next], &view);
            return Vec::new();
        }
        if key == K::ArrowDown
            && self.focus == Some(Target::Search)
            && let Some(first) = self.first_row(&view)
        {
            self.keyed = true;
            self.focus_on(first, &view);
            return Vec::new();
        }
        let focused = view
            .rows
            .iter()
            .find(|r| Some(Target::Row(r.id)) == self.focus);
        if matches!(key, K::ArrowLeft | K::ArrowRight)
            && self.can_change_value
            && let Some(row) = focused
            && row.live
            && let Value::Slider {
                position, steps, ..
            } = row.value
        {
            let width = view.cols.track().1 + 8.0;
            let next = slider_step(position, steps, width, key == K::ArrowRight);
            self.screen.slide(s, row.id, next);
            self.did(&[DRAGGED]);
            let mut outputs = vec![DRAGGED];
            outputs.extend(self.screen.slider_released());
            return outputs;
        }
        // A choice's `<` `>`: Left steps it back and Right on, as a Shift press and a press.
        if matches!(key, K::ArrowLeft | K::ArrowRight)
            && let Some(row) = focused
            && row.live
            && matches!(row.value, Value::Choice(_))
        {
            return self.activate(s, row, key == K::ArrowLeft, None, true);
        }
        if matches!(
            key,
            K::ArrowUp | K::ArrowDown | K::ArrowLeft | K::ArrowRight
        ) {
            let is_row = |t| matches!(t, Target::Row(_));
            if let Some(next) = arrow_step(&focusables, current, key, &view.cols, is_row) {
                self.keyed = true;
                self.focus_on(focusables[next], &view);
                self.can_change_value = false;
            }
            return Vec::new();
        }
        if !matches!(key, K::Space | K::Enter | K::NumpadEnter) {
            return Vec::new();
        }
        match self.focus {
            Some(Target::Page(page)) => {
                self.pick(page, &view);
                vec![CLICK]
            }
            Some(Target::Row(_)) => match focused {
                // A row greyed out while focused does nothing.
                Some(row) if !row.live => Vec::new(),
                Some(Row {
                    value: Value::Slider { .. },
                    ..
                }) => {
                    self.can_change_value = !self.can_change_value;
                    Vec::new()
                }
                Some(row) => self.activate(s, row, shift, None, false),
                None => Vec::new(),
            },
            Some(Target::Reset(row)) => self.reset(s, row),
            Some(Target::Undo) => {
                // Undo goes away with the change it undid: Done has the focus.
                self.focus = Some(Target::Done);
                self.undo(s)
            }
            Some(Target::Apply) => {
                self.focus = Some(Target::Done);
                self.apply(s)
            }
            Some(Target::Done) => vec![CLICK, Output::Done],
            Some(Target::Search) | None => Vec::new(),
        }
    }
    fn typing(&self) -> bool {
        self.focus == Some(Target::Search)
    }
    fn blur_search(&mut self) {
        if self.typing() {
            self.focus = None;
        }
    }
    fn clear_focus(&mut self) {
        self.focus = None;
        self.can_change_value = false;
    }
    fn type_text(&mut self, text: &str) {
        if !text.is_empty() {
            self.search.insert_text(text);
            self.searched();
        }
    }
    fn shortcut(
        &mut self,
        shortcut: crate::clipboard::EditShortcut,
        clipboard: &mut dyn crate::clipboard::Clipboard,
    ) -> bool {
        if !self.typing() {
            return false;
        }
        let before = self.search.value.clone();
        self.search.shortcut(shortcut, clipboard);
        if self.search.value != before {
            self.searched();
        }
        // Whatever it did: the key is the field's, not something to type.
        true
    }
}

#[cfg(test)]
impl<S: Screen> Shell<S> {
    /// A point on `row`, scrolled into view.
    pub(crate) fn point_at(&mut self, s: &Settings, size: (f32, f32), row: S::Row) -> (f32, f32) {
        let view = self.view(s, size);
        let place = |shell: &Self, view| {
            let mut placed = shell.placed(view);
            placed.find(|(r, _)| r.id == row).map(|(_, y)| y).unwrap()
        };
        let rect = view.cols.row(place(self, &view));
        scroll_into_view(&mut self.scroll, rect, &view.cols, view.content);
        (
            view.cols.list_x + 4.0,
            place(self, &view) + style::ROW / 2.0,
        )
    }
    pub(crate) fn scroll_amount(&self) -> f32 {
        self.scroll.amount
    }
    /// What the shell's own focus is on.
    pub(crate) fn focused(&self) -> Option<Target<S::Page, S::Row>> {
        self.focus
    }
    /// Put the focus there, as a key would.
    pub(crate) fn set_focus(&mut self, focus: Option<Target<S::Page, S::Row>>) {
        self.focus = focus;
        self.keyed = true;
    }
    /// Left and Right move the focused slider.
    pub(crate) fn changes_value(&self) -> bool {
        self.can_change_value
    }
    /// Undo shows: the screen changed something since it opened.
    pub(crate) fn changed(&self) -> bool {
        self.changed
    }
    /// The scrollbar is held.
    pub(crate) fn scroller_held(&self) -> bool {
        self.scroll.held()
    }
    /// What the search field holds now, set as typing would.
    pub(crate) fn set_search(&mut self, text: &str) {
        self.search.set_value(text);
        self.searched();
    }
    /// The page shown while nothing is searched, as opened or picked.
    pub(crate) fn open_page(&self) -> Option<S::Page> {
        self.page
    }
    /// The list's lines, in order, and its content's height.
    pub(crate) fn lines_at(
        &mut self,
        s: &Settings,
        size: (f32, f32),
    ) -> (Vec<Line<S::Page, usize>>, f32) {
        let view = self.view(s, size);
        (view.lines.clone(), view.content)
    }
    /// Each row of the list, in order, with its page.
    pub(crate) fn listed(&mut self, s: &Settings, size: (f32, f32)) -> Vec<(S::Page, S::Row)> {
        let view = self.view(s, size);
        let lines = view.lines.iter();
        lines
            .filter_map(|l| Some((l.page, view.rows[l.setting?].id)))
            .collect()
    }
    /// The page the plate is headed with.
    pub(crate) fn current_page(&mut self, s: &Settings, size: (f32, f32)) -> Option<S::Page> {
        let view = self.view(s, size);
        self.current(&view)
    }
    /// Whether `row` is all in the list's view now.
    pub(crate) fn in_view(&mut self, s: &Settings, size: (f32, f32), row: S::Row) -> bool {
        let view = self.view(s, size);
        let cols = view.cols;
        let mut placed = self.placed(&view);
        placed.any(|(r, y)| r.id == row && y >= cols.top && y + style::ROW <= cols.bottom)
    }
    /// Whether Tab reaches `row` now.
    pub(crate) fn tab_reaches(&mut self, s: &Settings, size: (f32, f32), row: S::Row) -> bool {
        let view = self.view(s, size);
        let focusables = self.focusables(&view, false);
        focusables.iter().any(|(t, _)| *t == Target::Row(row))
    }
    /// The row the shell's own focus is on.
    pub(crate) fn focused_row(&self) -> Option<S::Row> {
        match self.focus {
            Some(Target::Row(row)) => Some(row),
            _ => None,
        }
    }
    /// The names of the rows the list has now.
    pub(crate) fn row_names(&mut self, s: &Settings) -> Vec<String> {
        let view = self.view(s, (854.0, 480.0));
        view.rows.iter().map(|r| r.name.clone()).collect()
    }
    /// The captions of the rows the list has now, as the widget tree reads them.
    pub(crate) fn row_labels(&mut self, s: &Settings) -> Vec<String> {
        let view = self.view(s, (854.0, 480.0));
        let rows = view.rows.iter();
        rows.map(|r| r.label(r.value_text().as_deref())).collect()
    }
    /// The page list as last drawn: each place to pick a page.
    pub(crate) fn page_places(&self) -> Vec<(S::Page, [f32; 4], Part)> {
        self.drawn
            .as_ref()
            .map_or_else(Vec::new, |d| d.places.clone())
    }
    /// The collapsed row as last drawn, if the pages didn't fit: whether there is a page before
    /// and after the one it shows.
    pub(crate) fn strip_ends(&self) -> Option<(bool, bool)> {
        let strip = self.drawn.as_ref()?.strip.as_ref()?;
        Some((strip.0.has_prev, strip.0.has_next))
    }
    /// The page the page list lit, as last drawn.
    pub(crate) fn lit_page(&self) -> Option<S::Page> {
        self.drawn.as_ref()?.lit
    }
    /// The foot's buttons now, in order, with where they are.
    pub(crate) fn foot_at(
        &mut self,
        s: &Settings,
        size: (f32, f32),
    ) -> Vec<(Target<S::Page, S::Row>, [f32; 4])> {
        self.keep(s);
        let view = self.view(s, size);
        self.foot(&view.cols)
    }
}

#[cfg(test)]
mod tests;
