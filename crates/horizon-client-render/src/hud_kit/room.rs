//! Room: what of a view Horizon's own HUD overlays may use. Vanilla's HUD elements are
//! keep-outs; an overlay asks the room where it fits instead of assuming a fixed spot, so it
//! doesn't cover them. The keep-outs: the boss bars at the top centre, and the status rows above
//! the hotbar at the bottom centre.
//!
//! The room is built once a frame from what the HUD shows (`Game::hud_room` in gameplay). With
//! nothing showing it is empty, and every overlay stays exactly where it always was.
//!
//! All sizes are GUI px, from the view's top left. An area is `[x, y, w, h]`.
//!
//! ```
//! use horizon_client_render::hud_kit::room::{Keep, Room};
//! use horizon_client_render::hud_overlays::boss_bars_bottom;
//!
//! // A centred notice wants y = 18.
//! let empty = Room::new(320.0, 240.0);
//! assert_eq!(empty.below_top(18.0), 18.0);
//! // One boss bar ends at y 17: no overlap, so the notice stays.
//! let one = Room::new(320.0, 240.0).with(Keep::BossBars, [69.0, 0.0, 182.0, 17.0]);
//! assert_eq!(one.below_top(18.0), 18.0);
//! // Two end at y 36: the notice goes below them, a gap past.
//! let bars = boss_bars_bottom(240, 2) as f32;
//! let two = Room::new(320.0, 240.0).with(Keep::BossBars, [69.0, 0.0, 182.0, bars]);
//! assert_eq!(two.below_top(18.0), bars + 2.0);
//! ```

/// An area in GUI px: `[x, y, w, h]` from the view's top left.
pub type Area = [f32; 4];

/// The gap, in GUI px, an overlay leaves below a keep-out it moved past.
pub const GAP: f32 = 2.0;

/// A HUD element Horizon's overlays keep clear of.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Keep {
    /// The boss bars and their names at the top centre (`hud_overlays::boss_bars_bottom`).
    BossBars,
    /// The survival rows above the hotbar at the bottom centre: health with its absorption rows,
    /// armour, food or the mount's hearts, and air; from the highest row's top to the view's
    /// bottom.
    StatusRows,
}
impl Keep {
    /// It sits at the top of the view, not the bottom.
    fn at_top(self) -> bool {
        matches!(self, Self::BossBars)
    }
}

/// A view and the keep-outs its HUD shows this frame.
///
/// Invariants: the view's size is what the overlays draw in (the `HudBatch`'s size), and an
/// empty keep-out (no width or height) is never stored, so a room built from hidden elements
/// equals `Room::new`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Room {
    width: f32,
    height: f32,
    keeps: Vec<(Keep, Area)>,
}

impl Room {
    /// A `width` x `height` GUI px view with nothing to keep clear of.
    pub fn new(width: f32, height: f32) -> Self {
        Self {
            width,
            height,
            keeps: Vec::new(),
        }
    }

    /// This room with `area` kept clear of as `keep`; an empty area is ignored.
    pub fn with(mut self, keep: Keep, area: Area) -> Self {
        self.add(keep, area);
        self
    }

    /// Keeps clear of `area` as `keep`; an empty area is ignored.
    pub fn add(&mut self, keep: Keep, area: Area) {
        if area[2] > 0.0 && area[3] > 0.0 {
            self.keeps.push((keep, area));
        }
    }

    /// The view's width in GUI px.
    pub fn width(&self) -> f32 {
        self.width
    }

    /// The view's height in GUI px.
    pub fn height(&self) -> f32 {
        self.height
    }

    /// The keep-outs, in the order they were added.
    pub fn keeps(&self) -> impl Iterator<Item = (Keep, Area)> + '_ {
        self.keeps.iter().copied()
    }

    /// How far, in GUI px from the view's top, the keep-outs at the top reach down: the boss
    /// bars' bottom, or 0 with none.
    pub fn top(&self) -> f32 {
        self.keeps()
            .filter(|(keep, _)| keep.at_top())
            .map(|(_, [_, y, _, h])| y + h)
            .fold(0.0, f32::max)
    }

    /// How far, in GUI px from the view's top, the keep-outs at the bottom reach up: the status
    /// rows' top, or the view's height with none.
    pub fn bottom(&self) -> f32 {
        self.keeps()
            .filter(|(keep, _)| !keep.at_top())
            .map(|(_, [_, y, _, _])| y)
            .fold(self.height, f32::min)
    }

    /// Where a centred overlay that wants its top at `wanted` (GUI px) goes: at `wanted` unless
    /// a keep-out reaches past it (`top() > wanted`, a real overlap; touching isn't one), else
    /// `GAP` below that keep-out.
    pub fn below_top(&self, wanted: f32) -> f32 {
        let top = self.top();
        if top > wanted { top + GAP } else { wanted }
    }

    /// The view below the top keep-outs, with `GAP` past them: where a centred overlay may go.
    /// The whole view when nothing is kept clear of.
    pub fn usable(&self) -> Area {
        let top = self.top();
        let top = if top > 0.0 { top + GAP } else { 0.0 };
        [0.0, top, self.width, (self.height - top).max(0.0)]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_empty_room_is_the_whole_view_and_moves_nothing() {
        let room = Room::new(320.0, 240.0).with(Keep::BossBars, [69.0, 0.0, 182.0, 0.0]);
        assert_eq!(room, Room::new(320.0, 240.0));
        assert_eq!(room.usable(), [0.0, 0.0, 320.0, 240.0]);
        assert_eq!(room.below_top(18.0), 18.0);
    }

    #[test]
    fn an_overlay_moves_only_when_the_bars_reach_past_it() {
        let bars =
            |bottom: f32| Room::new(500.0, 300.0).with(Keep::BossBars, [159.0, 0.0, 182.0, bottom]);
        // Ending above or at its top: no overlap.
        assert_eq!(bars(17.0).below_top(18.0), 18.0);
        assert_eq!(bars(18.0).below_top(18.0), 18.0);
        // Past it: below, a gap away.
        assert_eq!(bars(36.0).below_top(18.0), 38.0);
        assert_eq!(bars(36.0).usable(), [0.0, 38.0, 500.0, 262.0]);
    }

    #[test]
    fn the_status_rows_are_kept_at_the_bottom_and_leave_the_top_alone() {
        let room = Room::new(427.0, 240.0);
        assert_eq!(room.bottom(), 240.0);
        // Hearts and armour from y 191 down.
        let rows = room.with(Keep::StatusRows, [122.0, 191.0, 182.0, 49.0]);
        assert_eq!(rows.bottom(), 191.0);
        assert_eq!(rows.top(), 0.0);
        assert_eq!(rows.below_top(18.0), 18.0);
    }
}
