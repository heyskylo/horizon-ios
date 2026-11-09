//! Controllers: Horizon's own feature (vanilla 26.3 has no gamepad input).
//! Its default buttons are Horizon's own; analog walking and toggle sneak follow Bedrock Edition on
//! consoles. See docs/controllers.md.
//!
//! This module is the backend-free part: positional buttons, the action
//! bindings, deadzones and the look curve, button edges from successive pad
//! states, and `horizon-controller.txt`, kept next to `options.txt` so that
//! file stays exactly vanilla's. The game reads the pads with gilrs.
use std::{
    collections::BTreeMap,
    fs,
    path::Path,
    time::{Duration, Instant},
};

pub const FILE: &str = "horizon-controller.txt";

/// A button by its position on the pad (gilrs' and SDL's layout): `South`
/// is Xbox A, PlayStation ✕, Nintendo B.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Button {
    South,
    East,
    West,
    North,
    LeftBumper,
    RightBumper,
    LeftTrigger,
    RightTrigger,
    Select,
    Start,
    LeftStick,
    RightStick,
    DPadUp,
    DPadDown,
    DPadLeft,
    DPadRight,
}
impl Button {
    pub const ALL: [Button; 16] = [
        Button::South,
        Button::East,
        Button::West,
        Button::North,
        Button::LeftBumper,
        Button::RightBumper,
        Button::LeftTrigger,
        Button::RightTrigger,
        Button::Select,
        Button::Start,
        Button::LeftStick,
        Button::RightStick,
        Button::DPadUp,
        Button::DPadDown,
        Button::DPadLeft,
        Button::DPadRight,
    ];
    /// The name in `horizon-controller.txt` and `horizon-ctl pad`.
    pub fn name(self) -> &'static str {
        match self {
            Button::South => "south",
            Button::East => "east",
            Button::West => "west",
            Button::North => "north",
            Button::LeftBumper => "lb",
            Button::RightBumper => "rb",
            Button::LeftTrigger => "lt",
            Button::RightTrigger => "rt",
            Button::Select => "select",
            Button::Start => "start",
            Button::LeftStick => "ls",
            Button::RightStick => "rs",
            Button::DPadUp => "up",
            Button::DPadDown => "down",
            Button::DPadLeft => "left",
            Button::DPadRight => "right",
        }
    }
    pub fn parse(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|b| b.name() == name)
    }
    fn bit(self) -> u32 {
        1 << self as u32
    }
    /// The face button that plays this one's part on a Nintendo pad going by its letters
    /// (`PadSettings::nintendo_letters`): the A it prints on the right is South, which confirms
    /// and jumps as A does on a Switch; B (bottom) is East, X (top) West, Y (left) North. It is
    /// its own inverse, so it turns a part into the button playing it and back.
    pub fn by_letter(self) -> Button {
        match self {
            Button::South => Button::East,
            Button::East => Button::South,
            Button::West => Button::North,
            Button::North => Button::West,
            other => other,
        }
    }
}

/// A pad's inputs at one moment. Sticks are `[x, y]` with right and up
/// positive; triggers run 0..1.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PadState {
    /// Digital buttons as a bit set (triggers are in `triggers`).
    pub buttons: u32,
    pub left: [f32; 2],
    pub right: [f32; 2],
    pub triggers: [f32; 2],
}
impl PadState {
    pub fn set(&mut self, button: Button, down: bool) {
        if down {
            self.buttons |= button.bit();
        } else {
            self.buttons &= !button.bit();
        }
    }
    pub fn held(&self, button: Button) -> bool {
        self.buttons & button.bit() != 0
    }
    /// A Nintendo pad's buttons read by their letters (`Button::by_letter`).
    pub fn by_letter(&self) -> PadState {
        let mut state = *self;
        for part in [Button::South, Button::East, Button::West, Button::North] {
            state.set(part, self.held(part.by_letter()));
        }
        state
    }
    /// Two sources at once (a real pad and `horizon-ctl pad`): buttons held on
    /// either, sticks and triggers added.
    pub fn merge(&self, other: &PadState) -> PadState {
        let add = |a: [f32; 2], b: [f32; 2]| {
            [
                (a[0] + b[0]).clamp(-1.0, 1.0),
                (a[1] + b[1]).clamp(-1.0, 1.0),
            ]
        };
        PadState {
            buttons: self.buttons | other.buttons,
            left: add(self.left, other.left),
            right: add(self.right, other.right),
            triggers: [
                (self.triggers[0] + other.triggers[0]).clamp(0.0, 1.0),
                (self.triggers[1] + other.triggers[1]).clamp(0.0, 1.0),
            ],
        }
    }
    /// Anything pushed past the deadzones (the pad is in use).
    pub fn active(&self, settings: &PadSettings) -> bool {
        self.buttons != 0
            || length(self.left) > settings.move_deadzone
            || length(self.right) > settings.look_deadzone
            || self
                .triggers
                .iter()
                .any(|t| *t >= settings.trigger_threshold)
    }
}

/// Button presses and releases between successive states. Triggers count as
/// buttons past `threshold`, with a little hysteresis so a trigger resting
/// near the threshold doesn't chatter.
#[derive(Clone, Debug, Default)]
pub struct Edges {
    held: u32,
    suppressed: u32,
}
impl Edges {
    pub fn update(&mut self, state: &PadState, threshold: f32) -> Vec<(Button, bool)> {
        let mut now = state.buttons;
        for (trigger, button) in [(0, Button::LeftTrigger), (1, Button::RightTrigger)] {
            let was = (self.held | self.suppressed) & button.bit() != 0;
            let value = state.triggers[trigger];
            let on = if was {
                value >= (threshold - 0.1).max(0.05)
            } else {
                value >= threshold
            };
            if on {
                now |= button.bit();
            } else {
                now &= !button.bit();
            }
        }
        self.suppressed &= now;
        now &= !self.suppressed;
        let changed = now ^ self.held;
        self.held = now;
        Button::ALL
            .into_iter()
            .filter(|b| changed & b.bit() != 0)
            .map(|b| (b, now & b.bit() != 0))
            .collect()
    }
    /// Release everything (the pad was unplugged or turned off).
    pub fn release_all(&mut self) -> Vec<(Button, bool)> {
        let held = std::mem::take(&mut self.held);
        Button::ALL
            .into_iter()
            .filter(|b| held & b.bit() != 0)
            .map(|b| (b, false))
            .collect()
    }
    pub fn is_held(&self, button: Button) -> bool {
        self.held & button.bit() != 0
    }
    /// Cancel a context without turning its still-held buttons into new presses.
    pub fn cancel(&mut self) -> Vec<(Button, bool)> {
        self.suppressed |= self.held;
        self.release_all()
    }
}

/// Intentional activity may take input ownership; an unchanged hold may not.
#[derive(Default)]
pub struct Activity {
    buttons: u32,
    sticks: [[f32; 2]; 2],
}
impl Activity {
    pub fn observe(&mut self, state: &PadState, settings: &PadSettings) -> bool {
        let mut buttons = state.buttons;
        for (i, button) in [(0, Button::LeftTrigger), (1, Button::RightTrigger)] {
            let threshold = if self.buttons & button.bit() != 0 {
                (settings.trigger_threshold - 0.1).max(0.05)
            } else {
                settings.trigger_threshold
            };
            if state.triggers[i] >= threshold {
                buttons |= button.bit();
            }
        }
        let mut activity = buttons & !self.buttons != 0;
        self.buttons = buttons;
        for (i, (stick, deadzone)) in [
            (state.left, settings.move_deadzone),
            (state.right, settings.look_deadzone),
        ]
        .into_iter()
        .enumerate()
        {
            if length(stick) <= deadzone {
                self.sticks[i] = [0.0; 2];
            } else if length([stick[0] - self.sticks[i][0], stick[1] - self.sticks[i][1]]) > 0.08 {
                activity = true;
                self.sticks[i] = stick;
            }
        }
        activity
    }
}

/// Horizon's pad-only actions (no vanilla key mapping).
pub const HOTBAR_PREVIOUS: &str = "pad.hotbarPrevious";
pub const HOTBAR_NEXT: &str = "pad.hotbarNext";
/// Held, opens the radial menu (`radial.rs`).
pub const RADIAL: &str = "pad.radial";

mod radial_file;
pub use radial_file::{RadialMode, RadialStick, opener_refused};

/// The `bindLayout` the settings file is written in. 1: the defaults of the binds list as they
/// are; 2: the radial menu's opener resolved against the player's own buttons.
const BIND_LAYOUT: u32 = 2;
/// The layout from which the file, not the default, says where the opener is.
const OPENER_LAYOUT: u32 = 2;

/// Bindable actions and their default buttons: vanilla key mappings (the
/// game acts as their default key) and Horizon's pad-only actions. Start is
/// always pause / back and isn't listed.
pub const ACTIONS: &[(&str, Option<Button>)] = &[
    ("key.jump", Some(Button::South)),
    ("key.drop", Some(Button::DPadDown)),
    ("key.swapOffhand", Some(Button::West)),
    ("key.inventory", Some(Button::North)),
    (HOTBAR_PREVIOUS, Some(Button::LeftBumper)),
    (HOTBAR_NEXT, Some(Button::RightBumper)),
    ("key.use", Some(Button::LeftTrigger)),
    ("key.attack", Some(Button::RightTrigger)),
    ("key.sprint", Some(Button::LeftStick)),
    ("key.sneak", Some(Button::RightStick)),
    ("key.togglePerspective", Some(Button::Select)),
    ("key.pickItem", Some(Button::DPadLeft)),
    ("key.chat", Some(Button::DPadUp)),
    (RADIAL, Some(Button::DPadRight)),
    ("key.advancements", None),
    ("key.playerlist", None),
    ("key.command", None),
    ("key.screenshot", None),
    ("key.toggleGui", None),
    ("key.smoothCamera", None),
    // Horizon's emote wheel: no button is free by default; the stick's cursor picks.
    ("key.emote", None),
    ("key.fullscreen", None),
    ("key.socialInteractions", None),
    ("key.quickActions", None),
    ("key.saveToolbarActivator", None),
    ("key.loadToolbarActivator", None),
    ("key.hotbar.1", None),
    ("key.hotbar.2", None),
    ("key.hotbar.3", None),
    ("key.hotbar.4", None),
    ("key.hotbar.5", None),
    ("key.hotbar.6", None),
    ("key.hotbar.7", None),
    ("key.hotbar.8", None),
    ("key.hotbar.9", None),
];

/// How a bindable action reaches the game: one of its own handlers, shared
/// with the keyboard where the keyboard has one, or else the vanilla default
/// key or button of its mapping, as a rebound keyboard key would.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Route {
    HotbarStep {
        next: bool,
    },
    /// Held, or flipped when the pad's toggle sneak is on.
    Sneak,
    Screenshot,
    SmoothCamera,
    Fullscreen,
    /// Does nothing by itself: what reads it asks whether the button is held
    /// (`Minecraft.handleKeybinds`' toolbar keys, held with a hotbar key).
    Held,
    Key(winit::keyboard::KeyCode),
    Mouse(winit::event::MouseButton),
}

/// The route of a bindable action; `None` for one the pad can't reach.
pub fn route(action: &str) -> Option<Route> {
    match action {
        HOTBAR_PREVIOUS => Some(Route::HotbarStep { next: false }),
        HOTBAR_NEXT => Some(Route::HotbarStep { next: true }),
        // Nothing in play reads the opener yet.
        RADIAL => Some(Route::Held),
        "key.sneak" => Some(Route::Sneak),
        "key.screenshot" => Some(Route::Screenshot),
        "key.smoothCamera" => Some(Route::SmoothCamera),
        "key.fullscreen" => Some(Route::Fullscreen),
        "key.saveToolbarActivator" | "key.loadToolbarActivator" => Some(Route::Held),
        _ => {
            let default = crate::keybinds::KeyBinds::default_of(action);
            crate::keybinds::mouse_button(default)
                .map(Route::Mouse)
                .or_else(|| crate::keybinds::key_code(default).map(Route::Key))
        }
    }
}

/// The radial menu's opener, as the binds list and the gameplay guide name it.
pub const RADIAL_CAPTION: &str = "horizon.pad.action.radial";

/// An action's name in the binds list: vanilla's translation key, or
/// Horizon's own English for the pad-only actions.
pub fn action_caption(action: &str) -> &str {
    match action {
        HOTBAR_PREVIOUS => "Hotbar: Previous Slot",
        HOTBAR_NEXT => "Hotbar: Next Slot",
        RADIAL => RADIAL_CAPTION,
        other => other,
    }
}

/// How long a held drop button throws single items before it throws the rest of the stack.
pub const DROP_STACK_HOLD: Duration = Duration::from_secs(1);

/// What a held drop button does now, past its press.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DropStep {
    /// Held key.drop (a repeat reaches `KeyMapping.click` as a press).
    One,
    /// Control + key.drop: a pad has no Control to hold, so holding on is how it asks.
    Stack,
}

/// A held drop button's repeats: a tap throws one item, as Q does; held, it repeats at the pad's
/// repeat delay and interval; and held for [`DROP_STACK_HOLD`] it throws what's left.
#[derive(Clone, Copy, Debug)]
pub struct HeldDrop {
    since: Instant,
    next: Instant,
}
impl HeldDrop {
    pub fn new(now: Instant, delay: Duration) -> Self {
        Self {
            since: now,
            next: now + delay,
        }
    }
    /// The step due at `now`, if any; at most one per call.
    pub fn due(&mut self, now: Instant, interval: Duration) -> Option<DropStep> {
        if now < self.next {
            return None;
        }
        self.next = now + interval;
        Some(if now.duration_since(self.since) >= DROP_STACK_HOLD {
            DropStep::Stack
        } else {
            DropStep::One
        })
    }
}

/// A job a menu's fixed buttons do. Menus aren't rebindable: this is the one map from a job to
/// its button, and a glyph for a menu asks for the job (`GlyphOf::Menu`), never the button.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MenuJob {
    /// A: press, take, type.
    Confirm,
    /// B: back, close.
    Back,
    /// X: the screen's second job (Apply, Options, half a stack, delete a letter).
    Alternate,
    /// Y: the screen's third job (the exit, a quick move, a space).
    Extra,
    /// LB: the page or tab before.
    PagePrev,
    /// RB: the page or tab after.
    PageNext,
    /// LT: the group before; held, the modifier (shift, direct craft).
    GroupPrev,
    /// RT: the group after.
    GroupNext,
    /// View: throw.
    View,
    /// Menu (Start): done, and cancelling a binding.
    Start,
    /// The left stick pressed in.
    LeftStick,
}
impl MenuJob {
    pub const ALL: [MenuJob; 11] = [
        MenuJob::Confirm,
        MenuJob::Back,
        MenuJob::Alternate,
        MenuJob::Extra,
        MenuJob::PagePrev,
        MenuJob::PageNext,
        MenuJob::GroupPrev,
        MenuJob::GroupNext,
        MenuJob::View,
        MenuJob::Start,
        MenuJob::LeftStick,
    ];
    /// The part that does the job (by place; a pad going by its letters is read so the part
    /// still lands: `PadState::by_letter`).
    pub const fn button(self) -> Button {
        match self {
            MenuJob::Confirm => Button::South,
            MenuJob::Back => Button::East,
            MenuJob::Alternate => Button::West,
            MenuJob::Extra => Button::North,
            MenuJob::PagePrev => Button::LeftBumper,
            MenuJob::PageNext => Button::RightBumper,
            MenuJob::GroupPrev => Button::LeftTrigger,
            MenuJob::GroupNext => Button::RightTrigger,
            MenuJob::View => Button::Select,
            MenuJob::Start => Button::Start,
            MenuJob::LeftStick => Button::LeftStick,
        }
    }
    /// The job a part does in menus.
    pub fn of(button: Button) -> Option<MenuJob> {
        Self::ALL.into_iter().find(|job| job.button() == button)
    }
}

/// What a glyph stands for: what the button does, never the button. `pad_glyph::Glyph` is built
/// from one, so a rebinding or a pad's letters reach every glyph.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GlyphOf {
    /// A gameplay action (`key.jump`...), on the player's binding.
    Bound(&'static str),
    /// A menu's fixed job.
    Menu(MenuJob),
}
impl From<MenuJob> for GlyphOf {
    fn from(job: MenuJob) -> Self {
        GlyphOf::Menu(job)
    }
}
impl GlyphOf {
    /// The part that does it under these settings; none while the action is unbound.
    pub fn button(self, settings: &PadSettings) -> Option<Button> {
        match self {
            GlyphOf::Bound(action) => {
                // A misspelt action would quietly have no glyph.
                debug_assert!(
                    ACTIONS.iter().any(|a| a.0 == action),
                    "no pad action {action}"
                );
                settings.binding(action)
            }
            GlyphOf::Menu(job) => Some(job.button()),
        }
    }
}

/// Which button pictures prompts use.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum GlyphStyle {
    /// From the pad's maker.
    #[default]
    Auto,
    Xbox,
    PlayStation,
    Nintendo,
}
impl GlyphStyle {
    pub fn name(self) -> &'static str {
        match self {
            GlyphStyle::Auto => "auto",
            GlyphStyle::Xbox => "xbox",
            GlyphStyle::PlayStation => "playstation",
            GlyphStyle::Nintendo => "nintendo",
        }
    }
    pub fn parse(name: &str) -> Self {
        match name {
            "xbox" => GlyphStyle::Xbox,
            "playstation" => GlyphStyle::PlayStation,
            "nintendo" => GlyphStyle::Nintendo,
            _ => GlyphStyle::Auto,
        }
    }
    /// The style the player chose, else the connected pad's.
    pub fn resolve(self, detected: GlyphStyle) -> GlyphStyle {
        match self {
            GlyphStyle::Auto => match detected {
                GlyphStyle::Auto => GlyphStyle::Xbox,
                d => d,
            },
            chosen => chosen,
        }
    }
    pub const ALL: [GlyphStyle; 4] = [
        GlyphStyle::Auto,
        GlyphStyle::Xbox,
        GlyphStyle::PlayStation,
        GlyphStyle::Nintendo,
    ];
    /// The style for a pad by its USB ids: Sony's and Nintendo's own, and a few officially licensed
    /// third-party pads made for their consoles. Microsoft, Valve's Steam Deck and Steam Input's
    /// virtual pad, and everything else look like Xbox.
    pub fn of_device(vendor: Option<u16>, product: Option<u16>) -> Self {
        match (vendor, product) {
            (Some(0x054c), _) => GlyphStyle::PlayStation,
            (Some(0x057e), _) => GlyphStyle::Nintendo,
            // HORIPAD 4 FPS and FPS Plus; Razer Raiju; Nacon Revolution Pro.
            (Some(0x0f0d), Some(0x0055 | 0x0066))
            | (Some(0x1532), Some(0x1000))
            | (Some(0x146b), Some(0x0d01)) => GlyphStyle::PlayStation,
            // HORIPAD for Nintendo Switch; PowerA Wired Plus; PDP Faceoff Wired Pro.
            (Some(0x0f0d), Some(0x00c1))
            | (Some(0x20d6), Some(0xa711))
            | (Some(0x0e6f), Some(0x0180)) => GlyphStyle::Nintendo,
            _ => GlyphStyle::Xbox,
        }
    }
}

/// How much the gameplay guide says.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum GuideDetail {
    #[default]
    Full,
    /// Only what can be done to what's in front of the player or under the crosshair.
    Basic,
    Off,
}
impl GuideDetail {
    pub const ALL: [GuideDetail; 3] = [GuideDetail::Full, GuideDetail::Basic, GuideDetail::Off];

    /// The name in `horizon-controller.txt`.
    pub fn name(self) -> &'static str {
        match self {
            GuideDetail::Full => "full",
            GuideDetail::Basic => "basic",
            GuideDetail::Off => "off",
        }
    }

    /// Reads a name, or the `true` and `false` this option first saved. Anything else is full.
    pub fn parse(value: &str) -> Self {
        match value {
            "basic" => GuideDetail::Basic,
            "off" | "false" => GuideDetail::Off,
            _ => GuideDetail::Full,
        }
    }
}

/// The on-screen keyboard A opens on a text field in focus mode.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum TextKeyboard {
    /// Steam's on a Steam Deck under Steam, else Horizon's.
    #[default]
    Auto,
    Horizon,
    Steam,
}
impl TextKeyboard {
    pub const ALL: [TextKeyboard; 3] = [
        TextKeyboard::Auto,
        TextKeyboard::Horizon,
        TextKeyboard::Steam,
    ];
    pub fn name(self) -> &'static str {
        match self {
            TextKeyboard::Auto => "auto",
            TextKeyboard::Horizon => "horizon",
            TextKeyboard::Steam => "steam",
        }
    }
    pub fn parse(name: &str) -> Self {
        match name {
            "horizon" => TextKeyboard::Horizon,
            "steam" => TextKeyboard::Steam,
            _ => TextKeyboard::Auto,
        }
    }
}

/// Whether A on a text field asks Steam for its keyboard
/// (`steam://open/keyboard`) instead of opening Horizon's: chosen, or `Auto`
/// on a Steam Deck (Steam sets `SteamDeck=1` for the games it starts).
pub fn steam_keyboard(choice: TextKeyboard) -> bool {
    match choice {
        TextKeyboard::Steam => true,
        TextKeyboard::Horizon => false,
        TextKeyboard::Auto => std::env::var("SteamDeck").is_ok_and(|v| v == "1"),
    }
}

/// The controller settings (`horizon-controller.txt`).
#[derive(Clone, Debug, PartialEq)]
pub struct PadSettings {
    pub enabled: bool,
    pub vibration: bool,
    /// The pad keeps driving the game while its window has no focus.
    pub play_in_background: bool,
    /// A DualSense's triggers push back (Horizon's; docs/controllers.md).
    pub adaptive_triggers: bool,
    pub lightbar: Lightbar,
    /// 0..1; 0.5 is the default speed, 1 twice that.
    pub look_sensitivity: f32,
    pub invert_x: bool,
    pub invert_y: bool,
    pub move_deadzone: f32,
    pub look_deadzone: f32,
    pub trigger_threshold: f32,
    /// The sneak button flips sneaking (Bedrock's default) instead of
    /// holding it, unless vanilla's toggle crouch is on already.
    pub toggle_sneak: bool,
    /// LT + A on a recipe crafts on every server, not only in single player and on LAN worlds
    /// found in the server list. Off by default: a server's rules may count a repeated craft as a
    /// macro.
    pub craft_on_servers: bool,
    pub glyphs: GlyphStyle,
    /// A Nintendo pad's face buttons go by their letters (A confirms and jumps, as on the
    /// Switch's own menus and keyboard) instead of their places (B, at the bottom, does): on
    /// by default on the Switch only.
    pub nintendo_letters: bool,
    pub keyboard: TextKeyboard,
    /// The gameplay guide: what the buttons do now, in the HUD's corners while the pad plays.
    pub guide_detail: GuideDetail,
    /// Menu pointer multiplier, scroll lines/second, and repeat times in seconds.
    pub pointer_speed: f32,
    pub scroll_speed: f32,
    pub repeat_delay: f32,
    pub repeat_interval: f32,
    /// Action name -> button, `None` for unbound; missing means the default.
    bindings: BTreeMap<&'static str, Option<Button>>,
    /// The radial menu's pages.
    pub radial: crate::radial::Pages,
    /// Whether the radial menu's opener is held or pressed once.
    pub radial_mode: RadialMode,
    /// The stick that selects in it.
    pub radial_stick: RadialStick,
    /// Lines this game doesn't read (a newer Horizon's settings, an action it doesn't have, a
    /// button it doesn't know), kept as they were so a save here loses nothing.
    extra: Vec<(String, String)>,
    /// The newest `bindLayout` the file has been written in: this game's, or a newer Horizon's,
    /// kept so that one doesn't resolve its file again as older than it is.
    bind_layout: u32,
    /// The opener last used with each stick (`RadialStick::ALL` order), so stepping the Selects
    /// With row and back gives each stick's own opener again.
    openers: radial_file::StickOpeners,
}
impl Default for PadSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            vibration: true,
            play_in_background: false,
            adaptive_triggers: true,
            lightbar: Lightbar::Health,
            look_sensitivity: 0.5,
            invert_x: false,
            invert_y: false,
            move_deadzone: 0.15,
            look_deadzone: 0.12,
            trigger_threshold: 0.4,
            toggle_sneak: true,
            craft_on_servers: false,
            glyphs: GlyphStyle::Auto,
            nintendo_letters: cfg!(target_os = "horizon"),
            keyboard: TextKeyboard::Auto,
            guide_detail: GuideDetail::Full,
            pointer_speed: 1.0,
            scroll_speed: 12.0,
            repeat_delay: 0.35,
            repeat_interval: 0.09,
            bindings: BTreeMap::new(),
            radial: Default::default(),
            radial_mode: RadialMode::Hold,
            radial_stick: RadialStick::Right,
            extra: Vec::new(),
            bind_layout: BIND_LAYOUT,
            openers: radial_file::StickOpeners::default(),
        }
    }
}
fn bool_text(v: bool) -> &'static str {
    if v { "true" } else { "false" }
}
impl PadSettings {
    /// A pad of `family` goes by its letters: only a Nintendo pad has letters to go by.
    pub fn by_letter(&self, family: GlyphStyle) -> bool {
        self.nintendo_letters && family == GlyphStyle::Nintendo
    }
    pub fn load(dir: &Path) -> Self {
        fs::read_to_string(dir.join(FILE))
            .map(|t| Self::parse(&t))
            .unwrap_or_default()
    }
    /// Written once something differs from the defaults, so players who
    /// never touch a controller get no new file.
    pub fn save(&self, dir: &Path) {
        let path = dir.join(FILE);
        if *self != Self::default() || path.exists() {
            let _ = fs::write(path, self.serialize());
        }
    }
    pub fn parse(text: &str) -> Self {
        let mut s = Self::default();
        let mut saved_opener = None;
        for line in text.lines() {
            let Some((key, value)) = line.split_once(':') else {
                continue;
            };
            let value = value.trim();
            let flag = |d: bool| match value {
                "true" => true,
                "false" => false,
                _ => d,
            };
            let unit = |d: f32| {
                value
                    .parse::<f32>()
                    .ok()
                    .filter(|v| v.is_finite())
                    .map_or(d, |v| v.clamp(0.0, 1.0))
            };
            let bounded = |default: f32, min: f32, max: f32| {
                value
                    .parse::<f32>()
                    .ok()
                    .filter(|v| v.is_finite())
                    .map_or(default, |v| v.clamp(min, max))
            };
            match key.trim() {
                "enabled" => s.enabled = flag(s.enabled),
                "vibration" => s.vibration = flag(s.vibration),
                "playInBackground" => s.play_in_background = flag(s.play_in_background),
                "adaptiveTriggers" => s.adaptive_triggers = flag(s.adaptive_triggers),
                "lightbar" => s.lightbar = Lightbar::parse(value),
                "lookSensitivity" => s.look_sensitivity = unit(s.look_sensitivity),
                "invertX" => s.invert_x = flag(s.invert_x),
                "invertY" => s.invert_y = flag(s.invert_y),
                "moveDeadzone" => s.move_deadzone = unit(s.move_deadzone).min(0.9),
                "lookDeadzone" => s.look_deadzone = unit(s.look_deadzone).min(0.9),
                "triggerThreshold" => {
                    s.trigger_threshold = unit(s.trigger_threshold).clamp(0.1, 0.9)
                }
                "toggleSneak" => s.toggle_sneak = flag(s.toggle_sneak),
                "craftOnServers" => s.craft_on_servers = flag(s.craft_on_servers),
                "glyphs" => s.glyphs = GlyphStyle::parse(value),
                "nintendoLetters" => s.nintendo_letters = flag(s.nintendo_letters),
                "keyboard" => s.keyboard = TextKeyboard::parse(value),
                "gameplayGuide" => s.guide_detail = GuideDetail::parse(value),
                "pointerSpeed" => s.pointer_speed = bounded(s.pointer_speed, 0.25, 3.0),
                "scrollSpeed" => s.scroll_speed = bounded(s.scroll_speed, 1.0, 36.0),
                "repeatDelay" => s.repeat_delay = bounded(s.repeat_delay, 0.1, 1.0),
                "repeatInterval" => s.repeat_interval = bounded(s.repeat_interval, 0.03, 0.3),
                "radialMode" => s.radial_mode = RadialMode::parse(value),
                "radialStick" => s.radial_stick = RadialStick::parse(value),
                // Read after the loop, from the whole text.
                "bindLayout" => {}
                // The radial menu's pages (`radial::Pages::parse`).
                other if other.starts_with("radial_") || other == crate::radial::LAYOUT_KEY => {}
                other => {
                    let known = other
                        .strip_prefix("bind_")
                        .filter(|action| ACTIONS.iter().any(|a| a.0 == *action));
                    let button = if value == "none" {
                        Some(None)
                    } else {
                        Button::parse(value).map(Some)
                    };
                    match (known, button) {
                        // The opener once the selecting stick is known.
                        (Some(RADIAL), Some(button)) => saved_opener = Some(button),
                        (Some(action), Some(button)) => s.bind(action, button),
                        _ => s.extra.push((other.to_owned(), value.to_owned())),
                    }
                }
            }
        }
        // An opener on a button it can't be with this stick reads as never saved.
        let refused_opener = match saved_opener {
            Some(Some(b)) if opener_refused(s.radial_stick, b) => true,
            Some(button) => {
                s.bind(RADIAL, button);
                false
            }
            None => false,
        };
        if let Some(pages) = crate::radial::Pages::parse(
            text.lines()
                .filter_map(|line| line.split_once(':'))
                .map(|(key, value)| (key.trim(), value.trim())),
        ) {
            s.radial = pages;
        }
        let lines = || text.lines().map(str::trim);
        let saved = |action: &str| {
            let prefix = format!("bind_{action}:");
            lines().any(|line| line.starts_with(&prefix))
        };
        // The layout the file was written in; none in a file from before these defaults.
        let layout = lines()
            .find_map(|line| line.strip_prefix("bindLayout:"))
            .map(|value| value.trim().parse::<u32>().unwrap_or(0));
        s.bind_layout = layout.map_or(BIND_LAYOUT, |layout| layout.max(BIND_LAYOUT));
        // Once, for a file from before the radial menu's opener, or one with the opener on a
        // button it can no longer be: a button the player bound by hand stays theirs, and the
        // opener's default doesn't take it. From then on the file says where the opener is, and
        // a conflict made since stays shown in the binds list.
        let before_opener = layout.is_none_or(|layout| layout < OPENER_LAYOUT);
        if !text.is_empty() && (refused_opener || (before_opener && !saved(RADIAL))) {
            let default_opener = ACTIONS
                .iter()
                .find(|(action, _)| *action == RADIAL)
                .and_then(|(_, button)| *button);
            let taken = s.bindings.iter().any(|(action, button)| {
                *action != RADIAL && button.is_some() && *button == default_opener
            });
            if taken {
                s.bindings.insert(RADIAL, None);
            }
        }
        // The default too can be under the selecting thumb (D-pad Right, the left stick
        // selecting): no button opens the menu until the player picks one.
        if s.binding(RADIAL)
            .is_some_and(|b| opener_refused(s.radial_stick, b))
        {
            s.bindings.insert(RADIAL, None);
        }
        // A file from before these defaults: its player's buttons stay where they were, and
        // nothing they saved shares a button with the radial menu's opener. They can bind one
        // in the binds list.
        if !text.is_empty() && layout.is_none() {
            s.bindings.entry(RADIAL).or_insert(None);
            for (action, button) in [
                ("key.drop", Button::East),
                ("key.togglePerspective", Button::DPadUp),
                ("key.chat", Button::DPadRight),
                ("key.playerlist", Button::Select),
                ("key.advancements", Button::DPadDown),
            ] {
                // A button saved by hand stays, even one that's now this action's default.
                if !saved(action) {
                    s.bindings.entry(action).or_insert(Some(button));
                }
            }
        }
        s
    }
    pub fn serialize(&self) -> String {
        let mut text = format!(
            "enabled:{}\nvibration:{}\nadaptiveTriggers:{}\nlightbar:{}\nlookSensitivity:{}\ninvertX:{}\ninvertY:{}\nmoveDeadzone:{}\nlookDeadzone:{}\ntriggerThreshold:{}\ntoggleSneak:{}\nglyphs:{}\nnintendoLetters:{}\nkeyboard:{}\n",
            bool_text(self.enabled),
            bool_text(self.vibration),
            bool_text(self.adaptive_triggers),
            self.lightbar.name(),
            self.look_sensitivity,
            bool_text(self.invert_x),
            bool_text(self.invert_y),
            self.move_deadzone,
            self.look_deadzone,
            self.trigger_threshold,
            bool_text(self.toggle_sneak),
            self.glyphs.name(),
            bool_text(self.nintendo_letters),
            self.keyboard.name(),
        );
        text.push_str(&format!(
            "craftOnServers:{}\ngameplayGuide:{}\npointerSpeed:{}\nscrollSpeed:{}\nrepeatDelay:{}\nrepeatInterval:{}\n",
            bool_text(self.craft_on_servers),
            self.guide_detail.name(),
            self.pointer_speed, self.scroll_speed, self.repeat_delay, self.repeat_interval,
        ));
        for (action, button) in &self.bindings {
            text.push_str(&format!(
                "bind_{action}:{}\n",
                button.map_or("none", Button::name)
            ));
        }
        text.push_str(&format!(
            "playInBackground:{}\n",
            bool_text(self.play_in_background)
        ));
        text.push_str(&format!(
            "radialMode:{}\nradialStick:{}\n",
            self.radial_mode.name(),
            self.radial_stick.name()
        ));
        text.push_str(&format!("bindLayout:{}\n", self.bind_layout));
        for (key, value) in &self.extra {
            // A button this game didn't know, for an action the player has bound here since.
            let rebound = key
                .strip_prefix("bind_")
                .is_some_and(|action| self.bindings.contains_key(action));
            if !rebound {
                text.push_str(&format!("{key}:{value}\n"));
            }
        }
        // The default pages aren't written: they can improve for a player who never changed them.
        if self.radial != crate::radial::Pages::default() {
            text.push_str(&self.radial.serialize());
        }
        text
    }
    pub fn default_of(action: &str) -> Option<Button> {
        ACTIONS.iter().find(|a| a.0 == action).and_then(|a| a.1)
    }
    /// The button an action is bound to.
    pub fn binding(&self, action: &str) -> Option<Button> {
        match self.bindings.get(action) {
            Some(button) => *button,
            None => Self::default_of(action),
        }
    }
    /// Put `action` on `button`, or unbind it. An unknown action, or a button it can't take
    /// (`can_bind`), changes nothing.
    pub fn bind(&mut self, action: &str, button: Option<Button>) {
        let Some((name, default)) = ACTIONS.iter().find(|a| a.0 == action) else {
            return;
        };
        if button.is_some_and(|b| !self.can_bind(action, b)) {
            return;
        }
        if button == *default {
            self.bindings.remove(name);
        } else {
            self.bindings.insert(name, button);
        }
    }
    pub fn reset_bindings(&mut self) {
        self.bindings.clear();
    }
    /// Other actions on the same button (marked in the binds list).
    pub fn conflicts(&self, action: &str) -> bool {
        self.binding(action)
            .is_some_and(|b| self.actions_of(b).iter().any(|a| *a != action))
    }
    pub fn is_default(&self, action: &str) -> bool {
        !self.bindings.contains_key(action)
    }
    /// The actions a button triggers, in `ACTIONS` order.
    pub fn actions_of(&self, button: Button) -> Vec<&'static str> {
        ACTIONS
            .iter()
            .map(|a| a.0)
            .filter(|a| self.binding(a) == Some(button))
            .collect()
    }
}

fn length(v: [f32; 2]) -> f32 {
    (v[0] * v[0] + v[1] * v[1]).sqrt()
}

/// 8BitDo Ultimate 2 Wireless (`2dc8:6012`). gilrs' bundled SDL DB only has a
/// Windows row for it; on Linux/Bluetooth the right stick is on ABS_Z/ABS_RX
/// (`LeftZ`/`RightStickX`) until an SDL mapping matches.
pub const BITDO_ULTIMATE2_VID_PID: (u16, u16) = (0x2dc8, 0x6012);

/// Stick axes after correcting pads whose right stick gilrs still exposes on
/// `LeftZ`/`RightStickX` (no matching SDL row for this device version).
/// `sdl_mapped` is true when gilrs already applied an SDL gamecontroller row.
pub fn stick_axes(
    left: [f32; 2],
    right: [f32; 2],
    left_z: f32,
    vendor: Option<u16>,
    product: Option<u16>,
    sdl_mapped: bool,
) -> ([f32; 2], [f32; 2]) {
    if !sdl_mapped
        && vendor == Some(BITDO_ULTIMATE2_VID_PID.0)
        && product == Some(BITDO_ULTIMATE2_VID_PID.1)
    {
        // Default map: RightStickX is the stick's Y, LeftZ its X; RightStickY
        // is the right trigger and must not drive look.
        // gilrs turns over only the axes it calls Y, and this Y is on one it doesn't.
        (left, [left_z, -right[0]])
    } else {
        (left, right)
    }
}

/// The tilt from which a stick holds a key in [`eight_direction_keys`]: holding one is full speed,
/// so the small tilts past the deadzone stay still.
const KEY_TILT: f32 = 0.3;
/// tan(22.5 degrees): where one of the 8 sectors meets the next.
const SECTOR_EDGE: f32 = 0.414_213_57;

/// The movement keys (forward, backward, left, right) a move vector (`[left, forward]`) holds: the
/// one of the 8 headings it is nearest, so it is at most 22.5 degrees off. An edge goes to the
/// straight heading.
pub fn eight_direction_keys([left, forward]: [f32; 2]) -> [bool; 4] {
    if length([left, forward]) < KEY_TILT {
        return [false; 4];
    }
    let along = forward.abs() > SECTOR_EDGE * left.abs();
    let across = left.abs() > SECTOR_EDGE * forward.abs();
    [
        along && forward > 0.0,
        along && forward < 0.0,
        across && left > 0.0,
        across && left < 0.0,
    ]
}

/// A radial deadzone: inside it the stick is centred; beyond it the rest of
/// the travel is rescaled to 0..1 in the same direction, so diagonals aren't
/// clipped and the first movement past the deadzone is small.
pub fn radial(v: [f32; 2], deadzone: f32) -> [f32; 2] {
    let len = length(v);
    if !len.is_finite() || len <= deadzone || len == 0.0 {
        return [0.0, 0.0];
    }
    let scaled = ((len - deadzone) / (1.0 - deadzone)).min(1.0);
    [v[0] / len * scaled, v[1] / len * scaled]
}

/// The left stick as the player's move vector, in `KeyboardInput`'s layout
/// (`[left, forward]`, length at most 1); `None` inside the deadzone.
pub fn move_vector(stick: [f32; 2], deadzone: f32) -> Option<[f32; 2]> {
    let v = radial(stick, deadzone);
    (v != [0.0, 0.0]).then_some([-v[0], v[1]])
}

/// Degrees per second at full tilt and the default sensitivity.
pub const YAW_SPEED: f32 = 200.0;
pub const PITCH_SPEED: f32 = 140.0;

/// The view turn from the right stick over `seconds`: `(yaw, pitch)` in
/// degrees, pitch positive looking down as in `Entity.turn`. A squared
/// curve keeps small tilts precise; a spyglass slows it down.
pub fn look_delta(
    stick: [f32; 2],
    settings: &PadSettings,
    seconds: f32,
    scoping: bool,
) -> (f32, f32) {
    let v = radial(stick, settings.look_deadzone);
    let len = length(v);
    if len == 0.0 {
        return (0.0, 0.0);
    }
    // The vector already has length `len`: scaling by it squares the tilt.
    let curve = len;
    let speed = settings.look_sensitivity * 2.0 * if scoping { 0.25 } else { 1.0 };
    let x = v[0] * curve * if settings.invert_x { -1.0 } else { 1.0 };
    let y = v[1] * curve * if settings.invert_y { -1.0 } else { 1.0 };
    (
        x * YAW_SPEED * speed * seconds,
        -y * PITCH_SPEED * speed * seconds,
    )
}

/// Menu cursor speed in GUI pixels per second at full tilt.
pub const CURSOR_SPEED: f32 = 300.0;

/// How far the menu cursor moves for a stick over `seconds`, in GUI pixels
/// (`y` down): a squared curve, so a light touch lands on a slot.
pub fn cursor_delta(stick: [f32; 2], deadzone: f32, seconds: f32) -> (f32, f32) {
    let v = radial(stick, deadzone);
    let len = length(v);
    if len == 0.0 {
        return (0.0, 0.0);
    }
    let speed = (CURSOR_SPEED * len + 30.0) * seconds;
    (v[0] * speed, -v[1] * speed)
}

/// The target a D-pad press moves focus to: the nearest one inside a cone
/// around the direction, weighing sideways distance double.
pub fn focus_step(from: (f32, f32), button: Button, targets: &[(f32, f32)]) -> Option<usize> {
    let (dx, dy) = match button {
        Button::DPadUp => (0.0, -1.0),
        Button::DPadDown => (0.0, 1.0),
        Button::DPadLeft => (-1.0, 0.0),
        Button::DPadRight => (1.0, 0.0),
        _ => return None,
    };
    targets
        .iter()
        .enumerate()
        .filter_map(|(i, t)| {
            let (x, y) = (t.0 - from.0, t.1 - from.1);
            let along = x * dx + y * dy;
            let across = (x * dy - y * dx).abs();
            (along > 2.0 && across <= along * 2.0).then_some((i, along + 2.0 * across))
        })
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(i, _)| i)
}

/// A rectangle `[x, y, w, h]` in GUI pixels.
pub type Rect = [f32; 4];

/// The D-pad direction a stick points to, when pushed past `threshold`
/// mostly along one axis (focus steps like the D-pad).
pub fn stick_direction(stick: [f32; 2], threshold: f32) -> Option<Button> {
    let [x, y] = stick;
    if length(stick) < threshold {
        return None;
    }
    Some(if x.abs() > y.abs() {
        if x > 0.0 {
            Button::DPadRight
        } else {
            Button::DPadLeft
        }
    } else if y > 0.0 {
        Button::DPadUp
    } else {
        Button::DPadDown
    })
}

/// How far a stick already stepping must fall back to let go: between this and
/// the step threshold it holds its direction, so a stick resting on the
/// threshold doesn't step again.
pub const STICK_RELEASE: f32 = 0.35;
/// How much more a stick must lean along another axis to change direction, so
/// a stick resting on a diagonal doesn't flip between the two.
const STICK_TURN: f32 = 1.5;

/// [`stick_direction`] for a stick that may already be stepping `held`: it stays
/// that way until the stick falls back past [`STICK_RELEASE`] or clearly turns.
pub fn stick_direction_held(
    stick: [f32; 2],
    threshold: f32,
    held: Option<Button>,
) -> Option<Button> {
    let fresh = stick_direction(stick, threshold);
    let Some((axis, positive)) = held.and_then(direction) else {
        return fresh;
    };
    if length(stick) < STICK_RELEASE {
        return None;
    }
    // `direction`'s y runs down the screen, a stick's up.
    let toward = stick[axis] * if positive == (axis == 0) { 1.0 } else { -1.0 };
    if toward > 0.0 && stick[1 - axis].abs() <= toward * STICK_TURN {
        held
    } else {
        fresh
    }
}

/// A list drawn with a scrollbar (`ui::scrollbar`), how far it is scrolled (a list
/// moving a whole row a notch draws the same rectangles, so they can't tell) and
/// the ways it can still scroll.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ScrollArea {
    pub rect: Rect,
    pub amount: f32,
    pub up: bool,
    pub down: bool,
}

impl ScrollArea {
    /// A row counts while any of it is in view: the stick can leave the focus on one nearly gone.
    pub fn holds(&self, r: &Rect) -> bool {
        let x = r[0] + r[2] / 2.0;
        let [ax, ay, aw, ah] = self.rect;
        (ax..ax + aw).contains(&x) && r[1] < ay + ah && r[1] + r[3] > ay
    }
    /// All of a row is in view.
    fn shows(&self, r: &Rect) -> bool {
        let [_, ay, _, ah] = self.rect;
        self.holds(r) && r[1] + 0.5 >= ay && r[1] + r[3] <= ay + ah + 0.5
    }
}

/// A vertical focus step, with lists: the strict step, unless it would land
/// outside the view of the list `from` is in, or only partly in it, while that
/// list can still scroll that way, when there is none (`None`) and the list
/// scrolls instead: its hidden rows come first and whole, as vanilla's arrow
/// keys walk a selection list's entries (`ensureVisible`).
pub fn list_step(
    from: Rect,
    button: Button,
    targets: &[Rect],
    areas: &[ScrollArea],
) -> Option<usize> {
    let step = focus_step_strict(from, button, targets)?;
    let can_scroll = |a: &ScrollArea| {
        if button == Button::DPadUp {
            a.up
        } else {
            a.down
        }
    };
    let leaves = areas
        .iter()
        .find(|a| a.holds(&from))
        .is_some_and(|a| can_scroll(a) && !a.shows(&targets[step]));
    (!leaves).then_some(step)
}

/// The farthest centres of two neighbouring slots: 18 apart, 22 over the hotbar's gap.
const SLOT_NEIGHBOUR: f32 = 24.0;

/// A held direction's repeat stops at the edge of the slots it is walking: it
/// goes on only from a slot to a slot next to it (the hotbar's small gap
/// included). A fresh press may go anywhere.
pub fn repeat_crosses_slots(from: (f32, f32), to: (f32, f32), to_slot: bool) -> bool {
    !to_slot || (to.0 - from.0).abs().max((to.1 - from.1).abs()) > SLOT_NEIGHBOUR
}

/// Arrow-key focus navigation as vanilla does it
/// (`ContainerEventHandler.nextFocusPathInDirection`), over every target of
/// the screen at once: from a strip one pixel past the focused edge, the
/// nearest target overlapping it on the other axis wins (ties: leftmost or
/// topmost); without one, the target whose facing edge's centre is nearest
/// the focused edge's centre (`nextFocusPathVaguelyInDirection`). Bounds are
/// inclusive as in `ScreenRectangle` (a right edge is `x + w - 1`).
pub fn focus_step_rect(from: Rect, button: Button, targets: &[Rect]) -> Option<usize> {
    focus_step_strict(from, button, targets).or_else(|| focus_step_vague(from, button, targets))
}

/// The direction as (axis: 0 = x, 1 = y; positive).
fn direction(button: Button) -> Option<(usize, bool)> {
    match button {
        Button::DPadUp => Some((1, false)),
        Button::DPadDown => Some((1, true)),
        Button::DPadLeft => Some((0, false)),
        Button::DPadRight => Some((0, true)),
        _ => None,
    }
}

/// The first pass of [`focus_step_rect`]: targets overlapping the focused
/// one on the other axis (a row, a column).
pub fn focus_step_strict(from: Rect, button: Button, targets: &[Rect]) -> Option<usize> {
    let (axis, positive) = direction(button)?;
    let other = 1 - axis;
    let lo = |r: &Rect, a: usize| r[a];
    let hi = |r: &Rect, a: usize| r[a] + r[a + 2] - 1.0;
    // Bound in a direction (`getBoundInDirection`).
    let bound = |r: &Rect, a: usize, pos: bool| if pos { hi(r, a) } else { lo(r, a) };
    let after = |a: f32, b: f32| if positive { a > b + 1e-3 } else { b > a + 1e-3 };
    // `getBorder(direction)`: the strip one pixel past the focused edge.
    let mut strip = from;
    strip[axis] = bound(&from, axis, positive) + if positive { 1.0 } else { -1.0 };
    strip[axis + 2] = 1.0;
    let first = bound(&strip, axis, !positive);
    let same = |a: &Rect, b: &Rect| a.iter().zip(b).all(|(x, y)| (x - y).abs() < 0.5);
    let mut strict: Vec<(usize, f32, f32)> = targets
        .iter()
        .enumerate()
        .filter(|(_, r)| !same(r, &from))
        .filter(|(_, r)| lo(r, other).max(lo(&strip, other)) <= hi(r, other).min(hi(&strip, other)))
        .filter(|(_, r)| {
            let child_first = bound(r, axis, !positive);
            after(child_first, first)
                || ((child_first - first).abs() < 1e-3
                    && after(bound(r, axis, positive), bound(&strip, axis, positive)))
        })
        .map(|(i, r)| (i, bound(r, axis, !positive), lo(r, other)))
        .collect();
    strict.sort_by(|a, b| {
        let primary = if positive {
            a.1.total_cmp(&b.1)
        } else {
            b.1.total_cmp(&a.1)
        };
        primary.then(a.2.total_cmp(&b.2))
    });
    strict.first().map(|s| s.0)
}

/// The second pass of [`focus_step_rect`]: the target whose facing edge's
/// centre is nearest the focused edge's centre.
pub fn focus_step_vague(from: Rect, button: Button, targets: &[Rect]) -> Option<usize> {
    let (axis, positive) = direction(button)?;
    let other = 1 - axis;
    let lo = |r: &Rect, a: usize| r[a];
    let hi = |r: &Rect, a: usize| r[a] + r[a + 2] - 1.0;
    let bound = |r: &Rect, a: usize, pos: bool| if pos { hi(r, a) } else { lo(r, a) };
    let after = |a: f32, b: f32| if positive { a > b + 1e-3 } else { b > a + 1e-3 };
    let same = |a: &Rect, b: &Rect| a.iter().zip(b).all(|(x, y)| (x - y).abs() < 0.5);
    let mut strip = from;
    strip[axis] = bound(&from, axis, positive) + if positive { 1.0 } else { -1.0 };
    strip[axis + 2] = 1.0;
    let centre = |r: &Rect, a: usize| (hi(r, a) + lo(r, a)) / 2.0;
    let edge = bound(&strip, axis, positive);
    let mid = centre(&strip, other);
    targets
        .iter()
        .enumerate()
        .filter(|(_, r)| !same(r, &from))
        .filter_map(|(i, r)| {
            let facing = bound(r, axis, !positive);
            after(facing, edge).then(|| {
                let (d1, d2) = (facing - edge, centre(r, other) - mid);
                (i, d1 * d1 + d2 * d2)
            })
        })
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(i, _)| i)
}

/// How far a list scrolled between two frames' targets: the vertical shift
/// most targets of the same position and size agree on (targets that didn't
/// move, outside the list, don't count), for a list scrolled towards its end
/// (`down`: the rows moved up) or its start. `None` when nothing moved that way.
/// It takes the direction because half a row's scroll votes the same both ways.
pub fn scroll_shift(before: &[Rect], after: &[Rect], down: bool) -> Option<f32> {
    let same = |a: &Rect, b: &Rect| a.iter().zip(b).all(|(x, y)| (x - y).abs() < 0.5);
    if before.len() == after.len() && before.iter().zip(after).all(|(a, b)| same(a, b)) {
        return None;
    }
    // Most votes; among equals the smallest move (the next row, not a far one).
    shift_votes(before, after)
        .into_iter()
        .filter(|v| (v.0 < 0.0) == down)
        .max_by(|a, b| a.1.cmp(&b.1).then(b.0.abs().total_cmp(&a.0.abs())))
        .map(|v| v.0)
}

/// Every vertical shift that would carry a `before` target onto an `after`
/// target of the same position and size, with how many it would.
fn shift_votes(before: &[Rect], after: &[Rect]) -> Vec<(f32, usize)> {
    let mut votes: Vec<(f32, usize)> = Vec::new();
    for b in before {
        for a in after {
            let dy = a[1] - b[1];
            if (a[0] - b[0]).abs() < 0.5
                && (a[2] - b[2]).abs() < 0.5
                && (a[3] - b[3]).abs() < 0.5
                && dy.abs() >= 0.5
            {
                match votes.iter_mut().find(|v| (v.0 - dy).abs() < 0.5) {
                    Some(v) => v.1 += 1,
                    None => votes.push((dy, 1)),
                }
            }
        }
    }
    votes
}

/// Where the focus goes when the right stick scrolled a list (`down`: towards
/// its end, so the rows moved up): the rectangle moved by the list's shift;
/// the same rectangle, for a widget outside the list; or, its row out of view,
/// the nearest target of its kind (`after`'s flag), the list's rows before
/// anything fixed. `None` when nothing moved that way, or none of its kind is
/// left. It takes the direction because half a row's scroll votes the same
/// both ways once a new row comes into view.
pub fn follow_scroll(
    focus: Rect,
    down: bool,
    before: &[Rect],
    after: &[(Rect, bool)],
) -> Option<usize> {
    let rects: Vec<Rect> = after.iter().map(|t| t.0).collect();
    // A column that stayed put isn't the list, even spaced like its shift.
    let column = |of: &[Rect], r: &Rect| -> Vec<f32> {
        let same = |o: &&Rect| o[0] == r[0] && o[2] == r[2] && o[3] == r[3];
        of.iter().filter(same).map(|o| o[1]).collect()
    };
    let moving = |r: &&Rect| column(before, r) != column(&rects, r);
    let was: Vec<Rect> = before.iter().filter(moving).copied().collect();
    let now: Vec<Rect> = rects.iter().filter(moving).copied().collect();
    let dy = shift_votes(&was, &now)
        .into_iter()
        .filter(|v| (v.0 < 0.0) == down)
        .max_by(|a, b| a.1.cmp(&b.1).then(b.0.abs().total_cmp(&a.0.abs())))?
        .0;
    let same = |a: &Rect, b: &Rect| a.iter().zip(b).all(|(x, y)| (x - y).abs() < 0.5);
    let moved = [focus[0], focus[1] + dy, focus[2], focus[3]];
    let found = |at: &Rect| rects.iter().position(|r| same(r, at));
    if !moving(&&focus) {
        return found(&focus);
    }
    if let Some(i) = found(&moved).or_else(|| found(&focus)) {
        return Some(i);
    }
    let in_list = |r: &Rect| {
        let was = [r[0], r[1] - dy, r[2], r[3]];
        before.iter().any(|b| same(b, &was))
    };
    let centre = (moved[0] + moved[2] / 2.0, moved[1] + moved[3] / 2.0);
    let distance = |r: &Rect| (r[0] + r[2] / 2.0 - centre.0).hypot(r[1] + r[3] / 2.0 - centre.1);
    after
        .iter()
        .enumerate()
        .filter(|(_, t)| t.1)
        .min_by(|(_, a), (_, b)| {
            in_list(&b.0)
                .cmp(&in_list(&a.0))
                .then(distance(&a.0).total_cmp(&distance(&b.0)))
        })
        .map(|(i, _)| i)
}

/// The target a focus rectangle stands for once the screen's size changed:
/// the same widget, by its place in the draw order when the screen drew as
/// many; else the target nearest where its centre falls at the new size.
pub fn refind_resized(
    focus: Rect,
    before: &[Rect],
    after: &[Rect],
    old: (f32, f32),
    new: (f32, f32),
) -> Option<usize> {
    let same = |r: &Rect| r.iter().zip(&focus).all(|(a, b)| (a - b).abs() < 0.5);
    if before.len() == after.len()
        && let Some(i) = before.iter().position(same)
    {
        return Some(i);
    }
    let at = rescaled(
        (focus[0] + focus[2] / 2.0, focus[1] + focus[3] / 2.0),
        old,
        new,
    );
    let centres: Vec<(f32, f32)> = after
        .iter()
        .map(|r| (r[0] + r[2] / 2.0, r[1] + r[3] / 2.0))
        .collect();
    nearest(at, &centres, f32::INFINITY)
}

/// A point at the same place, proportionally, on a screen of another size.
pub fn rescaled(at: (f32, f32), old: (f32, f32), new: (f32, f32)) -> (f32, f32) {
    (at.0 * new.0 / old.0.max(1.0), at.1 * new.1 / old.1.max(1.0))
}

/// The target a remembered focus rectangle stands for in a new frame: the
/// same rectangle, else the nearest centre within `radius` (the screen
/// moved it a little: a scrolled list, a resized window).
pub fn refind(focus: Rect, targets: &[Rect], radius: f32) -> Option<usize> {
    let same = |r: &Rect| r.iter().zip(&focus).all(|(a, b)| (a - b).abs() < 0.5);
    if let Some(i) = targets.iter().position(same) {
        return Some(i);
    }
    let c = |r: &Rect| (r[0] + r[2] / 2.0, r[1] + r[3] / 2.0);
    let centres: Vec<(f32, f32)> = targets.iter().map(c).collect();
    nearest(c(&focus), &centres, radius)
}

/// The next tab after a bumper among tab targets (left to right, then top
/// to bottom), wrapping; `None` with fewer than two tabs.
pub fn tab_step(tabs: &[(Rect, bool)], forward: bool) -> Option<usize> {
    if tabs.len() < 2 {
        return None;
    }
    let mut order: Vec<usize> = (0..tabs.len()).collect();
    order.sort_by(|&a, &b| {
        let (ra, rb) = (tabs[a].0, tabs[b].0);
        // A vertical strip of tabs (a sidebar) reads top to bottom.
        ra[1].total_cmp(&rb[1]).then(ra[0].total_cmp(&rb[0]))
    });
    let n = order.len();
    let at = order.iter().position(|&i| tabs[i].1).unwrap_or(0);
    Some(
        order[if forward {
            (at + 1) % n
        } else {
            (at + n - 1) % n
        }],
    )
}

/// A slider nudge: the pointer x that sets `value` (0..1) on a track
/// `[start, width]` (for vanilla's sliders `x + 4` and `w - 8`:
/// `AbstractSliderButton.setValueFromMouse` inverted).
pub fn slider_x(track: [f32; 2], value: f32) -> f32 {
    track[0] + value.clamp(0.0, 1.0) * track[1]
}

/// Pixels a held left/right moves a slider handle per step: one pixel (a
/// vanilla arrow key press) at first, faster while held.
pub fn slider_step(repeats: u32) -> f32 {
    [1.0, 1.0, 2.0, 2.0, 4.0, 4.0, 8.0][repeats.min(6) as usize]
}

/// The target within `radius` of `at`, nearest first (the cursor settles
/// onto a slot or button when the stick lets go).
pub fn nearest(at: (f32, f32), targets: &[(f32, f32)], radius: f32) -> Option<usize> {
    targets
        .iter()
        .enumerate()
        .map(|(i, t)| (i, (t.0 - at.0).hypot(t.1 - at.1)))
        .filter(|(_, d)| *d <= radius)
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(i, _)| i)
}

/// Damage taken, from the player's health between polls. The baseline is
/// forgotten whenever there is no player or a new player entity replaces
/// the old one (a world loaded, a respawn, a server joined, a dimension
/// change), so arriving with less health than before isn't damage.
#[derive(Clone, Copy, Debug, Default)]
pub struct HealthWatch {
    last: Option<f32>,
}
impl HealthWatch {
    /// The health now (`None` without a player); returns the damage taken
    /// since the last call.
    pub fn observe(&mut self, health: Option<f32>) -> Option<f32> {
        let before = std::mem::replace(&mut self.last, health);
        match (before, health) {
            (Some(before), Some(now)) if now < before => Some(before - now),
            _ => None,
        }
    }
    pub fn forget(&mut self) {
        self.last = None;
    }
}

/// What makes the pad vibrate (Bedrock-like: short, light pulses).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Rumble {
    /// The player lost this much health.
    Hurt(f32),
    BlockBroken,
    /// An explosion this far away (blocks) with this power.
    Explosion {
        distance: f64,
        radius: f32,
    },
    Totem,
    /// The radial menu's stick reached a slot that would run: a tick.
    RadialSlot,
    /// The radial menu's page turned: a shorter tick.
    RadialPage,
    /// The page turn went round an end: a second tick after the first.
    RadialWrap,
    /// A release ran nothing after something had been selected: a soft buzz.
    RadialNothing,
}
/// A vibration: the heavy (low-frequency) and light motors, 0..1, and how
/// long it lasts.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pulse {
    pub strong: f32,
    pub weak: f32,
    pub millis: u32,
}
use crate::radial::{RADIAL_NOTHING, RADIAL_PAGE_TICK, RADIAL_TICK};
impl Rumble {
    pub fn name(self) -> &'static str {
        match self {
            Rumble::Hurt(_) => "hurt",
            Rumble::BlockBroken => "block",
            Rumble::Explosion { .. } => "explosion",
            Rumble::Totem => "totem",
            Rumble::RadialSlot => "radial",
            Rumble::RadialPage => "radial_page",
            Rumble::RadialWrap => "radial_wrap",
            Rumble::RadialNothing => "radial_nothing",
        }
    }
    /// The pulse, or `None` when it is too faint to feel.
    pub fn pulse(self) -> Option<Pulse> {
        match self {
            Rumble::Hurt(amount) if amount > 0.0 => Some(Pulse {
                strong: (0.2 + 0.06 * amount).min(0.8),
                weak: 0.3,
                millis: (150.0 + 20.0 * amount).min(400.0) as u32,
            }),
            Rumble::Hurt(_) => None,
            Rumble::BlockBroken => Some(Pulse {
                strong: 0.0,
                weak: 0.25,
                millis: 60,
            }),
            Rumble::Explosion { distance, radius } => {
                // Felt up to four times the blast radius, fading with distance.
                let reach = f64::from(radius.max(1.0)) * 4.0;
                let near = (1.0 - distance / reach) as f32;
                (near > 0.05).then_some(Pulse {
                    strong: 0.9 * near,
                    weak: 0.6 * near,
                    millis: 350,
                })
            }
            Rumble::RadialSlot => Some(RADIAL_TICK),
            Rumble::RadialPage | Rumble::RadialWrap => Some(RADIAL_PAGE_TICK),
            Rumble::RadialNothing => Some(RADIAL_NOTHING),
            Rumble::Totem => Some(Pulse {
                strong: 0.7,
                weak: 0.7,
                millis: 500,
            }),
        }
    }

    /// The pulse shaped, for a pad that plays steps (a DualSense): a hit starts at full
    /// strength and dies away, an explosion rolls on, a totem swells; a broken block is a tick.
    pub fn shape(self) -> Vec<Pulse> {
        let Some(p) = self.pulse() else {
            return Vec::new();
        };
        let step = |level: f32, millis: u32| Pulse {
            strong: p.strong * level,
            weak: p.weak * level,
            millis,
        };
        match self {
            Rumble::BlockBroken => vec![Pulse { millis: 35, ..p }],
            Rumble::RadialSlot
            | Rumble::RadialPage
            | Rumble::RadialWrap
            | Rumble::RadialNothing => vec![p],
            Rumble::Hurt(_) => {
                let tail = p.millis.saturating_sub(50) / 2;
                vec![step(1.0, 50), step(0.55, tail), step(0.25, tail)]
            }
            Rumble::Explosion { .. } => vec![
                step(1.0, 80),
                step(0.6, 150),
                step(0.35, 150),
                step(0.15, 150),
            ],
            Rumble::Totem => vec![
                step(0.4, 100),
                step(0.7, 100),
                step(1.0, 200),
                step(0.5, 100),
            ],
        }
    }
}

/// What a trigger feels like on a DualSense, from what its action is doing (docs/controllers.md
/// "Adaptive triggers" for the choices). Only the pad's official modes: Nielk1's notes warn the
/// others may leave the firmware.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TriggerFeel {
    Free,
    /// Drawing a bow, charging a trident, loading a crossbow: resistance that grows with the
    /// charge, 0..1.
    Draw(f32),
    /// A loaded crossbow: a gun trigger's catch that gives way where it fires.
    Click,
    /// Pulling it boosts (a rocket while gliding, the stick while riding a pig or a strider):
    /// a lighter catch that gives way where it fires.
    Boost,
    /// A shield in the use hand, raised or ready: its weight.
    Brace,
    /// That shield disabled by an axe: the trigger locks until it recovers.
    Locked,
    /// Mining a block of this hardness: a buzz, stronger and slower on harder blocks.
    Texture(f32),
}
#[cfg(feature = "controllers")]
impl TriggerFeel {
    /// The effect, for a trigger that presses its action at `threshold` (0..1) of its travel.
    pub fn effect(self, threshold: f32) -> horizon_dualsense::TriggerEffect {
        use horizon_dualsense::trigger;
        // The zone (of ten along the travel) where the action presses: a catch gives way there,
        // so the click and the shot coincide.
        let press = ((threshold.clamp(0.0, 1.0) * 10.0) as u8).clamp(3, 8);
        match self {
            TriggerFeel::Free => horizon_dualsense::TRIGGER_OFF,
            // From the third zone, so the press itself stays light; at most 6 of 8, as the
            // draw is held.
            TriggerFeel::Draw(charge) => {
                trigger::feedback(2, 1 + (5.0 * charge.clamp(0.0, 1.0)).round() as u8)
            }
            TriggerFeel::Click => trigger::weapon((press - 2).max(2), press, 8),
            TriggerFeel::Boost => trigger::weapon((press - 2).max(2), press, 5),
            TriggerFeel::Brace => trigger::feedback(2, 3),
            TriggerFeel::Locked => trigger::feedback(1, 8),
            TriggerFeel::Texture(hardness) if hardness > 0.0 => {
                // Dirt 2, stone 3, deepslate 4, obsidian 5 of 8.
                let amplitude = (1.0 + 1.6 * hardness.sqrt()).round().clamp(1.0, 5.0);
                // Sand and dirt a fine 40 Hz grit; stone 30; wood, deepslate and ores 20;
                // obsidian a 10 Hz chisel.
                let hertz = match hardness {
                    h if h < 1.0 => 40,
                    h if h < 2.0 => 30,
                    h if h < 5.0 => 20,
                    _ => 10,
                };
                trigger::vibration(1, amplitude as u8, hertz)
            }
            TriggerFeel::Texture(_) => horizon_dualsense::TRIGGER_OFF,
        }
    }
}
#[cfg(feature = "controllers")]
impl PadSettings {
    /// The left and right triggers' effects: each feels the action bound to it, `key.use` or
    /// `key.attack`.
    pub fn trigger_effects(
        &self,
        using: TriggerFeel,
        attacking: TriggerFeel,
    ) -> [horizon_dualsense::TriggerEffect; 2] {
        [Button::LeftTrigger, Button::RightTrigger].map(|trigger| {
            let actions = self.actions_of(trigger);
            let feel = if !self.adaptive_triggers {
                TriggerFeel::Free
            } else if actions.contains(&"key.use") {
                using
            } else if actions.contains(&"key.attack") {
                attacking
            } else {
                TriggerFeel::Free
            };
            feel.effect(self.trigger_threshold)
        })
    }
}
/// What a DualSense's lightbar shows (`lightbar:`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lightbar {
    /// Green at full health through amber to red.
    Health,
    /// A colour per dimension.
    Dimension,
    /// The pad's own.
    Off,
}
impl Lightbar {
    pub const ALL: [Lightbar; 3] = [Lightbar::Health, Lightbar::Dimension, Lightbar::Off];
    pub fn name(self) -> &'static str {
        match self {
            Lightbar::Health => "health",
            Lightbar::Dimension => "dimension",
            Lightbar::Off => "off",
        }
    }
    pub fn parse(name: &str) -> Self {
        match name.trim() {
            "dimension" => Lightbar::Dimension,
            "off" => Lightbar::Off,
            _ => Lightbar::Health,
        }
    }
    /// The colour for a player with this share of their health in this dimension; at half
    /// brightness, as the pad's full light is glaring.
    pub fn colour(self, health: f32, dimension: &str) -> Option<[u8; 3]> {
        let rgb = match self {
            Lightbar::Off => return None,
            Lightbar::Health => {
                // Green to amber over the top half, amber to red below it.
                let (green, amber, red) = (
                    [34.0, 197.0, 94.0],
                    [245.0, 158.0, 11.0],
                    [239.0, 68.0, 68.0],
                );
                let h = health.clamp(0.0, 1.0);
                let (from, to, k) = if h >= 0.5 {
                    (amber, green, (h - 0.5) * 2.0)
                } else {
                    (red, amber, h * 2.0)
                };
                [0, 1, 2].map(|i| from[i] + (to[i] - from[i]) * k)
            }
            Lightbar::Dimension => match dimension {
                "minecraft:the_nether" => [234.0, 88.0, 12.0],
                "minecraft:the_end" => [139.0, 92.0, 246.0],
                _ => [14.0, 165.0, 233.0],
            },
        };
        Some(rgb.map(|v| (v * 0.5).round() as u8))
    }
}

/// `BowItem.getPowerForTime`: a bow's draw after this many ticks, 0..1.
pub fn bow_power(ticks: u32) -> f32 {
    let f = ticks as f32 / 20.0;
    ((f * f + f * 2.0) / 3.0).min(1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_bindable_action_reaches_the_game() {
        for (action, _) in ACTIONS {
            assert!(route(action).is_some(), "{action} has nowhere to go");
        }
    }

    #[test]
    fn a_held_drop_repeats_then_throws_the_stack() {
        let t = Instant::now();
        let at = |ms: u64| t + Duration::from_millis(ms);
        let (delay, interval) = (Duration::from_millis(350), Duration::from_millis(90));
        let mut held = HeldDrop::new(t, delay);
        assert_eq!(held.due(at(100), interval), None, "a tap is one item");
        assert_eq!(held.due(at(349), interval), None);
        assert_eq!(held.due(at(350), interval), Some(DropStep::One));
        assert_eq!(held.due(at(400), interval), None, "not before the interval");
        assert_eq!(held.due(at(440), interval), Some(DropStep::One));
        assert_eq!(held.due(at(999), interval), Some(DropStep::One));
        assert_eq!(held.due(at(1000), interval), None);
        assert_eq!(held.due(at(1100), interval), Some(DropStep::Stack));
        assert_eq!(held.due(at(1200), interval), Some(DropStep::Stack));
        assert_eq!(held.due(at(1201), interval), None);
    }

    #[test]
    fn the_later_actions_have_no_default_button_and_leave_saved_bindings() {
        let defaults = PadSettings::default();
        for action in [
            "key.fullscreen",
            "key.socialInteractions",
            "key.quickActions",
            "key.saveToolbarActivator",
            "key.loadToolbarActivator",
            "key.hotbar.1",
            "key.hotbar.9",
        ] {
            assert_eq!(defaults.binding(action), None, "{action}");
        }
        let read = PadSettings::parse(
            "bindLayout:1
bind_key.jump:east
bind_key.hotbar.2:north
",
        );
        assert_eq!(read.binding("key.jump"), Some(Button::East));
        assert_eq!(read.binding("key.inventory"), Some(Button::North));
        assert_eq!(
            read.actions_of(Button::North),
            ["key.inventory", "key.hotbar.2"]
        );
    }

    #[test]
    fn navigation_preferences_migrate_and_stay_with_each_player() {
        let dir = std::env::temp_dir().join(format!(
            "horizon-pad-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let p2 = dir.join("P2");
        fs::create_dir_all(&p2).unwrap();
        let fresh = PadSettings::load(&dir);
        assert_eq!(fresh, PadSettings::default());
        // An older file, with the removed menuNavigation key: bindings kept.
        let legacy =
            PadSettings::parse("menuNavigation:pointer\nbind_key.jump:east\nlookSensitivity:0.7\n");
        assert_eq!(legacy.binding("key.jump"), Some(Button::East));
        assert_eq!(legacy.look_sensitivity, 0.7);
        fresh.save(&dir);
        assert!(!dir.join(FILE).exists(), "defaults write no file");
        let mut other = legacy;
        other.pointer_speed = 2.5;
        other.scroll_speed = 24.0;
        other.repeat_delay = 0.6;
        other.repeat_interval = 0.12;
        other.save(&p2);
        assert_eq!(PadSettings::load(&p2), other);
        assert_eq!(PadSettings::load(&dir), fresh, "P2 did not write P1");
        let invalid = PadSettings::parse(
            "pointerSpeed:NaN\nscrollSpeed:inf\nrepeatDelay:-10\nrepeatInterval:20\n",
        );
        assert_eq!((invalid.pointer_speed, invalid.scroll_speed), (1.0, 12.0));
        assert_eq!((invalid.repeat_delay, invalid.repeat_interval), (0.1, 0.3));
        let bounded = PadSettings::parse(
            "pointerSpeed:10\nscrollSpeed:-1\nrepeatDelay:NaN\nrepeatInterval:inf\n",
        );
        assert_eq!((bounded.pointer_speed, bounded.scroll_speed), (3.0, 1.0));
        assert_eq!(
            (bounded.repeat_delay, bounded.repeat_interval),
            (0.35, 0.09)
        );
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn ownership_requires_new_activity_and_ignores_drift() {
        let settings = PadSettings::default();
        let mut p1 = Activity::default();
        let mut p2 = Activity::default();
        let mut state = PadState {
            left: [0.04, 0.03],
            ..PadState::default()
        };
        assert!(!p1.observe(&state, &settings));
        state.set(Button::South, true);
        assert!(p1.observe(&state, &settings));
        assert!(
            !p1.observe(&state, &settings),
            "held pad cannot reclaim mouse ownership"
        );
        assert!(
            p2.observe(&state, &settings),
            "another player's tracker is independent"
        );
        state.set(Button::South, false);
        assert!(!p1.observe(&state, &settings));
        state.left = [0.5, 0.0];
        assert!(p1.observe(&state, &settings));
        state.left = [0.52, 0.0];
        assert!(!p1.observe(&state, &settings));
        for x in [0.54, 0.56] {
            state.left = [x, 0.0];
            assert!(!p1.observe(&state, &settings));
        }
        state.left = [0.6, 0.0];
        assert!(
            p1.observe(&state, &settings),
            "slow intentional movement accumulates"
        );
        state.left = [0.0; 2];
        state.triggers[0] = 0.5;
        assert!(p1.observe(&state, &settings));
        state.triggers[0] = 0.35;
        assert!(!p1.observe(&state, &settings));
        state.triggers[0] = 0.45;
        assert!(
            !p1.observe(&state, &settings),
            "trigger threshold noise is not new activity"
        );
    }

    #[test]
    fn cancelled_holds_stay_released_until_neutral_then_press_again() {
        let mut edges = Edges::default();
        let mut state = PadState::default();
        state.set(Button::South, true);
        state.triggers[1] = 0.8;
        assert_eq!(
            edges.update(&state, 0.4),
            vec![(Button::South, true), (Button::RightTrigger, true)]
        );
        assert_eq!(
            edges.cancel(),
            vec![(Button::South, false), (Button::RightTrigger, false)]
        );
        assert!(edges.update(&state, 0.4).is_empty());
        state.triggers[1] = 0.35;
        assert!(edges.update(&state, 0.4).is_empty());
        assert!(edges.update(&PadState::default(), 0.4).is_empty());
        assert_eq!(edges.update(&state, 0.4), vec![(Button::South, true)]);
        state.triggers[1] = 0.7;
        assert_eq!(
            edges.update(&state, 0.4),
            vec![(Button::RightTrigger, true)]
        );
    }

    #[test]
    fn every_action_has_its_default() {
        let s = PadSettings::default();
        assert_eq!(s.actions_of(Button::South), ["key.jump"]);
        assert_eq!(s.actions_of(Button::RightTrigger), ["key.attack"]);
        assert_eq!(s.actions_of(Button::LeftTrigger), ["key.use"]);
        assert_eq!(s.actions_of(Button::North), ["key.inventory"]);
        assert_eq!(s.actions_of(Button::RightBumper), [HOTBAR_NEXT]);
        assert!(s.actions_of(Button::Start).is_empty());
        // Every default action names a key mapping or a pad action.
        for (action, _) in ACTIONS {
            assert!(
                action.starts_with("pad.")
                    || crate::keybinds::all_mappings().any(|m| m.0 == *action),
                "{action}"
            );
        }
    }

    #[test]
    fn playing_in_the_background_is_off_until_saved_on() {
        assert!(!PadSettings::parse("vibration:false\n").play_in_background);
        let mut on = PadSettings::default();
        assert!(!on.play_in_background);
        on.play_in_background = true;
        assert!(PadSettings::parse(&on.serialize()).play_in_background);
    }

    #[test]
    fn a_file_from_before_these_defaults_keeps_its_buttons() {
        // Saved settings that never bound D-pad down: it stays on the advancements.
        let s = PadSettings::parse("vibration:false\nbind_key.chat:up\n");
        assert_eq!(s.binding("key.advancements"), Some(Button::DPadDown));
        assert_eq!(s.binding("key.chat"), Some(Button::DPadUp));
        assert_eq!(s.binding("key.drop"), Some(Button::East));
        assert_eq!(s.binding("key.togglePerspective"), Some(Button::DPadUp));
        assert_eq!(s.binding("key.playerlist"), Some(Button::Select));
        assert!(!s.conflicts("key.advancements"));
        // Advancements moved by hand stays moved, and the file stays this way from then on.
        let s = PadSettings::parse("bind_key.advancements:left\nbind_key.drop:down\n");
        assert_eq!(s.binding("key.advancements"), Some(Button::DPadLeft));
        assert_eq!(s.binding("key.drop"), Some(Button::DPadDown));
        assert_eq!(PadSettings::parse(&s.serialize()), s);
    }

    #[test]
    fn a_new_player_drops_on_d_pad_down_and_chats_on_d_pad_up() {
        for s in [PadSettings::default(), PadSettings::parse("")] {
            assert_eq!(s.binding("key.drop"), Some(Button::DPadDown));
            assert_eq!(s.binding("key.chat"), Some(Button::DPadUp));
            assert_eq!(s.binding("key.togglePerspective"), Some(Button::Select));
            assert_eq!(s.binding("key.playerlist"), None);
            assert_eq!(s.binding("key.advancements"), None);
        }
        let s = PadSettings::parse(&PadSettings::default().serialize());
        assert_eq!(s, PadSettings::default());
    }

    #[test]
    fn settings_round_trip_and_store_only_changes() {
        let mut s = PadSettings::default();
        assert!(!s.serialize().contains("bind_"));
        s.bind("key.jump", Some(Button::East));
        s.bind("key.drop", None);
        s.bind("key.sneak", Some(Button::RightStick));
        s.invert_y = true;
        s.look_sensitivity = 0.75;
        s.glyphs = GlyphStyle::PlayStation;
        s.nintendo_letters = !s.nintendo_letters;
        s.guide_detail = GuideDetail::Basic;
        let text = s.serialize();
        assert!(text.contains(&format!("nintendoLetters:{}\n", s.nintendo_letters)));
        assert!(text.contains("bind_key.jump:east\n"));
        assert!(text.contains("bind_key.drop:none\n"));
        assert!(!text.contains("key.sneak"));
        let back = PadSettings::parse(&text);
        assert_eq!(back, s);
        assert_eq!(back.binding("key.drop"), None);
        assert_eq!(back.actions_of(Button::East), ["key.jump"]);
        // Garbage keeps the defaults.
        let odd = PadSettings::parse(
            "lookSensitivity:NaN\nbind_key.nope:south\nbind_key.jump:x9\nbindLayout:1\n",
        );
        assert_eq!(
            odd.look_sensitivity,
            PadSettings::default().look_sensitivity
        );
        assert_eq!(odd.binding("key.jump"), Some(Button::South));
        assert_eq!(odd.actions_of(Button::South), ["key.jump"]);
    }

    #[test]
    fn lines_this_game_doesnt_read_survive_a_save() {
        // A newer Horizon's setting, an action this game doesn't have, a button it doesn't know.
        let text =
            "bindLayout:2\nhapticStrength:0.4\nbind_horizon:dash:south\nbind_key.jump:paddle1\n";
        let s = PadSettings::parse(text);
        assert_eq!(
            s.binding("key.jump"),
            Some(Button::South),
            "the default meanwhile"
        );
        let saved = s.serialize();
        for line in [
            "hapticStrength:0.4\n",
            "bind_horizon:dash:south\n",
            "bind_key.jump:paddle1\n",
        ] {
            assert!(saved.contains(line), "{line} lost from\n{saved}");
        }
        assert_eq!(PadSettings::parse(&saved), s);
        // Bound here since, the action's own line is the one written.
        let mut s = s;
        s.bind("key.jump", Some(Button::West));
        let saved = s.serialize();
        assert!(!saved.contains("paddle1"), "{saved}");
        assert_eq!(
            PadSettings::parse(&saved).binding("key.jump"),
            Some(Button::West)
        );
    }

    #[test]
    fn a_newer_bind_layout_is_kept() {
        let newer = BIND_LAYOUT + 1;
        let s = PadSettings::parse(&format!("bindLayout:{newer}\nvibration:false\n"));
        assert!(s.serialize().contains(&format!("bindLayout:{newer}\n")));
        let s = PadSettings::parse("bindLayout:1\nvibration:false\n");
        assert!(
            s.serialize()
                .contains(&format!("bindLayout:{BIND_LAYOUT}\n"))
        );
    }

    #[test]
    fn ultimate2_right_stick_on_z_without_sdl() {
        let (left, right) = stick_axes(
            [0.2, 0.3],
            [0.7, -0.9],
            -0.5,
            Some(BITDO_ULTIMATE2_VID_PID.0),
            Some(BITDO_ULTIMATE2_VID_PID.1),
            false,
        );
        assert_eq!(left, [0.2, 0.3]);
        assert_eq!(
            right,
            [-0.5, -0.7],
            "X from LeftZ, Y from RightStickX, turned over"
        );
        let kept = stick_axes(
            [0.2, 0.3],
            [0.7, -0.9],
            -0.5,
            Some(BITDO_ULTIMATE2_VID_PID.0),
            Some(BITDO_ULTIMATE2_VID_PID.1),
            true,
        );
        assert_eq!(kept, ([0.2, 0.3], [0.7, -0.9]), "SDL row already remapped");
        let other = stick_axes(
            [0.0, 0.0],
            [1.0, 0.0],
            0.5,
            Some(0x045e),
            Some(0x028e),
            false,
        );
        assert_eq!(other, ([0.0, 0.0], [1.0, 0.0]));
    }

    #[test]
    fn a_nintendo_pad_by_its_letters_confirms_with_a() {
        // The pad's A sits on the right (East): going by letters it plays South, confirm and jump.
        let mut pad = PadState::default();
        pad.set(Button::East, true);
        pad.set(Button::LeftBumper, true);
        pad.left = [0.5, -0.25];
        let parts = pad.by_letter();
        assert!(parts.held(Button::South));
        assert!(!parts.held(Button::East));
        // Other buttons and the sticks stay; reading by letters again gives the pad back.
        assert!(parts.held(Button::LeftBumper));
        assert_eq!(parts.left, [0.5, -0.25]);
        assert_eq!(parts.by_letter(), pad);
    }

    #[test]
    fn a_stick_holds_the_keys_of_its_nearest_heading() {
        // Forward, backward, left, right, by the heading counted from forward toward left.
        let headings = [
            [true, false, false, false],
            [true, false, true, false],
            [false, false, true, false],
            [false, true, true, false],
            [false, true, false, false],
            [false, true, false, true],
            [false, false, false, true],
            [true, false, false, true],
        ];
        for tenth in 0..3600 {
            let degrees = tenth as f32 / 10.0;
            if degrees % 45.0 == 22.5 {
                continue;
            }
            for tilt in [0.31, 0.5, 1.0] {
                let (s, c) = degrees.to_radians().sin_cos();
                let keys = eight_direction_keys([tilt * s, tilt * c]);
                let sector = ((degrees / 45.0).round() as usize) % 8;
                assert_eq!(keys, headings[sector], "{degrees} at {tilt}");
                let [left, forward] =
                    crate::input::keyboard_direction(keys[0], keys[1], keys[2], keys[3]);
                let off = (left.atan2(forward).to_degrees() - degrees).rem_euclid(360.0);
                assert!(off.min(360.0 - off) <= 22.5 + 1e-3, "{degrees}: {off}");
            }
        }
    }

    #[test]
    fn a_sector_edge_goes_to_the_straight_heading() {
        assert_eq!(
            eight_direction_keys([0.414_213_57, 1.0]),
            [true, false, false, false]
        );
        assert_eq!(
            eight_direction_keys([0.4143, 1.0]),
            [true, false, true, false]
        );
        assert_eq!(
            eight_direction_keys([1.0, 0.414_213_57]),
            [false, false, true, false]
        );
    }

    #[test]
    fn a_small_tilt_holds_no_key() {
        assert_eq!(eight_direction_keys([0.0, 0.0]), [false; 4]);
        assert_eq!(eight_direction_keys([0.2, 0.2]), [false; 4]);
        assert_eq!(
            eight_direction_keys([0.0, 0.31]),
            [true, false, false, false]
        );
    }

    #[test]
    fn radial_deadzone_rescales() {
        assert_eq!(radial([0.1, 0.05], 0.15), [0.0, 0.0]);
        let full = radial([1.0, 0.0], 0.15);
        assert!((full[0] - 1.0).abs() < 1e-6);
        let half = radial([0.0, 0.575], 0.15);
        assert!((half[1] - 0.5).abs() < 1e-6);
        // A worn stick reporting more than 1 still caps at 1.
        let over = radial([1.0, 1.0], 0.15);
        assert!((length(over) - 1.0).abs() < 1e-6);
        assert_eq!(move_vector([0.0, 0.0], 0.15), None);
        let m = move_vector([1.0, 0.0], 0.15).unwrap();
        assert!(m[0] < -0.99 && m[1] == 0.0, "right is negative left");
    }

    #[test]
    fn look_turns_right_and_up() {
        let s = PadSettings::default();
        let (yaw, pitch) = look_delta([1.0, 0.0], &s, 1.0, false);
        assert!((yaw - YAW_SPEED).abs() < 1e-3 && pitch == 0.0);
        let (_, pitch) = look_delta([0.0, 1.0], &s, 0.5, false);
        assert!((pitch + PITCH_SPEED * 0.5).abs() < 1e-3, "up looks up");
        let inverted = PadSettings {
            invert_y: true,
            ..PadSettings::default()
        };
        assert!(look_delta([0.0, 1.0], &inverted, 1.0, false).1 > 0.0);
        let (small, _) = look_delta([0.5, 0.0], &s, 1.0, false);
        assert!(
            small < YAW_SPEED * 0.5,
            "the curve is gentler near the centre"
        );
        assert_eq!(look_delta([0.05, 0.05], &s, 1.0, false), (0.0, 0.0));
        assert!(look_delta([1.0, 0.0], &s, 1.0, true).0 < YAW_SPEED * 0.5);
    }

    #[test]
    fn edges_with_trigger_hysteresis() {
        let mut edges = Edges::default();
        let mut state = PadState::default();
        state.set(Button::South, true);
        assert_eq!(edges.update(&state, 0.4), [(Button::South, true)]);
        assert!(edges.update(&state, 0.4).is_empty());
        state.triggers[1] = 0.45;
        assert_eq!(edges.update(&state, 0.4), [(Button::RightTrigger, true)]);
        state.triggers[1] = 0.35;
        assert!(
            edges.update(&state, 0.4).is_empty(),
            "inside the hysteresis"
        );
        state.triggers[1] = 0.2;
        state.set(Button::South, false);
        assert_eq!(
            edges.update(&state, 0.4),
            [(Button::South, false), (Button::RightTrigger, false)]
        );
        state.set(Button::DPadUp, true);
        edges.update(&state, 0.4);
        assert_eq!(edges.release_all(), [(Button::DPadUp, false)]);
    }

    #[test]
    fn merge_and_activity() {
        let s = PadSettings::default();
        let mut a = PadState::default();
        assert!(!a.active(&s));
        a.left = [0.1, 0.0];
        assert!(!a.active(&s), "drift inside the deadzone");
        let mut b = PadState::default();
        b.set(Button::North, true);
        b.left = [0.95, 0.0];
        let m = a.merge(&b);
        assert!(m.held(Button::North) && m.left[0] == 1.0 && m.active(&s));
    }

    #[test]
    fn focus_moves_between_slots() {
        // A 3x2 grid of slots 18 apart and a button below it.
        let mut targets: Vec<(f32, f32)> = (0..6)
            .map(|i| (8.0 + (i % 3) as f32 * 18.0, 8.0 + (i / 3) as f32 * 18.0))
            .collect();
        targets.push((26.0, 80.0));
        let from = targets[0];
        assert_eq!(focus_step(from, Button::DPadRight, &targets), Some(1));
        assert_eq!(focus_step(from, Button::DPadDown, &targets), Some(3));
        assert_eq!(focus_step(from, Button::DPadUp, &targets), None);
        assert_eq!(focus_step(from, Button::DPadLeft, &targets), None);
        assert_eq!(focus_step(targets[4], Button::DPadDown, &targets), Some(6));
        // The far end of a row skips nothing in between.
        assert_eq!(focus_step(targets[2], Button::DPadLeft, &targets), Some(1));
        assert_eq!(nearest((12.0, 10.0), &targets, 9.0), Some(0));
        assert_eq!(nearest((100.0, 100.0), &targets, 9.0), None);
        let (x, y) = cursor_delta([0.0, 1.0], 0.15, 0.1);
        assert!(x == 0.0 && y < -30.0, "up moves up the screen");
        assert_eq!(cursor_delta([0.1, 0.0], 0.15, 0.1), (0.0, 0.0));
    }

    #[test]
    fn focus_follows_vanilla_arrow_navigation() {
        // An options screen: two columns of 150-wide buttons 20 high with
        // 4 pixels between rows, a full-width Done button under them.
        let mut rects: Vec<Rect> = (0..6)
            .map(|i| {
                let (col, row) = (i % 2, i / 2);
                [
                    5.0 + col as f32 * 160.0,
                    40.0 + row as f32 * 24.0,
                    150.0,
                    20.0,
                ]
            })
            .collect();
        rects.push([85.0, 120.0, 150.0, 20.0]);
        assert_eq!(
            focus_step_rect(rects[0], Button::DPadRight, &rects),
            Some(1)
        );
        assert_eq!(focus_step_rect(rects[1], Button::DPadRight, &rects), None);
        assert_eq!(focus_step_rect(rects[0], Button::DPadDown, &rects), Some(2));
        assert_eq!(focus_step_rect(rects[3], Button::DPadUp, &rects), Some(1));
        assert_eq!(focus_step_rect(rects[0], Button::DPadUp, &rects), None);
        // Done overlaps both columns: down from either bottom button lands
        // on it, up from it picks the leftmost overlapping one.
        assert_eq!(focus_step_rect(rects[5], Button::DPadDown, &rects), Some(6));
        assert_eq!(focus_step_rect(rects[4], Button::DPadDown, &rects), Some(6));
        assert_eq!(focus_step_rect(rects[6], Button::DPadUp, &rects), Some(4));
        // Nothing overlaps on the other axis: the vague pass takes the
        // nearest facing edge (a crafting result right of the grid).
        let grid: Vec<Rect> = vec![
            [10.0, 10.0, 16.0, 16.0],
            [28.0, 10.0, 16.0, 16.0],
            [10.0, 28.0, 16.0, 16.0],
            [28.0, 28.0, 16.0, 16.0],
            [80.0, 50.0, 16.0, 16.0],
        ];
        assert_eq!(focus_step_rect(grid[1], Button::DPadRight, &grid), Some(4));
        assert_eq!(focus_step_rect(grid[4], Button::DPadLeft, &grid), Some(3));
        assert_eq!(focus_step_rect(grid[0], Button::DPadDown, &grid), Some(2));
        // The focused rectangle itself is never a step, even listed twice.
        let dup = vec![grid[0], grid[0]];
        assert_eq!(focus_step_rect(grid[0], Button::DPadRight, &dup), None);
        assert_eq!(focus_step_rect(grid[0], Button::South, &grid), None);
    }

    #[test]
    fn follow_scroll_keeps_rows_and_fixed_widgets() {
        // Rows 36 tall scrolled down 18: a fifth row comes into view, so +18
        // carries as many rows as the real -18. Back, below, stays.
        let back = [296.0, 212.0, 71.0, 20.0];
        let rows = |ys: &[f32]| -> Vec<(Rect, bool)> {
            let rows = ys.iter().map(|y| ([78.0, *y, 270.0, 36.0], true));
            rows.chain([(back, false)]).collect()
        };
        let rects = |t: &[(Rect, bool)]| -> Vec<Rect> { t.iter().map(|t| t.0).collect() };
        let before = rects(&rows(&[33.0, 69.0, 105.0, 141.0]));
        let after = rows(&[15.0, 51.0, 87.0, 123.0, 159.0]);
        assert_eq!(follow_scroll(before[1], true, &before, &after), Some(1));
        assert_eq!(follow_scroll(back, true, &before, &after), Some(5));
        // And back up.
        let up = rows(&[33.0, 69.0, 105.0, 141.0]);
        let was = rects(&after);
        assert_eq!(follow_scroll(was[2], false, &was, &up), Some(2));
        // Nothing moved, as on a screen with no list.
        let still = rows(&[33.0, 69.0, 105.0, 141.0]);
        assert_eq!(follow_scroll(before[1], true, &before, &still), None);
        // A sidebar's tabs spaced exactly like the list's shift stay put.
        let tab = |y: f32| [8.0, y, 112.0, 16.0];
        let with_tabs = |t: &[(Rect, bool)]| -> Vec<(Rect, bool)> {
            t.iter()
                .copied()
                .chain([(tab(100.0), true), (tab(118.0), true)])
                .collect()
        };
        let was = rects(&with_tabs(&rows(&[33.0, 69.0, 105.0, 141.0])));
        let now = with_tabs(&after);
        assert_eq!(follow_scroll(tab(118.0), true, &was, &now), Some(7));
    }

    #[test]
    fn follow_scroll_lands_on_the_nearest_row_when_its_row_leaves() {
        // The row at 33 left the top; the next one is at 51. Nothing fixed.
        let row = |y: f32| [78.0, y, 270.0, 36.0];
        let before = [row(33.0), row(69.0), row(105.0)];
        let after = [(row(51.0), true), (row(87.0), true)];
        assert_eq!(follow_scroll(row(33.0), true, &before, &after), Some(0));
        // The last row left the bottom: the list's new last row, not a fixed
        // widget of its kind (at 200) nearer where the row went.
        let fixed = [78.0, 200.0, 270.0, 36.0];
        let before = [row(15.0), row(51.0), row(87.0), row(159.0), fixed];
        let after = [
            (row(33.0), true),
            (row(69.0), true),
            (row(105.0), true),
            (fixed, true),
        ];
        assert_eq!(follow_scroll(row(159.0), false, &before, &after), Some(2));
        // Nothing of its kind left.
        let other = after.map(|t| (t.0, false));
        assert_eq!(follow_scroll(row(159.0), false, &before, &other), None);
    }

    #[test]
    fn refind_tabs_sliders_and_stick_steps() {
        let rects: Vec<Rect> = vec![[0.0, 0.0, 10.0, 10.0], [0.0, 20.0, 10.0, 10.0]];
        assert_eq!(refind([0.0, 20.0, 10.0, 10.0], &rects, 8.0), Some(1));
        // A list scrolled by 4 pixels keeps the row.
        assert_eq!(refind([0.0, 24.0, 10.0, 10.0], &rects, 8.0), Some(1));
        assert_eq!(refind([90.0, 90.0, 10.0, 10.0], &rects, 8.0), None);
        // Rows 24 apart scrolled up by 24 under a fixed button.
        let before: Vec<Rect> = (0..5)
            .map(|i| [10.0, 40.0 + 24.0 * i as f32, 100.0, 20.0])
            .chain([[10.0, 200.0, 50.0, 20.0]])
            .collect();
        let after: Vec<Rect> = (0..5)
            .map(|i| [10.0, 16.0 + 24.0 * i as f32, 100.0, 20.0])
            .chain([[10.0, 200.0, 50.0, 20.0]])
            .collect();
        assert_eq!(scroll_shift(&before, &after, true), Some(-24.0));
        // Rows this far apart line up the other way too; the direction is what tells them apart.
        assert_eq!(scroll_shift(&before, &after, false), Some(24.0));
        assert_eq!(scroll_shift(&before, &before, true), None);
        let tabs = [
            ([60.0, 0.0, 26.0, 32.0], false),
            ([0.0, 0.0, 26.0, 32.0], true),
            ([30.0, 0.0, 26.0, 32.0], false),
        ];
        assert_eq!(tab_step(&tabs, true), Some(2));
        assert_eq!(tab_step(&tabs, false), Some(0), "wraps to the last");
        assert_eq!(tab_step(&tabs[..1], true), None);
        assert_eq!(slider_x([14.0, 142.0], 0.0), 14.0);
        assert_eq!(slider_x([14.0, 142.0], 1.0), 156.0);
        assert!(slider_step(0) == 1.0 && slider_step(99) == 8.0);
        assert_eq!(stick_direction([0.9, 0.2], 0.5), Some(Button::DPadRight));
        assert_eq!(stick_direction([-0.2, -0.8], 0.5), Some(Button::DPadDown));
        assert_eq!(stick_direction([0.3, 0.3], 0.5), None);
        let s = PadSettings {
            keyboard: TextKeyboard::Horizon,
            ..PadSettings::default()
        };
        assert_eq!(
            PadSettings::parse(&s.serialize()).keyboard,
            TextKeyboard::Horizon
        );
        assert!(steam_keyboard(TextKeyboard::Steam) && !steam_keyboard(TextKeyboard::Horizon));
    }

    #[test]
    fn a_list_step_scrolls_rather_than_leave_a_list_that_can() {
        let rows = |n: usize| -> Vec<Rect> {
            (0..n)
                .map(|i| [100.0, 40.0 + i as f32 * 36.0, 270.0, 36.0])
                .collect()
        };
        let back: Rect = [150.0, 220.0, 150.0, 20.0];
        let search: Rect = [150.0, 10.0, 150.0, 20.0];
        let mut targets = rows(4);
        targets.extend([back, search]);
        let area = |up, down| ScrollArea {
            rect: [0.0, 40.0, 380.0, 144.0],
            amount: 0.0,
            up,
            down,
        };
        let (down, up) = (Button::DPadDown, Button::DPadUp);
        let step = |from, button, area: &[ScrollArea]| list_step(from, button, &targets, area);
        // A row below in view comes first.
        assert_eq!(step(targets[1], down, &[area(true, true)]), Some(2));
        // The last row in view: more below scrolls; none leaves.
        assert_eq!(step(targets[3], down, &[area(true, true)]), None);
        assert_eq!(step(targets[3], down, &[area(true, false)]), Some(4));
        // The first row in view, the same upwards.
        assert_eq!(step(targets[0], up, &[area(true, false)]), None);
        assert_eq!(step(targets[0], up, &[area(false, true)]), Some(5));
        // From outside the list it enters it, and no list means a plain step.
        assert_eq!(step(back, up, &[area(true, true)]), Some(3));
        assert_eq!(step(targets[3], down, &[]), Some(4));
    }

    #[test]
    fn a_list_step_shows_its_row_whole_and_starts_from_one_nearly_gone() {
        let area = |up, down| ScrollArea {
            rect: [0.0, 40.0, 380.0, 100.0],
            amount: 50.0,
            up,
            down,
        };
        let (down, up) = (Button::DPadDown, Button::DPadUp);
        // The next row is cut by the view's bottom: it scrolls first, unless that's the end.
        let targets: Vec<Rect> = vec![[100.0, 100.0, 270.0, 25.0], [100.0, 125.0, 270.0, 25.0]];
        assert_eq!(
            list_step(targets[0], down, &targets, &[area(true, true)]),
            None
        );
        assert_eq!(
            list_step(targets[0], down, &targets, &[area(true, false)]),
            Some(1)
        );
        // The same at its top.
        let targets: Vec<Rect> = vec![[100.0, 30.0, 270.0, 25.0], [100.0, 55.0, 270.0, 25.0]];
        assert_eq!(
            list_step(targets[1], up, &targets, &[area(true, true)]),
            None
        );
        // A focused row with its last pixels in view is still in the list: going up scrolls,
        // even with something above the list to go to.
        let row: Rect = [100.0, 18.0, 270.0, 25.0];
        let field: Rect = [150.0, 0.0, 150.0, 15.0];
        assert!(area(true, true).holds(&row));
        assert_eq!(list_step(row, up, &[row, field], &[area(true, true)]), None);
        // Above the view altogether, it isn't.
        assert!(!area(true, true).holds(&[100.0, 10.0, 270.0, 25.0]));
    }

    #[test]
    fn a_stick_holds_its_direction_until_it_lets_go_or_turns() {
        let step = 0.55;
        let (up, right) = (Some(Button::DPadUp), Some(Button::DPadRight));
        // Only a push past the threshold starts a step.
        assert_eq!(stick_direction_held([0.0, 0.5], step, None), None);
        assert_eq!(stick_direction_held([0.0, 0.6], step, None), up);
        // Wobbling on the threshold keeps it; falling back past the release lets go.
        assert_eq!(stick_direction_held([0.0, 0.5], step, up), up);
        assert_eq!(stick_direction_held([0.0, 0.3], step, up), None);
        // A diagonal doesn't flip it, but a clear turn does.
        assert_eq!(stick_direction_held([0.6, 0.55], step, up), up);
        assert_eq!(stick_direction_held([0.55, 0.6], step, right), right);
        assert_eq!(stick_direction_held([0.9, 0.3], step, up), right);
        // Straight across to the other side.
        assert_eq!(
            stick_direction_held([0.0, -0.9], step, up),
            Some(Button::DPadDown)
        );
        // Past the release on the far side, but not far enough for a step.
        assert_eq!(stick_direction_held([0.0, -0.45], step, up), None);
    }

    #[test]
    fn a_repeat_stops_where_the_slots_end() {
        let at = (100.0, 100.0);
        assert!(!repeat_crosses_slots(at, (118.0, 100.0), true));
        // The hotbar, over its small gap, is the same run of slots.
        assert!(!repeat_crosses_slots(at, (100.0, 122.0), true));
        // Another block of slots, or a button.
        assert!(repeat_crosses_slots(at, (100.0, 132.0), true));
        assert!(repeat_crosses_slots(at, (118.0, 100.0), false));
    }

    #[test]
    fn a_resize_keeps_the_same_widget() {
        let before: Vec<Rect> = vec![[10.0, 10.0, 20.0, 20.0], [100.0, 10.0, 20.0, 20.0]];
        let after: Vec<Rect> = vec![[300.0, 90.0, 20.0, 20.0], [400.0, 90.0, 20.0, 20.0]];
        let (old, new) = ((200.0, 100.0), (800.0, 400.0));
        // The same count: the same place in the draw order, however far it moved.
        assert_eq!(
            refind_resized(before[0], &before, &after, old, new),
            Some(0)
        );
        // A list showing more rows: the nearest to where it would be.
        let more: Vec<Rect> = vec![
            [30.0, 30.0, 20.0, 20.0],
            [395.0, 35.0, 20.0, 20.0],
            [700.0, 300.0, 20.0, 20.0],
        ];
        assert_eq!(refind_resized(before[1], &before, &more, old, new), Some(1));
        assert_eq!(refind_resized(before[0], &before, &[], old, new), None);
    }

    #[test]
    fn styles_resolve_and_conflicts() {
        assert_eq!(
            GlyphStyle::Auto.resolve(GlyphStyle::PlayStation),
            GlyphStyle::PlayStation
        );
        assert_eq!(
            GlyphStyle::Xbox.resolve(GlyphStyle::PlayStation),
            GlyphStyle::Xbox
        );
        assert_eq!(GlyphStyle::Auto.resolve(GlyphStyle::Auto), GlyphStyle::Xbox);
        let mut s = PadSettings::default();
        assert!(!s.conflicts("key.jump"));
        s.bind("key.drop", Some(Button::South));
        assert!(s.conflicts("key.jump") && s.conflicts("key.drop"));
        assert!(!s.is_default("key.drop") && s.is_default("key.jump"));
        assert_eq!(action_caption(HOTBAR_NEXT), "Hotbar: Next Slot");
        assert_eq!(action_caption("key.jump"), "key.jump");
    }

    #[test]
    fn health_watch_only_counts_damage_in_one_life() {
        let mut w = HealthWatch::default();
        assert_eq!(w.observe(Some(20.0)), None);
        assert_eq!(w.observe(Some(15.0)), Some(5.0));
        assert_eq!(w.observe(Some(18.0)), None, "healing");
        // A new player entity (world, respawn, server, dimension).
        w.forget();
        assert_eq!(w.observe(Some(6.0)), None);
        // No player in between (title, loading) forgets too.
        assert_eq!(w.observe(None), None);
        assert_eq!(w.observe(Some(2.0)), None);
        assert_eq!(w.observe(Some(1.0)), Some(1.0));
    }

    #[cfg(feature = "controllers")]
    #[test]
    fn triggers_feel_their_action() {
        use horizon_dualsense::{TRIGGER_OFF, trigger};
        let mut s = PadSettings::default();
        // Use is LT, attack RT by default.
        let [left, right] = s.trigger_effects(TriggerFeel::Draw(1.0), TriggerFeel::Texture(1.5));
        assert_eq!(left, trigger::feedback(2, 6));
        assert_eq!(right, trigger::vibration(1, 3, 30));
        assert_eq!(
            s.trigger_effects(TriggerFeel::Draw(0.0), TriggerFeel::Free),
            [trigger::feedback(2, 1), TRIGGER_OFF]
        );
        // The catch gives way in the zone where the default threshold (0.4) presses: weapon
        // 0x25, zones 2 and 4, strength 8 (stored 7) or 5 (stored 4).
        let catch = |strength| [0x25, 0x14, 0x00, strength, 0, 0, 0, 0, 0, 0, 0];
        assert_eq!(TriggerFeel::Click.effect(0.4), catch(7));
        assert_eq!(TriggerFeel::Boost.effect(0.4), catch(4));
        // A deeper threshold moves it: zones 5 and 7.
        assert_eq!(TriggerFeel::Click.effect(0.7)[1..3], [0xa0, 0x00]);
        assert_eq!(TriggerFeel::Click.effect(0.1), trigger::weapon(2, 3, 8));
        assert_eq!(TriggerFeel::Brace.effect(0.4), trigger::feedback(2, 3));
        assert_eq!(TriggerFeel::Locked.effect(0.4), trigger::feedback(1, 8));
        // Harder blocks buzz harder and slower; instant-break blocks don't.
        assert_eq!(
            TriggerFeel::Texture(0.5).effect(0.4),
            trigger::vibration(1, 2, 40)
        );
        assert_eq!(
            TriggerFeel::Texture(3.0).effect(0.4),
            trigger::vibration(1, 4, 20)
        );
        assert_eq!(
            TriggerFeel::Texture(50.0).effect(0.4),
            trigger::vibration(1, 5, 10)
        );
        assert_eq!(TriggerFeel::Texture(0.0).effect(0.4), TRIGGER_OFF);
        assert_eq!(TriggerFeel::Texture(-1.0).effect(0.4), TRIGGER_OFF);
        s.adaptive_triggers = false;
        assert_eq!(
            s.trigger_effects(TriggerFeel::Click, TriggerFeel::Boost),
            [TRIGGER_OFF; 2]
        );
        assert!(!PadSettings::parse(&s.serialize()).adaptive_triggers);
    }

    #[test]
    fn lightbar_colours() {
        let full = Lightbar::Health.colour(1.0, "minecraft:overworld").unwrap();
        let empty = Lightbar::Health.colour(0.0, "minecraft:overworld").unwrap();
        assert!(full[1] > full[0], "green at full health: {full:?}");
        assert!(empty[0] > empty[1], "red when dying: {empty:?}");
        assert_eq!(Lightbar::Health.colour(0.5, ""), Some([123, 79, 6]));
        assert_ne!(
            Lightbar::Dimension.colour(1.0, "minecraft:the_nether"),
            Lightbar::Dimension.colour(1.0, "minecraft:overworld")
        );
        assert_eq!(Lightbar::Off.colour(1.0, ""), None);
        for l in Lightbar::ALL {
            assert_eq!(Lightbar::parse(l.name()), l);
        }
    }

    #[test]
    fn bow_draw() {
        assert_eq!(bow_power(0), 0.0);
        assert!((bow_power(10) - 5.0 / 12.0).abs() < 1e-6);
        assert_eq!(bow_power(20), 1.0);
        assert_eq!(bow_power(40), 1.0);
    }

    #[test]
    fn rumble_pulses() {
        let small = Rumble::Hurt(1.0).pulse().unwrap();
        let big = Rumble::Hurt(10.0).pulse().unwrap();
        assert!(big.strong > small.strong && big.millis > small.millis);
        assert!(Rumble::Hurt(100.0).pulse().unwrap().strong <= 0.8);
        assert_eq!(Rumble::Hurt(0.0).pulse(), None);
        let near = Rumble::Explosion {
            distance: 2.0,
            radius: 4.0,
        }
        .pulse()
        .unwrap();
        let far = Rumble::Explosion {
            distance: 12.0,
            radius: 4.0,
        }
        .pulse()
        .unwrap();
        assert!(near.strong > far.strong);
        assert_eq!(
            Rumble::Explosion {
                distance: 20.0,
                radius: 4.0
            }
            .pulse(),
            None
        );
        assert_eq!(Rumble::BlockBroken.pulse().unwrap().strong, 0.0);
        // Shapes open at the pulse's strength and fade; nothing for what isn't felt.
        for rumble in [
            Rumble::Hurt(4.0),
            Rumble::Explosion {
                distance: 2.0,
                radius: 4.0,
            },
        ] {
            let (pulse, shape) = (rumble.pulse().unwrap(), rumble.shape());
            assert_eq!(shape[0].strong, pulse.strong);
            assert!(shape.windows(2).all(|w| w[1].strong < w[0].strong));
        }
        let hurt: u32 = Rumble::Hurt(4.0).shape().iter().map(|s| s.millis).sum();
        assert!(hurt.abs_diff(Rumble::Hurt(4.0).pulse().unwrap().millis) <= 1);
        assert_eq!(
            Rumble::Totem.shape().iter().map(|s| s.millis).sum::<u32>(),
            500
        );
        assert_eq!(Rumble::BlockBroken.shape()[0].millis, 35);
        assert!(Rumble::Hurt(0.0).shape().is_empty());
    }

    #[test]
    fn glyphs_follow_the_vendor() {
        let of = |vendor, product| GlyphStyle::of_device(Some(vendor), Some(product));
        // Any product of the consoles' makers.
        assert_eq!(of(0x054c, 0x0ce6), GlyphStyle::PlayStation);
        assert_eq!(of(0x054c, 0x0001), GlyphStyle::PlayStation);
        assert_eq!(of(0x057e, 0x2009), GlyphStyle::Nintendo);
        // A pad made for a console by another maker, and that maker's others.
        assert_eq!(of(0x0f0d, 0x0066), GlyphStyle::PlayStation);
        assert_eq!(of(0x0f0d, 0x00c1), GlyphStyle::Nintendo);
        assert_eq!(of(0x0f0d, 0x0067), GlyphStyle::Xbox);
        assert_eq!(of(0x20d6, 0xa711), GlyphStyle::Nintendo);
        // Valve's virtual pad, Microsoft's, nothing known, nothing reported.
        assert_eq!(of(0x28de, 0x11ff), GlyphStyle::Xbox);
        assert_eq!(of(0x045e, 0x0b12), GlyphStyle::Xbox);
        assert_eq!(of(0x1234, 0x5678), GlyphStyle::Xbox);
        assert_eq!(GlyphStyle::of_device(None, None), GlyphStyle::Xbox);
        // A vendor alone is enough only for the consoles' makers.
        assert_eq!(
            GlyphStyle::of_device(Some(0x054c), None),
            GlyphStyle::PlayStation
        );
        assert_eq!(GlyphStyle::of_device(Some(0x0f0d), None), GlyphStyle::Xbox);
    }
}
