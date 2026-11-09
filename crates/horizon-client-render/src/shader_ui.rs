//! Horizon's shader settings screen: the frame of the graphics settings (`settings_shell`) over
//! the shader packs in `shaderpacks/` and the options of the picked one. Its pages are the pack
//! list, then the pack's own screens (a `shaders/` pack's `screen.NAME`s, a Horizon pack's
//! `[[pages]]`), and the search finds an option on whichever screen holds it. A `shaders/` pack's
//! options apply when Apply is pressed or the screen closes, as the pack loads again with them; a
//! Horizon pack's apply at once.
use crate::shader_pack_list::Kind;
use crate::widgets::Action as Declared;
use crate::{
    graphics_style as style,
    hud::{HudAtlas, HudBatch},
    hud_kit::tabs,
    lang,
    settings::Settings,
    settings_shell::{self as shell, Columns},
    text::Text,
    ui,
};
use horizon_shader_api::{OptionKind, PageItem};

/// A page of the screen: the pack list, or screen `.0` of the picked pack (`screens`, 0 its
/// main one).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tab {
    Packs,
    Screen(u16),
}

/// A row.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Item {
    /// `s.shader_packs[.0]`: picks it, or turns it off when it is the pick.
    Pack(u16),
    /// Opens `shaderpacks/`.
    Folder,
    /// Option `.0` of the picked pack: of a `shaders/` pack, the `.0`th of its options by name;
    /// of a Horizon pack, its `[[options]]` entry.
    Option(u16),
    /// Opens screen `.0`.
    Link(u16),
    /// The next of a `shaders/` pack's profiles.
    Profile,
    /// Every option of the pack back to its default.
    Reset,
    /// Horizon's own Shadow Cache (`Graphics::shadow_cache`), for every pack: not the pack's.
    ShadowCache,
}

/// What the strip says of a pack's own screens: they are the pack's, not Horizon's.
const PACK_SETTINGS: &str = "Settings made by the pack's author: they change only this pack.";

/// One of the picked pack's screens.
struct Screen {
    title: String,
    /// What the strip says while nothing on it is pointed at.
    about: String,
    items: Vec<Item>,
    /// How many links away from the main screen: the page list holds the first two levels.
    depth: u8,
}

/// The picked pack: its index in `s.shader_packs` and kind, and for a `shaders/` pack whether its
/// options (`s.shader_options`) are read.
fn picked(s: &Settings) -> Option<(usize, Kind)> {
    let i = s
        .shader_packs
        .iter()
        .position(|p| !s.horizon.shader_pack.is_empty() && p.key == s.horizon.shader_pack)?;
    Some((i, s.shader_packs[i].kind))
}

/// The picked `shaders/` pack's options, when they are the picked pack's.
fn classic(s: &Settings) -> Option<&crate::shader_options::ShaderOptions> {
    s.shader_options
        .as_ref()
        .filter(|o| o.pack == s.horizon.shader_pack)
}

/// The `.0`th option name of a `shaders/` pack.
fn option_name(o: &crate::shader_options::ShaderOptions, i: u16) -> Option<&str> {
    o.options.keys().nth(usize::from(i)).map(String::as_str)
}

fn plain(t: Text) -> String {
    t.strip_legacy().plain()
}

/// A place to pick in the strip of a pack's screens (`ShaderUi::side`): the shown screen's name,
/// or an arrow to the one before or after, for the pointer, the keyboard, a controller's focus and
/// its bumpers (a `Tab` target, lit for the open screen), and the widget tree and narrator under
/// `name` (the screen's, or vanilla's Previous Page and Next Page for an arrow). The strip draws
/// the text; focused, `ring` is drawn round.
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

/// The picked pack's screens, its main one first, then in the order their links are met.
fn screens(s: &Settings) -> Vec<Screen> {
    match picked(s) {
        Some((_, Kind::Classic)) => classic(s).map_or_else(Vec::new, classic_screens),
        Some((i, Kind::Horizon)) => native_screens(&s.shader_packs[i]),
        None => Vec::new(),
    }
}

fn classic_screens(o: &crate::shader_options::ShaderOptions) -> Vec<Screen> {
    use horizon_shaders::ScreenItem;
    let keys: Vec<&String> = o.options.keys().collect();
    let mut names = vec![(String::new(), 0u8)];
    let mut out = Vec::new();
    let mut at = 0;
    while let Some((name, depth)) = names.get(at).cloned() {
        at += 1;
        let mut items = Vec::new();
        for item in o.screens.screen(&name).map_or(&[][..], |(items, _)| items) {
            match item {
                ScreenItem::Option(n) => {
                    if let Some(k) = keys.iter().position(|k| *k == n) {
                        items.push(Item::Option(k as u16));
                    }
                }
                ScreenItem::Screen(n) if o.screens.screen(n).is_some() => {
                    let k = match names.iter().position(|(m, _)| m == n) {
                        Some(k) => k,
                        None => {
                            names.push((n.clone(), depth.saturating_add(1)));
                            names.len() - 1
                        }
                    };
                    items.push(Item::Link(k as u16));
                }
                ScreenItem::Profile if !o.screens.profiles.is_empty() => items.push(Item::Profile),
                _ => {}
            }
        }
        let comment = |key: &str| o.comment(key).map(plain);
        let (title, about) = if name.is_empty() {
            items.push(Item::Reset);
            (lang::get("General"), lang::get(PACK_SETTINGS))
        } else {
            let title = plain(o.screen_label(&name));
            let title = title.strip_suffix("...").unwrap_or(&title).to_owned();
            let about = comment(&format!("screen.{name}")).unwrap_or_default();
            (title, about)
        };
        out.push(Screen {
            title,
            about,
            items,
            depth,
        });
    }
    out
}

fn native_screens(pack: &crate::shader_pack_list::Entry) -> Vec<Screen> {
    let count = pack.options.len().min(pack.values.len());
    let reset = |mut items: Vec<Item>, first: bool| {
        if first {
            items.push(Item::Reset);
        }
        items
    };
    if pack.pages.is_empty() {
        if count == 0 {
            return Vec::new();
        }
        let items = (0..count).map(|o| Item::Option(o as u16)).collect();
        return vec![Screen {
            title: lang::get("General"),
            about: lang::get(PACK_SETTINGS),
            items: reset(items, true),
            depth: 0,
        }];
    }
    // A page's depth: links from the first page are one away, the rest two.
    let linked: Vec<usize> = pack.pages[0]
        .items
        .iter()
        .filter_map(|i| match i {
            PageItem::Page(p) => Some(*p),
            _ => None,
        })
        .collect();
    pack.pages
        .iter()
        .enumerate()
        .map(|(i, page)| {
            let items = page
                .items
                .iter()
                .filter_map(|item| match *item {
                    PageItem::Option(o) if o < count => Some(Item::Option(o as u16)),
                    PageItem::Page(p) if p < pack.pages.len() && p != i => {
                        Some(Item::Link(p as u16))
                    }
                    _ => None,
                })
                .collect();
            Screen {
                title: if i == 0 {
                    lang::get("General")
                } else {
                    page.name.clone()
                },
                about: page.description.clone(),
                items: reset(items, i == 0),
                depth: match i {
                    0 => 0,
                    _ if linked.contains(&i) => 1,
                    _ => 2,
                },
            }
        })
        .collect()
}

/// What the options screen must do after an interaction here.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    ButtonClick,
    Changed,
    SliderDrag,
    /// Another pack was picked, or the pick turned off (`s.horizon.shader_pack` holds it now).
    Picked,
    OpenFolder,
    /// Save the `shaders/` pack's changed options and load it again with them.
    Apply,
    /// The options of the Horizon pack `s.shader_packs[.0]` changed: save them.
    NativeChanged(usize),
    Done,
}

/// What the keyboard or a controller can focus.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    Item(Item),
    Tab(Tab),
    Search,
    Apply,
    Done,
}

type Line = shell::Line<Tab, Item>;

/// From one tab of the page list to the next, in GUI px (`Columns::page`).
const ROW_STEP: f32 = 18.0;

/// What a place in the page list is: a tab, or a strip's arrow to the screen before or after.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Part {
    Tab,
    Prev,
    Next,
}

/// The page list as laid out (`ShaderUi::side`).
struct Side {
    /// Where each tab is picked: in a strip, its name and the arrows either side of it are the
    /// shown screen and its neighbours (while searching, the neighbours with matches).
    places: Vec<(Tab, [f32; 4], Part)>,
    /// The pack's screens collapsed, when they don't all fit down the column, and the screen it
    /// shows.
    strip: Option<(tabs::Strip, Tab)>,
    /// The tab the page list lights (`ShaderUi::lit`).
    lit: Option<Tab>,
}

pub struct ShaderUi {
    scroll: ui::Scroll,
    dragging: Option<Item>,
    pub focus: Option<Target>,
    pub can_change_value: bool,
    pub search: ui::EditBox,
    /// The page the plate shows while nothing is searched.
    tab: Tab,
    /// The screens a link was followed from, to go back to.
    path: Vec<u16>,
    /// While searching, the page last picked in the page list.
    picked: Option<Tab>,
    /// The Horizon pack whose slider a drag moved: saved once, when it lets go.
    drag_changed: Option<usize>,
}
impl Default for ShaderUi {
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
            search,
            tab: Tab::Packs,
            path: Vec::new(),
            picked: None,
            drag_changed: None,
        }
    }
}

impl ShaderUi {
    /// The screen opens, at `tab`.
    pub fn open(&mut self, tab: Tab) {
        *self = Self {
            tab,
            ..Self::default()
        };
    }
    /// The `shaders/` pack's options changed since they were last applied.
    pub fn pending(s: &Settings) -> bool {
        classic(s).is_some_and(|o| o.changed)
    }
    fn query(&self) -> String {
        self.search.value.trim().to_lowercase()
    }
    /// A tab's name in the page list and over its rows.
    fn tab_name(tab: Tab, screens: &[Screen]) -> String {
        match tab {
            Tab::Packs => lang::get("Shader packs"),
            Tab::Screen(i) => screens
                .get(usize::from(i))
                .map_or_else(String::new, |s| s.title.clone()),
        }
    }
    /// The tabs of the page list: the pack list, then the screens near the main one.
    fn side_tabs(screens: &[Screen]) -> Vec<Tab> {
        std::iter::once(Tab::Packs)
            .chain(
                screens
                    .iter()
                    .enumerate()
                    .filter(|(_, s)| s.depth <= 1)
                    .map(|(i, _)| Tab::Screen(i as u16)),
            )
            .collect()
    }
    /// How many tabs fit down the column, from the pack list to above the foot's buttons, past
    /// the line naming the picked pack.
    fn side_room(cols: &Columns) -> usize {
        let room = cols.button(1)[1] - 4.0 - cols.page(0)[1];
        ((room / ROW_STEP).floor() as usize)
            .saturating_sub(1)
            .max(1)
    }
    /// The page list as drawn. Its tabs one under another while they fit; past that, the pack's
    /// screens collapse into one strip, `‹ Name ›` with a pip per screen, rather than leaving
    /// some out.
    fn side(&self, atlas: &HudAtlas, screens: &[Screen], cols: &Columns, lines: &[Line]) -> Side {
        let all = Self::side_tabs(screens);
        let lit = self.lit(self.current(lines), &all);
        if all.len() <= Self::side_room(cols) {
            let places = (all.iter().enumerate())
                .map(|(i, &tab)| (tab, Self::tab_rect(cols, i), Part::Tab))
                .collect();
            return Side {
                places,
                strip: None,
                lit,
            };
        }
        let pages = &all[1..];
        let shown = pages.iter().position(|&t| Some(t) == lit).unwrap_or(0);
        let names: Vec<String> = pages.iter().map(|&t| Self::tab_name(t, screens)).collect();
        let row = Self::tab_rect(cols, 1);
        let mut strip = tabs::collapsed(atlas, &names, shown, row);
        // While searching, the arrows skip the screens with no matches.
        let prev = pages[..shown].iter().rev().find(|&&t| self.has(t, lines));
        let next = pages[shown + 1..].iter().find(|&&t| self.has(t, lines));
        (strip.has_prev, strip.has_next) = (prev.is_some(), next.is_some());
        let [x, y, w, h] = row;
        let [nx, _, nw, _] = strip.label_area;
        // Either side of the name, as far as the row's ends, the screen before and the one after.
        let mut places = vec![(Tab::Packs, Self::tab_rect(cols, 0), Part::Tab)];
        if let Some(&prev) = prev {
            places.push((prev, [x, y, nx - x, h], Part::Prev));
        }
        places.push((pages[shown], [nx, y, nw, h], Part::Tab));
        if let Some(&next) = next {
            places.push((next, [nx + nw, y, x + w - nx - nw, h], Part::Next));
        }
        Side {
            places,
            strip: Some((strip, pages[shown])),
            lit,
        }
    }
    /// Where the page list's `i`th tab sits: the pack list first, then, past the line naming
    /// the picked pack, its own screens (or the strip they collapse into, at `1`).
    fn tab_rect(cols: &Columns, i: usize) -> [f32; 4] {
        cols.page(if i == 0 { 0 } else { i + 1 })
    }
    /// A row as its name and its value; `None` makes it a button across the row.
    fn parts(item: Item, s: &Settings, screens: &[Screen]) -> (String, Option<String>) {
        let native = picked(s)
            .filter(|p| p.1 == Kind::Horizon)
            .map(|p| &s.shader_packs[p.0]);
        match item {
            Item::Pack(i) => {
                let pack = s.shader_packs.get(usize::from(i));
                let name = pack.map_or_else(String::new, |p| p.title.clone());
                (name, Some(String::new()))
            }
            Item::Folder => (lang::get("pack.openFolder"), None),
            Item::Link(i) => (
                format!("{}...", Self::tab_name(Tab::Screen(i), screens)),
                None,
            ),
            Item::Reset => (lang::get("Reset all options"), None),
            Item::ShadowCache => (lang::get("Shadow Cache"), Some(String::new())),
            Item::Profile => match classic(s) {
                Some(o) => shell::name_and_value(&plain(o.profile_label())),
                None => (String::new(), None),
            },
            Item::Option(i) => {
                if let Some(pack) = native {
                    let i = usize::from(i);
                    let (Some(o), Some(v)) = (pack.options.get(i), pack.values.get(i)) else {
                        return (String::new(), None);
                    };
                    let value = crate::shader_pack_list::show_value(&o.kind, *v);
                    return (o.name.clone(), Some(value));
                }
                match classic(s).and_then(|o| Some((o, option_name(o, i)?))) {
                    Some((o, name)) => {
                        (plain(o.option_name(name)), Some(plain(o.value_text(name))))
                    }
                    None => (String::new(), None),
                }
            }
        }
    }
    /// A tick box's state, for a pack (picked) or an on/off option.
    fn switch(item: Item, s: &Settings) -> Option<bool> {
        match item {
            Item::ShadowCache => Some(s.horizon.shadow_cache),
            Item::Pack(i) => Some(
                s.shader_packs
                    .get(usize::from(i))
                    .is_some_and(|p| p.key == s.horizon.shader_pack),
            ),
            Item::Option(i) => match picked(s)? {
                (p, Kind::Horizon) => {
                    let pack = &s.shader_packs[p];
                    let i = usize::from(i);
                    match pack.options.get(i)?.kind {
                        OptionKind::Bool { .. } => Some(*pack.values.get(i)? != 0.0),
                        _ => None,
                    }
                }
                (_, Kind::Classic) => {
                    let o = classic(s)?;
                    let name = option_name(o, i)?;
                    o.options[name]
                        .switch
                        .then(|| o.value(name) == Some("true"))
                }
            },
            _ => None,
        }
    }
    fn slider(item: Item, s: &Settings) -> Option<f32> {
        let Item::Option(i) = item else { return None };
        match picked(s)? {
            (p, Kind::Horizon) => s.shader_packs[p].slider_position(usize::from(i)),
            (_, Kind::Classic) => {
                let o = classic(s)?;
                o.slider_position(option_name(o, i)?)
            }
        }
    }
    /// What a row does, or says of itself, for the strip.
    fn description(item: Item, s: &Settings, screens: &[Screen]) -> String {
        let native = picked(s)
            .filter(|p| p.1 == Kind::Horizon)
            .map(|p| &s.shader_packs[p.0]);
        match item {
            Item::Pack(i) => {
                let Some(pack) = s.shader_packs.get(usize::from(i)) else {
                    return String::new();
                };
                let kind = lang::get(match pack.kind {
                    Kind::Horizon => "A Horizon shader pack.",
                    Kind::Classic => "An OptiFine or Iris shader pack.",
                });
                let what = lang::get(if pack.key == s.horizon.shader_pack {
                    "In use: press to turn shaders off."
                } else {
                    "Press to use it."
                });
                format!("{kind} {what} ({})", pack.file)
            }
            Item::Folder => lang::get("Put shader pack .zip files or folders in it."),
            Item::Reset => lang::get("Puts every option of this pack back to its default."),
            Item::ShadowCache => lang::get(
                "Reuses the pack shadow map when sun, view and world match. Off by default.",
            ),
            Item::Profile => classic(s)
                .and_then(|o| o.comment("profile"))
                .map(plain)
                .unwrap_or_else(|| {
                    lang::get("Sets many options at once, as the pack groups them.")
                }),
            Item::Link(i) => screens
                .get(usize::from(i))
                .map(|s| s.about.clone())
                .unwrap_or_default(),
            Item::Option(i) => match native {
                Some(pack) => pack
                    .options
                    .get(usize::from(i))
                    .map(|o| o.description.clone())
                    .unwrap_or_default(),
                None => classic(s)
                    .and_then(|o| o.comment(&format!("option.{}", option_name(o, i)?)))
                    .map(plain)
                    .unwrap_or_default(),
            },
        }
    }
    /// The rows of a tab.
    fn items(tab: Tab, s: &Settings, screens: &[Screen]) -> Vec<Item> {
        match tab {
            Tab::Packs => (0..s.shader_packs.len())
                .map(|i| Item::Pack(i as u16))
                .chain([Item::Folder, Item::ShadowCache])
                .collect(),
            Tab::Screen(i) => screens
                .get(usize::from(i))
                .map(|s| s.items.clone())
                .unwrap_or_default(),
        }
    }
    /// The list's lines and its content height: the open page or, while searching, every page's
    /// rows whose name or description has the search in it.
    fn lines(&self, s: &Settings, screens: &[Screen]) -> (Vec<Line>, f32) {
        let query = self.query();
        let mut lines = shell::Lines::new();
        let tabs =
            std::iter::once(Tab::Packs).chain((0..screens.len()).map(|i| Tab::Screen(i as u16)));
        for tab in tabs {
            if query.is_empty() && tab != self.tab {
                continue;
            }
            let rows: Vec<Item> = Self::items(tab, s, screens)
                .into_iter()
                .filter(|&item| {
                    query.is_empty()
                        || self.focus == Some(Target::Item(item))
                        || self.dragging == Some(item)
                        || (!matches!(item, Item::Link(_) | Item::Reset)
                            && (Self::parts(item, s, screens)
                                .0
                                .to_lowercase()
                                .contains(&query)
                                || Self::description(item, s, screens)
                                    .to_lowercase()
                                    .contains(&query)))
                })
                .collect();
            if rows.is_empty() {
                continue;
            }
            // The plate's heading names the open page; results go under their page's name.
            // Horizon's own settings stand apart from the packs, and the packs' from Horizon's.
            let (built_in, rows): (Vec<Item>, Vec<Item>) =
                rows.into_iter().partition(|&i| i == Item::ShadowCache);
            match (query.is_empty(), tab) {
                (true, Tab::Packs) => lines.group(tab, "Installed", rows),
                (true, Tab::Screen(_)) => lines.group(tab, "Pack settings", rows),
                (false, _) if !rows.is_empty() => lines.titled(tab, rows),
                (false, _) => {}
            }
            if !built_in.is_empty() {
                lines.group(tab, "Built-in settings", built_in);
            }
        }
        lines.finish()
    }
    fn layout(&mut self, s: &Settings, w: f32, h: f32) -> (Columns, Vec<Screen>, Vec<Line>, f32) {
        let cols = Columns::of(w, h);
        let screens = screens(s);
        if let Tab::Screen(i) = self.tab
            && usize::from(i) >= screens.len()
        {
            self.tab = Tab::Packs;
            self.path.clear();
        }
        let (lines, content) = self.lines(s, &screens);
        self.scroll.clamp(cols.top, cols.bottom, content);
        (cols, screens, lines, content)
    }
    /// Open `tab` from the page list: the plate shows it or, while searching, scrolls to it.
    fn pick(&mut self, tab: Tab, lines: &[Line], cols: &Columns, content: f32) {
        self.tab = tab;
        self.path.clear();
        self.scroll.amount = 0.0;
        if self.query().is_empty() {
            return;
        }
        if let Some(line) = lines.iter().find(|l| l.page == tab) {
            self.scroll.amount = line.y;
            self.scroll.clamp(cols.top, cols.bottom, content);
            self.picked = Some(tab);
        }
    }
    fn has(&self, tab: Tab, lines: &[Line]) -> bool {
        self.query().is_empty() || lines.iter().any(|l| l.page == tab)
    }
    /// The page the plate shows: the open one or, while searching, the one picked, else the
    /// topmost line's.
    fn current(&self, lines: &[Line]) -> Option<Tab> {
        if self.query().is_empty() {
            return Some(self.tab);
        }
        self.picked.or_else(|| {
            lines
                .iter()
                .find(|l| l.y + l.h() > self.scroll.amount)
                .map(|l| l.page)
        })
    }
    /// The page list's tab that holds `tab`: a screen deeper than the list shows lights the one
    /// its links were followed from.
    fn lit(&self, tab: Option<Tab>, side: &[Tab]) -> Option<Tab> {
        let tab = tab?;
        if side.contains(&tab) {
            return Some(tab);
        }
        self.path
            .iter()
            .rev()
            .map(|&i| Tab::Screen(i))
            .find(|t| side.contains(t))
    }
    fn row_y(&self, cols: &Columns, line: &Line) -> f32 {
        cols.top + line.y - self.scroll.amount
    }

    pub fn draw(
        &mut self,
        s: &Settings,
        batch: &mut HudBatch,
        atlas: &HudAtlas,
        cursor: (f32, f32),
        in_world: bool,
    ) {
        let (w, h) = (batch.width, batch.height);
        let (cols, screens, lines, content) = self.layout(s, w, h);
        shell::backdrop(batch, atlas, in_world);
        let current = self.current(&lines);
        let side = self.side(atlas, &screens, &cols, &lines);
        shell::mark(batch, atlas, &cols, &lang::get("Shaders"));
        if let Some((strip, _)) = &side.strip {
            // The strip's screen is open: its row lit as the list's open tab is.
            if side.lit.is_some_and(|t| t != Tab::Packs) {
                let row @ [x, y, _, h] = Self::tab_rect(&cols, 1);
                style::fill(batch, atlas, row, style::OPEN);
                style::fill(batch, atlas, [x, y, 2.0, h], style::ACCENT);
            }
            tabs::draw(batch, atlas, strip);
        }
        for &(tab, rect, part) in &side.places {
            let name = match part {
                Part::Tab => Self::tab_name(tab, &screens),
                Part::Prev => lang::get("spectatorMenu.previous_page"),
                Part::Next => lang::get("spectatorMenu.next_page"),
            };
            let flags = (
                self.has(tab, &lines),
                side.lit == Some(tab),
                self.focus == Some(Target::Tab(tab)),
            );
            if let Some((_, shown)) = side.strip
                && tab != Tab::Packs
            {
                // The shown screen's ring goes round the whole row, as a listed tab's does.
                let ring = if tab == shown {
                    Self::tab_rect(&cols, 1)
                } else {
                    rect
                };
                strip_place(batch, atlas, (rect, ring), &name, flags, cursor);
            } else {
                let name = style::fit(atlas, &name, style::CONTROL_W - 16.0);
                shell::page_tab(batch, atlas, rect, &name, flags, cursor);
            }
        }
        // The picked pack's own screens go under its name, apart from Horizon's pack list.
        if side.places.len() > 1 {
            let [x, y, w, _] = cols.page(1);
            let pack = match picked(s) {
                Some((i, Kind::Horizon)) => s.shader_packs[i].title.as_str(),
                _ => s
                    .horizon
                    .shader_pack
                    .strip_suffix(".zip")
                    .unwrap_or(&s.horizon.shader_pack),
            };
            let caption = lang::tr("%s settings", &[&pack]);
            let rect = [x + 2.0, y + 6.0, w - 4.0, 8.0];
            crate::widgets::record(&caption, rect, false);
            let caption = Text::literal(style::fit(atlas, &caption, w - 4.0));
            batch.text_styled(atlas, &caption, rect[0], rect[1], style::MUTED, true);
        }
        let focused = self.focus == Some(Target::Search);
        shell::search_field(&mut self.search, batch, atlas, &cols, cursor, focused);

        let query = self.search.value.trim();
        let heading = if query.is_empty() {
            Self::tab_name(current.unwrap_or(self.tab), &screens)
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
            |tab| Self::tab_name(tab, &screens),
            |batch, item: Item, y| {
                let [rx, ry, rw, rh] = cols.row(y);
                let hover = over_list && ui::hovered(cursor, rx, ry, rw, rh);
                let focused = self.focus == Some(Target::Item(item));
                if hover || self.dragging == Some(item) || (focused && described.is_none()) {
                    described = Some(item);
                }
                let (name, value) = Self::parts(item, s, &screens);
                let label = match &value {
                    Some(v) if !v.is_empty() => format!("{name}: {v}"),
                    _ => name.clone(),
                };
                let view = shell::RowView {
                    label: &label,
                    name: &name,
                    value: value.as_deref(),
                    switch: Self::switch(item, s),
                    slider: Self::slider(item, s),
                    live: true,
                    hover,
                    focused,
                    held: self.dragging == Some(item),
                    key: None,
                };
                shell::row(batch, atlas, &cols, y, &view);
            },
        );
        shell::scroller(batch, atlas, &cols, content, self.scroll.amount);

        // The strip: the pointed-at row, else what the page holds.
        let [x, y, ..] = cols.dock;
        style::panel(batch, atlas, cols.dock);
        let (name, text) = match described {
            Some(item) => (
                Self::parts(item, s, &screens).0,
                Self::description(item, s, &screens),
            ),
            None => match current {
                Some(Tab::Packs) | None => (
                    lang::get("Shader packs"),
                    lang::get(if s.shader_packs.is_empty() {
                        "No shader packs yet: open the pack folder and put some in it."
                    } else {
                        "Pick a shader pack. Picking the one in use turns shaders off."
                    }),
                ),
                Some(tab) => {
                    let about = match tab {
                        Tab::Screen(i) => screens[usize::from(i)].about.clone(),
                        Tab::Packs => String::new(),
                    };
                    let about = if Self::pending(s) {
                        lang::get("Changed options apply with Apply, or when you leave.")
                    } else {
                        about
                    };
                    (Self::tab_name(tab, &screens), about)
                }
            },
        };
        batch.text_styled(
            atlas,
            &Text::literal(name),
            x + 12.0,
            y + 8.0,
            style::WHITE,
            true,
        );
        shell::dock_note(batch, atlas, &cols, text, style::GREY);

        let mut foot = vec![(cols.button(0), "gui.done", Target::Done, Declared::Done)];
        if Self::pending(s) {
            foot.push((cols.button(1), "Apply", Target::Apply, Declared::Apply));
        }
        for (rect, label, target, declared) in foot {
            let label = Text::literal(lang::get(label));
            let focused = self.focus == Some(target);
            crate::widgets::declare_action(declared, rect, true);
            shell::foot_button(batch, atlas, rect, &label, cursor, (focused, true));
        }
    }

    /// A press (or a drag with `drag`) at `(x, y)`.
    pub fn click(
        &mut self,
        s: &mut Settings,
        atlas: &HudAtlas,
        (x, y): (f32, f32),
        (w, h): (f32, f32),
        drag: bool,
        right: bool,
        shift: bool,
    ) -> Vec<Action> {
        let (cols, screens, lines, content) = self.layout(s, w, h);
        let at = |[rx, ry, rw, rh]: [f32; 4]| ui::hovered((x, y), rx, ry, rw, rh);
        if drag {
            if self.scroll.held() {
                self.picked = None;
                self.scroll.drag(y, cols.top, cols.bottom, content);
                return Vec::new();
            }
            if let Some(item) = self.dragging {
                let actions = Self::set_slider(s, item, cols.slider_at(x));
                return self.hold(actions);
            }
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
        if Self::pending(s) && at(cols.button(1)) {
            self.focus = None;
            return vec![Action::ButtonClick, Action::Apply];
        }
        for (tab, rect, _) in self.side(atlas, &screens, &cols, &lines).places {
            if at(rect) && self.has(tab, &lines) {
                self.focus = Some(Target::Tab(tab));
                self.pick(tab, &lines, &cols, content);
                return vec![Action::ButtonClick];
            }
        }
        if y < cols.top || y >= cols.bottom {
            return Vec::new();
        }
        let Some(item) = lines.iter().find_map(|line| {
            let item = line.setting?;
            at(cols.row(self.row_y(&cols, line))).then_some(item)
        }) else {
            return Vec::new();
        };
        self.focus = Some(Target::Item(item));
        self.activate(s, item, right || shift, Some((x, &cols)))
    }
    /// Option `item`'s slider at `at` (0 to 1).
    fn set_slider(s: &mut Settings, item: Item, at: f32) -> Vec<Action> {
        let Item::Option(i) = item else {
            return Vec::new();
        };
        match picked(s) {
            Some((p, Kind::Horizon)) => {
                if s.shader_packs[p].set_slider_position(usize::from(i), at) {
                    return vec![Action::SliderDrag, Action::NativeChanged(p)];
                }
            }
            Some((_, Kind::Classic)) => {
                let pack = s.horizon.shader_pack.clone();
                if let Some(o) = s.shader_options.as_mut().filter(|o| o.pack == pack)
                    && let Some(name) = option_name(o, i).map(str::to_owned)
                {
                    o.set_position(&name, at);
                    return vec![Action::SliderDrag];
                }
            }
            None => {}
        }
        Vec::new()
    }
    /// Option `item`'s slider one of its own steps on (back with `!forward`), stopping at its ends.
    fn step_slider(s: &mut Settings, item: Item, forward: bool) -> Vec<Action> {
        let Item::Option(i) = item else {
            return Vec::new();
        };
        match picked(s) {
            Some((p, Kind::Horizon)) => {
                if s.shader_packs[p].nudge_option(usize::from(i), !forward) {
                    return vec![Action::SliderDrag, Action::NativeChanged(p)];
                }
                Vec::new()
            }
            Some((_, Kind::Classic)) => {
                let Some(o) = classic(s) else {
                    return Vec::new();
                };
                let Some(name) = option_name(o, i) else {
                    return Vec::new();
                };
                let n = o.options[name].values.len().saturating_sub(1).max(1) as f32;
                let Some(pos) = o.slider_position(name) else {
                    return Vec::new();
                };
                let next = ((pos * n).round() + if forward { 1.0 } else { -1.0 }) / n;
                if !(0.0..=1.0).contains(&next) {
                    return Vec::new();
                }
                Self::set_slider(s, item, next)
            }
            None => Vec::new(),
        }
    }
    /// Press `item`: `at` is where on its row and its slider's track (`None` from the keyboard).
    fn activate(
        &mut self,
        s: &mut Settings,
        item: Item,
        back: bool,
        at: Option<(f32, &Columns)>,
    ) -> Vec<Action> {
        if Self::slider(item, s).is_some() {
            self.can_change_value = true;
            return match at {
                Some((x, cols)) if x >= cols.track().0 => {
                    self.dragging = Some(item);
                    let actions = Self::set_slider(s, item, cols.slider_at(x));
                    self.hold(actions)
                }
                _ => Vec::new(),
            };
        }
        self.can_change_value = false;
        let pack = s.horizon.shader_pack.clone();
        let native = picked(s).filter(|p| p.1 == Kind::Horizon).map(|p| p.0);
        let options = s.shader_options.as_mut().filter(|o| o.pack == pack);
        let changed = vec![Action::ButtonClick, Action::Changed];
        match item {
            Item::Pack(i) => {
                let Some(p) = s.shader_packs.get(usize::from(i)) else {
                    return Vec::new();
                };
                s.horizon.shader_pack = if s.horizon.shader_pack == p.key {
                    String::new()
                } else {
                    p.key.clone()
                };
                vec![Action::ButtonClick, Action::Picked]
            }
            Item::Folder => vec![Action::ButtonClick, Action::OpenFolder],
            Item::ShadowCache => {
                s.horizon.shadow_cache = !s.horizon.shadow_cache;
                changed
            }
            Item::Link(i) => {
                if let Tab::Screen(from) = self.tab {
                    self.path.push(from);
                }
                self.tab = Tab::Screen(i);
                self.scroll.amount = 0.0;
                self.search.set_value("");
                self.picked = None;
                self.focus = None;
                vec![Action::ButtonClick]
            }
            Item::Profile => match options {
                Some(o) => {
                    o.next_profile();
                    changed
                }
                None => Vec::new(),
            },
            Item::Reset => match (native, options) {
                (Some(p), _) => {
                    s.shader_packs[p].reset_options();
                    vec![Action::ButtonClick, Action::NativeChanged(p)]
                }
                (None, Some(o)) => {
                    o.reset();
                    changed
                }
                _ => Vec::new(),
            },
            Item::Option(i) => match (native, options) {
                (Some(p), _) if s.shader_packs[p].step_option(usize::from(i), back, false) => {
                    vec![Action::ButtonClick, Action::NativeChanged(p)]
                }
                (None, Some(o)) => match option_name(o, i).map(str::to_owned) {
                    Some(name) => {
                        o.cycle(&name, back);
                        changed
                    }
                    None => Vec::new(),
                },
                _ => Vec::new(),
            },
        }
    }
    /// A drag's change to a Horizon pack waits for the drag to end.
    fn hold(&mut self, mut actions: Vec<Action>) -> Vec<Action> {
        actions.retain(|a| match *a {
            Action::NativeChanged(p) => {
                self.drag_changed = Some(p);
                false
            }
            _ => true,
        });
        actions
    }
    /// Lets go of a slider or the scrollbar: whether a slider was held (it plays its click), and
    /// the Horizon pack its drag changed.
    pub fn release(&mut self) -> (bool, Option<usize>) {
        self.scroll.release();
        (self.dragging.take().is_some(), self.drag_changed.take())
    }
    /// The mouse wheel scrolls the list.
    pub fn scroll(&mut self, s: &Settings, (w, h): (f32, f32), amount: f32) {
        let (cols, _, _, content) = self.layout(s, w, h);
        self.picked = None;
        self.scroll
            .wheel(amount, 18.0, cols.top, cols.bottom, content);
    }
    /// Escape: back out of a screen a link opened. False when there is none to go back from.
    pub fn back(&mut self) -> bool {
        let Some(from) = self.path.pop() else {
            return false;
        };
        self.tab = Tab::Screen(from);
        self.scroll.amount = 0.0;
        self.focus = None;
        true
    }
    fn focusables(
        &self,
        s: &Settings,
        atlas: &HudAtlas,
        cols: &Columns,
        screens: &[Screen],
        lines: &[Line],
        near: bool,
    ) -> Vec<(Target, [f32; 4])> {
        let mut out = vec![(Target::Search, cols.search())];
        for (tab, rect, _) in self.side(atlas, screens, cols, lines).places {
            if self.has(tab, lines) {
                out.push((Target::Tab(tab), rect));
            }
        }
        let rows: Vec<(Item, f32, bool)> = lines
            .iter()
            .filter_map(|line| Some((line.setting?, self.row_y(cols, line), true)))
            .collect();
        for &(item, y, _) in &rows[shell::reach(&rows, cols, near)] {
            out.push((Target::Item(item), cols.row(y)));
        }
        if Self::pending(s) {
            out.push((Target::Apply, cols.button(1)));
        }
        out.push((Target::Done, cols.button(0)));
        out
    }

    /// Keys: typing in the search field, Tab and the arrows between controls, Left and Right on a
    /// slider, Enter or Space to press. False: the options screen handles it (Escape goes back).
    pub fn key_modifiers(
        &mut self,
        s: &mut Settings,
        atlas: &HudAtlas,
        key: winit::keyboard::KeyCode,
        text: Option<&str>,
        shift: bool,
        (w, h): (f32, f32),
    ) -> (Vec<Action>, bool) {
        use winit::keyboard::KeyCode as K;
        if key == K::Escape {
            self.can_change_value = false;
            return (Vec::new(), self.back());
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
        let (cols, screens, lines, content) = self.layout(s, w, h);
        let focusables = self.focusables(s, atlas, &cols, &screens, &lines, key != K::Tab);
        let current = self
            .focus
            .and_then(|f| focusables.iter().position(|&(t, _)| t == f));
        let focus_on = |ui: &mut Self, (target, rect): (Target, [f32; 4])| {
            ui.focus = Some(target);
            ui.can_change_value = false;
            if matches!(target, Target::Item(_)) {
                shell::scroll_into_view(&mut ui.scroll, rect, &cols, content);
                ui.picked = None;
            }
        };
        if key == K::Tab {
            let next = shell::tab_step(focusables.len(), current, shift);
            focus_on(self, focusables[next]);
            return (Vec::new(), true);
        }
        if matches!(key, K::ArrowLeft | K::ArrowRight)
            && self.can_change_value
            && let Some(Target::Item(item)) = self.focus
            && Self::slider(item, s).is_some()
        {
            return (Self::step_slider(s, item, key == K::ArrowRight), true);
        }
        if matches!(
            key,
            K::ArrowUp | K::ArrowDown | K::ArrowLeft | K::ArrowRight
        ) {
            let is_row = |t: Target| matches!(t, Target::Item(_));
            let Some(next) = shell::arrow_step(&focusables, current, key, &cols, is_row) else {
                return (Vec::new(), false);
            };
            focus_on(self, focusables[next]);
            return (Vec::new(), true);
        }
        if matches!(key, K::Space | K::Enter | K::NumpadEnter)
            && let Some(target) = self.focus
        {
            let actions = match target {
                Target::Search => Vec::new(),
                Target::Tab(tab) => {
                    self.pick(tab, &lines, &cols, content);
                    vec![Action::ButtonClick]
                }
                Target::Item(item) if Self::slider(item, s).is_some() => {
                    self.can_change_value = !self.can_change_value;
                    Vec::new()
                }
                Target::Item(item) => self.activate(s, item, shift, None),
                Target::Apply => {
                    self.focus = Some(Target::Done);
                    vec![Action::ButtonClick, Action::Apply]
                }
                Target::Done => vec![Action::ButtonClick, Action::Done],
            };
            return (actions, true);
        }
        (Vec::new(), false)
    }
    pub fn typing(&self) -> bool {
        self.focus == Some(Target::Search)
    }
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
        true
    }
    fn searched(&mut self) {
        self.scroll.amount = 0.0;
        self.picked = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shader_options::ShaderOptions;
    use crate::shader_pack_list::Entry;
    use horizon_shaders::{Lang, Macros, PackOption, Properties, Screens};
    use std::collections::BTreeMap;
    use winit::keyboard::KeyCode as K;

    const SIZE: (f32, f32) = (854.0, 480.0);

    fn atlas() -> HudAtlas {
        HudAtlas::build(std::path::Path::new("/nonexistent/horizon-test-assets")).unwrap()
    }

    fn entry(file: &str, kind: Kind) -> Entry {
        Entry {
            file: file.into(),
            key: file.into(),
            title: file.into(),
            kind,
            options: Vec::new(),
            values: Vec::new(),
            pages: Vec::new(),
        }
    }

    /// Two packs, the first picked: a `shaders/` pack with a main screen, a Lighting screen and
    /// a Shadows screen inside it.
    fn settings() -> Settings {
        let mut s = Settings {
            shader_packs: vec![entry("A.zip", Kind::Classic), entry("B.zip", Kind::Classic)],
            ..Settings::default()
        };
        s.horizon.shader_pack = "A.zip".into();
        let option = |name: &str, values: &[&str], switch: bool| {
            let o = PackOption {
                name: name.into(),
                default: values[0].into(),
                values: values.iter().map(|v| (*v).into()).collect(),
                switch,
                tested: true,
            };
            (name.to_owned(), o)
        };
        let options = BTreeMap::from([
            option("BLOOM", &["false", "true"], true),
            option("SHADOW_RES", &["1024", "2048", "4096"], false),
            option("SUN", &["1", "2"], false),
        ]);
        let text = "screen = <profile> [LIGHTING] BLOOM\nscreen.LIGHTING = SUN [SHADOWS]\n\
                    screen.SHADOWS = SHADOW_RES\nsliders = SHADOW_RES\n\
                    profile.LOW = SHADOW_RES=1024\nprofile.HIGH = SHADOW_RES=4096\n";
        let screens = Screens::parse(&Properties::parse(text, &Macros::standard()), &options);
        s.shader_options = Some(ShaderOptions::new(
            "A.zip".into(),
            options,
            screens,
            Lang::default(),
            BTreeMap::new(),
        ));
        s
    }

    #[test]
    fn a_packs_screens_are_pages_its_main_one_first() {
        let s = settings();
        let screens = screens(&s);
        let titles: Vec<&str> = screens.iter().map(|s| s.title.as_str()).collect();
        assert_eq!(titles, ["General", "LIGHTING", "SHADOWS"]);
        assert_eq!(
            screens[0].items,
            [Item::Profile, Item::Link(1), Item::Option(0), Item::Reset]
        );
        assert_eq!(screens[1].items, [Item::Option(2), Item::Link(2)]);
        // The page list holds the main screen and the ones it links to.
        let side = ShaderUi::side_tabs(&screens);
        assert_eq!(side, [Tab::Packs, Tab::Screen(0), Tab::Screen(1)]);
        // Another pack's options are not this one's.
        let mut other = s.clone();
        other.horizon.shader_pack = "B.zip".into();
        assert!(super::screens(&other).is_empty());
    }

    /// The picked pack with `n` screens, each linked from its main one: more than the page list
    /// of a short window holds.
    fn many_screens(n: usize) -> Settings {
        let mut s = settings();
        let names: Vec<String> = (0..n).map(|i| format!("S{i}")).collect();
        let options: BTreeMap<String, PackOption> = names
            .iter()
            .map(|name| {
                let o = PackOption {
                    name: format!("O_{name}"),
                    default: "false".into(),
                    values: vec!["false".into(), "true".into()],
                    switch: true,
                    tested: true,
                };
                (o.name.clone(), o)
            })
            .collect();
        let mut text = format!(
            "screen = {}\n",
            names
                .iter()
                .map(|n| format!("[{n}]"))
                .collect::<Vec<_>>()
                .join(" ")
        );
        for name in &names {
            text += &format!("screen.{name} = O_{name}\n");
        }
        let screens = Screens::parse(&Properties::parse(&text, &Macros::standard()), &options);
        s.shader_options = Some(ShaderOptions::new(
            "A.zip".into(),
            options,
            screens,
            Lang::default(),
            BTreeMap::new(),
        ));
        s
    }

    /// A short window: 854x480 at GUI scale 2, or 720p at its automatic scale.
    const SHORT: (f32, f32) = (427.0, 240.0);

    fn side_of(ui: &ShaderUi, s: &Settings, size: (f32, f32)) -> (Vec<(Tab, [f32; 4])>, bool) {
        let screens = screens(s);
        let (lines, _) = ui.lines(s, &screens);
        let side = ui.side(&atlas(), &screens, &Columns::of(size.0, size.1), &lines);
        let places = side.places.iter().map(|p| (p.0, p.1)).collect();
        (places, side.strip.is_some())
    }

    #[test]
    fn a_page_list_too_short_for_the_screens_collapses_them_and_leaves_none_out() {
        let s = many_screens(12);
        let ui = ShaderUi::default();
        // Tall enough, the list as it always was: every screen a tab, one under another.
        let (tall, strip) = side_of(&ui, &s, SIZE);
        assert!(!strip);
        assert_eq!(tall.len(), 1 + 13);
        assert!(tall.windows(2).all(|p| p[1].1[1] > p[0].1[1]));
        // Short: the pack list, then General between nothing and its next screen.
        let (short, strip) = side_of(&ui, &s, SHORT);
        assert!(strip);
        let tabs: Vec<Tab> = short.iter().map(|p| p.0).collect();
        assert_eq!(tabs, [Tab::Packs, Tab::Screen(0), Tab::Screen(1)]);
        // Every place is inside the column, the strip's on one row.
        let cols = Columns::of(SHORT.0, SHORT.1);
        for (_, [x, _, w, _]) in &short {
            assert!(*x >= cols.side_x && x + w <= cols.side_x + style::CONTROL_W);
        }
        assert_eq!(short[1].1[1], short[2].1[1]);
    }

    #[test]
    fn the_strips_arrows_walk_every_screen_and_its_ends_go_no_further() {
        let mut s = many_screens(12);
        let mut ui = ShaderUi::default();
        ui.open(Tab::Screen(0));
        let atlas = atlas();
        let middle = |[x, y, w, h]: [f32; 4]| (x + w / 2.0, y + h / 2.0);
        let mut seen = vec![ui.tab];
        for _ in 0..20 {
            let (places, _) = side_of(&ui, &s, SHORT);
            let name_x = places.iter().find(|p| p.0 == ui.tab).unwrap().1[0];
            let Some(&(next, rect)) = places.iter().find(|p| p.1[0] > name_x) else {
                break;
            };
            ui.click(&mut s, &atlas, middle(rect), SHORT, false, false, false);
            assert_eq!(ui.tab, next);
            seen.push(ui.tab);
        }
        let all: Vec<Tab> = (0..13).map(Tab::Screen).collect();
        assert_eq!(seen, all);
        // At the last screen there is nothing after it to click: its arrow's side picks nothing.
        let cols = Columns::of(SHORT.0, SHORT.1);
        let [x, y, w, h] = ShaderUi::tab_rect(&cols, 1);
        ui.click(
            &mut s,
            &atlas,
            (x + w - 2.0, y + h / 2.0),
            SHORT,
            false,
            false,
            false,
        );
        assert_eq!(ui.tab, Tab::Screen(12));
        // The one before it, from the keyboard: Enter on the arrow opens it, and the focus
        // follows it to the middle.
        let (places, _) = side_of(&ui, &s, SHORT);
        let before = places.iter().find(|p| p.0 == Tab::Screen(11)).unwrap().1;
        ui.focus = Some(Target::Tab(Tab::Screen(11)));
        ui.key_modifiers(&mut s, &atlas, K::Enter, None, false, SHORT);
        assert_eq!(ui.tab, Tab::Screen(11));
        let (places, _) = side_of(&ui, &s, SHORT);
        let now = places.iter().find(|p| p.0 == Tab::Screen(11)).unwrap().1;
        assert!(now[0] > before[0], "{now:?} {before:?}");
    }

    #[test]
    fn while_searching_the_strips_arrows_skip_screens_without_matches() {
        let mut s = many_screens(12);
        let mut ui = ShaderUi::default();
        // O_S1, O_S10 and O_S11: screens 2, 11 and 12, none before the first.
        ui.type_text("o_s1");
        let side = |ui: &ShaderUi, s: &Settings| {
            let screens = screens(s);
            let (lines, _) = ui.lines(s, &screens);
            let side = ui.side(&atlas(), &screens, &Columns::of(SHORT.0, SHORT.1), &lines);
            let (strip, _) = side.strip.unwrap();
            let tabs: Vec<Tab> = side.places.iter().map(|p| p.0).collect();
            (tabs, strip.has_prev, strip.has_next, side.places)
        };
        let (tabs, prev, next, places) = side(&ui, &s);
        assert_eq!(tabs, [Tab::Packs, Tab::Screen(2), Tab::Screen(11)]);
        assert_eq!((prev, next), (false, true));
        // Its arrow opens the next match, which has the first one before it and the last after.
        let [x, y, w, h] = places[2].1;
        ui.click(
            &mut s,
            &atlas(),
            (x + w / 2.0, y + h / 2.0),
            SHORT,
            false,
            false,
            false,
        );
        let (tabs, prev, next, _) = side(&ui, &s);
        assert_eq!(
            tabs,
            [Tab::Packs, Tab::Screen(2), Tab::Screen(11), Tab::Screen(12)]
        );
        assert_eq!((prev, next), (true, true));
    }

    #[test]
    fn a_controllers_bumpers_find_the_strips_screens_as_tabs() {
        let s = many_screens(12);
        let mut ui = ShaderUi::default();
        ui.open(Tab::Screen(4));
        let mut batch = HudBatch::new([854, 480]);
        ui.draw(&s, &mut batch, &atlas(), (0.0, 0.0), false);
        crate::widgets::end_frame(u64::MAX);
        let tabs: Vec<(f32, bool)> = crate::widgets::last_targets()
            .into_iter()
            .filter_map(|t| match t.kind {
                crate::widgets::Kind::Tab(open) => Some((t.rect[0], open)),
                _ => None,
            })
            .collect();
        // The pack list, the screen before, the open one (lit) and the one after.
        assert_eq!(tabs.len(), 4, "{tabs:?}");
        assert_eq!(tabs.iter().filter(|t| t.1).count(), 1);
        let (before, open, after) = (tabs[1].0, tabs[2], tabs[3].0);
        assert!(open.1 && before < open.0 && open.0 < after);
    }

    #[test]
    fn picking_a_pack_and_picking_it_again_turns_shaders_off() {
        for (kind, key) in [(Kind::Classic, "B.zip"), (Kind::Horizon, "horizon:warm")] {
            let mut s = settings();
            s.shader_packs[1].kind = kind;
            s.shader_packs[1].key = key.into();
            let mut ui = ShaderUi::default();
            let actions = ui.activate(&mut s, Item::Pack(1), false, None);
            assert_eq!(actions, [Action::ButtonClick, Action::Picked]);
            assert_eq!(s.horizon.shader_pack, key);
            assert_eq!(picked(&s), Some((1, kind)));
            assert_eq!(ShaderUi::switch(Item::Pack(1), &s), Some(true));
            ui.activate(&mut s, Item::Pack(1), false, None);
            assert_eq!(s.horizon.shader_pack, "");
            assert_eq!(ShaderUi::switch(Item::Pack(0), &s), Some(false));
        }
    }

    #[test]
    fn options_change_until_applied_and_links_go_in_and_back_out() {
        let mut s = settings();
        let mut ui = ShaderUi::default();
        ui.open(Tab::Screen(0));
        assert!(!ShaderUi::pending(&s));
        ui.activate(&mut s, Item::Option(0), false, None);
        assert_eq!(ShaderUi::switch(Item::Option(0), &s), Some(true));
        assert!(ShaderUi::pending(&s));
        ui.activate(&mut s, Item::Link(1), false, None);
        ui.activate(&mut s, Item::Link(2), false, None);
        assert_eq!(ui.tab, Tab::Screen(2));
        // The page list lights the screen the link was followed from.
        let screens = screens(&s);
        let side = ShaderUi::side_tabs(&screens);
        assert_eq!(ui.lit(Some(ui.tab), &side), Some(Tab::Screen(1)));
        let (_, handled) = ui.key_modifiers(&mut s, &atlas(), K::Escape, None, false, SIZE);
        assert!(handled);
        assert_eq!(ui.tab, Tab::Screen(1));
        ui.key_modifiers(&mut s, &atlas(), K::Escape, None, false, SIZE);
        let (_, handled) = ui.key_modifiers(&mut s, &atlas(), K::Escape, None, false, SIZE);
        assert!(!handled, "the main screen's Escape closes the screen");
        // Reset puts the option back.
        ui.activate(&mut s, Item::Reset, false, None);
        assert_eq!(ShaderUi::switch(Item::Option(0), &s), Some(false));
    }

    #[test]
    fn the_search_finds_an_option_on_a_screen_inside_another() {
        let s = settings();
        let mut ui = ShaderUi::default();
        ui.type_text("shadow_res");
        let screens = screens(&s);
        let (lines, _) = ui.lines(&s, &screens);
        let rows: Vec<Item> = lines.iter().filter_map(|l| l.setting).collect();
        assert_eq!(rows, [Item::Option(1)]);
        assert!(lines.iter().all(|l| l.page == Tab::Screen(2)));
        assert_eq!(ShaderUi::slider(Item::Option(1), &s), Some(0.0));
    }

    #[test]
    fn tab_reaches_every_pack_the_folder_and_the_built_in_setting() {
        let mut s = settings();
        let mut ui = ShaderUi::default();
        let mut seen = Vec::new();
        for _ in 0..12 {
            ui.key_modifiers(&mut s, &atlas(), K::Tab, None, false, SIZE);
            if let Some(Target::Item(item)) = ui.focus
                && !seen.contains(&item)
            {
                seen.push(item);
            }
        }
        assert_eq!(
            seen,
            [
                Item::Pack(0),
                Item::Pack(1),
                Item::Folder,
                Item::ShadowCache
            ]
        );
    }

    #[test]
    fn a_horizon_packs_slider_drag_is_saved_once_when_let_go() {
        let mut pack = entry("H", Kind::Horizon);
        pack.key = "horizon:slider_test".into();
        pack.options = vec![horizon_shader_api::PackOption {
            id: "strength".into(),
            name: "Strength".into(),
            description: String::new(),
            kind: OptionKind::Float {
                default: 0.5,
                min: 0.0,
                max: 1.0,
                step: 0.1,
            },
        }];
        pack.values = vec![0.5];
        let mut s = Settings {
            shader_packs: vec![pack],
            ..Settings::default()
        };
        s.horizon.shader_pack = "horizon:slider_test".into();
        let mut ui = ShaderUi::default();
        ui.open(Tab::Screen(0));
        let cols = Columns::of(SIZE.0, SIZE.1);
        let x = cols.track().0 + 2.0;
        let actions = ui.activate(&mut s, Item::Option(0), false, Some((x, &cols)));
        assert_eq!(actions, [Action::SliderDrag]);
        assert_eq!(s.shader_packs[0].values[0], 0.0);
        assert_eq!(ui.release(), (true, Some(0)));
        assert_eq!(ui.release(), (false, None));
    }

    #[test]
    fn every_text_is_in_the_language_table() {
        let mut s = settings();
        let (_, missed) = lang::scoped(lang::Language::load(&[], "de_de"), || {
            let screens = screens(&s);
            // The folder's and the profile's names are vanilla's (`pack.openFolder`).
            for item in [Item::Pack(0), Item::Pack(1), Item::Reset] {
                ShaderUi::parts(item, &s, &screens);
                ShaderUi::description(item, &s, &screens);
            }
            s.shader_packs[0].kind = Kind::Horizon;
            ShaderUi::description(Item::Pack(0), &s, &screens);
            for text in [
                "Shaders",
                "Shader packs",
                "Installed",
                "General",
                "Pack settings",
                "Built-in settings",
                "%s settings",
                PACK_SETTINGS,
                "Shadow Cache",
                "Reuses the pack shadow map when sun, view and world match. Off by default.",
                "Apply",
                "Search options",
                "Results for \"%s\"",
                "No shader packs yet: open the pack folder and put some in it.",
                "Pick a shader pack. Picking the one in use turns shaders off.",
                "Changed options apply with Apply, or when you leave.",
            ] {
                lang::get(text);
            }
        });
        assert_eq!(missed, Vec::<String>::new());
    }
}
