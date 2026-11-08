//! A held world (`WorldHold`, the native hook `native_world_hold`): the terrain goes on being
//! drawn as it stands while what changes under it is meshed, and all of it takes its place on
//! one frame. A section already on screen that is meshed again is uploaded to the arena as ever,
//! over as many frames as its mesh takes to come, but waits beside the mesh drawn (`Hold`);
//! letting go only swaps which of the two is drawn, with nothing left to upload. Sections not on
//! screen yet (terrain streaming in) are not held.
//!
//! Both meshes of a waiting section are in memory, so a hold costs up to what the changed land's
//! new meshes come to. It is bounded, and each bound lets go at once rather than losing anything:
//! `MAX_BYTES` of waiting meshes, `MAX_HOLD` of time, and the arena running out
//! (`Renderer::upload`). It never outlives what asked for it: the mods are asked every frame,
//! and the sections' being cleared (the world left, another dimension) drops what waits. A hold
//! that ended by itself (spent) stays ended until no mod has asked for a frame.
use super::SectionKey;
use crate::fast_hash::{FxHashMap, FxHashSet};
use std::time::{Duration, Instant};

/// The longest a hold lasts.
pub(super) const MAX_HOLD: Duration = Duration::from_secs(30);
/// The most bytes of new meshes that wait.
pub(super) const MAX_BYTES: u64 = 1 << 30;
/// The most circles a hold is kept to (`WorldHold::hold_within`).
pub const MAX_AREAS: usize = 64;

/// What the mods ask of the terrain's picture this frame (`native_world_hold`): nothing, all of
/// it held, or only the sections inside some circles. Several mods add up: one holding all of it
/// holds all of it, and circles join.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct WorldHold {
    all: bool,
    within: Vec<[f64; 3]>,
}
impl WorldHold {
    /// Hold the whole world's terrain as it is drawn, this frame.
    pub fn hold(&mut self) {
        self.all = true;
    }
    /// Hold the sections whose middle lies within `radius` blocks of (`x`, `z`), this frame. A
    /// circle that isn't finite or has a negative radius is left out, as are those past
    /// `MAX_AREAS`.
    pub fn hold_within(&mut self, x: f64, z: f64, radius: f64) {
        let good = x.is_finite() && z.is_finite() && radius.is_finite() && radius >= 0.0;
        if good && self.within.len() < MAX_AREAS {
            self.within.push([x, z, radius]);
        }
    }
    /// Whether anything is held.
    pub fn is_held(&self) -> bool {
        self.all || !self.within.is_empty()
    }
    /// Whether section `key` is in what is held: all of the world, or a circle its middle lies in.
    pub fn covers(&self, key: SectionKey) -> bool {
        if self.all {
            return true;
        }
        let (x, z) = (f64::from(key.0) * 16.0 + 8.0, f64::from(key.2) * 16.0 + 8.0);
        self.within
            .iter()
            .any(|&[cx, cz, r]| (x - cx).powi(2) + (z - cz).powi(2) <= r * r)
    }
}

/// What a frame's ask means for the sections waiting.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Release {
    /// They go on waiting.
    None,
    /// Those the hold no longer covers take their place.
    Outside,
    /// All of them take their place.
    All,
}

/// A section waiting behind the hold and the bytes its new mesh takes.
struct Waiting<S> {
    section: S,
    bytes: u64,
}

/// The renderer's side of a hold: whether it is on, what it covers, and the sections waiting.
pub(super) struct Hold<S> {
    ask: WorldHold,
    since: Option<Instant>,
    spent: bool,
    waiting: FxHashMap<SectionKey, Waiting<S>>,
    bytes: u64,
}
impl<S> Default for Hold<S> {
    fn default() -> Self {
        Self {
            ask: WorldHold::default(),
            since: None,
            spent: false,
            waiting: FxHashMap::default(),
            bytes: 0,
        }
    }
}
impl<S> Hold<S> {
    /// Whether a new mesh of section `key` waits.
    pub fn holds(&self, key: SectionKey) -> bool {
        self.since.is_some() && !self.spent && self.ask.covers(key)
    }
    pub fn is_empty(&self) -> bool {
        self.waiting.is_empty()
    }
    pub fn bytes(&self) -> u64 {
        self.bytes
    }
    /// A frame's ask, at `now`.
    pub fn ask(&mut self, ask: &WorldHold, now: Instant) -> Release {
        if !ask.is_held() {
            let was = self.since.is_some();
            (self.since, self.spent) = (None, false);
            self.ask = WorldHold::default();
            return if was && !self.waiting.is_empty() {
                Release::All
            } else {
                Release::None
            };
        }
        if self.spent {
            return Release::None;
        }
        let since = *self.since.get_or_insert(now);
        if now.saturating_duration_since(since) >= MAX_HOLD {
            log::warn!(
                "Horizon: a mod held the world for {} s: let go",
                MAX_HOLD.as_secs()
            );
            self.spend();
            return Release::All;
        }
        if *ask == self.ask {
            return Release::None;
        }
        // (fewer circles or smaller: some of what waits is let go; more: nothing waits that didn't)
        self.ask.clone_from(ask);
        if self.waiting.is_empty() {
            Release::None
        } else {
            Release::Outside
        }
    }
    /// Ended by itself: nothing waits from now on, until no mod has asked for a frame.
    /// What waited is the caller's to place (`take_all`) or drop.
    pub fn spend(&mut self) {
        self.spent = self.since.is_some();
    }
    /// Section `key`'s new mesh waits: the one that waited before for it comes back, to free.
    /// The bool is whether what waits is now too much (`MAX_BYTES`).
    pub fn wait(&mut self, key: SectionKey, section: S, bytes: u64) -> (Option<S>, bool) {
        let old = self.take(key);
        self.bytes += bytes;
        self.waiting.insert(key, Waiting { section, bytes });
        (old, self.bytes > MAX_BYTES)
    }
    /// Section `key`'s waiting mesh, no longer waiting.
    pub fn take(&mut self, key: SectionKey) -> Option<S> {
        if self.waiting.is_empty() {
            return None;
        }
        let w = self.waiting.remove(&key)?;
        self.bytes -= w.bytes;
        Some(w.section)
    }
    /// Everything waiting, no longer waiting.
    pub fn take_all(&mut self) -> Vec<(SectionKey, S)> {
        self.bytes = 0;
        self.waiting.drain().map(|(k, w)| (k, w.section)).collect()
    }
    /// What waits that the hold no longer covers.
    pub fn take_outside(&mut self) -> Vec<(SectionKey, S)> {
        let ask = &self.ask;
        let all = self.since.is_none() || self.spent;
        let out: Vec<_> = self
            .waiting
            .extract_if(|&k, _| all || !ask.covers(k))
            .collect();
        self.bytes -= out.iter().map(|(_, w)| w.bytes).sum::<u64>();
        out.into_iter().map(|(k, w)| (k, w.section)).collect()
    }
    /// What waits in these columns, which are going.
    pub fn take_columns(&mut self, columns: &FxHashSet<(i32, i32)>) -> Vec<S> {
        if self.waiting.is_empty() {
            return Vec::new();
        }
        let out: Vec<_> = self
            .waiting
            .extract_if(|k, _| columns.contains(&(k.0, k.2)))
            .collect();
        self.bytes -= out.iter().map(|(_, w)| w.bytes).sum::<u64>();
        out.into_iter().map(|(_, w)| w.section).collect()
    }
    /// The sections are all gone: what waits goes with them and the hold is spent.
    pub fn clear(&mut self) {
        self.waiting.clear();
        self.bytes = 0;
        self.spend();
    }
    pub fn sections(&self) -> impl Iterator<Item = &S> {
        self.waiting.values().map(|w| &w.section)
    }
    pub fn sections_mut(&mut self) -> impl Iterator<Item = &mut S> {
        self.waiting.values_mut().map(|w| &mut w.section)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn all() -> WorldHold {
        let mut h = WorldHold::default();
        h.hold();
        h
    }
    fn within(circles: &[[f64; 3]]) -> WorldHold {
        let mut h = WorldHold::default();
        for &[x, z, r] in circles {
            h.hold_within(x, z, r);
        }
        h
    }

    #[test]
    fn a_hold_covers_sections_by_their_middle() {
        assert!(all().covers((900, 0, -900)));
        assert!(!WorldHold::default().covers((0, 0, 0)));
        assert!(within(&[[8.0, 8.0, 1.0]]).covers((0, 3, 0)));
        assert!(!within(&[[0.0, 0.0, 11.0]]).covers((0, 3, 0)));
        assert!(within(&[[0.0, 0.0, 12.0]]).covers((0, 3, 0)));
        assert!(within(&[[1000.0, 0.0, 1.0], [24.0, 8.0, 0.5]]).covers((1, 0, 0)));
        assert!(within(&[[-8.0, -8.0, 0.0]]).covers((-1, 0, -1)));
    }
    #[test]
    fn bad_circles_are_left_out_and_there_are_at_most_max_areas() {
        let h = within(&[
            [f64::NAN, 0.0, 5.0],
            [0.0, f64::INFINITY, 5.0],
            [0.0, 0.0, -1.0],
            [0.0, 0.0, f64::INFINITY],
        ]);
        assert!(!h.is_held());
        let mut many = WorldHold::default();
        for i in 0..MAX_AREAS + 10 {
            many.hold_within(i as f64 * 100.0, 0.0, 1.0);
        }
        assert_eq!(many.within.len(), MAX_AREAS);
        // (several mods: all of it from one holds all of it)
        many.hold();
        assert!(many.covers((-5000, 0, 5000)));
    }
    #[test]
    fn a_hold_starts_and_lets_go_when_no_one_asks() {
        let (mut g, t) = (Hold::<u32>::default(), Instant::now());
        assert_eq!(g.ask(&WorldHold::default(), t), Release::None);
        assert!(!g.holds((0, 0, 0)));
        assert_eq!(g.ask(&all(), t), Release::None);
        assert!(g.holds((0, 0, 0)));
        assert_eq!(g.ask(&all(), t + Duration::from_secs(5)), Release::None);
        // (nothing waited: nothing to place)
        assert_eq!(g.ask(&WorldHold::default(), t), Release::None);
        g.ask(&all(), t);
        g.wait((0, 0, 0), 7, 100);
        assert_eq!(g.ask(&WorldHold::default(), t), Release::All);
        assert_eq!(g.take_all(), vec![((0, 0, 0), 7)]);
        assert!(!g.holds((0, 0, 0)));
    }
    #[test]
    fn a_hold_ends_by_itself_and_stays_ended_until_let_go() {
        let (mut g, t) = (Hold::<u32>::default(), Instant::now());
        g.ask(&all(), t);
        assert_eq!(g.ask(&all(), t + MAX_HOLD), Release::All);
        assert!(!g.holds((0, 0, 0)));
        assert_eq!(g.ask(&all(), t + MAX_HOLD * 2), Release::None);
        assert!(!g.holds((0, 0, 0)));
        g.ask(&WorldHold::default(), t + MAX_HOLD * 2);
        g.ask(&all(), t + MAX_HOLD * 3);
        assert!(g.holds((0, 0, 0)));
    }
    #[test]
    fn smaller_circles_let_go_what_they_leave() {
        let (mut g, t) = (Hold::<u32>::default(), Instant::now());
        g.ask(&within(&[[0.0, 0.0, 100.0]]), t);
        assert!(g.holds((0, 0, 0)) && g.holds((5, 0, 0)) && !g.holds((7, 0, 0)));
        g.wait((0, 0, 0), 1, 10);
        g.wait((5, 0, 0), 2, 20);
        assert_eq!(g.ask(&within(&[[0.0, 0.0, 100.0]]), t), Release::None);
        assert_eq!(g.ask(&within(&[[0.0, 0.0, 40.0]]), t), Release::Outside);
        assert_eq!(g.take_outside(), vec![((5, 0, 0), 2)]);
        assert_eq!((g.sections().count(), g.bytes()), (1, 10));
        assert!(!g.holds((5, 0, 0)));
        // (to all of it again: nothing more is let go, and all of it waits again)
        assert_eq!(g.ask(&all(), t), Release::Outside);
        assert!(g.take_outside().is_empty());
        assert!(g.holds((5, 0, 0)));
    }
    #[test]
    fn what_waits_is_counted_and_capped() {
        let (mut g, t) = (Hold::<u32>::default(), Instant::now());
        g.ask(&all(), t);
        assert_eq!(g.wait((0, 0, 0), 1, MAX_BYTES - 10), (None, false));
        // (a newer mesh of the same section replaces it, and the old one comes back to free)
        assert_eq!(g.wait((0, 0, 0), 2, MAX_BYTES - 20), (Some(1), false));
        assert_eq!(g.bytes(), MAX_BYTES - 20);
        assert_eq!(g.wait((1, 0, 0), 3, 20), (None, false));
        assert_eq!(g.wait((2, 0, 0), 4, 1), (None, true));
        assert_eq!(g.take((1, 0, 0)), Some(3));
        assert_eq!(g.take((1, 0, 0)), None);
        assert_eq!(g.bytes(), MAX_BYTES - 19);
    }
    #[test]
    fn columns_going_and_the_sections_cleared_drop_what_waits() {
        let (mut g, t) = (Hold::<u32>::default(), Instant::now());
        g.ask(&all(), t);
        g.wait((0, 0, 0), 1, 10);
        g.wait((0, 5, 0), 2, 10);
        g.wait((3, 0, 0), 3, 10);
        let gone: FxHashSet<(i32, i32)> = [(0, 0)].into_iter().collect();
        let mut dropped = g.take_columns(&gone);
        dropped.sort_unstable();
        assert_eq!(dropped, vec![1, 2]);
        assert_eq!((g.sections().count(), g.bytes()), (1, 10));
        g.clear();
        assert!(g.is_empty() && g.bytes() == 0 && !g.holds((3, 0, 0)));
        // (cleared while held: spent until let go; with no hold, nothing to end)
        assert_eq!(g.ask(&all(), t), Release::None);
        assert!(!g.holds((3, 0, 0)));
        let mut none = Hold::<u32>::default();
        none.clear();
        none.ask(&all(), t);
        assert!(none.holds((3, 0, 0)));
    }
}
