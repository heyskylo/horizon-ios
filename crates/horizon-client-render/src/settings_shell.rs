//! What Horizon's searchable settings screens share (`graphics_ui`, `controller_ui`): the frame's
//! geometry, its list of pages, groups and rows, and how the keyboard moves through them. Which
//! settings a screen has, and what they do, stay in that screen; the look comes from
//! `graphics_style`.
use crate::{
    graphics_style as style,
    hud::{HudAtlas, HudBatch},
    lang,
    text::Text,
    ui,
};

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
}
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
        }
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
    /// A page in the page list, under the search field.
    pub(crate) fn page(&self, i: usize) -> [f32; 4] {
        [
            self.side_x,
            self.side_y + 56.0 + i as f32 * 18.0,
            style::CONTROL_W,
            16.0,
        ]
    }
    /// Done (`0`), and Undo over it (`1`): under the page list, level with the strip.
    pub(crate) fn button(&self, i: usize) -> [f32; 4] {
        let foot = self.dock[1] + self.dock[3] - style::BUTTON_H;
        let y = if i == 0 {
            foot
        } else {
            foot - 4.0 - style::BUTTON_H
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
/// draws a setting at its `y`.
pub(crate) fn list<P: Copy, S: Copy>(
    batch: &mut HudBatch,
    atlas: &HudAtlas,
    cols: &Columns,
    lines: &[Line<P, S>],
    scroll: f32,
    page_name: impl Fn(P) -> &'static str,
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
                Some((name, _)) => (name, style::MUTED, 3.0),
                None => (page_name(line.page), style::ACCENT, 4.0),
            };
            let name = Text::literal(lang::get(name));
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
    for (i, line) in lines.iter().take(2).enumerate() {
        batch.text_styled(
            atlas,
            line,
            x + 12.0,
            y + 20.0 + i as f32 * 10.0,
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
    (focused, primary): (bool, bool),
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
    style::button(batch, atlas, rect, label, (hover, true, primary));
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
