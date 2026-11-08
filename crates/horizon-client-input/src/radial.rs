//! The radial menu (Horizon's own; vanilla 26.3 has no gamepad input; docs/controllers.md):
//! what the opener, the selecting stick (the right one unless the player picks another) and the
//! bumpers do while it is open. It knows pages only by their count and slots by their index.
//! Nothing here draws or acts: the game feeds it the pad, and resolves the slot
//! `RadialMenu::commit` gives back.
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
}
