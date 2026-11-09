//! Horizon's controller settings screen, on the settings shell (`settings_shell`): one player's
//! `PadSettings`, which that player's options hold, as pages of rows, and what pressing them does.
//! The shell draws and drives it, Undo, the search and each binding's wait included. It says whose
//! settings they are and what device the game sees; which controller drives which player stays
//! the splitscreen router's.
use crate::{
    gamepad::{self, GlyphOf, GlyphStyle, MenuJob, PadSettings},
    lang,
    options_ui::Event,
    pad_glyph::{Glyph, Glyphs},
    settings::{Opt, Settings},
    settings_shell::{
        self as shell, Binding, Capture, Group, Output, Reset, Row, Screen, Sheet, Shown, Value,
    },
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
/// What the chosen value does, for a setting whose values each do something different.
fn choice_description(opt: Opt, pad: &PadSettings) -> Option<&'static str> {
    Some(match opt {
        Opt::PadGameplayGuide => match pad.guide_detail {
            gamepad::GuideDetail::Full => {
                "Shows what your buttons do right now, in play and in inventories."
            }
            gamepad::GuideDetail::Basic => {
                "Shows only what you can do with what you're looking at, and the main inventory buttons."
            }
            gamepad::GuideDetail::Off => "Doesn't show what your buttons do.",
        },
        Opt::PadGlyphs => match pad.glyphs {
            GlyphStyle::Auto => "Shows the buttons of the controller you're using.",
            GlyphStyle::Xbox => "Always shows Xbox buttons.",
            GlyphStyle::PlayStation => "Always shows PlayStation buttons.",
            GlyphStyle::Nintendo => "Always shows Nintendo buttons.",
        },
        Opt::PadNintendoButtons => {
            if pad.nintendo_letters {
                "A, on the right, confirms and jumps and B goes back, as on a Switch."
            } else {
                "B, at the bottom, confirms and jumps and A goes back, as on other controllers."
            }
        }
        Opt::PadKeyboard => match pad.keyboard {
            gamepad::TextKeyboard::Auto => {
                "Opens Steam's keyboard on a Steam Deck under Steam, and Horizon's otherwise."
            }
            gamepad::TextKeyboard::Horizon => "Always opens Horizon's keyboard.",
            gamepad::TextKeyboard::Steam => "Always opens Steam's keyboard.",
        },
        Opt::PadLightbar => match pad.lightbar {
            gamepad::Lightbar::Health => {
                "A DualSense's light shows your health, from green through amber to red."
            }
            gamepad::Lightbar::Dimension => {
                "A DualSense's light changes with the dimension you're in."
            }
            gamepad::Lightbar::Off => "A DualSense keeps its own light.",
        },
        _ => return None,
    })
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
                    vec![
                        option(
                            Opt::PadGameplayGuide,
                            "Show the buttons for what you can do, while you play with a controller.",
                        ),
                        option(
                            Opt::PadCraftOnServers,
                            "Lets LT + A craft on servers other than your own worlds. Some servers count it as a macro.",
                        ),
                        option(
                            Opt::PadTriggerThreshold,
                            "How far a trigger is pulled before it counts as pressed.",
                        ),
                    ],
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
                        option(
                            Opt::PadNintendoButtons,
                            "Whether a Nintendo controller's A and B work by where they sit or by their letters.",
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
    /// The glyphs menus show, letter mode included.
    pub glyphs: Glyphs,
}

fn glyphs_name(style: GlyphStyle) -> String {
    lang::get(match style {
        GlyphStyle::Auto => "Unknown",
        GlyphStyle::Xbox => "horizon.options.glyphs.xbox",
        GlyphStyle::PlayStation => "horizon.options.glyphs.playstation",
        GlyphStyle::Nintendo => "horizon.options.glyphs.nintendo",
    })
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
            Item::Option(Opt::PadCraftOnServers) => Some(c.craft_on_servers),
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

/// The title under the mark.
const TITLE: &str = "Controller";
/// A Reset All row's name, vanilla's.
const RESET_ALL: &str = "controls.resetAll";

impl Context {
    /// A line to read: its name (a language key) and what it says now.
    fn read(&self, which: Info) -> (&'static str, String) {
        match which {
            Info::Player => ("Player profile", self.player.clone()),
            Info::Device => ("Controller", self.device_name()),
            Info::Ids => (
                "Vendor and product",
                match self.device.as_ref().map(|d| (d.vendor, d.product)) {
                    Some((Some(vendor), Some(product))) => format!("{vendor:04x}:{product:04x}"),
                    _ => lang::get("Unknown"),
                },
            ),
            Info::Detected => ("Detected glyphs", glyphs_name(self.detected)),
            Info::Using => ("Glyphs in use", glyphs_name(self.glyphs.family())),
        }
    }
    fn device_name(&self) -> String {
        self.device
            .as_ref()
            .map_or_else(|| lang::get("No controller"), |d| d.name.clone())
    }
}

/// An action's binding: its button in the player's glyphs, what else that button does, and a
/// reset while it isn't the default.
fn binding(action: &'static str, pad: &PadSettings, glyphs: Glyphs) -> Binding {
    let shared = match pad.binding(action) {
        Some(button) if pad.conflicts(action) => pad
            .actions_of(button)
            .into_iter()
            .filter(|other| *other != action)
            .map(|other| lang::get(gamepad::action_caption(other)))
            .collect(),
        _ => Vec::new(),
    };
    Binding {
        capture: Capture::Pad(action),
        shown: Shown::Pad {
            glyph: Glyph::of(GlyphOf::Bound(action), pad, glyphs),
            cancel: Glyph::menu(MenuJob::Start, glyphs),
        },
        shared,
        reset: if pad.is_default(action) {
            Reset::Default
        } else {
            Reset::Changed
        },
    }
}

/// A setting's row, as these settings and the game's context read now.
fn row(setting: Setting, s: &Settings, context: &Context) -> Row<Item> {
    let item = setting.item;
    let about = lang::get(setting.text);
    match item {
        Item::Option(o) => {
            let (name, value) = shell::name_and_value(&o.label(s).plain());
            let text = value.unwrap_or_default();
            let value = match (item.switch(s), item.slider(s)) {
                (Some(on), _) => Value::Switch(on),
                (None, Some(position)) => Value::Slider {
                    position,
                    steps: o.slider_steps(),
                    text,
                },
                (None, None) => Value::Choice(text),
            };
            Row {
                chosen: choice_description(o, &s.controller).map(lang::get),
                ..Row::new(item, name, value).about(about)
            }
        }
        Item::Bind(action) => {
            let name = lang::get(gamepad::action_caption(action));
            let value = Value::Binding(binding(action, &s.controller, context.glyphs));
            Row::new(item, name, value).about(about)
        }
        Item::ResetBinds => Row::new(item, lang::get(RESET_ALL), Value::Press).about(about),
        Item::Info(which) => {
            let (name, value) = context.read(which);
            Row::new(item, lang::get(name), Value::Read(value)).about(about)
        }
    }
}

/// The controller settings screen: what it shows besides the settings is the game's.
#[derive(Default)]
pub(crate) struct ControllerScreen {
    /// Who and what the screen is about, which the game sets as it draws.
    pub(crate) context: Context,
    /// The window's size in pixels, which `Opt::cycle` takes: the options screen sets it before
    /// each input.
    pub(crate) framebuffer: [i32; 2],
}

impl Screen for ControllerScreen {
    type Page = Page;
    type Row = Item;
    fn title(&self) -> &'static str {
        TITLE
    }
    /// The rows read the controller settings and the game's context.
    fn stamp(&self, s: &Settings) -> u64 {
        shell::stamp_of(&(&s.controller, &self.context))
    }
    fn sheets(&self, s: &Settings) -> Vec<Sheet<Page, Item>> {
        let sheet = |(page, groups): (Page, Vec<(&'static str, Vec<Setting>)>)| Sheet {
            id: page,
            name: lang::get(page.name()),
            about: lang::get(page.about()),
            groups: groups
                .into_iter()
                .map(|(name, settings)| Group {
                    name,
                    rows: settings
                        .into_iter()
                        .map(|setting| row(setting, s, &self.context))
                        .collect(),
                })
                .collect(),
            listed: true,
            heading: None,
        };
        rows().into_iter().map(sheet).collect()
    }
    /// A tick box or a value steps; Reset All puts every action back. The shell starts a
    /// binding's wait and slides a slider; a line to read does nothing.
    fn press(&mut self, s: &mut Settings, item: Item, back: bool) -> Vec<Output> {
        match item {
            Item::Option(o) => {
                o.cycle(s, back, self.framebuffer);
                vec![Output::Event(Event::Changed)]
            }
            Item::ResetBinds => {
                s.controller.reset_bindings();
                vec![Output::Event(Event::Changed)]
            }
            Item::Bind(_) | Item::Info(_) => Vec::new(),
        }
    }
    fn slide(&mut self, s: &mut Settings, item: Item, position: f32) {
        if let Item::Option(o) = item {
            o.set_slider(s, position);
        }
    }
    /// With the "Scroll wheel changes settings" extra, the wheel steps a value between `<` `>`;
    /// the other rows leave it to scroll the list.
    fn wheel_step(&mut self, s: &mut Settings, item: Item, back: bool) -> Option<Vec<Output>> {
        let Item::Option(o) = item else {
            return None;
        };
        item.steps(s).then(|| {
            o.cycle(s, back, self.framebuffer);
            vec![
                Output::Event(Event::ButtonClick),
                Output::Event(Event::Changed),
            ]
        })
    }
    fn reset(&mut self, s: &mut Settings, item: Item) -> Vec<Output> {
        let Item::Bind(action) = item else {
            return Vec::new();
        };
        s.controller.bind(action, PadSettings::default_of(action));
        vec![Output::Event(Event::Changed)]
    }
    /// Whose profile these settings are, and the controller the game sees.
    fn side(&self) -> Vec<String> {
        let player = lang::tr("Editing: %s", &[&self.context.player]);
        vec![player, self.context.device_name()]
    }
    fn undoes(&self) -> bool {
        true
    }
    /// This player's controller settings, bindings included, as the screen found them.
    fn undo(&mut self, s: &mut Settings, before: &Settings) -> Vec<Output> {
        s.controller = before.controller.clone();
        Vec::new()
    }
    /// Undo is there exactly while the controller settings differ from what the screen opened
    /// with: a binding the game made while the row waited counts, a value stepped back doesn't.
    fn differs(&self, s: &Settings, before: &Settings) -> Option<bool> {
        Some(s.controller != before.controller)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        gamepad::Button,
        hud::{HudAtlas, HudBatch},
        settings_shell::{Columns, Hosted, Shell, Target},
    };
    use winit::keyboard::KeyCode as K;

    /// A 1708x960 window, and its GUI at the scale the game picks for it.
    const WINDOW: [u32; 2] = [1708, 960];
    const FB: [i32; 2] = [1708, 960];
    const SIZE: (f32, f32) = (427.0, 240.0);
    const CLICK: Output = Output::Event(Event::ButtonClick);
    const CHANGED: Output = Output::Event(Event::Changed);
    const JUMP: &str = "key.jump";

    type Ui = Shell<ControllerScreen>;
    fn ui(page: Page) -> Ui {
        let mut ui = Ui::default();
        ui.screen.framebuffer = FB;
        ui.show(page);
        ui
    }
    fn key(ui: &mut Ui, s: &mut Settings, key: K) -> Vec<Output> {
        ui.key(s, key, None, false, SIZE)
    }
    fn atlas() -> HudAtlas {
        HudAtlas::build(std::path::Path::new("/nonexistent/horizon-test-assets")).unwrap()
    }
    /// Draw a frame, as the options screen does with what it waits on, and the frame's widgets.
    fn draw(ui: &mut Ui, s: &Settings, waiting: Option<Capture>) -> HudBatch {
        let atlas = atlas();
        let mut batch = HudBatch::new(WINDOW);
        assert_eq!((batch.width, batch.height), SIZE);
        ui.draw(s, &mut batch, &atlas, (0.0, 0.0), false, waiting);
        crate::widgets::end_frame(crate::widgets::generation() + 1);
        batch
    }
    /// What the strip says now.
    fn strip(ui: &mut Ui, s: &Settings, waiting: Option<Capture>, over: Item) -> Option<String> {
        let atlas = atlas();
        let mut batch = HudBatch::new(WINDOW);
        assert_eq!((batch.width, batch.height), SIZE);
        let at = ui.point_at(s, SIZE, over);
        ui.draw(s, &mut batch, &atlas, at, false, waiting);
        crate::widgets::end_frame(crate::widgets::generation() + 1);
        let dock = Columns::of(SIZE.0, SIZE.1).dock;
        let labelled = crate::widgets::last_labelled();
        let found = labelled.iter().find(|w| [w.x, w.y, w.w, w.h] == dock);
        found.map(|w| w.label.clone())
    }
    fn label_of(ui: &mut Ui, s: &Settings, item: Item) -> String {
        let listed = ui.listed(s, SIZE);
        let at = listed.iter().position(|(_, i)| *i == item).unwrap();
        ui.row_labels(s)[at].clone()
    }

    /// Tab reaches every row of every page from nothing but the keyboard, as a controller's
    /// focus reaches the same targets.
    #[test]
    fn tab_reaches_every_setting_on_every_page() {
        let mut s = Settings::default();
        for (page, groups) in rows() {
            let mut ui = ui(page);
            let want: Vec<Item> = groups
                .iter()
                .flat_map(|(_, rows)| rows.iter().map(|r| r.item))
                .collect();
            let mut seen = Vec::new();
            for _ in 0..want.len() + Page::ALL.len() + 3 {
                key(&mut ui, &mut s, K::Tab);
                if let Some(item) = ui.focused_row() {
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
            Opt::PadGameplayGuide,
            Opt::PadCraftOnServers,
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
            Opt::PadNintendoButtons,
            Opt::PadKeyboard,
            Opt::PadPointerSpeed,
            Opt::PadScrollSpeed,
            Opt::PadRepeatDelay,
            Opt::PadRepeatInterval,
        ] {
            assert!(items.contains(&Item::Option(opt)), "{opt:?}");
        }
    }

    /// Undo comes with a change, the game's binding of a button too, and goes when a value is
    /// stepped back; pressed, it puts back the values and the bindings the screen opened with.
    #[test]
    fn undo_puts_back_the_values_and_bindings_the_screen_opened_with() {
        let mut s = Settings::default();
        let mut ui = ui(Page::Device);
        draw(&mut ui, &s, None);
        assert!(!ui.changed());
        let vibration = Item::Option(Opt::PadVibration);
        let at = ui.point_at(&s, SIZE, vibration);
        ui.click(&mut s, at, SIZE, false, false, false);
        assert!(!s.controller.vibration && ui.changed());
        // Ticked back, nothing differs: no Undo.
        ui.click(&mut s, at, SIZE, false, false, false);
        draw(&mut ui, &s, None);
        assert!(s.controller.vibration && !ui.changed());
        // The game binds the button pressed while a row waits; the screen sees the change.
        s.controller.bind(JUMP, Some(Button::North));
        s.controller.invert_y = !s.controller.invert_y;
        draw(&mut ui, &s, None);
        assert!(ui.changed());
        let reached = (0..100).any(|_| {
            key(&mut ui, &mut s, K::Tab);
            ui.focused() == Some(Target::Undo)
        });
        assert!(reached);
        assert_eq!(key(&mut ui, &mut s, K::Enter), [CLICK, CHANGED]);
        assert_eq!(s.controller, PadSettings::default());
        draw(&mut ui, &s, None);
        assert!(!ui.changed());
    }

    /// A press makes the host wait for a button; meanwhile the row is marked and the strip names
    /// the button that cancels. Cancelled, it reads as before and nothing changed.
    #[test]
    fn a_binding_row_waits_for_a_button_and_cancelling_changes_nothing() {
        let mut s = Settings::default();
        let mut ui = ui(Page::Bindings);
        let jump = Item::Bind(JUMP);
        let at = ui.point_at(&s, SIZE, jump);
        let outputs = ui.click(&mut s, at, SIZE, false, false, false);
        assert_eq!(outputs, [CLICK, Output::Bind(Capture::Pad(JUMP))]);
        // The row as drawn, by its caption in the widget tree.
        let name = lang::get(gamepad::action_caption(JUMP));
        let drawn = |ui: &mut Ui, s: &Settings, waiting| {
            draw(ui, s, waiting);
            let labelled = crate::widgets::last_labelled();
            let mut rows = labelled.into_iter().map(|w| w.label);
            rows.find(|label| label.starts_with(&name)).unwrap()
        };
        let waiting = Some(Capture::Pad(JUMP));
        let shown = drawn(&mut ui, &s, waiting);
        assert!(shown.ends_with("> A <"), "{shown}");
        let start = Glyph::menu(MenuJob::Start, Glyphs::default()).label();
        let said = strip(&mut ui, &s, waiting, jump).unwrap();
        assert!(said.contains(start) && said.contains("cancels"), "{said}");
        // Another row's wait doesn't mark this one.
        let other = Some(Capture::Pad("key.sneak"));
        let unmarked = drawn(&mut ui, &s, other);
        assert!(
            !unmarked.contains('>') && unmarked.ends_with('A'),
            "{unmarked}"
        );
        assert_eq!(drawn(&mut ui, &s, None), shown.replace("> A <", "A"));
        assert_eq!(s.controller, PadSettings::default());
        assert!(!ui.changed());
    }

    /// The strip says when an action has no button, or what else its button does.
    #[test]
    fn the_strip_says_a_binding_is_unbound_or_shares_its_button() {
        let mut s = Settings::default();
        let mut ui = ui(Page::Bindings);
        let jump = Item::Bind(JUMP);
        s.controller.bind(JUMP, None);
        assert!(label_of(&mut ui, &s, jump).ends_with(&lang::get("key.keyboard.unknown")));
        let said = strip(&mut ui, &s, None, jump).unwrap();
        assert!(said.starts_with("Not bound"), "{said}");
        let sneak = PadSettings::default().binding("key.sneak").unwrap();
        s.controller.bind(JUMP, Some(sneak));
        let said = strip(&mut ui, &s, None, jump).unwrap();
        let sneaks = lang::get(gamepad::action_caption("key.sneak"));
        assert_eq!(said, lang::tr("This button also does: %s", &[&sneaks]));
        assert!(label_of(&mut ui, &s, jump).contains("[ "));
    }

    /// A bound action shows its button's art; an unbound one, none.
    #[test]
    fn a_bound_action_draws_its_buttons_art_and_an_unbound_one_none() {
        // The Xbox South button's art, a 16 pixel square (`pad_glyph`'s own test).
        const ART: f32 = 16.0;
        let arts = |batch: &HudBatch, [x, y, w, h]: [f32; 4]| {
            let quads = batch.vertices.as_chunks::<6>().0;
            quads
                .iter()
                .filter(|quad| {
                    let xs = quad.iter().map(|v| v.pos[0]);
                    let ys = quad.iter().map(|v| v.pos[1]);
                    let (left, right) = (
                        xs.clone().fold(f32::MAX, f32::min),
                        xs.fold(f32::MIN, f32::max),
                    );
                    let (top, bottom) = (
                        ys.clone().fold(f32::MAX, f32::min),
                        ys.fold(f32::MIN, f32::max),
                    );
                    let inside =
                        left >= x && right <= x + w && top >= y - 2.0 && bottom <= y + h + 2.0;
                    inside && right - left == ART && bottom - top == ART
                })
                .count()
        };
        let mut s = Settings::default();
        let mut ui = ui(Page::Bindings);
        let (_, y) = ui.point_at(&s, SIZE, Item::Bind(JUMP));
        let row = Columns::of(SIZE.0, SIZE.1).row(y - crate::graphics_style::ROW / 2.0);
        s.controller.glyphs = GlyphStyle::Xbox;
        ui.screen.context.glyphs = Glyphs::of(&s.controller, GlyphStyle::Auto);
        let batch = draw(&mut ui, &s, None);
        assert_eq!(arts(&batch, row), 1);
        s.controller.bind(JUMP, None);
        let batch = draw(&mut ui, &s, None);
        assert_eq!(arts(&batch, row), 0);
    }

    #[test]
    fn the_search_finds_rows_across_pages_and_drops_empty_pages() {
        let s = Settings::default();
        let mut ui = ui(Page::General);
        ui.set_search("rumble");
        let pages = |ui: &mut Ui| {
            let listed = ui.listed(&s, SIZE);
            listed.into_iter().map(|(page, _)| page).collect::<Vec<_>>()
        };
        let found = pages(&mut ui);
        assert!(!found.is_empty() && found.iter().all(|p| *p == Page::Device));
        // By what it does, not only its name.
        ui.set_search("drift");
        assert!(pages(&mut ui).contains(&Page::General));
        ui.set_search("no setting says this");
        assert!(pages(&mut ui).is_empty());
    }

    #[test]
    fn a_changed_binding_gains_a_reset_that_the_arrows_reach_and_it_goes_away() {
        let mut s = Settings::default();
        s.controller.bind(JUMP, Some(Button::North));
        let mut ui = ui(Page::Bindings);
        // Search, the three pages, then the first action.
        for _ in 0..Page::ALL.len() + 2 {
            key(&mut ui, &mut s, K::Tab);
        }
        assert_eq!(ui.focused(), Some(Target::Row(Item::Bind(JUMP))));
        key(&mut ui, &mut s, K::ArrowRight);
        assert_eq!(ui.focused(), Some(Target::Reset(Item::Bind(JUMP))));
        assert_eq!(key(&mut ui, &mut s, K::Enter), [CLICK, CHANGED]);
        assert!(s.controller.is_default(JUMP));
        assert_eq!(ui.focused(), Some(Target::Row(Item::Bind(JUMP))));
        // A default binding has nothing to the right of it.
        key(&mut ui, &mut s, K::ArrowRight);
        assert_eq!(ui.focused(), Some(Target::Row(Item::Bind(JUMP))));
    }

    /// The reset takes a click only while it would do something: on a default binding, the
    /// click there presses the row, which waits for a button.
    #[test]
    fn a_reset_refuses_a_default_binding_and_resets_a_changed_one() {
        let mut s = Settings::default();
        let mut ui = ui(Page::Bindings);
        let (_, y) = ui.point_at(&s, SIZE, Item::Bind(JUMP));
        let cols = Columns::of(SIZE.0, SIZE.1);
        let on_reset = (cols.list_x + cols.list_w - 10.0, y);
        let outputs = ui.click(&mut s, on_reset, SIZE, false, false, false);
        assert_eq!(outputs, [CLICK, Output::Bind(Capture::Pad(JUMP))]);
        s.controller.bind(JUMP, Some(Button::North));
        let outputs = ui.click(&mut s, on_reset, SIZE, false, false, false);
        assert_eq!(outputs, [CLICK, CHANGED]);
        assert!(s.controller.is_default(JUMP));
    }

    #[test]
    fn reading_rows_change_nothing_and_a_slider_steps_after_enter() {
        let mut s = Settings::default();
        let mut device = ui(Page::Device);
        let before = s.controller.clone();
        let at = device.point_at(&s, SIZE, Item::Info(Info::Device));
        assert!(
            device
                .click(&mut s, at, SIZE, false, false, false)
                .is_empty()
        );
        assert_eq!(s.controller, before);
        let mut general = ui(Page::General);
        general.set_focus(Some(Target::Row(Item::Option(Opt::PadMoveDeadzone))));
        let deadzone = s.controller.move_deadzone;
        key(&mut general, &mut s, K::Enter);
        key(&mut general, &mut s, K::ArrowRight);
        assert!((s.controller.move_deadzone - deadzone - 0.01).abs() < 0.001);
    }

    /// Left and Right step a choice both ways, as `<` `>` show (#12332).
    #[test]
    fn left_and_right_step_a_choice_both_ways() {
        let mut s = Settings::default();
        let mut ui = ui(Page::General);
        let guide = Item::Option(Opt::PadGameplayGuide);
        ui.set_focus(Some(Target::Row(guide)));
        let first = s.controller.guide_detail;
        key(&mut ui, &mut s, K::ArrowRight);
        assert_ne!(s.controller.guide_detail, first);
        key(&mut ui, &mut s, K::ArrowLeft);
        assert_eq!(s.controller.guide_detail, first);
        key(&mut ui, &mut s, K::ArrowLeft);
        assert_ne!(s.controller.guide_detail, first);
    }

    /// The Device page's Nintendo Buttons row sits with the glyphs, steps between its two
    /// values rather than ticking a box, and what it sets is what the controller file keeps.
    #[test]
    fn the_nintendo_buttons_row_flips_letters_and_the_file_keeps_it() {
        let (page, group) = rows()
            .into_iter()
            .flat_map(|(page, groups)| groups.into_iter().map(move |g| (page, g)))
            .find(|(_, (_, rows))| {
                rows.iter()
                    .any(|r| r.item == Item::Option(Opt::PadNintendoButtons))
            })
            .unwrap();
        assert_eq!((page, group.0), (Page::Device, "Glyphs"));
        let mut s = Settings::default();
        let item = Item::Option(Opt::PadNintendoButtons);
        assert!(item.steps(&s));
        s.controller.nintendo_letters = false;
        let mut ui = ui(Page::Device);
        ui.set_focus(Some(Target::Row(item)));
        // Vanilla's caption and value template; Horizon's own words come bundled.
        struct Vanilla;
        impl lang::ResourceSource for Vanilla {
            fn read(&self, path: &str) -> Option<Vec<u8>> {
                (path == "assets/minecraft/lang/en_us.json")
                    .then(|| br#"{"options.generic_value": "%s: %s"}"#.to_vec())
            }
        }
        let value = |ui: &mut Ui, s: &Settings| {
            lang::scoped(lang::Language::load(&[&Vanilla], "en_us"), || {
                label_of(ui, s, item)
            })
            .0
        };
        assert_eq!(value(&mut ui, &s), "Nintendo Buttons: By Position");
        let outputs = key(&mut ui, &mut s, K::Enter);
        assert!(outputs.contains(&CHANGED), "{outputs:?}");
        assert!(s.controller.nintendo_letters);
        assert_eq!(value(&mut ui, &s), "Nintendo Buttons: By Letter");
        assert!(PadSettings::parse(&s.controller.serialize()).nintendo_letters);
        key(&mut ui, &mut s, K::Enter);
        assert!(!s.controller.nintendo_letters);
        assert!(!PadSettings::parse(&s.controller.serialize()).nintendo_letters);
    }

    /// What the strip says about each of these settings changes with the choice.
    #[test]
    fn each_choice_describes_itself() {
        let mut s = Settings::default();
        let context = Context::default();
        for (opt, choices) in [
            (Opt::PadGameplayGuide, 3),
            (Opt::PadGlyphs, 4),
            (Opt::PadNintendoButtons, 2),
            (Opt::PadKeyboard, 3),
            (Opt::PadLightbar, 3),
        ] {
            let setting = option(opt, "the fixed sentence");
            let mut seen = Vec::new();
            for _ in 0..choices {
                let text = row(setting, &s, &context).chosen.unwrap();
                assert_ne!(text, "the fixed sentence", "{opt:?}");
                assert!(!seen.contains(&text), "{opt:?} repeats {text}");
                seen.push(text);
                opt.cycle(&mut s, false, FB);
            }
        }
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
            let mut settings = Settings::default();
            for opt in [
                Opt::PadGameplayGuide,
                Opt::PadGlyphs,
                Opt::PadNintendoButtons,
                Opt::PadKeyboard,
                Opt::PadLightbar,
            ] {
                for _ in 0..4 {
                    lang::get(choice_description(opt, &settings.controller).unwrap());
                    opt.cycle(&mut settings, false, FB);
                }
            }
            let context = Context::default();
            for which in [
                Info::Player,
                Info::Device,
                Info::Ids,
                Info::Detected,
                Info::Using,
            ] {
                lang::get(context.read(which).0);
            }
            ControllerScreen::default().side();
            lang::get(TITLE);
            for text in [
                "Press the button for this. %s cancels.",
                "This button also does: %s",
                "Not bound: press this row, then the button that should do it.",
            ] {
                lang::get(text);
            }
        });
        assert_eq!(missed, Vec::<String>::new());
    }
}
