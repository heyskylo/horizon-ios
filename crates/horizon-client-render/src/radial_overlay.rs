//! The radial menu over play (Horizon's own; `radial.rs` has its rules), drawn while the opener
//! is held: eight tiles on a ring, the selected sector lit, a hub naming the selection, the pages
//! as tabs along the top and B's Cancel at the lower right. It isn't a screen: there are no
//! widgets to focus, and the world shows through. The game says which slots can run now and what
//! the toggles are; nothing here decides it.
//!
//! Its look is the HUD kit's pieces (`docs/ui-kit.md`): `shapes` for the ring and the hub's
//! disc, `motion` for its quiet motion, `icon` for the slots' pictures, `tabs` for the page strip
//! and `room` to stay below the boss bars. [`Layout`] sizes it from the view (the scaling rule in
//! its docs), and [`Look`] keeps the moments its motion runs from. Motion never selects: the
//! stick does.
use crate::{
    hud::{HudAtlas, HudBatch},
    hud_kit::{
        colour, fit,
        hint::{self, HintRow},
        icon::{self, Icon, IconPainter},
        motion::{self, Clock, Tween},
        plate::{self, Plate},
        room::Room,
        shapes::{self, Stop},
        tabs::{self, TabStrip},
    },
    lang,
    pad_glyph::{self, Glyph},
    radial::{Page, PageName, Pages, RadialAction, SLOTS, Slot},
};
use std::f32::consts::{FRAC_1_SQRT_2, FRAC_PI_4, FRAC_PI_8, TAU};
use std::time::{Duration, Instant};

/// A page's name: the default pages' and a new page's are language keys, a player's own is
/// itself.
pub fn page_label(page: &Page) -> String {
    match &page.name {
        PageName::Builtin(key) => lang::get(key),
        PageName::Custom(name) => name.clone(),
    }
}

/// A slot's action in a few words: the binds list's names ("Swap Item With Off Hand") can't be
/// read in a flick.
pub fn action_label(action: RadialAction) -> String {
    lang::get(action.caption_key())
}

/// A slot's caption: its action's, or that its action is missing (an id this game doesn't
/// know); `None` for an empty slot.
pub fn slot_caption(slot: &Slot) -> Option<String> {
    match slot {
        Slot::Empty => None,
        Slot::Action(action) => Some(action_label(*action)),
        Slot::Missing(_) => Some(lang::get(MISSING)),
    }
}
/// What a slot whose action this game doesn't know says.
pub const MISSING: &str = "horizon.radial.missing";
/// The hub's reason for a missing slot, over its id.
pub const WHY_MISSING: &str = "horizon.radial.why.missing";
/// What the hub says while the stick must be let go first.
pub const RELEASE_STICK: &str = "horizon.radial.release_stick";
/// The hub's line naming what a toggle turns to.
pub const NEXT: &str = "horizon.radial.next";
/// The hub's word for an empty slot.
pub const EMPTY: &str = "horizon.radial.empty";

/// Each action's picture, from the player's own vanilla resources (`hud_kit::icon`): items
/// through the game's painter, sprites from the GUI atlas.
pub fn icon(action: RadialAction) -> Icon {
    match action {
        RadialAction::Inventory => Icon::Item("minecraft:chest"),
        RadialAction::Chat => Icon::Item("minecraft:writable_book"),
        RadialAction::Command => Icon::Item("minecraft:command_block"),
        RadialAction::Advancements => Icon::Sprite("advancements/challenge_frame_obtained"),
        RadialAction::TogglePerspective => Icon::Item("minecraft:spyglass"),
        RadialAction::SwapOffhand => Icon::Item("minecraft:shield"),
        RadialAction::Drop => Icon::Sprite("statistics/item_dropped"),
        RadialAction::PickItem => Icon::Sprite("statistics/item_picked_up"),
        RadialAction::Screenshot => Icon::Item("minecraft:painting"),
        RadialAction::ToggleGui => Icon::Item("minecraft:glass"),
        RadialAction::SmoothCamera => Icon::Item("minecraft:feather"),
        RadialAction::SocialInteractions => Icon::Sprite("toast/social_interactions"),
        RadialAction::Emote => Icon::PlayerFace,
        RadialAction::HotbarPrevious => Icon::Sprite("spectator/scroll_left"),
        RadialAction::HotbarNext => Icon::Sprite("spectator/scroll_right"),
        // A frame round the picture: the window filling the screen.
        RadialAction::Fullscreen => Icon::Item("minecraft:item_frame"),
        // Vanilla's own info mark, as on its dialogs.
        RadialAction::QuickActions => Icon::Sprite("icon/info"),
        RadialAction::DebugOverlay => Icon::Item("minecraft:debug_stick"),
    }
}

/// The camera's view, as Perspective cycles it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum View {
    #[default]
    FirstPerson,
    ThirdPersonBack,
    ThirdPersonFront,
}

/// The states the toggles on the dial show, read by the game each frame.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Toggles {
    pub view: View,
    /// F1: the HUD is hidden.
    pub hud_hidden: bool,
    /// The cinematic camera (`Options.smoothCamera`) is on.
    pub cinematic: bool,
}

/// A toggle's state now and after a press, as language keys; `None` for an action that isn't a
/// toggle.
pub fn toggle_state(
    action: RadialAction,
    toggles: Toggles,
) -> Option<(&'static str, &'static str)> {
    const FIRST: &str = "horizon.radial.state.first_person";
    const BACK: &str = "horizon.radial.state.third_person_back";
    const FRONT: &str = "horizon.radial.state.third_person_front";
    const SHOWN: &str = "horizon.radial.state.shown";
    const HIDDEN: &str = "horizon.radial.state.hidden";
    const ON: &str = "horizon.radial.state.on";
    const OFF: &str = "horizon.radial.state.off";
    match action {
        RadialAction::TogglePerspective => Some(match toggles.view {
            View::FirstPerson => (FIRST, BACK),
            View::ThirdPersonBack => (BACK, FRONT),
            View::ThirdPersonFront => (FRONT, FIRST),
        }),
        RadialAction::ToggleGui if toggles.hud_hidden => Some((HIDDEN, SHOWN)),
        RadialAction::ToggleGui => Some((SHOWN, HIDDEN)),
        RadialAction::SmoothCamera if toggles.cinematic => Some((ON, OFF)),
        RadialAction::SmoothCamera => Some((OFF, ON)),
        _ => None,
    }
}

// ---------------------------------------------------------------- the scaling rule (GUI px)

/// At least this high a view is roomy: its hotbar stays clear, chips and the Cancel hint fit.
pub const ROOMY_H: f32 = 150.0;
/// The tab row's height, roomy and not.
pub const TAB_H: f32 = 12.0;
pub const TAB_H_SMALL: f32 = 10.0;
/// The header's top in a view with nothing to keep clear of, and the gap under its row.
pub const HEADER_TOP: f32 = 2.0;
pub const HEADER_GAP: f32 = 3.0;
/// Kept free over the view's bottom in a roomy view (the hotbar; more for the status rows above
/// it), and the gap above that.
pub const HOTBAR_RESERVE: f32 = 24.0;
pub const BOTTOM_GAP: f32 = 2.0;
/// A tile is this share of the view's height, between these sizes.
pub const TILE_SHARE: f32 = 0.11;
pub const TILE_MIN: f32 = 18.0;
pub const TILE_MAX: f32 = 26.0;
/// An icon in a tile, and the large one in the hub.
pub const ICON: f32 = 16.0;
pub const BIG_ICON: f32 = 32.0;
/// The ring's radius at most, and the clear space left at the view's sides and the band's ends.
pub const RADIUS_MAX: f32 = 64.0;
pub const SIDE_MARGIN: f32 = 4.0;
pub const BAND_MARGIN: f32 = 1.0;
/// Two neighbouring tiles are at least this far apart.
pub const TILE_GAP: f32 = 2.0;
/// The hub's clear space inside the tiles.
pub const HUB_GAP: f32 = 4.0;
/// The hub's text lines by its radius: 3, 2 or 1 at least these.
pub const HUB_LINES: [(f32, usize); 3] = [(34.0, 3), (22.0, 2), (12.0, 1)];
/// The hub shows the large icon from this radius, over at most this many lines, the lines'
/// middle this far below the hub's centre and the icon this far over them.
pub const BIG_ICON_HUB: f32 = 40.0;
pub const ICON_LINES_MAX: usize = 2;
pub const ICON_TEXT_DROP: f32 = 6.0;
pub const ICON_GAP: f32 = 2.0;
/// State chips under the tiles from this tile size.
pub const CHIP_TILE: f32 = 22.0;
/// A chip: its gap under its tile, its height, its text's padding and its widest.
pub const CHIP_GAP: f32 = 2.0;
pub const CHIP_H: f32 = 10.0;
pub const CHIP_PAD: f32 = 3.0;
pub const CHIP_MAX: f32 = 76.0;
/// The selected tile grows this much on each side.
pub const GROW: f32 = 1.0;
/// A line of text with its shadow, and the step between two in the hub.
pub const LINE_H: f32 = 9.0;
pub const LINE_STEP: f32 = 10.0;
/// The space a line keeps from the hub's edge, on each side.
pub const HUB_TEXT_INSET: f32 = 2.0;
/// The Cancel hint: its gap from the ring, its plate's padding and its height.
pub const HINT_GAP: f32 = 6.0;
/// The hint beside the ring sits this share of the ring's radius below its centre, level with the
/// lower diagonal tiles.
pub const HINT_DROP: f32 = 0.75;
pub const HINT_PAD: f32 = 3.0;
pub const HINT_H: f32 = 14.0;
/// The space between a glyph and its text.
pub const GLYPH_GAP: f32 = 3.0;
/// The vignette reaches this far past the tiles.
pub const VIGNETTE_REACH: f32 = 40.0;
/// The track reaches this far past the tiles on the outside.
pub const TRACK_OUT: f32 = 3.0;
/// The lit sector stops this far (radians) short of its neighbours' on each side.
pub const SECTOR_INSET: f32 = 0.02;
/// The lit sector's inner edge: a brighter line this thick at the hub's rim.
pub const SECTOR_EDGE: f32 = 2.0;
/// An unavailable action's slash: half its reach on each axis, and its width.
pub const SLASH_REACH: f32 = 7.0;
pub const SLASH_W: f32 = 2.0;
/// An empty slot's pip.
pub const PIP: f32 = 3.0;
/// A missing action's mark in its tile.
pub const MISSING_MARK: &str = "?";

// ---------------------------------------------------------------- colours

/// The design system's tokens (`crates/horizon-theme`): the dark page, the night accent and its
/// text shade, the muted text over a backdrop.
const DARK: u32 = horizon_theme::DARK.page;
/// The vignette at the centre, and its alpha at 60% out (0.7, 0.525).
const VIGNETTE: [u8; 2] = [179, 134];
const VIGNETTE_MID: f32 = 0.6;
/// The track a shade lighter than the dark fills, and its rims bright enough, so the ring reads
/// on the darkest skies (the End, night).
const TRACK_RGB: u32 = 0x22242E;
const TRACK: u32 = colour(TRACK_RGB, 140);
const RIM: u32 = colour(0xFFFFFF, 64);
const HUB_FILL: u32 = colour(DARK, 209);
const HUB_RIM: u32 = colour(0xFFFFFF, 56);
const SECTOR: u32 = colour(horizon_theme::night::NIGHT.accent, 140);
const SECTOR_HI: u32 = colour(horizon_theme::night::NIGHT.accent_hi, 255);
const TILE_BACK: u32 = colour(0x000000, 184);
const INK: u32 = colour(0xF2F3F7, 255);
const MUTED: u32 = colour(horizon_theme::BACKDROP_MUTED, 255);
/// The chips' backing when the player's Text Background Opacity is at its default (0 to 1).
pub const CHIP_BACKING: f32 = 0.8;
#[cfg(test)]
const CHIP_FILL: u32 = colour(DARK, 204);
const SLASH: u32 = colour(0xE05050, 255);
const PIP_COLOUR: u32 = colour(0xFFFFFF, 102);
const FLASH: u32 = colour(0xFFFFFF, 153);
/// The hotbar under a small view's dial.
const HOTBAR_VEIL: u32 = colour(DARK, 140);
/// An icon that can't run, and every icon while the stick must be let go.
const DIM: f32 = 0.3;
const WAITING_DIM: f32 = 0.55;

/// The vanilla sprites a tile is drawn with (the F3+F4 game mode switcher's).
const SLOT_SPRITE: &str = "gui/sprites/gamemode_switcher/slot";
const SELECTION_SPRITE: &str = "gui/sprites/gamemode_switcher/selection";
/// The hotbar's size, to dim it under a small view's dial.
const HOTBAR: [f32; 2] = [182.0, 22.0];

// ---------------------------------------------------------------- motion

/// Opening: a fade and a scale from `OPEN_SCALE`.
pub const OPEN: Tween = Tween::new(Duration::from_millis(90), horizon_theme::QUICK);
pub const OPEN_SCALE: f32 = 0.94;
/// The lit sector easing to a new slot, or sweeping open from `SWEEP_FROM` of its width.
pub const SELECT: Tween = Tween::new(Duration::from_millis(80), horizon_theme::MOVE);
pub const SWEEP_FROM: f32 = 0.5;
/// A page turn: the tiles slide `TURN_SLIDE` in from the bumper's side, fading over
/// `TURN_FADE_PX` of it, and the strip scrolls.
pub const TURN: Tween = Tween::new(Duration::from_millis(160), horizon_theme::MOVE);
pub const TURN_SLIDE: f32 = 8.0;
pub const TURN_FADE_PX: f32 = 12.0;
/// A commit: the chosen tile flashes, then everything fades. A cancel only fades.
pub const COMMIT_FLASH: Tween = Tween::new(Duration::from_millis(100), horizon_theme::QUICK);
pub const CLOSE: Tween = Tween::new(Duration::from_millis(80), horizon_theme::QUICK);

/// What the dial showed: the page, the selection and whether the stick must be let go.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Seen {
    pub page: usize,
    pub selected: Option<usize>,
    pub waiting: bool,
}

/// The moments the dial's motion runs from, kept by the game (`pad.radial_look`) and updated
/// each poll ([`Look::observe`]). Drawing only reads it.
#[derive(Clone, Debug, Default)]
pub struct Look {
    /// When it began to show, while it shows.
    shown_at: Option<Instant>,
    /// What it showed at the last poll, while it shows.
    seen: Option<Seen>,
    /// The selection's last change: the slot it came from (on the same page) and when.
    sector: Option<(Option<usize>, Instant)>,
    /// The last page turn: when, whether forward, and the page it came from.
    turn: Option<(Instant, bool, usize)>,
    /// The way the next page change turns (`turned`), until the poll that sees it.
    next_turn: Option<bool>,
    /// The slot a commit ran, until the close is seen.
    ran: Option<usize>,
    /// Closing: when, what it showed, and the slot that ran (it flashes).
    closing: Option<(Instant, Seen, Option<usize>)>,
}

impl Look {
    /// Each poll: what the dial shows now (`None` while it doesn't), at `now`.
    pub fn observe(&mut self, seen: Option<Seen>, now: Instant) {
        let Some(seen) = seen else {
            if let Some(last) = self.seen.take() {
                self.closing = Some((now, last, self.ran.take()));
            }
            self.shown_at = None;
            self.sector = None;
            self.turn = None;
            return;
        };
        let Some(last) = self.seen.replace(seen) else {
            *self = Self {
                shown_at: Some(now),
                seen: Some(seen),
                ..Self::default()
            };
            return;
        };
        if last.page != seen.page {
            self.sector = None;
            self.turn = Some((now, self.next_turn.take().unwrap_or(true), last.page));
        } else if last.selected != seen.selected && seen.selected.is_some() {
            self.sector = Some((last.selected, now));
        }
    }

    /// A page turn, forward (the next page) or back, before the poll that sees it.
    pub fn turned(&mut self, forward: bool) {
        self.next_turn = Some(forward);
    }

    /// The dial goes at once, with no flash or fade: the action took the view (a screen, a
    /// screenshot, the HUD hidden).
    pub fn cut(&mut self) {
        *self = Self::default();
    }

    /// A commit ran `slot`: it flashes as the dial closes.
    pub fn ran(&mut self, slot: usize) {
        self.ran = Some(slot);
    }

    /// Whether the dial is showing (open and past its draw delay), not only fading out.
    pub fn open(&self) -> bool {
        self.shown_at.is_some()
    }

    /// What to draw at `clock`: what it shows, or while it fades out what it last showed.
    pub fn showing(&self, clock: Clock) -> Option<Seen> {
        if let Some(seen) = self.seen {
            return Some(seen);
        }
        let (at, seen, flash) = self.closing?;
        (!self.closed(clock, at, flash)).then_some(seen)
    }

    /// The fade-out (after a commit's flash) has finished.
    fn closed(&self, clock: Clock, at: Instant, flash: Option<usize>) -> bool {
        let start = if flash.is_some() {
            at + COMMIT_FLASH.duration
        } else {
            at
        };
        clock.still || (clock.now >= start && CLOSE.done(clock, start))
    }

    /// The whole dial's alpha and scale at `clock`.
    fn presence(&self, clock: Clock) -> (f32, f32) {
        if let Some(at) = self.shown_at {
            return (
                motion::fade(OPEN, clock, at),
                motion::scale_in(OPEN, clock, at, OPEN_SCALE),
            );
        }
        match self.closing {
            Some((at, _, Some(_))) => {
                let start = at + COMMIT_FLASH.duration;
                let alpha = if clock.now < start {
                    1.0
                } else {
                    motion::fade_out(CLOSE, clock, start)
                };
                (alpha, 1.0)
            }
            Some((at, _, None)) => (motion::fade_out(CLOSE, clock, at), 1.0),
            None => (1.0, 1.0),
        }
    }

    /// The slot flashing after a commit, and how strongly (0 to 1).
    fn flash(&self, clock: Clock) -> Option<(usize, f32)> {
        let (at, _, slot) = self.closing?;
        Some((slot?, motion::flash(COMMIT_FLASH, clock, at)))
    }

    /// The tiles' sideways offset in GUI px after a page turn: in from the bumper's side.
    fn slide(&self, clock: Clock) -> f32 {
        self.turn.map_or(0.0, |(at, forward, _)| {
            let from = if forward { TURN_SLIDE } else { -TURN_SLIDE };
            motion::slide(TURN, clock, at, from)
        })
    }

    /// The lit sector's centre angle and its share of a full sector's width, for `selected`.
    fn sector(&self, clock: Clock, selected: usize) -> (f32, f32) {
        let to = slot_angle(selected);
        match self.sector {
            Some((Some(from), at)) => (motion::turn(SELECT, clock, at, slot_angle(from), to), 1.0),
            Some((None, at)) => (to, SELECT.lerp(clock, at, SWEEP_FROM, 1.0)),
            None => (to, 1.0),
        }
    }
}

/// Slot `slot`'s direction: radians from up, clockwise, as the stick selects it.
pub fn slot_angle(slot: usize) -> f32 {
    slot as f32 * FRAC_PI_4
}

// ---------------------------------------------------------------- layout

/// The dial's geometry in a `w` x `h` GUI px view (the scaling rule, all in GUI px):
///
/// - `roomy` is `h >= 150`. The tab row is 12 high (10 when not roomy), its top below the boss
///   bars (`Room::below_top(2)`), and the band under it ends 24 + 2 above the bottom (2 when not
///   roomy: the dial goes over a dimmed hotbar).
/// - `tile = clamp(floor(0.11 h), 18, 26)`.
/// - `r = min(64, band/2 - tile/2 - 1, w/2 - tile/2 - 4)`, at least `(tile + 2) / (2 sin 22.5°)`
///   so neighbours never touch; with chips the band keeps a chip's room at its bottom.
/// - `hub = r - tile/2 - 4`: the diagonals' inner corners rest on its rim, clear of its text.
/// - Hub lines 3, 2, 1 or 0 at hub ≥ 34, 22, 12; the large icon at hub ≥ 40; chips at tile ≥ 22;
///   the Cancel hint only when roomy.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Layout {
    pub w: f32,
    pub h: f32,
    pub roomy: bool,
    /// The tab row: its top and height.
    pub header_y: f32,
    pub tab_h: f32,
    /// The band the ring sits in, top and bottom.
    pub top: f32,
    pub bottom: f32,
    pub cx: f32,
    pub cy: f32,
    pub tile: f32,
    pub r: f32,
    pub hub: f32,
    pub lines: usize,
    pub chips: bool,
    pub big_icon: bool,
    pub cancel: bool,
    /// The radius the band and the view's width allow, before neighbours' clearance.
    fit: f32,
}

impl Layout {
    /// The dial in a `w` x `h` view whose HUD keeps `room` clear.
    pub fn new(w: f32, h: f32, room: &Room) -> Self {
        // A view too short for the ring between the boss bars and the status rows keeps the ring
        // whole: first the header goes back to the top, over the bars; then the ring goes down
        // over the rows, to the hotbar. Health stays clear before the bars do: the world goes on
        // behind the dial.
        [
            (room.below_top(HEADER_TOP), room.bottom()),
            (HEADER_TOP, room.bottom()),
        ]
        .into_iter()
        .map(|(header_y, bottom)| Self::at(w, h, header_y, bottom))
        .find(|layout| layout.r <= layout.fit)
        .unwrap_or_else(|| Self::at(w, h, HEADER_TOP, h))
    }

    /// The dial with its header's top at `header_y`, over a HUD whose bottom rows begin at
    /// `room_bottom`.
    fn at(w: f32, h: f32, header_y: f32, room_bottom: f32) -> Self {
        let roomy = h >= ROOMY_H;
        let tab_h = if roomy { TAB_H } else { TAB_H_SMALL };
        let top = header_y + tab_h + HEADER_GAP;
        // The hotbar, and the status rows above it where they show (`Keep::StatusRows`).
        let reserve = if roomy {
            HOTBAR_RESERVE.max(h - room_bottom)
        } else {
            0.0
        };
        let bottom = h - reserve - BOTTOM_GAP;
        let tile = (TILE_SHARE * h).floor().clamp(TILE_MIN, TILE_MAX);
        let chips = tile >= CHIP_TILE;
        let (cx, cy) = ((w / 2.0).floor(), ((top + bottom) / 2.0).floor());
        let chip_room = if chips { CHIP_GAP + CHIP_H } else { 0.0 };
        let below = bottom - cy - chip_room;
        let above = cy - top;
        let fit = RADIUS_MAX
            .min(above.min(below) - tile / 2.0 - BAND_MARGIN)
            .min(w / 2.0 - tile / 2.0 - SIDE_MARGIN);
        let apart = ((tile + TILE_GAP) / (2.0 * FRAC_PI_8.sin())).ceil();
        let fit = fit.floor();
        let r = fit.max(apart);
        let hub = (r - tile / 2.0 - HUB_GAP).floor();
        let lines = HUB_LINES
            .iter()
            .find(|(at, _)| hub >= *at)
            .map_or(0, |(_, n)| *n);
        Self {
            w,
            h,
            roomy,
            header_y,
            tab_h,
            top,
            bottom,
            cx,
            cy,
            tile,
            r,
            hub,
            lines,
            chips,
            big_icon: hub >= BIG_ICON_HUB,
            cancel: roomy,
            fit,
        }
    }

    /// Slot `slot`'s centre: on its true 45° ray, `r` out.
    pub fn centre(&self, slot: usize) -> [f32; 2] {
        shapes::on_circle([self.cx, self.cy], self.r, slot_angle(slot))
    }

    /// Slot `slot`'s tile, `[x, y, w, h]` on whole GUI px.
    pub fn tile_rect(&self, slot: usize) -> [f32; 4] {
        let [x, y] = self.centre(slot);
        let t = self.tile;
        [(x - t / 2.0).round(), (y - t / 2.0).round(), t, t]
    }

    /// The tab row, `[x, y, w, h]`, across the view.
    pub fn header(&self) -> [f32; 4] {
        [0.0, self.header_y, self.w, self.tab_h]
    }

    /// The track's inner and outer radii.
    fn track(&self) -> [f32; 2] {
        [self.hub + 1.0, self.r + self.tile / 2.0 + TRACK_OUT]
    }

    /// A chip `text_w` wide under slot `slot`'s tile (over it at the ring's top three; beside the
    /// ring's middle reaching outwards), if it fits there clear of every other tile, the hub,
    /// the header and the view's edges.
    pub fn chip(&self, slot: usize, text_w: f32) -> Option<[f32; 4]> {
        if !self.chips {
            return None;
        }
        let [x, y, t, _] = self.tile_rect(slot);
        let w = (text_w + 2.0 * CHIP_PAD).min(CHIP_MAX).ceil();
        // Under a tile, or over one at the ring's top, away from the hub.
        let upper = matches!(slot, 7 | 0 | 1);
        let chip_y = if upper {
            y - GROW - CHIP_GAP - CHIP_H
        } else {
            y + t + GROW + CHIP_GAP
        };
        // Beside the ring's middle it reaches outwards, away from the hub.
        let chip_x = match slot {
            1..=3 => x - GROW,
            5..=7 => x + t + GROW - w,
            _ => (x + t / 2.0 - w / 2.0).floor(),
        };
        let rect = [chip_x, chip_y, w, CHIP_H];
        let clear = (0..SLOTS)
            .filter(|other| *other != slot)
            .all(|other| !overlaps(rect, grown(self.tile_rect(other), GROW)));
        let inside = rect[0] >= 0.0
            && rect[0] + rect[2] <= self.w
            && rect[1] >= self.header_y + self.tab_h
            && rect[1] + rect[3] <= self.bottom;
        (clear && inside && !meets_disc(rect, [self.cx, self.cy], self.hub)).then_some(rect)
    }

    /// The Cancel hint `w` wide, `[x, y, w, h]`: beside the ring at the lower right, or in the
    /// view's corner when that is off the view or meets a tile or one of `chips`; `None` when
    /// neither is clear or the view isn't roomy.
    pub fn hint(&self, w: f32, chips: &[[f32; 4]]) -> Option<[f32; 4]> {
        if !self.cancel {
            return None;
        }
        let x = (self.cx + self.r + self.tile / 2.0 + HINT_GAP).floor();
        let y = (self.cy + self.r * HINT_DROP).floor();
        let beside = [x, y, w, HINT_H];
        let corner = [self.w - SIDE_MARGIN - w, self.bottom - HINT_H, w, HINT_H];
        let clear = |rect: [f32; 4]| {
            rect[0] + rect[2] <= self.w - SIDE_MARGIN
                && (0..SLOTS).all(|slot| !overlaps(rect, grown(self.tile_rect(slot), GROW)))
                && chips.iter().all(|chip| !overlaps(rect, *chip))
        };
        [beside, corner].into_iter().find(|rect| clear(*rect))
    }
}

/// Whether two `[x, y, w, h]` areas overlap (touching isn't).
fn overlaps(a: [f32; 4], b: [f32; 4]) -> bool {
    a[0] < b[0] + b[2] && b[0] < a[0] + a[2] && a[1] < b[1] + b[3] && b[1] < a[1] + a[3]
}

fn grown([x, y, w, h]: [f32; 4], by: f32) -> [f32; 4] {
    [x - by, y - by, w + 2.0 * by, h + 2.0 * by]
}

/// Whether an area reaches inside a disc.
fn meets_disc([x, y, w, h]: [f32; 4], [cx, cy]: [f32; 2], r: f32) -> bool {
    let nx = cx.clamp(x, x + w);
    let ny = cy.clamp(y, y + h);
    (nx - cx).powi(2) + (ny - cy).powi(2) < r * r
}

// ---------------------------------------------------------------- drawing

/// What the dial shows this frame, from the game.
pub struct Dial<'a> {
    pub pages: &'a Pages,
    /// The page, selection and waiting shown (live, or fading out after a close).
    pub seen: Seen,
    /// `available[i]`: slot `i`'s action can run now; one that can't is dimmed.
    pub available: [bool; SLOTS],
    /// Why slot `i` can't run, as a language key, for the hub.
    pub why: [Option<&'static str>; SLOTS],
    pub toggles: Toggles,
    /// The bumpers that turn back and on, `[back, on]`; `None` where that bumper doesn't (it
    /// opens the dial, and the other turns on).
    pub page_keys: [Option<Glyph>; 2],
    /// The button that cancels.
    pub cancel: Glyph,
    /// The chips' plates' alpha: the player's Text Background Opacity, as the guide's rows
    /// (`Settings::text_background` with `CHIP_BACKING` for its default).
    pub backing: u8,
    /// What the HUD keeps clear of (the boss bars).
    pub room: &'a Room,
}

/// Draws the dial in `batch`'s view at `clock`, its motion from `look`, items and the face through
/// `painter`.
pub fn draw_overlay(
    batch: &mut HudBatch,
    atlas: &HudAtlas,
    painter: &dyn IconPainter,
    dial: &Dial,
    look: &Look,
    clock: Clock,
) {
    let lay = Layout::new(batch.width, batch.height, dial.room);
    let (alpha, scale) = look.presence(clock);
    let page = dial.pages.page(dial.seen.page);
    let start = batch.vertices.len();
    if !lay.roomy {
        let [hw, hh] = HOTBAR;
        let hx = ((lay.w - hw) / 2.0).floor();
        batch.fill(atlas, hx, lay.h - hh, hw, hh, HOTBAR_VEIL);
    }
    draw_ring(batch, atlas, &lay, dial, look, clock);
    let tiles = batch.vertices.len();
    draw_tiles(batch, atlas, painter, &lay, dial, page, look, clock);
    let slide = look.slide(clock);
    batch.translate_since(tiles, slide, 0.0);
    batch.alpha_since(tiles, 1.0 - slide.abs() / TURN_FADE_PX);
    draw_hub(batch, atlas, painter, &lay, dial, page);
    batch.scale_since(start, lay.cx, lay.cy, scale, scale);
    // The header and the hint are rectangles: kept inside the view (a glyph's art reaches past
    // a small view's row). The ring's fans aren't clipped, which would seam them.
    let header = batch.vertices.len();
    draw_header(batch, atlas, &lay, dial, look, clock);
    draw_hint(batch, atlas, &lay, dial, page);
    batch.clip_since(header, 0.0, 0.0, lay.w, lay.h);
    batch.alpha_since(start, alpha);
}

/// The vignette, the track, the lit sector and the hub's disc.
fn draw_ring(
    batch: &mut HudBatch,
    atlas: &HudAtlas,
    lay: &Layout,
    dial: &Dial,
    look: &Look,
    clock: Clock,
) {
    let centre = [lay.cx, lay.cy];
    let reach = lay.r + lay.tile + VIGNETTE_REACH;
    batch.fill_disc_gradient(
        atlas,
        centre,
        reach,
        &[
            Stop(0.0, colour(DARK, VIGNETTE[0])),
            Stop(VIGNETTE_MID, colour(DARK, VIGNETTE[1])),
            Stop(1.0, colour(DARK, 0)),
        ],
    );
    let [r0, r1] = lay.track();
    batch.fill_arc(atlas, centre, [r0, r1], [0.0, TAU], TRACK);
    batch.fill_arc(atlas, centre, [r1, r1 + 1.0], [0.0, TAU], RIM);
    if let Some(selected) = dial.seen.selected.filter(|_| !dial.seen.waiting) {
        let (angle, share) = look.sector(clock, selected);
        let half = (FRAC_PI_8 - SECTOR_INSET) * share;
        batch.fill_arc(
            atlas,
            centre,
            [r0, r1],
            [angle - half, angle + half],
            SECTOR,
        );
        batch.fill_disc(atlas, centre, lay.hub, HUB_FILL);
        batch.fill_arc(
            atlas,
            centre,
            [lay.hub - SECTOR_EDGE, lay.hub],
            [angle - half, angle + half],
            SECTOR_HI,
        );
    } else {
        batch.fill_disc(atlas, centre, lay.hub, HUB_FILL);
    }
    batch.fill_arc(atlas, centre, [lay.hub, lay.hub + 1.0], [0.0, TAU], HUB_RIM);
}

/// The eight tiles: an action on vanilla's switcher slot with its icon, dimmed and slashed when
/// it can't run, a chip for a toggle's state; an empty slot as a pip.
#[allow(clippy::too_many_arguments)]
fn draw_tiles(
    batch: &mut HudBatch,
    atlas: &HudAtlas,
    painter: &dyn IconPainter,
    lay: &Layout,
    dial: &Dial,
    page: &Page,
    look: &Look,
    clock: Clock,
) {
    let flash = look.flash(clock);
    for slot in 0..SLOTS {
        let cell = page.slot(slot);
        let [cx, cy] = lay.centre(slot);
        if cell.is_empty() {
            let half = PIP / 2.0;
            batch.fill(
                atlas,
                (cx - half).round(),
                (cy - half).round(),
                PIP,
                PIP,
                PIP_COLOUR,
            );
            continue;
        }
        let chosen = dial.seen.selected == Some(slot) && !dial.seen.waiting;
        let rect = if chosen {
            grown(lay.tile_rect(slot), GROW)
        } else {
            lay.tile_rect(slot)
        };
        let [x, y, t, _] = rect;
        batch.fill(atlas, x, y, t, t, TILE_BACK);
        batch.sprite_scaled(atlas, SLOT_SPRITE, x, y, t, t, u32::MAX);
        if chosen {
            batch.sprite_scaled(atlas, SELECTION_SPRITE, x, y, t, t, u32::MAX);
        }
        if let Some((_, strength)) = flash.filter(|(s, _)| *s == slot) {
            batch.fill(atlas, x, y, t, t, shapes::faded(FLASH, strength));
        }
        let live = dial.available[slot];
        let mut dim = if live { 1.0 } else { DIM };
        if dial.seen.waiting {
            dim *= WAITING_DIM;
        }
        let icon_xy = [
            (x + (t - ICON) / 2.0).floor(),
            (y + (t - ICON) / 2.0).floor(),
        ];
        match cell {
            Slot::Action(action) => {
                let area = [icon_xy[0], icon_xy[1], ICON];
                icon::draw(batch, atlas, painter, icon(*action), area, dim);
            }
            Slot::Missing(_) => {
                // A question mark in the muted ink: the hub says what is missing.
                let mw = atlas.font.width(MISSING_MARK);
                let mx = (x + (t - mw) / 2.0).floor();
                let my = (y + (t - LINE_H) / 2.0).ceil();
                let waiting = if dial.seen.waiting { WAITING_DIM } else { 1.0 };
                batch.text(
                    atlas,
                    MISSING_MARK,
                    mx,
                    my,
                    shapes::faded(MUTED, waiting),
                    true,
                );
            }
            Slot::Empty => {}
        }
        // A missing action has its own mark instead of the slash.
        if !live && !matches!(cell, Slot::Missing(_)) {
            let [sx, sy] = [x + t / 2.0, y + t / 2.0];
            let (r, k) = (SLASH_REACH, SLASH_W / 2.0 * FRAC_1_SQRT_2);
            batch.fill_quad(
                atlas,
                [
                    [sx - r - k, sy + r - k],
                    [sx + r - k, sy - r - k],
                    [sx + r + k, sy - r + k],
                    [sx - r + k, sy + r + k],
                ],
                shapes::faded(SLASH, if dial.seen.waiting { WAITING_DIM } else { 1.0 }),
            );
        }
    }
    for (slot, text, rect) in chips(atlas, lay, dial, page) {
        let lit = dial.seen.selected == Some(slot) && !dial.seen.waiting;
        draw_chip(batch, atlas, rect, &text, lit, dial.backing);
    }
}

/// The state chips this page shows: the slot, its text cut to fit, and where it goes. A chip
/// that fits nowhere clear is left out.
fn chips(
    atlas: &HudAtlas,
    lay: &Layout,
    dial: &Dial,
    page: &Page,
) -> Vec<(usize, String, [f32; 4])> {
    (0..SLOTS)
        .filter_map(|slot| {
            let Slot::Action(action) = page.slot(slot) else {
                return None;
            };
            let (now, _) = toggle_state(*action, dial.toggles)?;
            let text =
                fit::ellipsize(atlas, &lang::get(now), CHIP_MAX - 2.0 * CHIP_PAD).into_owned();
            let rect = lay.chip(slot, atlas.font.width(&text).ceil())?;
            Some((slot, text, rect))
        })
        .collect()
}

/// A chip: its plate and its text, lit under the selected tile.
fn draw_chip(
    batch: &mut HudBatch,
    atlas: &HudAtlas,
    [x, y, w, h]: [f32; 4],
    text: &str,
    lit: bool,
    backing: u8,
) {
    batch.fill(atlas, x, y, w, h, colour(DARK, backing));
    let tw = atlas.font.width(text).ceil();
    let tx = (x + (w - tw) / 2.0).floor();
    batch.text(
        atlas,
        text,
        tx,
        y + 1.0,
        if lit { INK } else { MUTED },
        true,
    );
}

/// The hub: the large icon where there's room, the selection's name and what it says (a
/// toggle's state and next, why it can't run); or that the stick must be let go first.
fn draw_hub(
    batch: &mut HudBatch,
    atlas: &HudAtlas,
    painter: &dyn IconPainter,
    lay: &Layout,
    dial: &Dial,
    page: &Page,
) {
    let width = fit::chord(lay.hub, LINE_STEP + LINE_H / 2.0) - 2.0 * HUB_TEXT_INSET;
    let (lines, picture) = hub_lines(atlas, width, dial, page);
    let lines: Vec<(String, u32)> = lines.into_iter().take(lay.lines).collect();
    // The large icon over a name and at most one more line; three lines fill the hub alone.
    let picture = picture.filter(|_| lay.big_icon && lines.len() <= ICON_LINES_MAX);
    if lines.is_empty() && picture.is_none() {
        return;
    }
    let text_h = lines.len() as f32 * LINE_STEP - (LINE_STEP - LINE_H);
    // Under an icon the text sits a little below the centre, where the hub is widest, and the
    // icon right over it.
    let middle = if picture.is_some() {
        lay.cy + ICON_TEXT_DROP
    } else {
        lay.cy
    };
    let mut y = (middle - text_h / 2.0).floor();
    if let Some((picture, dim)) = picture {
        let area = [lay.cx - BIG_ICON / 2.0, y - ICON_GAP - BIG_ICON, BIG_ICON];
        icon::draw(batch, atlas, painter, picture, area, dim);
    }
    for (text, colour) in lines {
        // As wide as the hub's chord through the line's middle.
        let text = fit::fit_in_circle(atlas, &text, lay.hub, y - lay.cy, LINE_H, HUB_TEXT_INSET);
        let tw = atlas.font.width(&text).ceil();
        batch.text(atlas, &text, (lay.cx - tw / 2.0).floor(), y, colour, true);
        y += LINE_STEP;
    }
}

/// The hub's lines and its large icon (with its alpha) for what is selected.
/// A phrase that may take two lines is wrapped to `width` GUI px.
fn hub_lines(
    atlas: &HudAtlas,
    width: f32,
    dial: &Dial,
    page: &Page,
) -> (Vec<(String, u32)>, Option<(Icon, f32)>) {
    if dial.seen.waiting {
        let words = lang::get(RELEASE_STICK);
        let lines = fit::wrap(&words, width, |line| atlas.font.width(line));
        return (lines.into_iter().map(|l| (l, INK)).collect(), None);
    }
    let Some(slot) = dial.seen.selected else {
        return (Vec::new(), None);
    };
    match page.slot(slot) {
        Slot::Empty => (vec![(lang::get(EMPTY), MUTED)], None),
        Slot::Missing(id) => (
            vec![
                (lang::get(MISSING), INK),
                (lang::get(WHY_MISSING), MUTED),
                (id.clone(), MUTED),
            ],
            None,
        ),
        Slot::Action(action) => {
            let mut lines = vec![(action_label(*action), INK)];
            let more = |text: String, lines: &mut Vec<(String, u32)>| {
                // A reason or a state that is too wide for one line takes two.
                for line in fit::wrap(&text, width, |l| atlas.font.width(l)) {
                    lines.push((line, MUTED));
                }
            };
            let live = dial.available[slot];
            if let Some(why) = dial.why[slot].filter(|_| !live) {
                more(lang::get(why), &mut lines);
            } else if let Some((now, next)) = toggle_state(*action, dial.toggles) {
                lines.push((lang::get(now), MUTED));
                lines.push((lang::tr(NEXT, &[&lang::get(next)]), MUTED));
            }
            (lines, Some((icon(*action), if live { 1.0 } else { DIM })))
        }
    }
}

/// The page header: the tab strip between the bumpers that turn it; collapsed, the bumpers
/// carry the neighbouring pages' names. With one page, only its name.
fn draw_header(
    batch: &mut HudBatch,
    atlas: &HudAtlas,
    lay: &Layout,
    dial: &Dial,
    look: &Look,
    clock: Clock,
) {
    let pages = dial.pages;
    let names: Vec<String> = (0..pages.len())
        .map(|i| page_label(pages.page(i)))
        .collect();
    let [_, y, w, h] = lay.header();
    let paged = names.len() > 1;
    let keys = if paged { dial.page_keys } else { [None, None] };
    let key_w = keys.map(|k| k.map_or(0.0, |g| pad_glyph::width(atlas, g) + GLYPH_GAP));
    // The strip keeps the same margin both sides, so its middle is the view's.
    // As wide as the row needs (`tabs::span`), up to the view less the bumpers, centred; the
    // bumpers sit against its ends.
    let side = SIDE_MARGIN + key_w[0].max(key_w[1]);
    let room = (w - 2.0 * side).max(0.0);
    let width = tabs::span(atlas, &names).min(room);
    let left = ((w - width) / 2.0).floor();
    let right = left + width;
    // The row's text line, and the glyphs on it (a glyph reaches over its line).
    let text_y = y + ((h - LINE_H + 1.0) / 2.0).floor();
    // Scrolled in from where the new page's tab was.
    let scroll = look.turn.map_or(0.0, |(at, _, from)| {
        let distance = tabs::distance(atlas, &names, from, dial.seen.page);
        motion::slide(TURN, clock, at, distance)
    });
    let area = [left, y, width, h];
    let mut strip = tabs::tab_strip(atlas, &names, dial.seen.page, area, scroll);
    // The bumpers turn round past either end, so both arrows lead somewhere.
    if let TabStrip::Collapsed(folded) = &mut strip {
        (folded.has_prev, folded.has_next) = (paged, paged);
    }
    tabs::draw_tabs(batch, atlas, &strip, 1.0);
    if let Some(back) = keys[0] {
        pad_glyph::draw(batch, atlas, back, left - key_w[0] + 1.0, text_y);
    }
    if let Some(on) = keys[1] {
        pad_glyph::draw(batch, atlas, on, right + GLYPH_GAP + 1.0, text_y);
    }
    // Collapsed, the bumpers name the pages they turn to.
    if let TabStrip::Collapsed(tabs::Strip {
        arrows: [[before, _], [after, _]],
        ..
    }) = strip
    {
        let n = names.len();
        let page = dial.seen.page;
        if keys[0].is_some() {
            let room = before - GLYPH_GAP - left;
            let name = fit::ellipsize(atlas, &names[(page + n - 1) % n], room);
            batch.text(atlas, &name, left, text_y, MUTED, true);
        }
        if keys[1].is_some() {
            let start = after + atlas.font.width("›") + GLYPH_GAP;
            let name = fit::ellipsize(atlas, &names[(page + 1) % n], right - start);
            let nw = atlas.font.width(&name).ceil();
            batch.text(atlas, &name, right - nw, text_y, MUTED, true);
        }
    }
}

/// B's Cancel on a plate at the lower right, when the view is roomy.
fn draw_hint(batch: &mut HudBatch, atlas: &HudAtlas, lay: &Layout, dial: &Dial, page: &Page) {
    let row = HintRow::new([dial.cancel], lang::get("gui.cancel"));
    let w = (HINT_PAD + row.measure(atlas, &HINT_LOOK) + HINT_PAD).ceil();
    let chips: Vec<[f32; 4]> = chips(atlas, lay, dial, page)
        .into_iter()
        .map(|c| c.2)
        .collect();
    let Some([x, y, w, h]) = lay.hint(w, &chips) else {
        return;
    };
    plate::draw(batch, atlas, Plate::Notice, [x, y, w, h]);
    let ty = y + ((h - LINE_H + 1.0) / 2.0).floor();
    hint::draw_hint(batch, atlas, &row, &HINT_LOOK, x + HINT_PAD, ty);
}

/// The Cancel hint's row: a muted caption with its shadow, as the hub's second lines.
const HINT_LOOK: hint::Look = hint::Look {
    color: MUTED,
    shadow: true,
    ..hint::NOTICE
};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gamepad::MenuJob;
    use crate::hud_kit::icon::NoItems;
    use crate::hud_kit::room::Keep;
    use crate::pad_glyph::Glyphs;

    /// A 1280x720 window (and 854x480, and a quarter of a 1708x960 one), GUI scale 4 on
    /// 1280x720, a small view and a wide splitscreen half.
    const VIEWS: [(f32, f32); 4] = [
        (427.0, 240.0),
        (320.0, 180.0),
        (213.0, 120.0),
        (427.0, 120.0),
    ];
    const ALL: [bool; SLOTS] = [true; SLOTS];

    fn atlas() -> HudAtlas {
        HudAtlas::build(std::path::Path::new("/nonexistent/horizon-test-assets")).unwrap()
    }

    fn rooms(w: f32, h: f32) -> Vec<Room> {
        let bar = |n: usize| {
            let bottom = crate::hud_overlays::boss_bars_bottom(h as i32, n) as f32;
            Room::new(w, h).with(Keep::BossBars, [(w - 182.0) / 2.0, 0.0, 182.0, bottom])
        };
        vec![Room::new(w, h), bar(1), bar(2)]
    }

    fn inside(rect: [f32; 4], w: f32, h: f32) -> bool {
        rect[0] >= 0.0 && rect[1] >= 0.0 && rect[0] + rect[2] <= w && rect[1] + rect[3] <= h
    }

    #[test]
    fn the_scaling_rule_gives_the_brief_s_numbers() {
        let full = Layout::new(427.0, 240.0, &Room::new(427.0, 240.0));
        assert!(full.roomy && full.chips && full.cancel && full.big_icon);
        assert_eq!((full.tab_h, full.tile, full.r), (12.0, 26.0, 64.0));
        assert_eq!(full.lines, 3);
        let quarter = Layout::new(320.0, 180.0, &Room::new(320.0, 180.0));
        assert_eq!(quarter.tile, 19.0);
        assert!(quarter.roomy && !quarter.chips);
        let small = Layout::new(213.0, 120.0, &Room::new(213.0, 120.0));
        assert!(!small.roomy && !small.cancel && !small.chips);
        assert_eq!((small.tab_h, small.tile), (10.0, 18.0));
        assert_eq!(small.lines, 2);
    }

    #[test]
    fn the_ring_stays_off_the_status_rows_above_the_hotbar() {
        // Survival rows from their top down: hearts, food and armour; then absorption's second
        // row of hearts over that (vanilla: `h - 39`, rows 10 apart, armour above them).
        for (w, h) in [(427.0, 240.0), (320.0, 180.0)] {
            for top in [h - 49.0, h - 59.0] {
                let rows = [(w - 182.0) / 2.0, top, 182.0, h - top];
                let rooms = rooms(w, h)
                    .into_iter()
                    .map(|room| room.with(Keep::StatusRows, rows));
                for room in rooms {
                    let lay = Layout::new(w, h, &room);
                    let [_, outer] = lay.track();
                    assert!(
                        lay.cy + outer <= top,
                        "{w}x{h}, rows from {top}: the ring reaches {}",
                        lay.cy + outer
                    );
                    for slot in 0..SLOTS {
                        let a = grown(lay.tile_rect(slot), GROW);
                        assert!(!overlaps(a, rows), "{w}x{h}: slot {slot} covers the rows");
                    }
                }
            }
        }
    }

    #[test]
    fn tiles_sit_on_true_45_degree_rays_clear_of_each_other_the_hub_and_the_tabs() {
        for (w, h) in VIEWS {
            for room in rooms(w, h) {
                let lay = Layout::new(w, h, &room);
                let header = lay.header();
                // Only a view too short for the ring under them gives the bars' room up.
                assert!(
                    header[1] >= room.top() || h < ROOMY_H,
                    "{w}x{h}: the tabs under a boss bar"
                );
                for slot in 0..SLOTS {
                    let a = grown(lay.tile_rect(slot), GROW);
                    assert!(inside(a, w, h), "{w}x{h} slot {slot}: {a:?}");
                    assert!(!overlaps(a, header), "{w}x{h}: slot {slot} meets the tabs");
                    // The cardinal tiles stay off the hub; the diagonals' inner corners may
                    // rest on its rim, as in the design, clear of its text (tested below).
                    if slot % 2 == 0 {
                        assert!(
                            !meets_disc(a, [lay.cx, lay.cy], lay.hub),
                            "{w}x{h}: slot {slot} reaches into the hub"
                        );
                    }
                    for other in slot + 1..SLOTS {
                        let b = grown(lay.tile_rect(other), GROW);
                        assert!(!overlaps(a, b), "{w}x{h}: slots {slot} and {other} overlap");
                    }
                    // Where the stick points: the tile's middle on its ray.
                    let [x, y, t, _] = lay.tile_rect(slot);
                    let mid = [x + t / 2.0 - lay.cx, lay.cy - (y + t / 2.0)];
                    let angle = mid[0].atan2(mid[1]).rem_euclid(TAU);
                    let off = (angle - slot_angle(slot) + std::f32::consts::PI).rem_euclid(TAU)
                        - std::f32::consts::PI;
                    assert!(off.abs().to_degrees() < 1.5, "{w}x{h}: slot {slot} {angle}");
                    let distance = (mid[0] * mid[0] + mid[1] * mid[1]).sqrt();
                    assert!(
                        (distance - lay.r).abs() <= 0.75,
                        "{w}x{h}: slot {slot} off the ring"
                    );
                }
            }
        }
    }

    #[test]
    fn chips_and_the_hint_stay_clear_of_the_tiles_and_inside_the_view() {
        for (w, h) in [(427.0, 240.0), (640.0, 360.0), (854.0, 480.0)] {
            for room in rooms(w, h) {
                let lay = Layout::new(w, h, &room);
                for slot in 0..SLOTS {
                    // The widest chip.
                    if let Some(chip) = lay.chip(slot, CHIP_MAX) {
                        assert!(inside(chip, w, h), "{w}x{h} chip {slot}");
                        assert!(!meets_disc(chip, [lay.cx, lay.cy], lay.hub));
                        for other in 0..SLOTS {
                            let tile = grown(lay.tile_rect(other), GROW);
                            assert!(!overlaps(chip, tile), "{w}x{h}: chip {slot} on {other}");
                        }
                    }
                }
                let hint = lay.hint(60.0, &[]).expect("a roomy view has the hint");
                assert!(inside(hint, w, h), "{w}x{h} hint {hint:?}");
                // With every chip at its widest, the hint moves or goes, never over one.
                let chips: Vec<[f32; 4]> =
                    (0..SLOTS).filter_map(|s| lay.chip(s, CHIP_MAX)).collect();
                if let Some(hint) = lay.hint(60.0, &chips) {
                    assert!(inside(hint, w, h), "{w}x{h} hint {hint:?}");
                    for chip in &chips {
                        assert!(!overlaps(hint, *chip), "{w}x{h}: the hint on a chip");
                    }
                }
            }
        }
        // The full size view has a chip under every cardinal tile.
        let lay = Layout::new(427.0, 240.0, &Room::new(427.0, 240.0));
        for slot in [0, 2, 4, 6] {
            assert!(lay.chip(slot, 40.0).is_some(), "no chip under {slot}");
        }
        assert!(
            Layout::new(213.0, 120.0, &Room::new(213.0, 120.0))
                .hint(60.0, &[])
                .is_none()
        );
    }

    fn dial<'a>(
        pages: &'a Pages,
        room: &'a Room,
        seen: Seen,
        available: [bool; SLOTS],
    ) -> Dial<'a> {
        let glyphs = Glyphs::default();
        Dial {
            pages,
            seen,
            available,
            why: [None; SLOTS],
            toggles: Toggles::default(),
            page_keys: [MenuJob::PagePrev, MenuJob::PageNext].map(|j| Some(Glyph::menu(j, glyphs))),
            cancel: Glyph::menu(MenuJob::Back, glyphs),
            backing: 204,
            room,
        }
    }

    fn draw(size: [u32; 2], dial: &Dial, look: &Look) -> HudBatch {
        let mut batch = HudBatch::new(size);
        let clock = Clock::new(Instant::now(), true);
        draw_overlay(&mut batch, &atlas(), &NoItems, dial, look, clock);
        batch
    }

    fn shown(seen: Seen) -> Look {
        let mut look = Look::default();
        look.observe(Some(seen), Instant::now());
        look
    }

    #[test]
    fn everything_drawn_stays_inside_the_view() {
        let pages = Pages::default();
        for size in [
            [1280, 720],
            [854, 480],
            [640, 360],
            [427, 240],
            [213, 120],
            [427, 120],
        ] {
            let (w, h) = (size[0] as f32, size[1] as f32);
            let room = Room::new(w, h);
            let seen = Seen {
                page: 1,
                selected: Some(2),
                waiting: false,
            };
            let batch = draw(size, &dial(&pages, &room, seen, ALL), &shown(seen));
            // The dark fills (the vignette's rim) may run past the view's edge, where nothing draws;
            // the tiles, the text, the header and the hint may not.
            let dark = |c: u32| c & 0x00FF_FFFF == colour(DARK, 0);
            for v in batch.vertices.iter().filter(|v| !dark(v.color)) {
                let [x, y] = v.pos;
                assert!(
                    (-0.01..=batch.width + 0.01).contains(&x)
                        && (-0.01..=batch.height + 0.01).contains(&y),
                    "{size:?}: a vertex at {x} {y}"
                );
            }
        }
    }

    /// The colours drawn in a tile's square.
    fn colours_in(batch: &HudBatch, [x, y, w, h]: [f32; 4]) -> Vec<u32> {
        batch
            .vertices
            .iter()
            .filter(|v| (x..=x + w).contains(&v.pos[0]) && (y..=y + h).contains(&v.pos[1]))
            .map(|v| v.color)
            .collect()
    }

    #[test]
    fn an_action_that_can_t_run_is_slashed_and_the_hub_says_why() {
        let pages = Pages::default();
        let page = pages.page(0);
        let slot = (0..SLOTS).find(|i| !page.slot(*i).is_empty()).unwrap();
        let room = Room::new(427.0, 240.0);
        let seen = Seen {
            page: 0,
            selected: Some(slot),
            waiting: false,
        };
        let mut available = ALL;
        let live = draw(
            [427, 240],
            &dial(&pages, &room, seen, available),
            &shown(seen),
        );
        available[slot] = false;
        let mut blocked = dial(&pages, &room, seen, available);
        blocked.why[slot] = Some("horizon.radial.why.cosmetics");
        let dimmed = draw([427, 240], &blocked, &shown(seen));
        let lay = Layout::new(427.0, 240.0, &room);
        let rect = lay.tile_rect(slot);
        assert!(!colours_in(&live, rect).contains(&SLASH));
        assert!(colours_in(&dimmed, rect).contains(&SLASH));
        // The hub's second line is the reason, in the muted colour.
        let (lines, _) = hub_lines(&atlas(), 60.0, &blocked, page);
        assert_eq!(lines[1], (lang::get("horizon.radial.why.cosmetics"), MUTED));
    }

    #[test]
    fn a_toggle_shows_its_state_and_next_and_the_waiting_hub_says_let_go() {
        let mut pages = Pages::default();
        pages.set_slot(0, 0, Slot::Action(RadialAction::SmoothCamera));
        let room = Room::new(427.0, 240.0);
        let seen = Seen {
            page: 0,
            selected: Some(0),
            waiting: false,
        };
        let mut d = dial(&pages, &room, seen, ALL);
        let (lines, picture) = hub_lines(&atlas(), 60.0, &d, pages.page(0));
        assert_eq!(lines.len(), 3);
        assert_eq!(lines[1].0, lang::get("horizon.radial.state.off"));
        assert_eq!(
            lines[2].0,
            lang::tr(NEXT, &[&lang::get("horizon.radial.state.on")])
        );
        assert_eq!(picture, Some((Icon::Item("minecraft:feather"), 1.0)));
        d.toggles.cinematic = true;
        assert_eq!(
            hub_lines(&atlas(), 60.0, &d, pages.page(0)).0[1].0,
            lang::get("horizon.radial.state.on")
        );
        d.seen.waiting = true;
        let (lines, picture) = hub_lines(&atlas(), 60.0, &d, pages.page(0));
        assert_eq!(
            lines
                .iter()
                .map(|l| l.0.as_str())
                .collect::<Vec<_>>()
                .join(" "),
            lang::get(RELEASE_STICK)
        );
        assert!(picture.is_none());
    }

    #[test]
    fn the_hub_s_text_stays_inside_the_hub() {
        let english = lang::Language::load(&[], "en_us");
        let mut pages = Pages::default();
        pages.set_slot(0, 0, Slot::Action(RadialAction::Advancements));
        pages.set_slot(0, 2, Slot::Action(RadialAction::TogglePerspective));
        let ((), _) = lang::scoped(english, || {
            for (w, h) in VIEWS {
                let room = Room::new(w, h);
                let lay = Layout::new(w, h, &room);
                for slot in [0, 2] {
                    let seen = Seen {
                        page: 0,
                        selected: Some(slot),
                        waiting: false,
                    };
                    let batch = draw(
                        [w as u32 * 3, h as u32 * 3],
                        &dial(&pages, &room, seen, ALL),
                        &shown(seen),
                    );
                    // Text only, and nowhere near a tile's chip: inside the hub's disc.
                    let inner = lay.hub + 1.0;
                    for v in batch.vertices.iter().filter(|v| v.text != 0) {
                        let d = ((v.pos[0] - lay.cx).powi(2) + (v.pos[1] - lay.cy).powi(2)).sqrt();
                        let on_tile = (0..SLOTS).any(|t| {
                            let [x, y, w, h] = grown(lay.tile_rect(t), GROW);
                            (x..=x + w).contains(&v.pos[0]) && (y..=y + h).contains(&v.pos[1])
                        });
                        if d < lay.r - lay.tile / 2.0 {
                            assert!(!on_tile || d > lay.hub, "{w}x{h}: hub text under a tile");

                            assert!(
                                d <= inner,
                                "{w}x{h} slot {slot}: hub text at {d} of {inner}"
                            );
                        }
                    }
                }
            }
        });
    }

    #[test]
    fn perspective_cycles_through_its_three_views() {
        let t = |view| {
            toggle_state(
                RadialAction::TogglePerspective,
                Toggles {
                    view,
                    ..Toggles::default()
                },
            )
        };
        assert_eq!(
            t(View::FirstPerson).unwrap().1,
            t(View::ThirdPersonBack).unwrap().0
        );
        assert_eq!(
            t(View::ThirdPersonBack).unwrap().1,
            t(View::ThirdPersonFront).unwrap().0
        );
        assert_eq!(
            t(View::ThirdPersonFront).unwrap().1,
            t(View::FirstPerson).unwrap().0
        );
        assert_eq!(toggle_state(RadialAction::Chat, Toggles::default()), None);
    }

    #[test]
    fn a_missing_slot_shows_its_mark_and_the_hub_names_its_id() {
        let pages = Pages::parse([("radial_0_slots", "mymod:dash,,,,,,,")].into_iter()).unwrap();
        let room = Room::new(427.0, 240.0);
        let seen = Seen {
            page: 0,
            selected: Some(0),
            waiting: false,
        };
        let mut available = ALL;
        available[0] = false;
        let d = dial(&pages, &room, seen, available);
        let english = lang::Language::load(&[], "en_us");
        let ((lines, _), _) =
            lang::scoped(english, || hub_lines(&atlas(), 60.0, &d, pages.page(0)));
        let lines: Vec<&str> = lines.iter().map(|l| l.0.as_str()).collect();
        assert_eq!(lines, ["Missing", "Unknown action", "mymod:dash"]);
        let batch = draw([427, 240], &d, &shown(seen));
        let lay = Layout::new(427.0, 240.0, &room);
        let rect = lay.tile_rect(0);
        // Its mark, not the slash of an action that can't run, and no chip under it.
        let colours = colours_in(&batch, rect);
        assert!(!colours.contains(&SLASH), "a slash on a missing slot");
        assert!(colours.contains(&MUTED), "no mark in the missing slot");
        assert!(
            !batch.vertices.iter().any(|v| v.color == CHIP_FILL),
            "a chip under it"
        );
    }

    #[test]
    fn the_bumpers_sit_against_the_strip_and_only_those_that_turn_it() {
        let atlas = atlas();
        let room = Room::new(427.0, 240.0);
        let lay = Layout::new(427.0, 240.0, &room);
        // The header alone: how far it reaches left and right.
        let reach = |pages: &Pages, keys: [bool; 2]| {
            let seen = Seen::default();
            let mut d = dial(pages, &room, seen, ALL);
            for (key, on) in d.page_keys.iter_mut().zip(keys) {
                if !on {
                    *key = None;
                }
            }
            let mut batch = HudBatch::new([427, 240]);
            draw_header(
                &mut batch,
                &atlas,
                &lay,
                &d,
                &shown(seen),
                Clock::new(Instant::now(), true),
            );
            batch
                .vertices
                .iter()
                .fold((f32::MAX, f32::MIN), |(lo, hi), v| {
                    (lo.min(v.pos[0]), hi.max(v.pos[0]))
                })
        };
        let two = Pages::default();
        let names: Vec<String> = (0..two.len()).map(|i| page_label(two.page(i))).collect();
        let span = tabs::span(&atlas, &names);
        let (strip_l, strip_r) = (
            ((427.0 - span) / 2.0).floor(),
            ((427.0 - span) / 2.0).floor() + span,
        );
        let glyph_w = pad_glyph::width(&atlas, Glyph::menu(MenuJob::PagePrev, Glyphs::default()));
        // Both bumpers, against the strip's ends rather than the view's corners.
        let (lo, hi) = reach(&two, [true, true]);
        assert!(
            (lo - (strip_l - GLYPH_GAP - glyph_w)).abs() <= 3.0,
            "LB at {lo}, strip at {strip_l}"
        );
        assert!(
            (hi - (strip_r + GLYPH_GAP + glyph_w)).abs() <= 3.0,
            "RB to {hi}, strip to {strip_r}"
        );
        // A bumper opener: only the other.
        let (lo, hi) = reach(&two, [false, true]);
        assert!(lo >= strip_l - 1.0 && hi > strip_r + GLYPH_GAP, "{lo} {hi}");
        // One page: none.
        let mut one = Pages::default();
        one.remove(1);
        let names = vec![page_label(one.page(0))];
        let span = tabs::span(&atlas, &names);
        let (lo, hi) = reach(&one, [true, true]);
        assert!(
            lo >= ((427.0 - span) / 2.0).floor() - 1.0 && hi <= (427.0 + span) / 2.0 + 1.0,
            "{lo} {hi}"
        );
    }

    #[test]
    fn the_look_follows_open_selection_turn_and_close() {
        let t0 = Instant::now();
        let ms = Duration::from_millis;
        let mut look = Look::default();
        let seen = |selected| {
            Some(Seen {
                page: 0,
                selected,
                waiting: false,
            })
        };
        look.observe(seen(None), t0);
        assert!(look.open());
        // Half way through opening: faded and small.
        let (alpha, scale) = look.presence(Clock::new(t0 + ms(45), false));
        assert!(alpha < 1.0 && (OPEN_SCALE..1.0).contains(&scale));
        // A first selection sweeps open; the next eases from it.
        look.observe(seen(Some(2)), t0 + ms(100));
        let (angle, share) = look.sector(Clock::new(t0 + ms(100), false), 2);
        assert_eq!((angle, share), (slot_angle(2), SWEEP_FROM));
        look.observe(seen(Some(3)), t0 + ms(200));
        let (angle, _) = look.sector(Clock::new(t0 + ms(200), false), 3);
        assert_eq!(angle, slot_angle(2));
        let (angle, _) = look.sector(Clock::new(t0 + ms(400), false), 3);
        assert!((angle - slot_angle(3)).abs() < 1e-4);
        // A turn back slides the tiles in from the left.
        look.turned(false);
        look.observe(
            Some(Seen {
                page: 1,
                selected: None,
                waiting: false,
            }),
            t0 + ms(500),
        );
        assert_eq!(look.slide(Clock::new(t0 + ms(500), false)), -TURN_SLIDE);
        assert_eq!(look.slide(Clock::new(t0 + ms(1000), false)), 0.0);
        // A commit flashes its slot, then fades; it shows what it showed until then.
        look.observe(
            Some(Seen {
                page: 1,
                selected: Some(5),
                waiting: false,
            }),
            t0 + ms(600),
        );
        look.ran(5);
        look.observe(None, t0 + ms(700));
        assert!(!look.open());
        let at = |n| Clock::new(t0 + ms(n), false);
        assert_eq!(look.flash(at(700)), Some((5, 1.0)));
        assert_eq!(look.showing(at(750)).unwrap().selected, Some(5));
        assert_eq!(look.presence(at(790)).0, 1.0, "fading during the flash");
        assert!(look.showing(at(700 + 100 + 79)).is_some());
        assert!(look.showing(at(700 + 100 + 80)).is_none());
        // A cancel only fades.
        look.observe(seen(Some(1)), t0 + ms(2000));
        look.observe(None, t0 + ms(2100));
        assert_eq!(look.flash(at(2100)), None);
        assert!(look.showing(at(2100 + 80)).is_none());
        // A still clock shows the end of every motion: closed at once.
        look.observe(seen(Some(1)), t0 + ms(3000));
        look.observe(None, t0 + ms(3100));
        assert!(look.showing(Clock::new(t0 + ms(3100), true)).is_none());
        // A commit that took the view (a screen, a screenshot) goes at once, flash and all.
        look.observe(seen(Some(1)), t0 + ms(4000));
        look.ran(1);
        look.cut();
        look.observe(None, t0 + ms(4010));
        assert!(look.showing(at(4010)).is_none() && look.flash(at(4010)).is_none());
    }

    #[test]
    fn every_action_has_an_icon_and_every_string_a_horizon_key() {
        let english = lang::Language::load(&[], "en_us");
        let ((), missed) = lang::scoped(english, || {
            let mut pages = Pages::default();
            pages.set_slot(0, 1, Slot::Missing("mymod:dash".to_owned()));
            for (i, action) in [
                RadialAction::TogglePerspective,
                RadialAction::ToggleGui,
                RadialAction::SmoothCamera,
            ]
            .into_iter()
            .enumerate()
            {
                pages.set_slot(1, i + 3, Slot::Action(action));
            }
            let room = Room::new(427.0, 240.0);
            for page in 0..pages.len() {
                for selected in (0..SLOTS).map(Some).chain([None]) {
                    for waiting in [false, true] {
                        let seen = Seen {
                            page,
                            selected,
                            waiting,
                        };
                        let mut d = dial(&pages, &room, seen, ALL);
                        d.available[1] = false;
                        d.why[1] = Some("horizon.radial.why.cosmetics");
                        for view in [
                            View::FirstPerson,
                            View::ThirdPersonBack,
                            View::ThirdPersonFront,
                        ] {
                            d.toggles.view = view;
                            draw([854, 480], &d, &shown(seen));
                        }
                    }
                }
            }
            for why in [
                "horizon.radial.why.cosmetics",
                "horizon.radial.why.quick_actions",
                "horizon.radial.why.social",
            ] {
                lang::get(why);
            }
        });
        let missed: Vec<_> = missed
            .iter()
            .filter(|key| key.starts_with("horizon."))
            .collect();
        assert!(missed.is_empty(), "no text for {missed:?}");
        for action in RadialAction::ALL {
            // Every picture is the player's own: an item, a vanilla GUI sprite or their face.
            match icon(action) {
                Icon::Item(id) => assert!(id.starts_with("minecraft:"), "{action:?}"),
                Icon::Sprite(name) => assert!(!name.starts_with("horizon"), "{action:?}"),
                Icon::PlayerFace => assert_eq!(action, RadialAction::Emote),
                Icon::Glyph(_) => panic!("{action:?} drawn as a glyph"),
            }
        }
    }
}
