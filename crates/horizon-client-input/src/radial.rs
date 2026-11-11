//! The radial menu (Horizon's own; vanilla 26.3 has no gamepad input; docs/controllers.md): a
//! player's named pages of eight fixed slots, saved in `horizon-controller.txt`, and what the
//! opener, the selecting stick (the right one unless the player picks another) and the bumpers do
//! while it is open. Nothing here draws or acts: the
//! game feeds it the pad, and resolves the slot `RadialMenu::commit` gives back through its pages.
//!
//! Every tuning value of how the menu behaves and feels is here, at the top: tilts, timings and
//! the haptic pulses. How it looks (sizes, colours, motion) is `radial_overlay`'s, in its own
//! block of constants.
use crate::gamepad::Pulse;
use std::time::{Duration, Instant};

/// The tick as the stick reaches a slot: the light motor alone, briefly, felt rather than heard,
/// but firm enough to feel through a flick.
pub const RADIAL_TICK: Pulse = Pulse {
    strong: 0.0,
    weak: 0.3,
    millis: 45,
};
/// A page turn's tick: shorter than a slot's.
pub const RADIAL_PAGE_TICK: Pulse = Pulse {
    strong: 0.0,
    weak: 0.3,
    millis: 30,
};
/// How long after a page turn's tick the second one comes when the pages went round an end.
pub const WRAP_TICK_GAP: Duration = Duration::from_millis(90);
/// A release that ran nothing: both motors, softly and a little longer, unlike any tick.
pub const RADIAL_NOTHING: Pulse = Pulse {
    strong: 0.15,
    weak: 0.1,
    millis: 110,
};

/// The slots of a page, clockwise from the top.
pub const SLOTS: usize = 8;
/// How long a selection outlives a stick let go of: the stick springs back before the thumb on
/// the opener comes up. A first tuning value, to be checked by playing.
pub const NEUTRAL_RETENTION: Duration = Duration::from_millis(250);
/// How far the stick must lean to point at a slot at the deadzone the tilts were tuned at
/// (`TUNED_DEADZONE`); less is neutral. Other deadzones rescale it ([`Tilt`]).
pub const SELECT_TILT: f32 = 0.5;
/// A selected slot stays selected down to this share of the tilt, and this far (in radians) past
/// its wedge's edge, so a light hold or a stick on a boundary doesn't flicker. First tuning values.
const KEEP_TILT: f32 = 0.7;
const KEEP_MARGIN: f32 = 0.12;
/// A stick under this (or inside the deadzone) is at rest: one that had been leaning for a while
/// when the menu opened selects nothing until it has been let go.
const REST_TILT: f32 = 0.25;
/// The look deadzone the tilts above were tuned at: the default one.
pub const TUNED_DEADZONE: f32 = 0.12;
/// The most the stick must lean to select, whatever the deadzone: a worn stick that no longer
/// reaches its rim still reaches every slot.
pub const SELECT_TILT_MAX: f32 = 0.93;
/// How far over the deadzone a selection is let go of, at least: a stick drifting inside the
/// deadzone never keeps one.
pub const KEEP_OVER_DEADZONE: f32 = 0.03;
/// How long the stick must have leant before the opener went down for the menu to open waiting
/// for it to be let go: it was turning the camera. A stick that moved only just before is a flick
/// aimed at the menu, a thumb rolling onto both at once.
pub const LEAN_BEFORE_OPEN: Duration = Duration::from_millis(90);

/// The stick's lengths that select, keep a selection and are at rest, for a look deadzone: past
/// the deadzone the tilts keep their share of the stick's travel, so a raised deadzone neither
/// keeps a drifting selection nor puts the slots out of reach.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tilt {
    pub select: f32,
    pub keep: f32,
    pub rest: f32,
}
impl Tilt {
    pub fn for_deadzone(deadzone: f32) -> Self {
        let deadzone = if deadzone.is_finite() {
            deadzone.clamp(0.0, 1.0)
        } else {
            0.0
        };
        let travel = (1.0 - deadzone) / (1.0 - TUNED_DEADZONE);
        let at = |tuned: f32| deadzone + (tuned - TUNED_DEADZONE) * travel;
        let select = at(SELECT_TILT).min(SELECT_TILT_MAX);
        let keep = at(SELECT_TILT * KEEP_TILT)
            .max(deadzone + KEEP_OVER_DEADZONE)
            .min(select);
        Self {
            select,
            keep,
            rest: REST_TILT.max(deadzone),
        }
    }
}

/// Toggle mode, A commits: a selection younger than this is the thumb sliding off the stick
/// towards A, which crosses a neighbouring slot, so the one before it is run. A first tuning value.
pub const A_SETTLE: Duration = Duration::from_millis(90);

/// How long the opener is held before the overlay shows: a quick tap draws nothing (and, having
/// selected nothing, does nothing). The stick is followed from the press. A first tuning value.
pub const DRAW_DELAY: Duration = Duration::from_millis(120);

/// What a slot can hold: actions that do everything on their press. Holding one (attack, jump,
/// the player list) means nothing for a release-to-commit menu. Anything else in a saved file
/// stays in its slot as a [`Slot::Missing`], which never runs: a slot can never run an arbitrary
/// command.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum RadialAction {
    Inventory,
    Chat,
    Command,
    Advancements,
    TogglePerspective,
    SwapOffhand,
    Drop,
    PickItem,
    Screenshot,
    ToggleGui,
    SmoothCamera,
    SocialInteractions,
    /// Horizon's emote wheel; it opens only with cosmetics on.
    Emote,
    HotbarPrevious,
    HotbarNext,
    Fullscreen,
    /// The server's Quick Actions dialog; it opens only when the server gave one.
    QuickActions,
    /// F3's debug screen.
    DebugOverlay,
}
/// Horizon's own actions are named in its namespace; vanilla's by their key mapping.
#[cfg(test)]
const HORIZON: &str = "horizon:";
/// Ids a slot was saved under before, and the action each is now.
const ALIASES: [(&str, RadialAction); 3] = [
    ("key.emote", RadialAction::Emote),
    (
        crate::gamepad::HOTBAR_PREVIOUS,
        RadialAction::HotbarPrevious,
    ),
    (crate::gamepad::HOTBAR_NEXT, RadialAction::HotbarNext),
];
impl RadialAction {
    /// Every action a slot can hold.
    pub const ALL: [Self; 18] = [
        Self::Inventory,
        Self::Chat,
        Self::Command,
        Self::Advancements,
        Self::TogglePerspective,
        Self::SwapOffhand,
        Self::Drop,
        Self::PickItem,
        Self::Screenshot,
        Self::ToggleGui,
        Self::SmoothCamera,
        Self::SocialInteractions,
        Self::Emote,
        Self::HotbarPrevious,
        Self::HotbarNext,
        Self::Fullscreen,
        Self::QuickActions,
        Self::DebugOverlay,
    ];
    /// Its id in the settings file: a vanilla key mapping's name (`key.chat`), or one of
    /// Horizon's own in its namespace (`horizon:emote`).
    pub fn id(self) -> &'static str {
        match self {
            Self::Emote => "horizon:emote",
            Self::HotbarPrevious => "horizon:hotbar_previous",
            Self::HotbarNext => "horizon:hotbar_next",
            other => other.action(),
        }
    }
    /// The bindable action it runs, as the binds list names it (`gamepad::route`).
    pub fn action(self) -> &'static str {
        match self {
            Self::Inventory => "key.inventory",
            Self::Chat => "key.chat",
            Self::Command => "key.command",
            Self::Advancements => "key.advancements",
            Self::TogglePerspective => "key.togglePerspective",
            Self::SwapOffhand => "key.swapOffhand",
            Self::Drop => "key.drop",
            Self::PickItem => "key.pickItem",
            Self::Screenshot => "key.screenshot",
            Self::ToggleGui => "key.toggleGui",
            Self::SmoothCamera => "key.smoothCamera",
            Self::SocialInteractions => "key.socialInteractions",
            Self::Emote => "key.emote",
            Self::HotbarPrevious => crate::gamepad::HOTBAR_PREVIOUS,
            Self::HotbarNext => crate::gamepad::HOTBAR_NEXT,
            Self::Fullscreen => "key.fullscreen",
            Self::QuickActions => "key.quickActions",
            Self::DebugOverlay => "key.debug.overlay",
        }
    }
    /// The action saved under `id`, its own or one it had before (`ALIASES`).
    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|action| action.id() == id)
            .or_else(|| {
                ALIASES
                    .into_iter()
                    .find(|(old, _)| *old == id)
                    .map(|(_, action)| action)
            })
    }
    /// One of Horizon's own, not a vanilla key mapping.
    #[cfg(test)]
    fn is_horizons(self) -> bool {
        self.id().starts_with(HORIZON)
    }
    /// Its short caption for a slot: a slot is narrow, and the binds list's names are long.
    pub fn caption_key(self) -> &'static str {
        match self {
            Self::Inventory => "horizon.radial.action.key_inventory",
            Self::Chat => "horizon.radial.action.key_chat",
            Self::Command => "horizon.radial.action.key_command",
            Self::Advancements => "horizon.radial.action.key_advancements",
            Self::TogglePerspective => "horizon.radial.action.key_togglePerspective",
            Self::SwapOffhand => "horizon.radial.action.key_swapOffhand",
            Self::Drop => "horizon.radial.action.key_drop",
            Self::PickItem => "horizon.radial.action.key_pickItem",
            Self::Screenshot => "horizon.radial.action.key_screenshot",
            Self::ToggleGui => "horizon.radial.action.key_toggleGui",
            Self::SmoothCamera => "horizon.radial.action.key_smoothCamera",
            Self::SocialInteractions => "horizon.radial.action.key_socialInteractions",
            Self::Emote => "horizon.radial.action.key_emote",
            Self::HotbarPrevious => "horizon.radial.action.pad_hotbarPrevious",
            Self::HotbarNext => "horizon.radial.action.pad_hotbarNext",
            Self::Fullscreen => "horizon.radial.action.key_fullscreen",
            Self::QuickActions => "horizon.radial.action.key_quickActions",
            Self::DebugOverlay => "horizon.radial.action.key_debug_overlay",
        }
    }
}

/// What a slot holds.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub enum Slot {
    #[default]
    Empty,
    Action(RadialAction),
    /// An id this game doesn't know (a newer Horizon's action, a mod's, a typo): kept as it was
    /// saved, shown as missing, and never run.
    Missing(String),
}
impl Slot {
    /// The slot a saved id reads as.
    fn read(id: &str) -> Self {
        match id.trim() {
            "" => Self::Empty,
            id => {
                RadialAction::from_id(id).map_or_else(|| Self::Missing(id.to_owned()), Self::Action)
            }
        }
    }
    /// What the settings file says for it: nothing for an empty slot.
    pub fn id(&self) -> &str {
        match self {
            Self::Empty => "",
            Self::Action(action) => action.id(),
            Self::Missing(id) => id,
        }
    }
    /// The action it runs, if it is one this game knows.
    pub fn action(&self) -> Option<RadialAction> {
        match self {
            Self::Action(action) => Some(*action),
            _ => None,
        }
    }
    pub fn is_empty(&self) -> bool {
        *self == Self::Empty
    }
}
impl From<Option<RadialAction>> for Slot {
    fn from(action: Option<RadialAction>) -> Self {
        action.map_or(Self::Empty, Self::Action)
    }
}
/// A slot of a page that has none of its own (`PageKind::Hotbar`).
static EMPTY: Slot = Slot::Empty;

const GENERAL_PAGE_NAME: &str = "horizon.radial.page.general";
const CAMERA_PAGE_NAME: &str = "horizon.radial.page.camera";

/// What a page holds.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PageKind {
    /// Eight slots the player fills.
    Fixed([Slot; SLOTS]),
    /// The hotbar's items around the ring. Read and kept, not yet drawn or run: its slots are
    /// empty here.
    Hotbar,
}

/// One of the pages Horizon gives every player, by the id the settings file names it with. A
/// page the player hasn't changed is saved as its preset, so it follows the defaults as they
/// improve (docs/controllers.md).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Preset {
    General,
    Camera,
}
impl Preset {
    pub const ALL: [Self; 2] = [Self::General, Self::Camera];
    fn id(self) -> &'static str {
        match self {
            Self::General => "general",
            Self::Camera => "camera",
        }
    }
    fn of_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|preset| preset.id() == id)
    }
    /// The page as Horizon makes it now. Only actions with no default button that always run, so
    /// the menu adds to the pad rather than repeating it: the screens, then the camera, on the
    /// straight directions, the easiest to flick. `RadialAction::ALL` has the rest.
    fn page(self) -> Page {
        use RadialAction as A;
        let (name, [up, right, down]) = match self {
            Self::General => (
                GENERAL_PAGE_NAME,
                [A::Advancements, A::Command, A::SocialInteractions],
            ),
            Self::Camera => (
                CAMERA_PAGE_NAME,
                [A::Screenshot, A::ToggleGui, A::SmoothCamera],
            ),
        };
        let mut slots: [Slot; SLOTS] = Default::default();
        for (at, action) in [(0, up), (2, right), (4, down)] {
            slots[at] = Slot::Action(action);
        }
        Page {
            name: PageName::Builtin(name),
            kind: PageKind::Fixed(slots),
            preset: Some(self),
            origin: Some(self),
            extra: Vec::new(),
            overflow: Vec::new(),
        }
    }
    /// The preset whose name a page has: where it came from.
    fn named(name: &PageName) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|preset| PageName::Builtin(preset.page_name()) == *name)
    }
    fn page_name(self) -> &'static str {
        match self {
            Self::General => GENERAL_PAGE_NAME,
            Self::Camera => CAMERA_PAGE_NAME,
        }
    }
}

/// A named page of slots. Removing an action empties its slot and moves nothing else.
#[derive(Clone, Debug)]
pub struct Page {
    pub name: PageName,
    pub kind: PageKind,
    /// The preset it is, as it is now: saved as that, not as its slots.
    preset: Option<Preset>,
    /// The preset it came from (by its name): edited back to it, it is that preset again.
    origin: Option<Preset>,
    /// Its `radial_<n>_<field>` lines this game doesn't read, kept as they were.
    extra: Vec<(String, String)>,
    /// Slot ids past the eighth in its `slots` line (a newer Horizon's ring), kept as they were.
    overflow: Vec<String>,
}
/// What a page is, not where it came from.
impl PartialEq for Page {
    fn eq(&self, other: &Self) -> bool {
        (
            &self.name,
            &self.kind,
            self.preset,
            &self.extra,
            &self.overflow,
        ) == (
            &other.name,
            &other.kind,
            other.preset,
            &other.extra,
            &other.overflow,
        )
    }
}
impl Eq for Page {}
impl Page {
    /// The slot at `slot` (clockwise from the top).
    pub fn slot(&self, slot: usize) -> &Slot {
        match &self.kind {
            PageKind::Fixed(slots) => slots.get(slot).unwrap_or(&EMPTY),
            PageKind::Hotbar => &EMPTY,
        }
    }
    /// Its slots, clockwise from the top.
    pub fn slots(&self) -> [&Slot; SLOTS] {
        std::array::from_fn(|slot| self.slot(slot))
    }
    #[cfg(test)]
    fn preset(&self) -> Option<Preset> {
        self.preset
    }
}

/// Pages a file can hold that the menu still reads; more than the editor shows (`MAX_PAGES`) so
/// a file from a Horizon that shows more keeps them.
pub const PARSE_MAX: usize = 16;

/// The version of the default pages a file was written against (`radialLayout`). 1: the General
/// and Camera presets. A change to what a preset holds, or to the default page list, raises it.
/// A file without the line is read as layout 1, the first.
pub const RADIAL_LAYOUT: u32 = 1;
pub const LAYOUT_KEY: &str = "radialLayout";

/// A player's pages: at least one. The menu and the editor see the first `MAX_PAGES`; the rest
/// (from a file that has more) are kept, and come forward when a page before them goes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pages {
    pages: Vec<Page>,
    /// `radial_` lines no page of the first `PARSE_MAX` took, kept as they were.
    extra: Vec<(String, String)>,
    /// The newest `radialLayout` the file has been written against: this game's, or a newer
    /// Horizon's, whose number is kept so it doesn't read the file as older than it is.
    layout: u32,
}
impl Default for Pages {
    fn default() -> Self {
        Self {
            pages: Preset::ALL.map(Preset::page).to_vec(),
            extra: Vec::new(),
            layout: RADIAL_LAYOUT,
        }
    }
}
impl Pages {
    /// The pages the menu turns through.
    pub fn len(&self) -> usize {
        self.pages.len().min(MAX_PAGES)
    }
    /// Never true (there's always a page); clippy asks for it beside `len`.
    pub fn is_empty(&self) -> bool {
        self.pages.is_empty()
    }
    /// A page index that is one: the last page for one past the end (a page was deleted since).
    /// Every page index is clamped here.
    pub fn clamp(&self, page: usize) -> usize {
        page.min(self.len() - 1)
    }
    pub fn page(&self, page: usize) -> &Page {
        &self.pages[self.clamp(page)]
    }
    /// A newer Horizon wrote the file, whose presets may hold something else: a page is never
    /// turned into one of this game's presets, which would read there as its own preset.
    fn ahead(&self) -> bool {
        self.layout > RADIAL_LAYOUT
    }
}
/// The slot a stick points at: 0 is up, then clockwise. `None` under `tilt`.
pub fn slot_at(stick: [f32; 2], tilt: f32) -> Option<usize> {
    let length = stick_length(stick);
    if !length.is_finite() || length < tilt {
        return None;
    }
    let turns = stick[0].atan2(stick[1]) / std::f32::consts::TAU;
    Some((turns * SLOTS as f32).round().rem_euclid(SLOTS as f32) as usize % SLOTS)
}

/// `slot` is still what the stick points at, a little past its wedge and down to `keep`.
fn keeps_slot(stick: [f32; 2], keep: f32, slot: usize) -> bool {
    let length = stick_length(stick);
    if !length.is_finite() || length < keep {
        return false;
    }
    let tau = std::f32::consts::TAU;
    let wedge = tau / SLOTS as f32;
    let off = (stick[0].atan2(stick[1]) - slot as f32 * wedge).rem_euclid(tau);
    off.min(tau - off) <= wedge / 2.0 + KEEP_MARGIN
}

/// A stick's length: how far it leans.
pub fn stick_length([x, y]: [f32; 2]) -> f32 {
    (x * x + y * y).sqrt()
}

/// What the selecting stick did lately, whether the menu is open or not: how long it had leant
/// when the opener went down.
#[derive(Clone, Debug, Default)]
struct StickHistory(std::collections::VecDeque<(Instant, f32)>);
impl StickHistory {
    /// The stick's length at `now`. Kept: the newest sample at least `LEAN_BEFORE_OPEN` old, and
    /// those after it.
    fn push(&mut self, now: Instant, length: f32) {
        self.0.push_back((now, length));
        while self
            .0
            .get(1)
            .is_some_and(|(at, _)| *at + LEAN_BEFORE_OPEN <= now)
        {
            self.0.pop_front();
        }
    }
    /// The stick has leant past `rest` for all of `LEAN_BEFORE_OPEN` up to `now`.
    fn leant(&self, now: Instant, rest: f32) -> bool {
        self.0
            .front()
            .is_some_and(|(oldest, _)| *oldest + LEAN_BEFORE_OPEN <= now)
            && self.0.iter().all(|(_, length)| *length >= rest)
    }
}

/// The opener is down.
#[derive(Clone, Copy, Debug)]
struct Held {
    /// When the opener went down.
    since: Instant,
    /// The stick at the last frame.
    stick: [f32; 2],
    selected: Option<usize>,
    /// When the stick went neutral with a selection.
    neutral_since: Option<Instant>,
    /// The stick was leaning when the menu opened: it was turning the camera, so it selects nothing
    /// until it has been at rest.
    leaning: bool,
    /// The slot the stick pointed at when the page turned: it selects nothing until the stick moves
    /// off it, so a stick held through a page turn can't pick something the player never aimed for.
    ignored: Option<usize>,
    /// Something was selected during this hold.
    aimed: bool,
    /// Toggle mode: the opener came up and the menu stays, shown, until a commit or a cancel.
    latched: bool,
    /// When `selected` last changed, and what it was before, if that had stayed `A_SETTLE`.
    selected_at: Instant,
    settled: Option<usize>,
}
impl Held {
    fn new(since: Instant, stick: [f32; 2]) -> Self {
        Self {
            since,
            stick,
            selected_at: since,
            settled: None,
            selected: None,
            neutral_since: None,
            leaning: false,
            ignored: None,
            aimed: false,
            latched: false,
        }
    }
}

/// One hold of the opener, which always opens on the first page, so a flick aimed blind always
/// lands on the same slot. It knows pages only by their count and slots by their index: what a
/// slot holds is the caller's.
#[derive(Clone, Debug, Default)]
pub struct RadialMenu {
    page: usize,
    held: Option<Held>,
    /// The left stick's, then the right's.
    history: [StickHistory; 2],
}
impl RadialMenu {
    pub fn is_open(&self) -> bool {
        self.held.is_some()
    }
    pub fn page(&self) -> usize {
        self.page
    }
    /// The slot a release would commit.
    pub fn selected(&self) -> Option<usize> {
        self.held.and_then(|held| held.selected)
    }
    /// Something was selected since the opener went down, even if nothing is now.
    pub fn aimed(&self) -> bool {
        self.held.is_some_and(|held| held.aimed)
    }
    /// The stick selects nothing until it has been let go: it was leaning when the menu
    /// opened, or held through a page turn on the slot it pointed at.
    pub fn waiting(&self) -> bool {
        self.held
            .is_some_and(|held| held.leaning || held.ignored.is_some())
    }
    /// The stick to draw: where it leans, no further than the unit circle.
    pub fn stick(&self) -> [f32; 2] {
        let [x, y] = self.held.map_or([0.0; 2], |held| held.stick);
        let length = stick_length([x, y]);
        if !length.is_finite() {
            [0.0; 2]
        } else if length > 1.0 {
            [x / length, y / length]
        } else {
            [x, y]
        }
    }
    /// The overlay shows: the opener has been held `DRAW_DELAY`, or it stays open on its own.
    pub fn shown(&self, now: Instant) -> bool {
        self.held
            .is_some_and(|held| held.latched || now.duration_since(held.since) >= DRAW_DELAY)
    }
    /// Toggle mode: it stays open with the opener up.
    pub fn latched(&self) -> bool {
        self.held.is_some_and(|held| held.latched)
    }
    /// Both sticks (left, right) at `now`, every poll, open or not, before any press of the poll.
    pub fn sample(&mut self, sticks: [[f32; 2]; 2], now: Instant) {
        for (history, stick) in self.history.iter_mut().zip(sticks) {
            let length = stick_length(stick);
            history.push(now, if length.is_finite() { length } else { 0.0 });
        }
    }
    /// The opener went down: on the first page, selecting with `stick` (0 left, 1 right), now at
    /// `at`. A stick that has leant since well before the press waits to be let go.
    pub fn open(&mut self, stick: usize, at: [f32; 2], tilt: Tilt, now: Instant) {
        self.page = 0;
        let leant = self.history[stick.min(1)].leant(now, tilt.rest);
        self.held = Some(Held {
            leaning: slot_at(at, tilt.rest).is_some() && leant,
            ..Held::new(now, at)
        });
    }
    /// Either stick selecting: the other stick took over, one at rest since the open (the
    /// caller's rule). What the first stick was waiting for is its own, so the new one selects at
    /// once.
    pub fn took_over(&mut self) {
        if let Some(held) = &mut self.held {
            held.leaning = false;
            held.ignored = None;
        }
    }
    /// Toggle mode: the opener came up and the menu stays open, a selection with it.
    pub fn latch(&mut self) {
        if let Some(held) = &mut self.held {
            held.latched = true;
            held.neutral_since = None;
        }
    }
    /// The stick this frame. `now` is monotonic time.
    pub fn update(&mut self, stick: [f32; 2], tilt: Tilt, now: Instant) {
        let Some(held) = &mut self.held else {
            return;
        };
        held.stick = stick;
        // Let go is at rest (inside the player's deadzone too): a camera stick wobbling under
        // the select tilt is still leaning, and would select the moment it swung back out.
        if held.leaning {
            held.leaning = slot_at(stick, tilt.rest).is_some();
            return;
        }
        let kept = held
            .selected
            .filter(|slot| keeps_slot(stick, tilt.keep, *slot));
        let at = kept.or_else(|| slot_at(stick, tilt.select));
        if held.ignored.is_some() {
            if held.ignored == at {
                return;
            }
            held.ignored = None;
        }
        match at {
            Some(slot) => {
                if held.selected != Some(slot) {
                    if now.duration_since(held.selected_at) >= A_SETTLE {
                        held.settled = held.selected;
                    }
                    held.selected_at = now;
                }
                held.selected = Some(slot);
                held.aimed = true;
                held.neutral_since = None;
            }
            // Latched open (toggle mode) the stick can be let go: the selection stays until another
            // slot, a page turn or a cancel.
            None if held.latched => {}
            None => {
                if held.selected.is_some() {
                    held.neutral_since.get_or_insert(now);
                }
                held.expire(now);
            }
        }
    }
    /// The stick this frame, after `peak`, the furthest it leant since the last one: a flick out
    /// and back between two frames still selects (and the retention keeps it).
    pub fn update_with_peak(&mut self, stick: [f32; 2], peak: [f32; 2], tilt: Tilt, now: Instant) {
        if stick_length(peak) > stick_length(stick) {
            self.update(peak, tilt, now);
        }
        self.update(stick, tilt, now);
    }
    /// A bumper: the next or previous page, around the ends, with nothing selected; whether it
    /// went round an end, or `None` with one page, where there's nowhere to turn and the
    /// selection stays. A stick still leaning since the menu opened goes on waiting to be let go.
    pub fn turn_page(
        &mut self,
        forward: bool,
        stick: [f32; 2],
        tilt: Tilt,
        pages: usize,
    ) -> Option<bool> {
        let held = self.held.filter(|_| pages > 1)?;
        let next = (self.page + if forward { 1 } else { pages - 1 }) % pages;
        let wrapped = if forward {
            next < self.page
        } else {
            next > self.page
        };
        self.page = next;
        self.held = Some(Held {
            ignored: slot_at(stick, tilt.select),
            leaning: held.leaning,
            aimed: held.aimed,
            latched: held.latched,
            ..Held::new(held.since, stick)
        });
        Some(wrapped)
    }
    /// The opener came up: the menu closes, and the slot of `page()` to run once, if one is still
    /// selected. A second release, or one after `cancel`, gives nothing.
    pub fn commit(&mut self, now: Instant) -> Option<usize> {
        let mut held = self.held.take()?;
        if !held.latched {
            held.expire(now);
        }
        held.selected
    }
    /// Toggle mode, A pressed: as `commit`, but a selection made within `A_SETTLE` is the thumb
    /// sliding over to A, and the slot it left runs instead, if one had settled.
    pub fn commit_settled(&mut self, now: Instant) -> Option<usize> {
        let held = self.held?;
        let sliding = now.duration_since(held.selected_at) < A_SETTLE;
        let slot = self.commit(now)?;
        Some(if sliding {
            held.settled.unwrap_or(slot)
        } else {
            slot
        })
    }
    /// B, a disconnect, focus loss, a screen opening: the menu closes and nothing runs, however
    /// recently something was selected.
    pub fn cancel(&mut self) {
        self.held = None;
    }
}
impl Held {
    fn expire(&mut self, now: Instant) {
        if self
            .neutral_since
            .is_some_and(|since| now.duration_since(since) >= NEUTRAL_RETENTION)
        {
            self.selected = None;
            self.neutral_since = None;
        }
    }
}

/// After the menu closes on a D-pad opener, how long a press of another D-pad button is ignored:
/// a thumb rolling off the opener lands on its neighbour (Drop, Chat). It runs from the later of
/// the close and the opener's release: B can cancel with the opener still held, and toggle mode
/// commits on the opener's press, so the thumb rolls off after the close.
pub const DPAD_SETTLE: Duration = Duration::from_millis(100);

/// When the menu last closed on a D-pad opener, or the opener came up after that.
#[derive(Clone, Copy, Debug, Default)]
pub struct DpadSettle(Option<Instant>);
impl DpadSettle {
    pub fn closed(&mut self, now: Instant) {
        self.0 = Some(now);
    }
    /// The opener came up: after a close, the window runs from here.
    pub fn opener_up(&mut self, now: Instant) {
        if self.0.is_some() {
            self.0 = Some(now);
        }
    }
    /// A D-pad press starting at `now` is a rolled thumb: the opener is still held since the
    /// close (`opener_held`), or came up within `DPAD_SETTLE`. The opener's own press never is,
    /// so the menu opens again at once.
    pub fn quiet(&self, opener: bool, opener_held: bool, now: Instant) -> bool {
        !opener
            && self
                .0
                .is_some_and(|since| opener_held || now.duration_since(since) < DPAD_SETTLE)
    }
}

/// How long a selection stays before the narrator says it: a stick sweeping across slots says
/// only where it stops.
pub const NARRATE_SETTLE: Duration = Duration::from_millis(150);

/// What the narrator says of the selection, once it has settled.
#[derive(Clone, Copy, Debug)]
pub struct Settle<T> {
    pending: Option<(T, Instant)>,
    said: Option<T>,
}
impl<T> Default for Settle<T> {
    fn default() -> Self {
        Self {
            pending: None,
            said: None,
        }
    }
}
impl<T: Copy + PartialEq> Settle<T> {
    /// What is selected at `now`: given back once, when it has stayed `NARRATE_SETTLE` and wasn't
    /// the last said. Nothing selected forgets what was said, so aiming there again says it again.
    pub fn poll(&mut self, at: Option<T>, now: Instant) -> Option<T> {
        let Some(at) = at else {
            *self = Self::default();
            return None;
        };
        if self.said == Some(at) {
            return None;
        }
        match self.pending {
            Some((pending, since)) if pending == at => {
                if now.duration_since(since) < NARRATE_SETTLE {
                    return None;
                }
                self.pending = None;
                self.said = Some(at);
                Some(at)
            }
            _ => {
                self.pending = Some((at, now));
                None
            }
        }
    }
    /// Said already (a page turn says the page instead).
    pub fn reset(&mut self) {
        self.pending = None;
        self.said = None;
    }
}

/// As many pages as the settings' page list shows beside them on a 240-high GUI, clear of Undo.
pub const MAX_PAGES: usize = 6;
/// A page's name at most, in characters.
pub const NAME_MAX: usize = 32;
/// A new page's name, a language key like the default pages'.
pub const NEW_PAGE_NAME: &str = "horizon.radial.page.new";
/// The names Horizon gives pages, which follow the player's language.
const BUILTIN_NAMES: [&str; 3] = [GENERAL_PAGE_NAME, CAMERA_PAGE_NAME, NEW_PAGE_NAME];

/// A page's name: one Horizon gave it, a language key the screens translate, or the player's own
/// text, shown as typed even when it looks like a key.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PageName {
    Builtin(&'static str),
    Custom(String),
}
impl PageName {
    /// A name as it is kept: one line, not too long; nothing left is a new page's name.
    pub fn custom(name: &str) -> Self {
        let name: String = name
            .chars()
            .filter(|c| !c.is_control())
            .take(NAME_MAX)
            .collect();
        match name.trim() {
            "" => Self::Builtin(NEW_PAGE_NAME),
            name => Self::Custom(name.to_owned()),
        }
    }
    /// A name Horizon gives a page, by its language key.
    fn builtin(key: &str) -> Option<Self> {
        BUILTIN_NAMES
            .into_iter()
            .find(|name| *name == key)
            .map(Self::Builtin)
    }
}

impl Page {
    fn empty() -> Self {
        Self {
            name: PageName::Builtin(NEW_PAGE_NAME),
            kind: PageKind::Fixed(Default::default()),
            preset: None,
            origin: None,
            extra: Vec::new(),
            overflow: Vec::new(),
        }
    }
    /// The player changed it: it is saved by its slots, and a preset it names from a newer
    /// Horizon goes, which would otherwise take its place when read there.
    fn touched(&mut self) -> &mut Self {
        self.preset = None;
        self.extra.retain(|(field, _)| field != "preset");
        self
    }
    /// Edited back to what the preset it came from is now, it is that preset again, and goes on
    /// following the defaults; not in a file a newer Horizon wrote (`ahead`).
    fn settle(&mut self, ahead: bool) {
        // A preset this game doesn't know keeps its line, and only it says what the page is.
        if ahead || self.extra.iter().any(|(field, _)| field == "preset") {
            return;
        }
        if let Some(origin) = self.origin {
            let preset = origin.page();
            if (&self.name, &self.kind) == (&preset.name, &preset.kind) {
                self.preset = Some(origin);
            }
        }
    }
}

/// What the editor does to a player's pages.
impl Pages {
    /// Put an action in a slot, or empty it. A page without slots of its own gets them, empty.
    /// The same action again changes nothing.
    pub fn set_slot(&mut self, page: usize, slot: usize, to: Slot) {
        let page = self.clamp(page);
        let ahead = self.ahead();
        let page = &mut self.pages[page];
        let foreign = page.foreign_kind();
        if slot >= SLOTS
            || (!foreign && matches!(page.kind, PageKind::Fixed(_)) && *page.slot(slot) == to)
        {
            return;
        }
        let page = page.touched();
        if let PageKind::Hotbar = page.kind {
            page.kind = PageKind::Fixed(Default::default());
        }
        // A kind this game doesn't know showed as empty slots: filled here, it is a page of slots.
        page.extra
            .retain(|(field, _)| field != KIND_FIELD && field != SLOTS_FIELD);
        if let PageKind::Fixed(slots) = &mut page.kind {
            slots[slot] = to;
        }
        page.settle(ahead);
    }
    /// The same name again changes nothing.
    pub fn rename(&mut self, page: usize, name: &str) {
        let page = self.clamp(page);
        let ahead = self.ahead();
        let page = &mut self.pages[page];
        let name = PageName::custom(name);
        if page.name != name {
            page.touched().name = name;
            page.settle(ahead);
        }
    }
    /// Another empty page at the end of those shown; its index, or `None` at `MAX_PAGES`.
    pub fn add(&mut self) -> Option<usize> {
        (self.pages.len() < MAX_PAGES).then(|| {
            self.pages.push(Page::empty());
            self.pages.len() - 1
        })
    }
    /// Delete a page; the last one stays.
    pub fn remove(&mut self, page: usize) {
        if self.pages.len() > 1 && page < self.len() {
            self.pages.remove(page);
        }
    }
}

/// What the settings file does to a player's pages.
impl Pages {
    /// `radial_<page>_<field>` lines into a page list; `None` without any, for the default
    /// pages. Pages keep their order and gaps in the numbers close: a file numbered 0, 2 and 7
    /// keeps all three. The first `PARSE_MAX` are read; the lines of any after them are kept as
    /// they are, as are fields this game doesn't know.
    ///
    /// A page is its `preset` (the default page of that id as it is now), or its own: a name
    /// Horizon gave is written as its language key in `name`, the player's own text in `label`
    /// (so text that looks like a key stays theirs; a `name` that is no key of Horizon's leaves
    /// a new page's name), and its `slots` by id (`kind:hotbar` for a hotbar page).
    ///
    /// `radialLayout` is the layout the file was written against. One newer than this game's is
    /// kept, and its pages stay as saved; one older goes through `migrate`.
    pub fn parse<'a>(lines: impl Iterator<Item = (&'a str, &'a str)>) -> Option<Self> {
        let mut numbered = std::collections::BTreeMap::<usize, Vec<(&str, &str)>>::new();
        let mut extra = Vec::new();
        let mut saved_layout = None;
        for (key, value) in lines {
            if key == LAYOUT_KEY {
                // Unreadable is the first layout: nothing is lost, presets read as they are.
                saved_layout = Some(value.parse::<u32>().unwrap_or(1));
                continue;
            }
            let Some(rest) = key.strip_prefix("radial_") else {
                continue;
            };
            match rest
                .split_once('_')
                .and_then(|(index, field)| Some((index.parse::<usize>().ok()?, field)))
            {
                Some((index, field)) => numbered.entry(index).or_default().push((field, value)),
                None => extra.push((key.to_owned(), value.to_owned())),
            }
        }
        let from = saved_layout.unwrap_or(1);
        let layout = from.max(RADIAL_LAYOUT);
        if numbered.is_empty() && extra.is_empty() && layout == RADIAL_LAYOUT {
            return None;
        }
        let ahead = layout > RADIAL_LAYOUT;
        let mut pages = Vec::new();
        for (index, fields) in numbered {
            if pages.len() == PARSE_MAX {
                extra.extend(
                    fields.into_iter().map(|(field, value)| {
                        (format!("radial_{index}_{field}"), value.to_owned())
                    }),
                );
            } else {
                pages.push(Page::read(fields, ahead));
            }
        }
        // A newer Horizon that wrote only the layout (its pages were its defaults) has ours.
        if pages.is_empty() {
            pages = Self::default().pages;
        }
        let mut read = Self {
            pages,
            extra,
            layout,
        };
        read.migrate(from);
        Some(read)
    }
    /// What a file written against an older `radialLayout` needs to read as this one's. Layout 1
    /// is the first, so there is nothing yet. A change to a preset adds a step here for the
    /// layouts before it: a page saved by its slots that is that preset as it was then is the
    /// preset again (`settle` only knows the preset as it is now).
    fn migrate(&mut self, from: u32) {
        debug_assert!(from <= self.layout);
    }
    /// The lines `parse` reads, after the layout they were written against.
    pub fn serialize(&self) -> String {
        let mut text = format!("{LAYOUT_KEY}:{}\n", self.layout);
        let mut line = |key: &str, value: &str| text.push_str(&format!("{key}:{value}\n"));
        for (i, page) in self.pages.iter().enumerate() {
            let field = |name: &str| format!("radial_{i}_{name}");
            match (page.preset, &page.kind) {
                (Some(preset), _) => line(&field("preset"), preset.id()),
                (None, kind) => {
                    match &page.name {
                        PageName::Builtin(key) => line(&field("name"), key),
                        PageName::Custom(name) => line(&field("label"), name),
                    }
                    match kind {
                        // A kind this game doesn't know keeps its own lines (`extra`).
                        _ if page.foreign_kind() => {}
                        PageKind::Fixed(slots) => {
                            let ids: Vec<&str> = slots
                                .iter()
                                .map(Slot::id)
                                .chain(page.overflow.iter().map(String::as_str))
                                .collect();
                            line(&field(SLOTS_FIELD), &ids.join(","));
                        }
                        PageKind::Hotbar => line(&field(KIND_FIELD), HOTBAR_KIND),
                    }
                }
            }
            for (name, value) in &page.extra {
                line(&field(name), value);
            }
        }
        for (key, value) in &self.extra {
            line(key, value);
        }
        text
    }
}

/// `radial_<n>_kind` of a hotbar page.
const HOTBAR_KIND: &str = "hotbar";
/// A page's fields that say what it holds.
const KIND_FIELD: &str = "kind";
const SLOTS_FIELD: &str = "slots";

impl Page {
    /// Its `kind` is one this game doesn't know (a newer Horizon's): it shows empty, and its own
    /// lines say what it is until the player fills a slot.
    fn foreign_kind(&self) -> bool {
        self.extra.iter().any(|(field, _)| field == KIND_FIELD)
    }
    /// A page from its fields, in the file's order; `ahead` when a newer Horizon wrote them.
    fn read(fields: Vec<(&str, &str)>, ahead: bool) -> Self {
        let preset = fields
            .iter()
            .find(|(field, _)| *field == "preset")
            .and_then(|(_, id)| Preset::of_id(id));
        let mut page = preset.map_or_else(Page::empty, Preset::page);
        let settle = preset.is_none();
        let foreign = preset.is_none()
            && fields
                .iter()
                .any(|(field, value)| *field == KIND_FIELD && *value != HOTBAR_KIND);
        for (field, value) in fields {
            match field {
                // A known preset is the whole page; the fields it makes are its own.
                "preset" if preset.is_some() => {}
                "name" | "label" | "slots" | "kind" if preset.is_some() => {}
                "name" => {
                    if let Some(name) = PageName::builtin(value) {
                        page.name = name;
                    }
                }
                "label" => page.name = PageName::custom(value),
                SLOTS_FIELD if !foreign => {
                    let mut slots: [Slot; SLOTS] = Default::default();
                    let mut ids = value.split(',');
                    for (cell, id) in slots.iter_mut().zip(ids.by_ref()) {
                        *cell = Slot::read(id);
                    }
                    page.overflow = ids.map(|id| id.trim().to_owned()).collect();
                    // Trailing commas hold nothing.
                    if page.overflow.iter().all(String::is_empty) {
                        page.overflow.clear();
                    }
                    page.kind = PageKind::Fixed(slots);
                }
                "kind" if value == HOTBAR_KIND => page.kind = PageKind::Hotbar,
                _ => page.extra.push((field.to_owned(), value.to_owned())),
            }
        }
        // A page saved by its slots that is its default page as it is now (from a file before
        // presets, or edited back) follows the defaults again.
        if settle {
            page.origin = Preset::named(&page.name);
            page.settle(ahead);
        }
        page
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const UP: [f32; 2] = [0.0, 1.0];
    const RIGHT: [f32; 2] = [1.0, 0.0];
    const NEUTRAL: [f32; 2] = [0.0, 0.0];
    /// The default pages' count.
    const PAGES: usize = 2;

    fn tilt() -> Tilt {
        Tilt::for_deadzone(TUNED_DEADZONE)
    }
    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    /// Open with the stick at rest, then point it up.
    fn pointing_up(now: Instant) -> RadialMenu {
        let mut menu = RadialMenu::default();
        menu.open(1, NEUTRAL, tilt(), now);
        menu.update(UP, tilt(), now);
        menu
    }

    /// A menu opened with the stick leant at `stick` since well before the press, as when it
    /// was turning the camera.
    fn opened_leaning(stick: [f32; 2], now: Instant) -> RadialMenu {
        let mut menu = RadialMenu::default();
        let before = now - LEAN_BEFORE_OPEN - ms(20);
        for step in [0, 10, 20, 50, 80, 110] {
            menu.sample([NEUTRAL, stick], before + ms(step));
        }
        menu.open(1, stick, tilt(), now);
        menu
    }

    #[test]
    fn the_stick_points_at_eight_slots_clockwise_from_the_top() {
        let d = std::f32::consts::FRAC_1_SQRT_2;
        let expected = [
            ([0.0, 1.0], 0),
            ([d, d], 1),
            ([1.0, 0.0], 2),
            ([d, -d], 3),
            ([0.0, -1.0], 4),
            ([-d, -d], 5),
            ([-1.0, 0.0], 6),
            ([-d, d], 7),
        ];
        for (stick, slot) in expected {
            assert_eq!(slot_at(stick, SELECT_TILT), Some(slot), "{stick:?}");
        }
        // Just left of straight up is still the top slot, not an out-of-range ninth.
        assert_eq!(slot_at([-0.01, 1.0], SELECT_TILT), Some(0));
        assert_eq!(slot_at([0.2, 0.2], SELECT_TILT), None);
        assert_eq!(slot_at([f32::NAN, 1.0], SELECT_TILT), None);
    }

    #[test]
    fn a_release_commits_the_selection_once() {
        let now = Instant::now();
        let mut menu = pointing_up(now);
        assert_eq!(menu.commit(now), Some(0));
        assert!(!menu.is_open());
        assert_eq!(menu.commit(now), None, "one execution per selection");
    }

    #[test]
    fn the_overlay_waits_for_a_held_opener_but_the_stick_counts_from_the_press() {
        let t = Instant::now();
        let mut menu = RadialMenu::default();
        menu.open(1, NEUTRAL, tilt(), t);
        assert!(!menu.shown(t + DRAW_DELAY - ms(1)));
        assert!(menu.shown(t + DRAW_DELAY));
        menu.update(UP, tilt(), t + ms(10));
        assert_eq!(menu.selected(), Some(0), "aimed before it showed");
        // A quick tap: nothing shown, nothing selected, nothing run.
        let mut menu = RadialMenu::default();
        menu.open(1, NEUTRAL, tilt(), t);
        menu.update(NEUTRAL, tilt(), t + ms(30));
        assert_eq!(menu.commit(t + ms(40)), None);
        // Latched open (toggle mode), it shows at once.
        let mut menu = RadialMenu::default();
        menu.open(1, NEUTRAL, tilt(), t);
        menu.latch();
        assert!(menu.latched() && menu.shown(t));
    }

    #[test]
    fn the_stick_drawn_and_the_wait_for_it_to_be_let_go() {
        let t = Instant::now();
        let mut menu = RadialMenu::default();
        assert!(!menu.waiting());
        let mut menu_leaning = opened_leaning(UP, t);
        assert!(menu_leaning.waiting(), "leaning when it opened");
        menu_leaning.update([0.0, 2.0], tilt(), t);
        assert_eq!(menu_leaning.stick(), [0.0, 1.0], "no further than the edge");
        menu_leaning.update([0.3, 0.0], tilt(), t);
        assert_eq!(menu_leaning.stick(), [0.3, 0.0]);
        menu_leaning.update(NEUTRAL, tilt(), t);
        assert!(!menu_leaning.waiting());
        menu_leaning.update(UP, tilt(), t);
        menu_leaning.turn_page(true, UP, tilt(), PAGES);
        assert!(menu_leaning.waiting(), "held through the page turn");
        menu_leaning.update(RIGHT, tilt(), t);
        assert!(!menu_leaning.waiting());
        menu_leaning.cancel();
        assert!(!menu_leaning.waiting());
        menu.cancel();
    }

    #[test]
    fn a_flick_that_starts_with_the_press_selects_and_a_lean_from_before_waits() {
        let t = Instant::now();
        // The thumb rolls onto the opener as the other flicks: leant for less than the lean.
        let mut menu = RadialMenu::default();
        menu.sample([NEUTRAL, NEUTRAL], t - ms(200));
        menu.sample([NEUTRAL, NEUTRAL], t - ms(100));
        menu.sample([NEUTRAL, UP], t - ms(40));
        menu.sample([NEUTRAL, UP], t);
        menu.open(1, UP, tilt(), t);
        assert!(
            !menu.waiting(),
            "a flick with the press is aimed at the menu"
        );
        menu.update(UP, tilt(), t + ms(10));
        assert_eq!(menu.selected(), Some(0));
        // Leant since before: turning the camera.
        let mut menu = opened_leaning(UP, t);
        menu.update(UP, tilt(), t);
        assert_eq!(menu.selected(), None);
        // A stick let go in between counts from when it leant again.
        let mut menu = RadialMenu::default();
        menu.sample([NEUTRAL, UP], t - ms(300));
        menu.sample([NEUTRAL, UP], t - ms(150));
        menu.sample([NEUTRAL, NEUTRAL], t - ms(60));
        menu.sample([NEUTRAL, UP], t - ms(30));
        menu.open(1, UP, tilt(), t);
        assert!(!menu.waiting());
        // No history (the first poll): not leaning.
        let mut menu = RadialMenu::default();
        menu.open(1, UP, tilt(), t);
        assert!(!menu.waiting());
    }

    #[test]
    fn a_release_without_a_selection_does_nothing() {
        let mut menu = RadialMenu::default();
        menu.open(1, NEUTRAL, tilt(), Instant::now());
        menu.update(NEUTRAL, tilt(), Instant::now());
        assert!(!menu.aimed());
        assert_eq!(menu.commit(Instant::now()), None);
    }

    #[test]
    fn neutral_keeps_the_selection_until_the_retention_runs_out() {
        let t = Instant::now();
        let just_before = t + NEUTRAL_RETENTION - ms(1);

        let mut menu = pointing_up(t);
        menu.update(NEUTRAL, tilt(), t);
        menu.update(NEUTRAL, tilt(), just_before);
        assert_eq!(menu.selected(), Some(0));
        assert_eq!(menu.commit(just_before), Some(0));

        // Expired, seen by a frame: still aimed, for the buzz of a release that runs nothing.
        let mut menu = pointing_up(t);
        menu.update(NEUTRAL, tilt(), t);
        menu.update(NEUTRAL, tilt(), t + NEUTRAL_RETENTION);
        assert_eq!(menu.selected(), None);
        assert!(menu.aimed());
        assert_eq!(menu.commit(t + NEUTRAL_RETENTION), None);

        // Expired between frames: the release itself looks at the clock.
        let mut menu = pointing_up(t);
        menu.update(NEUTRAL, tilt(), t);
        assert_eq!(menu.commit(t + NEUTRAL_RETENTION), None);
    }

    #[test]
    fn a_flick_out_and_back_between_two_frames_selects() {
        let t = Instant::now();
        let mut menu = RadialMenu::default();
        menu.open(1, NEUTRAL, tilt(), t);
        menu.update_with_peak(NEUTRAL, RIGHT, tilt(), t + ms(30));
        assert_eq!(menu.selected(), Some(2), "the peak since the last frame");
        assert_eq!(menu.stick(), NEUTRAL, "the stick drawn where it is");
        assert_eq!(menu.commit(t + ms(60)), Some(2));
        // A peak no further than the stick changes nothing.
        let mut menu = pointing_up(t);
        menu.update_with_peak(UP, [0.1, 0.1], tilt(), t);
        assert_eq!(menu.selected(), Some(0));
    }

    #[test]
    fn pointing_again_restarts_the_retention() {
        let t = Instant::now();
        let mut menu = pointing_up(t);
        menu.update(NEUTRAL, tilt(), t);
        menu.update(RIGHT, tilt(), t + ms(200));
        menu.update(NEUTRAL, tilt(), t + ms(300));
        let later = t + ms(300) + NEUTRAL_RETENTION - ms(1);
        assert_eq!(menu.commit(later), Some(2));
    }

    #[test]
    fn cancelling_commits_nothing_even_during_retention() {
        let t = Instant::now();
        let mut menu = pointing_up(t);
        menu.update(NEUTRAL, tilt(), t);
        menu.cancel();
        assert!(!menu.is_open());
        // The opener is still down when B cancels; it comes up afterwards.
        assert_eq!(menu.commit(t + ms(1)), None);
    }

    #[test]
    fn a_page_turn_clears_the_selection_wraps_and_says_so() {
        let t = Instant::now();
        let mut menu = pointing_up(t);
        assert_eq!(menu.turn_page(false, UP, tilt(), PAGES), Some(true));
        assert_eq!((menu.page(), menu.selected()), (1, None));
        assert_eq!(menu.turn_page(true, UP, tilt(), PAGES), Some(true));
        assert_eq!(menu.page(), 0);
        assert_eq!(menu.turn_page(true, UP, tilt(), 3), Some(false));
        // During retention too.
        menu.update(RIGHT, tilt(), t);
        menu.update(NEUTRAL, tilt(), t);
        menu.turn_page(true, NEUTRAL, tilt(), 3);
        assert_eq!(menu.commit(t), None);
    }

    #[test]
    fn a_stick_held_through_a_page_turn_or_the_opening_selects_only_once_it_moves() {
        let t = Instant::now();
        let mut menu = pointing_up(t);
        menu.turn_page(true, UP, tilt(), PAGES);
        menu.update(UP, tilt(), t);
        assert_eq!(
            menu.selected(),
            None,
            "still pointing where the old page's slot was"
        );
        menu.update(RIGHT, tilt(), t);
        assert_eq!((menu.page(), menu.commit(t)), (1, Some(2)));

        // A stick turning the camera when the opener goes down.
        let mut menu = opened_leaning(UP, t);
        menu.update(UP, tilt(), t);
        assert_eq!(menu.selected(), None);
        menu.update(NEUTRAL, tilt(), t);
        menu.update(UP, tilt(), t);
        assert_eq!(menu.selected(), Some(0), "back to neutral, then aimed");

        // Arcing round to another slot, or leaning further, isn't aiming either.
        let mut menu = opened_leaning(UP, t);
        menu.update(RIGHT, tilt(), t);
        assert_eq!(menu.selected(), None, "arced to another slot");
        let mut menu = opened_leaning([0.0, 0.4], t);
        menu.update(UP, tilt(), t);
        assert_eq!(
            menu.selected(),
            None,
            "under the tilt at open, then past it"
        );
        // A drifting stick at rest doesn't count as leaning.
        let mut menu = opened_leaning([0.1, 0.1], t);
        menu.update(UP, tilt(), t);
        assert_eq!(menu.selected(), Some(0));
    }

    #[test]
    fn a_leaning_stick_waits_until_it_is_at_rest_not_only_under_the_select_tilt() {
        let t = Instant::now();
        let mut menu = opened_leaning(UP, t);
        // Wobbling between the rest and the select tilts: still turning the camera.
        menu.update([0.0, 0.45], tilt(), t + ms(10));
        assert!(menu.waiting(), "under the select tilt is not let go");
        menu.update(UP, tilt(), t + ms(20));
        assert_eq!(menu.selected(), None);
        menu.update(NEUTRAL, tilt(), t + ms(30));
        assert!(!menu.waiting());
        menu.update(UP, tilt(), t + ms(40));
        assert_eq!(menu.selected(), Some(0));
    }

    #[test]
    fn a_stick_leaning_since_the_opening_still_waits_after_a_page_turn() {
        let t = Instant::now();
        let mut menu = opened_leaning(UP, t);
        // Swung round to another slot and the page turned, without ever being let go.
        menu.update(RIGHT, tilt(), t);
        menu.turn_page(true, RIGHT, tilt(), PAGES);
        menu.update(UP, tilt(), t);
        assert!(menu.waiting());
        assert_eq!(menu.selected(), None, "aimed without having been let go");
        menu.update(NEUTRAL, tilt(), t);
        menu.update(UP, tilt(), t);
        assert_eq!(menu.selected(), Some(0));
    }

    #[test]
    fn a_selected_slot_holds_past_its_wedge_and_under_the_tilt() {
        let t = Instant::now();
        let at = |degrees: f32, length: f32| {
            let r = degrees.to_radians();
            [r.sin() * length, r.cos() * length]
        };
        let mut menu = pointing_up(t);
        // The boundary with the next slot is 22.5 degrees; the margin is about 6.9.
        menu.update(at(25.0, 1.0), tilt(), t);
        assert_eq!(menu.selected(), Some(0));
        menu.update(at(-25.0, 1.0), tilt(), t);
        assert_eq!(menu.selected(), Some(0), "the same on its other side");
        menu.update(at(35.0, 1.0), tilt(), t);
        assert_eq!(menu.selected(), Some(1));
        // Back near the boundary it stays on the new slot.
        menu.update(at(20.0, 1.0), tilt(), t);
        assert_eq!(menu.selected(), Some(1));
        // A light hold: 0.4 keeps it, under 0.35 is neutral, and 0.4 doesn't select anew.
        let mut menu = pointing_up(t);
        menu.update(at(0.0, 0.4), tilt(), t);
        menu.update(at(0.0, 0.4), tilt(), t + ms(300));
        assert_eq!(menu.selected(), Some(0));
        menu.update(at(0.0, 0.3), tilt(), t + ms(300));
        menu.update(at(0.0, 0.3), tilt(), t + ms(600));
        assert_eq!(menu.selected(), None);
        menu.update(at(0.0, 0.4), tilt(), t + ms(600));
        assert_eq!(menu.selected(), None);
    }

    #[test]
    fn the_tilts_follow_a_raised_deadzone_and_stay_in_reach() {
        let t = Instant::now();
        // A worn stick that reaches no further than this on any side.
        const WORN_REACH: f32 = 0.94;
        for deadzone in [0.1, 0.36, 0.5, 0.9] {
            let tilt = Tilt::for_deadzone(deadzone);
            assert!(
                tilt.select >= tilt.keep && tilt.keep >= deadzone + KEEP_OVER_DEADZONE,
                "{deadzone}: {tilt:?}"
            );
            assert!(tilt.select <= SELECT_TILT_MAX, "{deadzone}: {tilt:?}");
            // Every slot is reached by the worn stick.
            for slot in 0..SLOTS {
                let angle = (slot as f32 * 45.0).to_radians();
                let stick = [angle.sin() * WORN_REACH, angle.cos() * WORN_REACH];
                let mut menu = RadialMenu::default();
                menu.open(1, NEUTRAL, tilt, t);
                menu.update(stick, tilt, t);
                assert_eq!(menu.selected(), Some(slot), "{deadzone}: slot {slot}");
            }
            // A stick drifting just inside the deadzone lets go of the selection.
            let mut menu = RadialMenu::default();
            menu.open(1, NEUTRAL, tilt, t);
            menu.update(UP, tilt, t);
            let drift = [0.0, deadzone - 0.01];
            menu.update(drift, tilt, t);
            menu.update(drift, tilt, t + NEUTRAL_RETENTION);
            assert_eq!(menu.selected(), None, "{deadzone}: a drift kept it");
        }
        // At the default deadzone they are as they were tuned.
        let tuned = Tilt::for_deadzone(crate::gamepad::PadSettings::default().look_deadzone);
        assert!((tuned.select - SELECT_TILT).abs() < 1e-6);
        assert!((tuned.keep - SELECT_TILT * KEEP_TILT).abs() < 1e-6);
    }

    #[test]
    fn a_bumper_on_a_single_page_keeps_the_selection() {
        let t = Instant::now();
        let mut menu = RadialMenu::default();
        menu.open(1, NEUTRAL, tilt(), t);
        menu.update(UP, tilt(), t);
        assert_eq!(menu.turn_page(true, UP, tilt(), 1), None);
        assert_eq!((menu.page(), menu.selected()), (0, Some(0)));
    }

    #[test]
    fn every_hold_opens_on_the_first_page() {
        let mut menu = RadialMenu::default();
        menu.open(1, NEUTRAL, tilt(), Instant::now());
        menu.turn_page(true, NEUTRAL, tilt(), PAGES);
        assert_eq!(menu.page(), 1);
        menu.cancel();
        menu.open(1, NEUTRAL, tilt(), Instant::now());
        assert_eq!(menu.page(), 0);
    }

    #[test]
    fn a_d_pad_press_just_after_the_close_is_a_rolled_thumb_but_not_the_opener() {
        let t = Instant::now();
        let mut settle = DpadSettle::default();
        assert!(!settle.quiet(false, false, t));
        settle.closed(t);
        assert!(
            settle.quiet(false, false, t + ms(10)),
            "rolled onto the next button"
        );
        assert!(
            !settle.quiet(true, false, t + ms(10)),
            "the opener again opens the menu"
        );
        assert!(
            !settle.quiet(false, false, t + DPAD_SETTLE),
            "a press of its own"
        );
    }

    #[test]
    fn the_d_pad_settles_from_the_openers_release_after_a_cancel_with_it_held() {
        // B cancels at t with the opener held; the thumb lets it go 250 ms later and rolls on.
        let t = Instant::now();
        let mut settle = DpadSettle::default();
        settle.closed(t);
        let up = t + ms(250);
        assert!(settle.quiet(false, true, up - ms(1)), "still held");
        settle.opener_up(up);
        assert!(
            settle.quiet(false, false, up + ms(30)),
            "rolled off the opener"
        );
        assert!(!settle.quiet(false, false, up + DPAD_SETTLE));
        // A release with no close before it settles nothing.
        let mut fresh = DpadSettle::default();
        fresh.opener_up(up);
        assert!(!fresh.quiet(false, false, up + ms(1)));
    }

    #[test]
    fn latched_open_the_selection_outlives_the_stick() {
        let t = Instant::now();
        let mut menu = RadialMenu::default();
        menu.open(1, NEUTRAL, tilt(), t);
        menu.latch();
        menu.update(UP, tilt(), t);
        menu.update(NEUTRAL, tilt(), t + ms(10));
        let later = t + NEUTRAL_RETENTION * 4;
        menu.update(NEUTRAL, tilt(), later);
        assert_eq!(
            menu.selected(),
            Some(0),
            "let go, then A pressed a while later"
        );
        // Another slot, or a page turn, still changes it.
        menu.update(RIGHT, tilt(), later);
        assert_eq!(menu.selected(), Some(2));
        menu.turn_page(true, NEUTRAL, tilt(), PAGES);
        assert_eq!(menu.selected(), None);
        menu.update(UP, tilt(), later);
        menu.update(NEUTRAL, tilt(), later + ms(10));
        assert_eq!(menu.commit(later + NEUTRAL_RETENTION * 2), Some(0));
        // Selected while held, then latched: kept too.
        let mut menu = pointing_up(t);
        menu.update(NEUTRAL, tilt(), t + ms(5));
        menu.latch();
        assert_eq!(menu.commit(t + NEUTRAL_RETENTION * 2), Some(0));
    }

    #[test]
    fn each_stick_leans_on_its_own() {
        let t = Instant::now();
        let mut menu = RadialMenu::default();
        // The left stick walked all along; the right moved only with the press.
        for step in [200, 150, 100, 50] {
            menu.sample([UP, NEUTRAL], t - ms(step));
        }
        menu.sample([UP, UP], t);
        menu.open(1, UP, tilt(), t);
        assert!(!menu.waiting(), "the right stick's own history");
        menu.cancel();
        menu.open(0, UP, tilt(), t);
        assert!(menu.waiting(), "the left had been leaning");
    }

    #[test]
    fn a_stick_that_takes_over_selects_at_once() {
        // Either: opened on the walking left stick, which waits; the right flicks up and takes over.
        let t = Instant::now();
        let mut menu = RadialMenu::default();
        for step in [200, 150, 100, 50] {
            menu.sample([UP, NEUTRAL], t - ms(step));
        }
        menu.open(0, UP, tilt(), t);
        assert!(menu.waiting());
        menu.took_over();
        menu.update(RIGHT, tilt(), t + ms(30));
        assert_eq!(menu.selected(), Some(2));
        assert_eq!(menu.commit(t + ms(40)), Some(2));
    }

    #[test]
    fn toggle_s_a_runs_the_slot_settled_on_not_the_one_the_thumb_crossed_on_its_way() {
        let t = Instant::now();
        let mut menu = RadialMenu::default();
        menu.open(1, NEUTRAL, tilt(), t);
        menu.latch();
        menu.update(RIGHT, tilt(), t + ms(10));
        // Held on Right a while, then the thumb slides off down-right towards A.
        let down_right = [0.7, -0.7];
        menu.update(down_right, tilt(), t + ms(400));
        assert_eq!(menu.selected(), Some(3));
        assert_eq!(
            menu.commit_settled(t + ms(430)),
            Some(2),
            "the slide crossed Down-Right"
        );
        // Aimed and left there, A runs it.
        let mut menu = RadialMenu::default();
        menu.open(1, NEUTRAL, tilt(), t);
        menu.latch();
        menu.update(RIGHT, tilt(), t + ms(10));
        menu.update(down_right, tilt(), t + ms(400));
        assert_eq!(menu.commit_settled(t + ms(400) + A_SETTLE), Some(3));
        // A first selection, however quick, is the one run.
        let mut menu = RadialMenu::default();
        menu.open(1, NEUTRAL, tilt(), t);
        menu.latch();
        menu.update(RIGHT, tilt(), t + ms(10));
        assert_eq!(menu.commit_settled(t + ms(20)), Some(2));
    }

    #[test]
    fn a_selection_is_said_once_it_settles() {
        let t = Instant::now();
        let mut settle = Settle::default();
        assert_eq!(settle.poll(Some(1), t), None);
        // Swept on before it settled: only where it stops is said.
        assert_eq!(settle.poll(Some(2), t + ms(50)), None);
        assert_eq!(settle.poll(Some(2), t + ms(50) + NARRATE_SETTLE), Some(2));
        assert_eq!(settle.poll(Some(2), t + ms(500)), None, "said once");
        // Let go, then aimed there again: said again.
        settle.poll(None, t + ms(600));
        settle.poll(Some(2), t + ms(700));
        assert_eq!(settle.poll(Some(2), t + ms(700) + NARRATE_SETTLE), Some(2));
    }

    #[test]
    fn the_default_pages_hold_only_actions_without_a_button_on_the_straight_directions() {
        let pages = Pages::default();
        assert_eq!(pages.len(), PAGES);
        for i in 0..pages.len() {
            for (slot, cell) in pages.page(i).slots().into_iter().enumerate() {
                let Some(action) = cell.action() else {
                    assert!(cell.is_empty());
                    continue;
                };
                assert_eq!(
                    crate::gamepad::PadSettings::default_of(action.action()),
                    None,
                    "{action:?} has a button of its own"
                );
                assert_eq!(slot % 2, 0, "{action:?} on a diagonal");
            }
        }
        use RadialAction as A;
        let actions = |page: usize| pages.page(page).slots().map(Slot::action);
        assert_eq!(
            actions(0),
            [
                Some(A::Advancements),
                None,
                Some(A::Command),
                None,
                Some(A::SocialInteractions),
                None,
                None,
                None,
            ]
        );
        // Emote runs only with cosmetics on: the picker has it, the defaults don't.
        assert!(
            (0..pages.len()).all(|i| !actions(i).contains(&Some(A::Emote))),
            "a default slot that may not run"
        );
        assert!(RadialAction::ALL.contains(&A::Emote));
    }

    #[test]
    fn every_action_reaches_the_game_and_has_one_id_in_its_namespace() {
        for action in RadialAction::ALL {
            assert!(
                crate::gamepad::route(action.action()).is_some(),
                "{action:?}"
            );
            assert_eq!(RadialAction::from_id(action.id()), Some(action));
            // Vanilla's by their key mapping, Horizon's own in its namespace.
            if !action.is_horizons() {
                assert_eq!(action.id(), action.action(), "{action:?}");
                assert!(action.id().starts_with("key."), "{action:?}");
            }
        }
        use RadialAction as A;
        let horizons: Vec<_> = RadialAction::ALL
            .into_iter()
            .filter(|action| action.is_horizons())
            .collect();
        assert_eq!(horizons, [A::Emote, A::HotbarPrevious, A::HotbarNext]);
        // Held actions, the opener itself and anything else aren't slot actions.
        for id in [
            "key.attack",
            crate::gamepad::RADIAL,
            "/op someone",
            "horizon:",
        ] {
            assert_eq!(RadialAction::from_id(id), None, "{id}");
        }
    }

    #[test]
    fn ids_saved_before_the_namespaces_read_as_their_actions() {
        use RadialAction as A;
        let read = Pages::parse(
            [(
                "radial_0_slots",
                "key.emote,pad.hotbarPrevious,pad.hotbarNext,,,,,",
            )]
            .into_iter(),
        )
        .unwrap();
        let actions = read.page(0).slots().map(Slot::action);
        assert_eq!(
            actions[..3],
            [Some(A::Emote), Some(A::HotbarPrevious), Some(A::HotbarNext)]
        );
        // Written back under their new ids, which read the same.
        let text = read.serialize();
        assert!(
            text.contains("horizon:emote,horizon:hotbar_previous,horizon:hotbar_next"),
            "{text}"
        );
        assert_eq!(Pages::parse(lines(&text)), Some(read));
    }

    fn lines(text: &str) -> impl Iterator<Item = (&str, &str)> {
        text.lines().filter_map(|l| l.split_once(':'))
    }

    #[test]
    fn pages_survive_their_text_with_empty_slots_where_they_were() {
        let mut pages = Pages::default();
        pages.rename(1, "Build: mine");
        pages.set_slot(1, 0, Slot::Empty);
        pages.set_slot(1, 7, Slot::Action(RadialAction::Drop));
        let added = pages.add().unwrap();
        pages.set_slot(added, 4, Slot::Action(RadialAction::Chat));
        // A player's text that looks like a key stays their text.
        let typed = pages.add().unwrap();
        pages.rename(typed, CAMERA_PAGE_NAME);
        let text = pages.serialize();
        assert_eq!(Pages::parse(lines(&text)), Some(pages));
    }

    #[test]
    fn what_this_game_doesnt_know_survives_a_save() {
        let file = [
            ("radialLayout", "1"),
            // A slot from a newer Horizon or a mod, a field and a kind it doesn't read.
            ("radial_0_label", "Mine"),
            ("radial_0_slots", "key.chat,mymod:dash,,,,,,"),
            ("radial_0_icon", "minecraft:diamond"),
            ("radial_1_kind", "items"),
            ("radial_x_name", "not a number"),
            ("radial_2_kind", "hotbar"),
            ("radial_3_preset", "combat"),
        ];
        let pages = Pages::parse(file.into_iter()).unwrap();
        let page = pages.page(0);
        assert_eq!(page.slot(0), &Slot::Action(RadialAction::Chat));
        assert_eq!(page.slot(1), &Slot::Missing("mymod:dash".to_owned()));
        assert_eq!(page.slot(1).action(), None, "never runs");
        assert_eq!(pages.page(2).kind, PageKind::Hotbar);
        assert!(pages.page(2).slots().iter().all(|slot| slot.is_empty()));
        let text = pages.serialize();
        for (key, value) in file {
            assert!(
                text.contains(&format!("{key}:{value}\n")),
                "{key}:{value} lost from\n{text}"
            );
        }
        assert_eq!(Pages::parse(lines(&text)), Some(pages));
    }

    #[test]
    fn a_kind_this_game_doesnt_know_keeps_its_lines_and_gains_none() {
        let file = [
            ("radial_0_label", "Loadout"),
            ("radial_0_kind", "items"),
            ("radial_0_slots", "minecraft:torch,minecraft:bread"),
        ];
        let mut pages = Pages::parse(file.into_iter()).unwrap();
        assert!(pages.page(0).slots().iter().all(|slot| slot.is_empty()));
        let text = pages.serialize();
        let written: Vec<_> = lines(&text)
            .filter(|(k, _)| k.starts_with("radial_"))
            .collect();
        assert_eq!(written, file, "nothing added, nothing lost:\n{text}");
        // Filled in the editor, it is a page of slots.
        pages.set_slot(0, 2, Slot::Action(RadialAction::Chat));
        let text = pages.serialize();
        assert!(!text.contains("_kind:"), "{text}");
        assert!(text.contains("radial_0_slots:,,key.chat,,,,,\n"), "{text}");
    }

    #[test]
    fn slot_ids_past_the_eighth_are_kept() {
        let ids = "key.chat,,,,,,,key.drop,mymod:ninth,mymod:tenth";
        let pages = Pages::parse([("radial_0_label", "Wide"), ("radial_0_slots", ids)].into_iter())
            .unwrap();
        assert_eq!(pages.page(0).slot(7), &Slot::Action(RadialAction::Drop));
        let text = pages.serialize();
        assert!(text.contains(&format!("radial_0_slots:{ids}\n")), "{text}");
        // Trailing commas are no slots.
        let pages = Pages::parse(
            [
                ("radial_0_label", "W"),
                ("radial_0_slots", "key.chat,,,,,,,,,"),
            ]
            .into_iter(),
        )
        .unwrap();
        assert!(
            pages
                .serialize()
                .contains("radial_0_slots:key.chat,,,,,,,\n")
        );
    }

    #[test]
    fn the_layout_is_read_and_a_newer_ones_kept() {
        // This game's layout, and a file from before the line: a page saved by its slots that is a
        // default page follows the defaults again.
        let general_by_slots = [
            ("radial_0_name", GENERAL_PAGE_NAME),
            (
                "radial_0_slots",
                "key.advancements,,key.command,,key.socialInteractions,,,",
            ),
        ];
        for layout in [None, Some("1")] {
            let file = layout
                .map(|l| (LAYOUT_KEY, l))
                .into_iter()
                .chain(general_by_slots);
            let pages = Pages::parse(file).unwrap();
            assert_eq!(pages.page(0).preset(), Some(Preset::General), "{layout:?}");
            assert!(
                pages
                    .serialize()
                    .starts_with(&format!("{LAYOUT_KEY}:{RADIAL_LAYOUT}\n"))
            );
        }
        // A newer Horizon's: its number stays, and its pages stay as saved, never turned into
        // this game's presets (which may hold something else there).
        let newer = RADIAL_LAYOUT + 1;
        let newer_text = newer.to_string();
        let file = [(LAYOUT_KEY, newer_text.as_str())]
            .into_iter()
            .chain(general_by_slots);
        let mut pages = Pages::parse(file).unwrap();
        assert_eq!(pages.page(0).preset(), None);
        pages.set_slot(0, 6, Slot::Action(RadialAction::Chat));
        pages.set_slot(0, 6, Slot::Empty);
        assert_eq!(pages.page(0).preset(), None, "edited back, still its own");
        let text = pages.serialize();
        assert!(
            text.starts_with(&format!("{LAYOUT_KEY}:{newer}\n")),
            "{text}"
        );
        assert!(!text.contains("_preset:"), "{text}");
        assert_eq!(Pages::parse(lines(&text)), Some(pages));
        // A newer layout alone (its pages were its defaults) is kept too.
        let only = Pages::parse([(LAYOUT_KEY, newer_text.as_str())].into_iter()).unwrap();
        assert!(
            only.serialize()
                .starts_with(&format!("{LAYOUT_KEY}:{newer}\n"))
        );
        assert_ne!(only, Pages::default(), "so the settings file writes it");
        // Today's layout alone is the defaults.
        assert_eq!(Pages::parse([(LAYOUT_KEY, "1")].into_iter()), None);
    }

    #[test]
    fn pages_past_the_editors_most_are_read_and_those_past_the_files_most_kept() {
        let file: Vec<_> = (0..PARSE_MAX + 2)
            .map(|i| (format!("radial_{i}_label"), format!("Page {i}")))
            .collect();
        let pages = Pages::parse(file.iter().map(|(k, v)| (k.as_str(), v.as_str()))).unwrap();
        assert_eq!(
            pages.len(),
            MAX_PAGES,
            "the menu and the editor show the first ones"
        );
        let text = pages.serialize();
        for (key, value) in &file {
            assert!(text.contains(&format!("{key}:{value}\n")), "{key} lost");
        }
        // A page that goes brings the next one forward.
        let mut fewer = pages.clone();
        fewer.remove(0);
        assert_eq!(
            page_label(fewer.page(MAX_PAGES - 1)),
            format!("Page {MAX_PAGES}")
        );
        // The editor adds none while more are kept.
        assert_eq!(fewer.clone().add(), None);
        assert_eq!(Pages::parse(lines(&text)), Some(pages));
    }

    fn page_label(page: &Page) -> String {
        match &page.name {
            PageName::Builtin(key) => (*key).to_owned(),
            PageName::Custom(name) => name.clone(),
        }
    }

    #[test]
    fn an_untouched_default_page_is_saved_as_its_preset_and_follows_the_defaults() {
        let mut pages = Pages::default();
        pages.set_slot(0, 1, Slot::Action(RadialAction::Chat));
        let text = pages.serialize();
        assert!(text.contains("radial_1_preset:camera\n"), "{text}");
        assert!(!text.contains("radial_1_slots"), "{text}");
        assert!(
            text.contains("radial_0_slots:"),
            "the changed page by its slots"
        );
        // Read back, the preset is the default page as it is now, whatever was saved beside it.
        let read = Pages::parse(
            [
                ("radial_0_preset", "camera"),
                ("radial_0_slots", "key.drop,,,,,,,"),
            ]
            .into_iter(),
        )
        .unwrap();
        assert_eq!(read.page(0), Pages::default().page(1));
        assert_eq!(read.page(0).preset(), Some(Preset::Camera));
        // Renamed, it is the player's.
        pages.rename(1, "Shots");
        assert_eq!(pages.page(1).preset(), None);
        assert!(pages.serialize().contains("radial_1_slots:key.screenshot"));
    }

    #[test]
    fn an_edit_back_to_the_default_keeps_the_page_following_the_defaults() {
        let mut pages = Pages::default();
        let chat = Slot::Action(RadialAction::Chat);
        // The same action again, or the same name, is no change.
        pages.set_slot(0, 0, Slot::Action(RadialAction::Advancements));
        assert_eq!(pages.page(0).preset(), Some(Preset::General));
        pages.rename(1, "Shots");
        let renamed = pages.clone();
        pages.rename(1, "Shots");
        assert_eq!(pages, renamed);
        // Changed, then changed back.
        pages.set_slot(0, 1, chat.clone());
        assert_eq!(pages.page(0).preset(), None);
        pages.set_slot(0, 1, Slot::Empty);
        assert_eq!(pages.page(0).preset(), Some(Preset::General));
        assert_eq!(pages.page(0), Pages::default().page(0));
        // Read back from its slots too (a file from before presets).
        let read = Pages::parse(
            [
                ("radial_0_name", GENERAL_PAGE_NAME),
                (
                    "radial_0_slots",
                    "key.advancements,,key.command,,key.socialInteractions,,,",
                ),
            ]
            .into_iter(),
        )
        .unwrap();
        assert_eq!(read.page(0).preset(), Some(Preset::General));
    }

    #[test]
    fn a_touched_page_drops_a_preset_it_cant_read() {
        let mut pages =
            Pages::parse([("radial_0_preset", "combat"), ("radial_0_label", "Fight")].into_iter())
                .unwrap();
        assert!(pages.serialize().contains("radial_0_preset:combat"));
        pages.set_slot(0, 0, Slot::Action(RadialAction::Chat));
        let text = pages.serialize();
        assert!(!text.contains("preset"), "{text}");
        assert!(text.contains("radial_0_slots:key.chat"), "{text}");
    }

    #[test]
    fn a_damaged_file_loses_only_the_damaged_parts() {
        let text = [
            ("radial_0_label", "Mine\u{7}"),
            // An unknown action keeps its position; the action after it stays seventh.
            ("radial_0_slots", "key.chat,/stop,,,,,key.drop,"),
            (
                "radial_1_slots",
                "key.drop,key.drop,key.drop,key.drop,key.drop,key.drop,key.drop,key.drop,key.chat",
            ),
            // Only Horizon's keys go in `name`: anything else leaves a new page's name.
            ("radial_2_name", "Building"),
        ];
        let pages = Pages::parse(text.into_iter()).unwrap();
        assert_eq!(pages.len(), 3);
        assert_eq!(pages.page(0).name, PageName::Custom("Mine".to_owned()));
        assert_eq!(pages.page(0).slot(0), &Slot::Action(RadialAction::Chat));
        assert_eq!(pages.page(0).slot(1), &Slot::Missing("/stop".to_owned()));
        assert_eq!(pages.page(0).slot(6), &Slot::Action(RadialAction::Drop));
        assert_eq!(pages.page(1).name, PageName::Builtin(NEW_PAGE_NAME));
        assert_eq!(pages.page(1).slot(7), &Slot::Action(RadialAction::Drop));
        assert_eq!(pages.page(2).name, PageName::Builtin(NEW_PAGE_NAME));
        assert_eq!(Pages::parse(std::iter::empty()), None);
    }

    #[test]
    fn page_names_keep_whose_they_are() {
        let read = [
            ("radial_0_name", GENERAL_PAGE_NAME),
            ("radial_1_label", "Building"),
        ];
        let pages = Pages::parse(read.into_iter()).unwrap();
        assert_eq!(pages.page(0).name, PageName::Builtin(GENERAL_PAGE_NAME));
        assert_eq!(pages.page(1).name, PageName::Custom("Building".to_owned()));
        // A renamed default page stays the player's, even renamed to a key.
        let mut pages = Pages::default();
        pages.rename(0, GENERAL_PAGE_NAME);
        assert_eq!(
            pages.page(0).name,
            PageName::Custom(GENERAL_PAGE_NAME.to_owned())
        );
        let read = Pages::parse(lines(&pages.serialize())).unwrap();
        assert_eq!(
            read.page(0).name,
            PageName::Custom(GENERAL_PAGE_NAME.to_owned())
        );
        assert_eq!(read.page(1).name, PageName::Builtin(CAMERA_PAGE_NAME));
        // A name with nothing in it is a new page's.
        assert_eq!(
            PageName::custom(" \u{7} "),
            PageName::Builtin(NEW_PAGE_NAME)
        );
    }

    #[test]
    fn removing_an_action_leaves_the_others_in_place() {
        let mut pages = Pages::default();
        let before = pages.page(0).clone();
        pages.set_slot(0, 2, Slot::Empty);
        let after = pages.page(0);
        assert!(after.slot(2).is_empty());
        for i in (0..SLOTS).filter(|i| *i != 2) {
            assert_eq!(after.slot(i), before.slot(i));
        }
    }

    #[test]
    fn a_slot_set_on_a_hotbar_page_gives_it_slots() {
        let mut pages = Pages::parse([("radial_0_kind", "hotbar")].into_iter()).unwrap();
        pages.set_slot(0, 3, Slot::Action(RadialAction::Chat));
        assert_eq!(pages.page(0).slot(3), &Slot::Action(RadialAction::Chat));
        assert!(matches!(pages.page(0).kind, PageKind::Fixed(_)));
    }

    #[test]
    fn the_last_page_cant_be_deleted_and_the_count_is_bounded() {
        let mut pages = Pages::default();
        pages.remove(1);
        pages.remove(0);
        assert_eq!(pages.len(), 1);
        while pages.add().is_some() {}
        assert_eq!(pages.len(), MAX_PAGES);
    }

    #[test]
    fn page_indices_past_the_end_mean_the_last_page() {
        let pages = Pages::default();
        assert_eq!(pages.clamp(5), 1);
        assert_eq!(pages.page(9), pages.page(1));
    }

    #[test]
    fn pages_numbered_with_gaps_close_up_in_order() {
        let label = |i: usize| (format!("radial_{i}_label"), format!("Page {i}"));
        let read = |numbers: &[usize]| {
            let lines: Vec<_> = numbers.iter().map(|i| label(*i)).collect();
            let pages = Pages::parse(lines.iter().map(|(k, v)| (k.as_str(), v.as_str())));
            let pages = pages.unwrap();
            (0..pages.len())
                .map(|i| pages.page(i).name.clone())
                .collect::<Vec<_>>()
        };
        let named = |numbers: &[usize]| {
            let names = numbers
                .iter()
                .map(|i| PageName::Custom(format!("Page {i}")));
            names.collect::<Vec<_>>()
        };
        // Page 7 was past the most pages, yet only the third: it stays.
        assert_eq!(read(&[7, 0, 2]), named(&[0, 2, 7]));
        // Eight pages: the first six by number show.
        let eight = [9, 1, 30, 4, 12, 0, 6, 2];
        assert_eq!(read(&eight), named(&[0, 1, 2, 4, 6, 9]));
    }
}
