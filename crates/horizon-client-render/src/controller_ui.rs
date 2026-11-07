//! Horizon's controller settings screen: the frame of the graphics settings (`settings_shell`)
//! over one player's `PadSettings`, which that player's options hold. It says whose settings they
//! are and what device the game sees; which controller drives which player stays the splitscreen
//! router's.
use crate::widgets::Action as Declared;
use crate::{
    gamepad::{self, Button, GlyphStyle, PadSettings},
    graphics_style as style,
    hud::{HudAtlas, HudBatch},
    lang,
    settings::{Opt, Settings},
    settings_shell::{self as shell, Columns},
    text::Text,
    ui,
};

/// The screen's pages, in its order. A page is a variant here, its words below and its groups in
/// `rows`; nothing else names them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Page {
    General,
    Bindings,
    Device,
}
impl Page {
    pub const ALL: [Page; 3] = [Page::General, Page::Bindings, Page::Device];
    pub fn name(self) -> &'static str {
        match self {
            Page::General => "General",
            Page::Bindings => "Bindings",
            Page::Device => "Device",
        }
    }
    /// What the page holds, in the strip while no setting is pointed at.
    pub fn about(self) -> &'static str {
        match self {
            Page::General => "How the sticks turn and move you, and how menus repeat and scroll.",
            Page::Bindings => "Which button does what. Each player has their own.",
            Page::Device => "The controller the game sees, its button glyphs and its feedback.",
        }
    }
}

/// A line of what the game knows, for reading rather than changing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Info {
    Player,
    Device,
    Ids,
    Detected,
    Using,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Item {
    Option(Opt),
    /// Wait for a button to bind to this action.
    Bind(&'static str),
    ResetBinds,
    Info(Info),
}

#[derive(Clone, Copy)]
pub struct Setting {
    pub item: Item,
    pub text: &'static str,
}
fn option(opt: Opt, text: &'static str) -> Setting {
    Setting {
        item: Item::Option(opt),
        text,
    }
}
fn info(info: Info, text: &'static str) -> Setting {
    Setting {
        item: Item::Info(info),
        text,
    }
}

/// Shown on a binding's row until it is focused, and in the search.
const BIND_TEXT: &str = "Press this row, then the button that should do it.";

/// Every page's settings, in named groups of related rows. Settings another controller feature
/// brings get a group here, or a new `Page` when they are a subject of their own.
pub fn rows() -> Vec<(Page, Vec<(&'static str, Vec<Setting>)>)> {
    let binds: Vec<Setting> = gamepad::ACTIONS
        .iter()
        .map(|(action, _)| Setting {
            item: Item::Bind(action),
            text: BIND_TEXT,
        })
        .collect();
    vec![
        (
            Page::General,
            vec![
                (
                    "Controller",
                    vec![
                        option(
                            Opt::PadEnabled,
                            "Off, the game ignores every controller for this player.",
                        ),
                        option(
                            Opt::PadPlayInBackground,
                            "The controller keeps playing while this window isn't focused, and losing focus won't pause.",
                        ),
                    ],
                ),
                (
                    "Look",
                    vec![
                        option(
                            Opt::PadLookSensitivity,
                            "How fast the right stick turns the view.",
                        ),
                        option(
                            Opt::PadInvertX,
                            "Turn the view the other way, left and right.",
                        ),
                        option(Opt::PadInvertY, "Turn the view the other way, up and down."),
                        option(
                            Opt::PadLookDeadzone,
                            "How far the right stick moves before the view turns. Raise it if the view drifts.",
                        ),
                    ],
                ),
                (
                    "Movement",
                    vec![
                        option(
                            Opt::PadToggleSneak,
                            "Hold the button to sneak, or press it once to start and again to stop.",
                        ),
                        option(
                            Opt::PadMoveDeadzone,
                            "How far the left stick moves before you walk. Raise it if you drift.",
                        ),
                    ],
                ),
                (
                    "Play",
                    vec![option(
                        Opt::PadTriggerThreshold,
                        "How far a trigger is pulled before it counts as pressed.",
                    )],
                ),
                (
                    "Menus",
                    vec![
                        option(
                            Opt::PadPointerSpeed,
                            "How fast the stick moves the crosshair in inventories.",
                        ),
                        option(
                            Opt::PadScrollSpeed,
                            "How fast the right stick scrolls a list.",
                        ),
                        option(
                            Opt::PadRepeatDelay,
                            "How long a held direction waits before it repeats.",
                        ),
                        option(
                            Opt::PadRepeatInterval,
                            "How long a held direction takes between repeats.",
                        ),
                        option(
                            Opt::PadKeyboard,
                            "Which on-screen keyboard opens when a text field is pressed.",
                        ),
                    ],
                ),
            ],
        ),
        (
            Page::Bindings,
            vec![
                ("Actions", binds),
                (
                    "Reset",
                    vec![Setting {
                        item: Item::ResetBinds,
                        text: "Every action back on its default button.",
                    }],
                ),
            ],
        ),
        (
            Page::Device,
            vec![
                (
                    "Detected",
                    vec![
                        info(
                            Info::Player,
                            "These settings belong to this player's profile.",
                        ),
                        info(
                            Info::Device,
                            "The controller driving this player, or else the first one connected.",
                        ),
                        info(
                            Info::Ids,
                            "The USB vendor and product the system reports. Useful when reporting a controller that misbehaves.",
                        ),
                        info(
                            Info::Detected,
                            "The glyph family the game picked from the controller's maker.",
                        ),
                    ],
                ),
                (
                    "Glyphs",
                    vec![
                        option(
                            Opt::PadGlyphs,
                            "Follow the controller, or always show one family's buttons.",
                        ),
                        info(Info::Using, "The buttons menus show right now."),
                    ],
                ),
                (
                    "Feedback",
                    vec![
                        option(Opt::PadVibration, "Rumble when something happens to you."),
                        option(
                            Opt::PadAdaptiveTriggers,
                            "A DualSense's triggers push back on bows and tools.",
                        ),
                        option(
                            Opt::PadLightbar,
                            "What a DualSense's light shows, over USB or Bluetooth.",
                        ),
                    ],
                ),
            ],
        ),
    ]
}

/// The controller a player's game sees, from its own reader or the splitscreen router.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Device {
    pub name: String,
    pub vendor: Option<u16>,
    pub product: Option<u16>,
}

/// Who and what the screen is about; the game fills it in as it draws.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Context {
    /// The player whose profile holds these settings.
    pub player: String,
    pub device: Option<Device>,
    /// The glyph family of `device`, before the player's choice.
    pub detected: GlyphStyle,
    /// The family menus show.
    pub style: GlyphStyle,
    /// The pad goes by its letters (`PadSettings::nintendo_letters`) and the glyphs are
    /// Nintendo's: each part shows the letter of the button playing it.
    pub by_letter: bool,
}

fn glyphs_name(style: GlyphStyle) -> String {
    lang::get(match style {
        GlyphStyle::Auto => "Unknown",
        GlyphStyle::Xbox => "horizon.options.glyphs.xbox",
        GlyphStyle::PlayStation => "horizon.options.glyphs.playstation",
        GlyphStyle::Nintendo => "horizon.options.glyphs.nintendo",
    })
}

/// Whether a binding differs from its default: only then can it be reset.
fn resets(s: &Settings, item: Item) -> Option<&'static str> {
    match item {
        Item::Bind(action) if !s.controller.is_default(action) => Some(action),
        _ => None,
    }
}

impl Item {
    fn switch(self, s: &Settings) -> Option<bool> {
        let c = &s.controller;
        match self {
            Item::Option(Opt::PadEnabled) => Some(c.enabled),
            Item::Option(Opt::PadVibration) => Some(c.vibration),
            Item::Option(Opt::PadPlayInBackground) => Some(c.play_in_background),
            Item::Option(Opt::PadAdaptiveTriggers) => Some(c.adaptive_triggers),
            Item::Option(Opt::PadInvertX) => Some(c.invert_x),
            Item::Option(Opt::PadInvertY) => Some(c.invert_y),
            _ => None,
        }
    }
    fn slider(self, s: &Settings) -> Option<f32> {
        match self {
            Item::Option(o) => o.slider_position(s),
            _ => None,
        }
    }
    /// A value stepped between `<` `>`: not a tick box, a slider or a press.
    fn steps(self, s: &Settings) -> bool {
        matches!(self, Item::Option(_)) && self.switch(s).is_none() && self.slider(s).is_none()
    }
}

/// What the options screen must do after an interaction here.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    ButtonClick,
    Changed,
    SliderDrag,
    /// Wait for a controller button to bind to this action.
    BindPad(&'static str),
    Done,
}

/// What the keyboard or a controller can focus.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    Item(Item),
    /// The reset button on a changed binding's row.
    ResetBind(&'static str),
    Page(Page),
    Search,
    Undo,
    Done,
}

type Line = shell::Line<Page, Setting>;

/// A binding row's reset button, at its right end.
fn reset_rect(cols: &Columns, y: f32) -> [f32; 4] {
    let w = 36.0;
    let right = cols.list_x + cols.list_w - 2.0;
    [right - w, y + 2.0, w, style::CONTROL_H]
}
/// A binding row without its reset button.
fn bind_rect(cols: &Columns, y: f32) -> [f32; 4] {
    let [x, y, w, h] = cols.row(y);
    [x, y, w - 40.0, h]
}

pub struct ControllerUi {
    /// The list's scroll amount and its held scrollbar (`AbstractScrollArea`).
    scroll: ui::Scroll,
    dragging: Option<Item>,
    pub focus: Option<Target>,
    pub can_change_value: bool,
    /// The action waiting for a button (`Action::BindPad`).
    pub binding: Option<&'static str>,
    pub search: ui::EditBox,
    /// The page the plate shows, alone, while nothing is searched.
    page: Page,
    /// While searching, the page last picked in the page list: current until the list scrolls.
    picked: Option<Page>,
    /// The settings as the screen found them, for Undo.
    before: Option<Box<PadSettings>>,
    pub context: Context,
}
impl Default for ControllerUi {
    fn default() -> Self {
        let mut search = ui::EditBox::new(0.0, 0.0, 0.0, 8.0);
        search.bordered = false;
        search.focused = false;
        search.message = lang::get("Search options");
        Self {
            scroll: ui::Scroll::default(),
            dragging: None,
            focus: None,
            can_change_value: false,
            binding: None,
            search,
            page: Page::General,
            picked: None,
            before: None,
            context: Context::default(),
        }
    }
}

impl ControllerUi {
    /// The screen opens, at `page`.
    pub fn open(&mut self, page: Page) {
        *self = Self {
            page,
            context: std::mem::take(&mut self.context),
            ..Self::default()
        };
    }
    /// Settings take effect as they change, so all that can be pending is going back on them:
    /// Undo is there only while they differ from what the screen opened with.
    pub fn pending(&self, s: &Settings) -> bool {
        self.before.as_deref().is_some_and(|b| *b != s.controller)
    }
    /// Undo's button while there is something to undo.
    pub fn undo_rect(&self, s: &Settings, (w, h): (f32, f32)) -> Option<[f32; 4]> {
        self.pending(s).then(|| Columns::of(w, h).button(1))
    }
    fn query(&self) -> String {
        self.search.value.trim().to_lowercase()
    }
    /// The button a binding reads as on this pad.
    fn shown(&self, b: Button) -> Button {
        if self.context.by_letter {
            b.by_letter()
        } else {
            b
        }
    }
    /// A row as its name and its value.
    fn parts(&self, item: Item, s: &Settings) -> (String, Option<String>) {
        match item {
            Item::Option(o) => shell::name_and_value(&o.label(s).plain()),
            Item::Bind(action) => {
                let name = lang::get(gamepad::action_caption(action));
                let value = match s.controller.binding(action) {
                    Some(b) => self.shown(b).label(self.context.style).to_owned(),
                    None => lang::get("key.keyboard.unknown"),
                };
                let value = if self.binding == Some(action) {
                    format!("> {value} <")
                } else if s.controller.conflicts(action) {
                    format!("[ {value} ]")
                } else {
                    value
                };
                (name, Some(value))
            }
            Item::ResetBinds => (lang::get("controls.resetAll"), None),
            Item::Info(which) => {
                let c = &self.context;
                let (name, value) = match which {
                    Info::Player => ("Player profile", c.player.clone()),
                    Info::Device => (
                        "Controller",
                        c.device
                            .as_ref()
                            .map_or_else(|| lang::get("No controller"), |d| d.name.clone()),
                    ),
                    Info::Ids => (
                        "Vendor and product",
                        match c.device.as_ref().map(|d| (d.vendor, d.product)) {
                            Some((Some(vendor), Some(product))) => {
                                format!("{vendor:04x}:{product:04x}")
                            }
                            _ => lang::get("Unknown"),
                        },
                    ),
                    Info::Detected => ("Detected glyphs", glyphs_name(c.detected)),
                    Info::Using => ("Glyphs in use", glyphs_name(c.style)),
                };
                (lang::get(name), Some(value))
            }
        }
    }
    /// The list's lines and its content height: the open page's settings in their groups or,
    /// while searching, the settings of every page whose name or description has the search in
    /// it, under their pages' names. The row being changed stays whatever it reads now.
    fn lines(&self, s: &Settings) -> (Vec<Line>, f32) {
        let query = self.query();
        let mut lines = shell::Lines::new();
        for (page, groups) in rows() {
            if query.is_empty() && page != self.page {
                continue;
            }
            let groups: Vec<(&'static str, Vec<Setting>)> = groups
                .into_iter()
                .map(|(name, rows)| {
                    let rows = rows
                        .into_iter()
                        .filter(|r| {
                            query.is_empty()
                                || self.focus == Some(Target::Item(r.item))
                                || self.dragging == Some(r.item)
                                || self.parts(r.item, s).0.to_lowercase().contains(&query)
                                || lang::get(r.text).to_lowercase().contains(&query)
                        })
                        .collect::<Vec<_>>();
                    (name, rows)
                })
                .filter(|(_, rows)| !rows.is_empty())
                .collect();
            if groups.is_empty() {
                continue;
            }
            if !query.is_empty() {
                lines.page(page);
            }
            for (name, rows) in groups {
                lines.group(page, name, rows);
            }
        }
        lines.finish()
    }
    /// The columns and the list, with the scroll kept within it.
    fn layout(&mut self, s: &Settings, w: f32, h: f32) -> (Columns, Vec<Line>, f32) {
        let cols = Columns::of(w, h);
        let (lines, content) = self.lines(s);
        self.scroll.clamp(cols.top, cols.bottom, content);
        (cols, lines, content)
    }
    /// Pick `page`: the plate shows it alone or, while searching, scrolls to its matches.
    fn pick(&mut self, page: Page, lines: &[Line], cols: &Columns, content: f32) {
        self.page = page;
        self.scroll.amount = 0.0;
        if self.query().is_empty() {
            return;
        }
        if let Some(line) = lines.iter().find(|l| l.page == page) {
            self.scroll.amount = line.y;
            self.scroll.clamp(cols.top, cols.bottom, content);
            self.picked = Some(page);
        }
    }
    /// Whether the page list's `page` has anything to show.
    fn has(&self, page: Page, lines: &[Line]) -> bool {
        self.query().is_empty() || lines.iter().any(|l| l.page == page)
    }
    /// The page the plate shows: the open one or, while searching, the one picked, else the
    /// topmost line's.
    fn current(&self, lines: &[Line]) -> Option<Page> {
        if self.query().is_empty() {
            return Some(self.page);
        }
        self.picked.or_else(|| {
            lines
                .iter()
                .find(|l| l.y + l.h() > self.scroll.amount)
                .map(|l| l.page)
        })
    }
    fn row_y(&self, cols: &Columns, line: &Line) -> f32 {
        cols.top + line.y - self.scroll.amount
    }
    /// Remember the settings before the first change, for Undo.
    fn keep(&mut self, s: &Settings) {
        if self.before.is_none() {
            self.before = Some(Box::new(s.controller.clone()));
        }
    }
    /// Undo: this player's controller settings back to what they were when the screen opened.
    fn undo(&mut self, s: &mut Settings) {
        if let Some(before) = self.before.as_deref() {
            s.controller = before.clone();
        }
    }

    pub fn draw(
        &mut self,
        s: &Settings,
        batch: &mut HudBatch,
        atlas: &HudAtlas,
        cursor: (f32, f32),
        in_world: bool,
    ) {
        self.keep(s);
        let (w, h) = (batch.width, batch.height);
        let (cols, lines, content) = self.layout(s, w, h);
        shell::backdrop(batch, atlas, in_world);
        let current = self.current(&lines);
        self.draw_side(batch, atlas, &cols, &lines, current, cursor);
        let focused = self.focus == Some(Target::Search);
        shell::search_field(&mut self.search, batch, atlas, &cols, cursor, focused);

        let query = self.search.value.trim();
        let heading = if query.is_empty() {
            lang::get(current.unwrap_or(self.page).name())
        } else {
            lang::get("Results for \"%s\"").replace("%s", query)
        };
        shell::plate(batch, atlas, &cols, &heading);

        let over_list = cursor.1 >= cols.top && cursor.1 < cols.bottom;
        let mut described = None;
        shell::list(
            batch,
            atlas,
            &cols,
            &lines,
            self.scroll.amount,
            |page| lang::get(page.name()),
            |batch, setting: Setting, y| {
                let item = setting.item;
                let [rx, ry, rw, rh] = cols.row(y);
                let hover = over_list && ui::hovered(cursor, rx, ry, rw, rh);
                let focused = self.focus == Some(Target::Item(item))
                    || matches!((self.focus, item), (Some(Target::ResetBind(a)), Item::Bind(b)) if a == b);
                // The pointer's row wins over the keyboard's.
                if hover || self.dragging == Some(item) || (focused && described.is_none()) {
                    described = Some(setting);
                }
                match item {
                    Item::Bind(action) => {
                        self.draw_bind(batch, atlas, s, &cols, action, y, cursor, over_list)
                    }
                    Item::Info(_) => self.draw_info(batch, atlas, s, &cols, item, y, hover),
                    _ => {
                        let label = match item {
                            Item::Option(o) => o.label(s).plain(),
                            _ => self.parts(item, s).0,
                        };
                        let (name, value) = self.parts(item, s);
                        let view = shell::RowView {
                            label: &label,
                            name: &name,
                            value: value.as_deref(),
                            switch: item.switch(s),
                            slider: item.slider(s),
                            live: true,
                            hover,
                            focused,
                            held: self.dragging == Some(item),
                            key: None,
                        };
                        shell::row(batch, atlas, &cols, y, &view);
                    }
                }
            },
        );
        shell::scroller(batch, atlas, &cols, content, self.scroll.amount);
        self.draw_dock(batch, atlas, s, &cols, described, current);

        let mut foot = vec![(cols.button(0), "Done", Target::Done, Declared::Done)];
        // Undo only while there is something to undo.
        if self.pending(s) {
            foot.push((cols.button(1), "Undo", Target::Undo, Declared::Apply));
        }
        for (rect, label, target, declared) in foot {
            let label = Text::literal(lang::get(label));
            let focused = self.focus == Some(target);
            let primary = target == Target::Done;
            crate::widgets::declare_action(declared, rect, true);
            shell::foot_button(batch, atlas, rect, &label, cursor, (focused, primary));
        }
    }
    /// The mark over the search field, the page list, and under it whose settings these are and
    /// which controller the game sees.
    fn draw_side(
        &self,
        batch: &mut HudBatch,
        atlas: &HudAtlas,
        cols: &Columns,
        lines: &[Line],
        current: Option<Page>,
        cursor: (f32, f32),
    ) {
        shell::mark(batch, atlas, cols, &lang::get("Controller"));
        for (i, page) in Page::ALL.into_iter().enumerate() {
            let has = self.has(page, lines);
            let open = current == Some(page);
            let focused = self.focus == Some(Target::Page(page));
            let name = lang::get(page.name());
            let flags = (has, open, focused);
            shell::page_tab(batch, atlas, cols.page(i), &name, flags, cursor);
        }
        let [x, y, w, _] = cols.page(Page::ALL.len());
        let device = self
            .context
            .device
            .as_ref()
            .map_or_else(|| lang::get("No controller"), |d| d.name.clone());
        let player = lang::tr("Editing: %s", &[&self.context.player]);
        for (i, line) in [player, device].iter().enumerate() {
            let rect = [x + 2.0, y + 2.0 + i as f32 * 10.0, w - 4.0, 8.0];
            crate::widgets::record(line, rect, false);
            let line = Text::literal(style::fit(atlas, line, w - 4.0));
            batch.text_styled(atlas, &line, rect[0], rect[1], style::MUTED, true);
        }
    }
    /// A binding's row: its name, its button, and a reset once it isn't the default.
    #[allow(clippy::too_many_arguments)]
    fn draw_bind(
        &self,
        batch: &mut HudBatch,
        atlas: &HudAtlas,
        s: &Settings,
        cols: &Columns,
        action: &'static str,
        y: f32,
        cursor: (f32, f32),
        over_list: bool,
    ) {
        let item = Item::Bind(action);
        let rect = bind_rect(cols, y);
        let reset = reset_rect(cols, y);
        let over = |[x, y, w, h]: [f32; 4]| over_list && ui::hovered(cursor, x, y, w, h);
        let hover = over(rect);
        let focused = self.focus == Some(Target::Item(item));
        let (name, value) = self.parts(item, s);
        let value = value.unwrap_or_default();
        if hover || focused {
            let lit = [rect[0] + 2.0, y + 1.0, rect[2] - 4.0, style::ROW - 2.0];
            style::fill(batch, atlas, lit, style::HOVER);
        }
        let label = format!("{name}: {value}");
        crate::widgets::widget_cursor(hover, true);
        crate::widgets::record(&label, rect, true);
        crate::widgets::narratable(|| {
            crate::widgets::Narratable::button(label.clone(), rect, true)
                .hovered(hover)
                .focused(focused)
        });
        crate::widgets::target(rect);
        let ty = y + ((style::ROW - 8.0) / 2.0).floor();
        let left = rect[0] + style::INSET;
        let chip_w = 56.0;
        let chip = [
            rect[0] + rect[2] - chip_w - 2.0,
            y + 2.0,
            chip_w,
            style::CONTROL_H,
        ];
        // The button in its colour; yellow over it when another action has it too.
        let shown = match s.controller.binding(action) {
            _ if self.binding == Some(action) || s.controller.conflicts(action) => {
                Text::literal(value).with_color(crate::text::YELLOW)
            }
            Some(b) => {
                let [r, g, bl] = self.shown(b).color(self.context.style);
                Text::literal(value)
                    .with_color(u32::from(r) << 16 | u32::from(g) << 8 | u32::from(bl))
            }
            None => Text::literal(value),
        };
        style::button(batch, atlas, chip, &shown, (hover, true, false));
        let shown = Text::literal(style::fit(atlas, &name, chip[0] - 6.0 - left));
        batch.text_styled(atlas, &shown, left, ty, style::WHITE, true);
        if focused {
            style::ring(batch, atlas, rect, style::WHITE);
        }
        // Reset: a target only where it would do something.
        let live = !s.controller.is_default(action);
        let reset_label = lang::get("controls.reset");
        let reset_hover = live && over(reset);
        let reset_focused = self.focus == Some(Target::ResetBind(action));
        crate::widgets::widget_cursor(reset_hover, live);
        crate::widgets::record(&reset_label, reset, live);
        crate::widgets::narratable(|| {
            crate::widgets::Narratable::button(
                lang::tr("narrator.controls.reset", &[&name]),
                reset,
                live,
            )
            .hovered(reset_hover)
            .focused(reset_focused)
        });
        if live {
            crate::widgets::target(reset);
        }
        let shown = Text::literal(reset_label);
        style::button(batch, atlas, reset, &shown, (reset_hover, live, false));
        if reset_focused {
            style::ring(batch, atlas, reset, style::WHITE);
        }
    }
    /// A line to read: focusable, so the narrator and a controller reach it, and it does nothing.
    #[allow(clippy::too_many_arguments)]
    fn draw_info(
        &self,
        batch: &mut HudBatch,
        atlas: &HudAtlas,
        s: &Settings,
        cols: &Columns,
        item: Item,
        y: f32,
        hover: bool,
    ) {
        let rect = cols.row(y);
        let focused = self.focus == Some(Target::Item(item));
        let (name, value) = self.parts(item, s);
        let value = value.unwrap_or_default();
        let label = format!("{name}: {value}");
        crate::widgets::record(&label, rect, true);
        crate::widgets::narratable(|| {
            crate::widgets::Narratable::button(label.clone(), rect, true)
                .hovered(hover)
                .focused(focused)
        });
        crate::widgets::target(rect);
        if hover || focused {
            let lit = [rect[0] + 2.0, y + 1.0, rect[2] - 4.0, style::ROW - 2.0];
            style::fill(batch, atlas, lit, style::HOVER);
        }
        let ty = y + ((style::ROW - 8.0) / 2.0).floor();
        let left = rect[0] + style::INSET;
        let name_w = atlas.font.width(&name);
        batch.text_styled(atlas, &Text::literal(name), left, ty, style::WHITE, true);
        let room = rect[2] - 2.0 * style::INSET - name_w - 8.0;
        let value = style::fit(atlas, &value, room);
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
    /// The strip under the plate: the hovered or focused setting's name, then what it does; with
    /// none, what the open page holds.
    fn draw_dock(
        &self,
        batch: &mut HudBatch,
        atlas: &HudAtlas,
        s: &Settings,
        cols: &Columns,
        setting: Option<Setting>,
        current: Option<Page>,
    ) {
        let [x, y, ..] = cols.dock;
        style::panel(batch, atlas, cols.dock);
        let (name, text) = match setting {
            Some(setting) => (self.parts(setting.item, s).0, self.description(setting, s)),
            None => {
                let Some(page) = current else { return };
                (lang::get(page.name()), lang::get(page.about()))
            }
        };
        let name = Text::literal(name);
        batch.text_styled(atlas, &name, x + 12.0, y + 8.0, style::WHITE, true);
        shell::dock_note(batch, atlas, cols, text, style::GREY);
    }
    /// What a row does. A binding names the controller's own buttons, and what else its button
    /// does when it shares it.
    fn description(&self, setting: Setting, s: &Settings) -> String {
        let Item::Bind(action) = setting.item else {
            return lang::get(setting.text);
        };
        let pad = &s.controller;
        let style = self.context.style;
        if self.binding == Some(action) {
            return lang::tr(
                "Press the button for this. %s cancels.",
                &[&Button::Start.label(style)],
            );
        }
        if pad.conflicts(action)
            && let Some(button) = pad.binding(action)
        {
            let others: Vec<String> = pad
                .actions_of(button)
                .into_iter()
                .filter(|other| *other != action)
                .map(|other| lang::get(gamepad::action_caption(other)))
                .collect();
            return lang::tr("This button also does: %s", &[&others.join(", ")]);
        }
        lang::get(setting.text)
    }

    /// A press (or a drag with `drag`) at `(x, y)`.
    #[allow(clippy::too_many_arguments)]
    pub fn click(
        &mut self,
        s: &mut Settings,
        (x, y): (f32, f32),
        (w, h): (f32, f32),
        framebuffer: [i32; 2],
        drag: bool,
        right: bool,
        shift: bool,
    ) -> Vec<Action> {
        self.keep(s);
        let (cols, lines, content) = self.layout(s, w, h);
        let at = |[rx, ry, rw, rh]: [f32; 4]| ui::hovered((x, y), rx, ry, rw, rh);
        if drag {
            if self.scroll.held() && !cols.scrolls() {
                self.scroll.release();
            }
            if self.scroll.held() {
                self.picked = None;
                self.scroll.drag(y, cols.top, cols.bottom, content);
                return Vec::new();
            }
            if let Some(Item::Option(o)) = self.dragging {
                o.set_slider(s, cols.slider_at(x));
                return vec![Action::SliderDrag];
            }
            return Vec::new();
        }
        if right {
            return Vec::new();
        }
        if cols.scrolls()
            && self
                .scroll
                .press((x, y), cols.bar_x(), cols.top, cols.bottom, content)
        {
            return Vec::new();
        }
        if at(cols.search()) {
            if !self.search.value.is_empty() && at(cols.clear()) {
                self.search.set_value("");
                self.searched();
                return vec![Action::ButtonClick];
            }
            self.focus = Some(Target::Search);
            self.search.reset_blink();
            self.search.move_cursor_to_end(false);
            return Vec::new();
        }
        if self.focus == Some(Target::Search) {
            self.focus = None;
        }
        if at(cols.button(0)) {
            self.focus = Some(Target::Done);
            return vec![Action::ButtonClick, Action::Done];
        }
        if self.pending(s) && at(cols.button(1)) {
            // Undo goes away with the change it undid.
            self.focus = None;
            self.undo(s);
            return vec![Action::ButtonClick, Action::Changed];
        }
        for (i, page) in Page::ALL.into_iter().enumerate() {
            if at(cols.page(i)) && self.has(page, &lines) {
                self.focus = Some(Target::Page(page));
                self.pick(page, &lines, &cols, content);
                return vec![Action::ButtonClick];
            }
        }
        if y < cols.top || y >= cols.bottom {
            return Vec::new();
        }
        let Some((item, row_y)) = lines.iter().find_map(|line| {
            let ry = self.row_y(&cols, line);
            let setting = line.setting?;
            at(cols.row(ry)).then_some((setting.item, ry))
        }) else {
            return Vec::new();
        };
        if let Some(action) = resets(s, item)
            && at(reset_rect(&cols, row_y))
        {
            return self.reset_bind(s, action);
        }
        self.focus = Some(Target::Item(item));
        self.activate(s, item, shift, framebuffer, Some((x, &cols)))
    }
    /// A binding back on its default; the focus goes back to its row, as its reset goes away.
    fn reset_bind(&mut self, s: &mut Settings, action: &'static str) -> Vec<Action> {
        s.controller.bind(action, PadSettings::default_of(action));
        self.focus = Some(Target::Item(Item::Bind(action)));
        vec![Action::ButtonClick, Action::Changed]
    }
    /// Press `item`: `at` is where on its row and its slider's track (`None` from the keyboard).
    fn activate(
        &mut self,
        s: &mut Settings,
        item: Item,
        back: bool,
        framebuffer: [i32; 2],
        at: Option<(f32, &Columns)>,
    ) -> Vec<Action> {
        match item {
            Item::Info(_) => Vec::new(),
            Item::Bind(action) => vec![Action::ButtonClick, Action::BindPad(action)],
            Item::ResetBinds => {
                s.controller.reset_bindings();
                vec![Action::ButtonClick, Action::Changed]
            }
            Item::Option(o) if o.slider_position(s).is_some() => {
                self.can_change_value = true;
                // A press on the track sets the value there; on the name, only focuses.
                match at {
                    Some((x, cols)) if x >= cols.track().0 => {
                        self.dragging = Some(item);
                        o.set_slider(s, cols.slider_at(x));
                        vec![Action::SliderDrag]
                    }
                    _ => Vec::new(),
                }
            }
            Item::Option(o) => {
                self.can_change_value = false;
                o.cycle(s, back, framebuffer);
                vec![Action::ButtonClick, Action::Changed]
            }
        }
    }
    /// Lets go of a slider (true: it plays its click) or the scrollbar.
    pub fn release(&mut self) -> bool {
        self.scroll.release();
        self.dragging.take().is_some()
    }
    /// The mouse wheel at the cursor: it scrolls the list, except that a hovered value stepped by
    /// `<` `>` takes the step while the "Scroll wheel changes settings" extra is on.
    pub fn wheel(
        &mut self,
        s: &mut Settings,
        (w, h): (f32, f32),
        (x, y): (f32, f32),
        framebuffer: [i32; 2],
        amount: f32,
    ) -> Vec<Action> {
        if amount != 0.0 && s.extras.wheel_cycles_settings {
            let (cols, lines, _) = self.layout(s, w, h);
            let hovered = (y >= cols.top && y < cols.bottom)
                .then(|| {
                    lines.iter().find_map(|line| {
                        let setting = line.setting?;
                        let ry = self.row_y(&cols, line);
                        ui::hovered((x, y), cols.list_x, ry, cols.list_w, style::ROW)
                            .then_some(setting.item)
                    })
                })
                .flatten();
            if let Some(item @ Item::Option(o)) = hovered
                && item.steps(s)
            {
                o.cycle(s, amount > 0.0, framebuffer);
                return vec![Action::ButtonClick, Action::Changed];
            }
        }
        self.scroll(s, (w, h), amount);
        Vec::new()
    }
    /// The mouse wheel scrolls the list.
    pub fn scroll(&mut self, s: &Settings, (w, h): (f32, f32), amount: f32) {
        let (cols, _, content) = self.layout(s, w, h);
        self.picked = None;
        self.scroll
            .wheel(amount, 18.0, cols.top, cols.bottom, content);
    }
    /// What Tab steps through, in order, with where each is on screen (`shell::reach`).
    fn focusables(
        &self,
        s: &Settings,
        cols: &Columns,
        lines: &[Line],
        near: bool,
    ) -> Vec<(Target, [f32; 4])> {
        let mut out = vec![(Target::Search, cols.search())];
        for (i, page) in Page::ALL.into_iter().enumerate() {
            if self.has(page, lines) {
                out.push((Target::Page(page), cols.page(i)));
            }
        }
        let rows: Vec<(Item, f32, bool)> = lines
            .iter()
            .filter_map(|line| Some((line.setting?.item, self.row_y(cols, line), true)))
            .collect();
        let reach = shell::reach(&rows, cols, near);
        for &(item, y, _) in &rows[reach] {
            let rect = match item {
                Item::Bind(_) => bind_rect(cols, y),
                _ => cols.row(y),
            };
            out.push((Target::Item(item), rect));
            if let Some(action) = resets(s, item) {
                out.push((Target::ResetBind(action), reset_rect(cols, y)));
            }
        }
        if self.pending(s) {
            out.push((Target::Undo, cols.button(1)));
        }
        out.push((Target::Done, cols.button(0)));
        out
    }
    /// Focus `target`, scrolling its row into the list's view.
    fn focus_on(
        &mut self,
        s: &Settings,
        target: Target,
        rect: [f32; 4],
        cols: &Columns,
        content: f32,
    ) {
        self.focus = Some(target);
        self.can_change_value = matches!(target, Target::Item(i) if i.slider(s).is_some());
        if matches!(target, Target::Item(_) | Target::ResetBind(_)) {
            shell::scroll_into_view(&mut self.scroll, rect, cols, content);
            self.picked = None;
        }
    }

    /// Keys: typing in the search field, Tab and the arrows between controls, Left and Right on a
    /// slider, Enter or Space to press. False: the screen handles it (Escape closes it).
    pub fn key_modifiers(
        &mut self,
        s: &mut Settings,
        key: winit::keyboard::KeyCode,
        text: Option<&str>,
        shift: bool,
        (w, h): (f32, f32),
        framebuffer: [i32; 2],
    ) -> (Vec<Action>, bool) {
        use winit::keyboard::KeyCode as K;
        self.keep(s);
        if key == K::Escape {
            self.can_change_value = false;
            return (Vec::new(), false);
        }
        if self.focus == Some(Target::Search)
            && !matches!(key, K::Tab | K::ArrowDown | K::Enter | K::NumpadEnter)
        {
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
            return (Vec::new(), true);
        }
        let (cols, lines, content) = self.layout(s, w, h);
        let near = key != K::Tab;
        let focusables = self.focusables(s, &cols, &lines, near);
        let current = self
            .focus
            .and_then(|f| focusables.iter().position(|&(t, _)| t == f));
        if key == K::Tab {
            let next = shell::tab_step(focusables.len(), current, shift);
            let (target, rect) = focusables[next];
            self.focus_on(s, target, rect, &cols, content);
            return (Vec::new(), true);
        }
        if matches!(key, K::ArrowLeft | K::ArrowRight)
            && self.can_change_value
            && let Some(Target::Item(Item::Option(o))) = self.focus
            && let Some(pos) = o.slider_position(s)
        {
            let next = shell::slider_step(
                pos,
                o.slider_steps(),
                cols.track().1 + 8.0,
                key == K::ArrowRight,
            );
            o.set_slider(s, next);
            return (vec![Action::SliderDrag], true);
        }
        if matches!(
            key,
            K::ArrowUp | K::ArrowDown | K::ArrowLeft | K::ArrowRight
        ) {
            let is_row = |t: Target| matches!(t, Target::Item(_));
            let next = shell::arrow_step(&focusables, current, key, &cols, is_row);
            let Some(next) = next else {
                return (Vec::new(), false);
            };
            let (target, rect) = focusables[next];
            self.focus_on(s, target, rect, &cols, content);
            self.can_change_value = false;
            return (Vec::new(), true);
        }
        if matches!(key, K::Space | K::Enter | K::NumpadEnter)
            && let Some(target) = self.focus
        {
            let actions = match target {
                Target::Search => Vec::new(),
                Target::Page(page) => {
                    self.pick(page, &lines, &cols, content);
                    vec![Action::ButtonClick]
                }
                Target::Item(item) if item.slider(s).is_some() => {
                    self.can_change_value = !self.can_change_value;
                    Vec::new()
                }
                Target::Item(item) => self.activate(s, item, shift, framebuffer, None),
                Target::ResetBind(action) => self.reset_bind(s, action),
                Target::Undo => {
                    self.undo(s);
                    // Undo goes away with the change it undid.
                    self.focus = Some(Target::Done);
                    return (vec![Action::ButtonClick, Action::Changed], true);
                }
                Target::Done => vec![Action::ButtonClick, Action::Done],
            };
            return (actions, true);
        }
        (Vec::new(), false)
    }
    /// Whether the search field takes the keys.
    pub fn typing(&self) -> bool {
        self.focus == Some(Target::Search)
    }
    /// Text typed into the search field (the keyboard, a controller's on-screen keyboard).
    pub fn type_text(&mut self, text: &str) {
        if !text.is_empty() {
            self.search.insert_text(text);
            self.searched();
        }
    }
    /// Ctrl+A/C/V/X in the search field.
    pub fn shortcut(
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
    /// The search changed: the list starts over at its top.
    fn searched(&mut self) {
        self.scroll.amount = 0.0;
        self.picked = None;
    }
    /// The names of the open page's rows, for the language tests.
    #[cfg(test)]
    pub(crate) fn row_names(&self, s: &Settings) -> Vec<String> {
        self.lines(s)
            .0
            .iter()
            .filter_map(|l| Some(self.parts(l.setting?.item, s).0))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use winit::keyboard::KeyCode as K;

    const SIZE: (f32, f32) = (854.0, 480.0);
    const FB: [i32; 2] = [1708, 960];

    fn key(ui: &mut ControllerUi, s: &mut Settings, key: K) -> Vec<Action> {
        ui.key_modifiers(s, key, None, false, SIZE, FB).0
    }

    /// Tab reaches every row of every page from nothing but the keyboard, as a controller's
    /// focus reaches the same targets.
    #[test]
    fn tab_reaches_every_setting_on_every_page() {
        let mut s = Settings::default();
        let mut ui = ControllerUi::default();
        for (page, groups) in rows() {
            ui.open(page);
            let want: Vec<Item> = groups
                .iter()
                .flat_map(|(_, rows)| rows.iter().map(|r| r.item))
                .collect();
            let mut seen = Vec::new();
            for _ in 0..want.len() + Page::ALL.len() + 3 {
                key(&mut ui, &mut s, K::Tab);
                if let Some(Target::Item(item)) = ui.focus {
                    seen.push(item);
                }
            }
            assert_eq!(seen, want, "{page:?}");
        }
    }

    #[test]
    fn every_bindable_action_has_a_row_and_no_option_is_listed_twice() {
        let items: Vec<Item> = rows()
            .into_iter()
            .flat_map(|(_, groups)| groups)
            .flat_map(|(_, rows)| rows)
            .map(|r| r.item)
            .collect();
        for (action, _) in gamepad::ACTIONS {
            assert!(items.contains(&Item::Bind(action)), "{action}");
        }
        for (i, item) in items.iter().enumerate() {
            assert!(!items[i + 1..].contains(item), "{item:?}");
        }
        // Every setting the old page had is still here.
        for opt in [
            Opt::PadEnabled,
            Opt::PadVibration,
            Opt::PadPlayInBackground,
            Opt::PadAdaptiveTriggers,
            Opt::PadLightbar,
            Opt::PadLookSensitivity,
            Opt::PadToggleSneak,
            Opt::PadInvertX,
            Opt::PadInvertY,
            Opt::PadMoveDeadzone,
            Opt::PadLookDeadzone,
            Opt::PadTriggerThreshold,
            Opt::PadGlyphs,
            Opt::PadKeyboard,
            Opt::PadPointerSpeed,
            Opt::PadScrollSpeed,
            Opt::PadRepeatDelay,
            Opt::PadRepeatInterval,
        ] {
            assert!(items.contains(&Item::Option(opt)), "{opt:?}");
        }
    }

    #[test]
    fn undo_is_there_only_while_something_differs() {
        let mut s = Settings::default();
        let mut ui = ControllerUi::default();
        ui.keep(&s);
        assert!(!ui.pending(&s));
        assert_eq!(ui.undo_rect(&s, SIZE), None);
        // Changing a value back isn't a change.
        let vibration = s.controller.vibration;
        s.controller.vibration = !vibration;
        assert!(ui.pending(&s));
        assert!(ui.undo_rect(&s, SIZE).is_some());
        s.controller.vibration = vibration;
        assert!(!ui.pending(&s));
        s.controller.bind("key.jump", Some(Button::North));
        s.controller.invert_y = !s.controller.invert_y;
        ui.undo(&mut s);
        assert_eq!(s.controller, PadSettings::default());
        assert!(!ui.pending(&s));
    }

    #[test]
    fn the_search_finds_rows_across_pages_and_drops_empty_pages() {
        let s = Settings::default();
        let mut ui = ControllerUi::default();
        ui.type_text("rumble");
        let (lines, _) = ui.lines(&s);
        assert!(lines.iter().all(|l| l.page == Page::Device));
        assert!(ui.has(Page::Device, &lines) && !ui.has(Page::General, &lines));
        // By what it does, not only its name.
        ui.search.set_value("drift");
        let (lines, _) = ui.lines(&s);
        assert!(lines.iter().any(|l| l.page == Page::General));
        ui.search.set_value("no setting says this");
        assert!(ui.lines(&s).0.is_empty());
    }

    #[test]
    fn a_changed_binding_gains_a_reset_that_the_arrows_reach_and_it_goes_away() {
        let mut s = Settings::default();
        s.controller.bind("key.jump", Some(Button::North));
        let mut ui = ControllerUi::default();
        ui.open(Page::Bindings);
        key(&mut ui, &mut s, K::Tab);
        // Search, the three pages, then the first action.
        for _ in 0..Page::ALL.len() + 1 {
            key(&mut ui, &mut s, K::Tab);
        }
        assert_eq!(ui.focus, Some(Target::Item(Item::Bind("key.jump"))));
        key(&mut ui, &mut s, K::ArrowRight);
        assert_eq!(ui.focus, Some(Target::ResetBind("key.jump")));
        let actions = key(&mut ui, &mut s, K::Enter);
        assert!(actions.contains(&Action::Changed));
        assert!(s.controller.is_default("key.jump"));
        assert_eq!(ui.focus, Some(Target::Item(Item::Bind("key.jump"))));
        // A default binding has nothing to the right of it.
        key(&mut ui, &mut s, K::ArrowRight);
        assert_eq!(ui.focus, Some(Target::Item(Item::Bind("key.jump"))));
    }

    #[test]
    fn pressing_a_binding_waits_for_a_button_and_reading_rows_change_nothing() {
        let mut s = Settings::default();
        let mut ui = ControllerUi::default();
        let actions = ui.activate(&mut s, Item::Bind("key.jump"), false, FB, None);
        assert_eq!(actions, [Action::ButtonClick, Action::BindPad("key.jump")]);
        let before = s.controller.clone();
        assert!(
            ui.activate(&mut s, Item::Info(Info::Device), false, FB, None)
                .is_empty()
        );
        assert_eq!(s.controller, before);
    }

    #[test]
    fn the_keyboard_steps_a_slider_after_enter_and_leaves_it_with_the_arrows() {
        let mut s = Settings::default();
        let mut ui = ControllerUi {
            focus: Some(Target::Item(Item::Option(Opt::PadMoveDeadzone))),
            ..ControllerUi::default()
        };
        let before = s.controller.move_deadzone;
        key(&mut ui, &mut s, K::Enter);
        key(&mut ui, &mut s, K::ArrowRight);
        assert!((s.controller.move_deadzone - before - 0.01).abs() < 0.001);
    }

    #[test]
    fn every_text_is_in_the_language_table() {
        let (_, missed) = lang::scoped(lang::Language::load(&[], "de_de"), || {
            for (page, groups) in rows() {
                lang::get(page.name());
                lang::get(page.about());
                for (group, rows) in groups {
                    lang::get(group);
                    for row in rows {
                        lang::get(row.text);
                    }
                }
            }
            for text in [
                "Controller",
                "Player profile",
                "Vendor and product",
                "Detected glyphs",
                "Glyphs in use",
                "Unknown",
                "No controller",
                "Editing: %s",
                "Press the button for this. %s cancels.",
                "This button also does: %s",
            ] {
                lang::get(text);
            }
        });
        assert_eq!(missed, Vec::<String>::new());
    }
}
