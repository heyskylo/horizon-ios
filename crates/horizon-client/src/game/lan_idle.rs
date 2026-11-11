//! Idle mobs and items that need no new description this tick (#10253).
//!
//! An entity outside the entity-ticking range does not tick, and `ChunkMap.tick` does not send
//! its changes (`EntityState::idle`) unless it changed section or needs sync. While it stays
//! where it was, its state is read only for where it is, and to pair it with a client that
//! starts tracking it. So such an entity is described bare (`EntityState::bare`: where it is,
//! none of its data) when no client could pair with it this tick: it was in the tracker's last
//! pass, has not moved, carries no passengers, and every client within its tracking range
//! already tracks it. Otherwise it is described in full, so a pairing always sends the entity
//! as it is.

use horizon_client::fast_hash::FxHashMap;
use horizon_client::lan::{hub::Hub, tracking};

#[cfg(test)]
#[path = "lan_idle_backend_tests.rs"]
mod backend_tests;

/// Per level, the idle entities the last pass noted (`Kept`).
#[derive(Default)]
pub(super) struct IdleStates {
    levels: FxHashMap<String, Kept>,
}

/// One level's idle entities as the last tracking pass noted them, in the
/// order it met them, and that pass. The game meets its entities in the same
/// order every tick (its lists keep their order), so a pass finds each one's
/// note at or just past where it found the last one: no lookup by id. One it
/// does not find there is described in full, which is always right.
#[derive(Default)]
struct Kept {
    pass: u32,
    ids: Vec<i32>,
    places: Vec<Place>,
    /// This pass's notes, which replace them when it finishes.
    next_ids: Vec<i32>,
    next_places: Vec<Place>,
}

/// How many notes past the last one found a pass looks: entities that left
/// the level since are skipped over.
const LOOK_AHEAD: usize = 4;

/// Where an entity is: its position, rotation (yaw, pitch, head yaw), whether
/// it is on the ground, and its velocity.
#[derive(Clone, Copy, PartialEq)]
pub(super) struct Place {
    pub position: [f64; 3],
    pub rotation: (f32, f32, f32),
    pub on_ground: bool,
    pub velocity: [f64; 3],
}

/// The idle entities of the level in place, for one pass.
pub(super) struct IdlePass<'a> {
    kept: &'a mut Kept,
    /// Whether the kept notes are the last pass's.
    last: bool,
    /// Where the next note is looked for.
    cursor: usize,
    /// The level's tracking passes so far (`Hub::tracking_ticks`). An entity
    /// noted one pass earlier was in the tracker's last pass.
    now: u32,
    /// The players the level's tracker runs for (`Hub::watches`: their
    /// index), where they are horizontally, and their view distance in blocks:
    /// the clients that could pair. Sorted by x, so an entity looks only at
    /// the ones within its range along x.
    viewers: Vec<(usize, [f64; 2], i32)>,
    /// `entity-broadcast-range-percentage`, as the tracker scales ranges.
    percent: i32,
    /// The last type asked about and its scaled tracking range: the
    /// entities of a type come in runs (a farm, a pile of items).
    last_kind: Option<(String, i32)>,
}

impl IdleStates {
    /// The level in place's idle entities, or None before its tracking ran.
    pub(super) fn pass(&mut self, hub: &Hub, percent: i32) -> Option<IdlePass<'_>> {
        let now = hub.tracking_ticks()?;
        let viewers = (hub.guests.iter().enumerate())
            .filter(|(_, g)| hub.watches(g))
            .map(|(i, g)| (i, [g.position[0], g.position[2]], g.view_distance * 16))
            .collect();
        let kept = self.levels.entry(hub.config.dimension.clone()).or_default();
        Some(IdlePass::new(kept, now, viewers, percent))
    }
}

impl<'a> IdlePass<'a> {
    fn new(
        kept: &'a mut Kept,
        now: u32,
        mut viewers: Vec<(usize, [f64; 2], i32)>,
        percent: i32,
    ) -> Self {
        // A player somewhere not finite is within no range: the distance
        // test below fails for it.
        viewers.retain(|(_, [x, z], _)| x.is_finite() && z.is_finite());
        viewers.sort_by(|a, b| a.1[0].total_cmp(&b.1[0]));
        kept.next_ids.clear();
        kept.next_places.clear();
        IdlePass {
            last: kept.pass.wrapping_add(1) == now,
            kept,
            cursor: 0,
            now,
            viewers,
            percent,
            last_kind: None,
        }
    }

    /// The type's tracking range in blocks, scaled as the tracker scales it.
    fn range(&mut self, kind: &str) -> i32 {
        match &mut self.last_kind {
            Some((last, range)) if last.as_str() == kind => *range,
            last => {
                let range = self.percent * (i32::from(tracking::settings(kind).0) * 16) / 100;
                *last = Some((kind.to_owned(), range));
                range
            }
        }
    }

    /// Whether the idle entity `id` of type `kind`, now at `at`, can be
    /// described bare: it was in the tracker's last pass at the same place,
    /// carries no passengers (whose ranges widen its own), and every client
    /// within its tracking range already tracks it. Notes it either way.
    pub(super) fn bare(&mut self, hub: &Hub, id: i32, kind: &str, at: &Place) -> bool {
        self.unchanged(id, kind, at, |i| hub.guests[i].tracker.is_tracking(id))
            && hub.riding.passengers(id).is_empty()
    }

    /// The place the last pass noted for `id`, if it is found where the
    /// pass's order says it would be.
    fn noted(&mut self, id: i32) -> Option<Place> {
        let ids = &self.kept.ids;
        let end = ids.len().min(self.cursor + LOOK_AHEAD);
        let k = (self.cursor..end).find(|&k| ids[k] == id)?;
        self.cursor = k + 1;
        Some(self.kept.places[k])
    }

    /// [`Self::bare`] but for the passengers, with whether the player `i`
    /// tracks the entity.
    fn unchanged(
        &mut self,
        id: i32,
        kind: &str,
        at: &Place,
        tracks: impl Fn(usize) -> bool,
    ) -> bool {
        let noted = if self.last { self.noted(id) } else { None };
        self.kept.next_ids.push(id);
        self.kept.next_places.push(*at);
        if noted != Some(*at) {
            return false;
        }
        // The tracker's `sees`, but for the chunk it also asks the client to have.
        let range = self.range(kind);
        // Only a client within `range` along x can be within it: a block
        // more spares the rounding of the squares.
        let reach = f64::from(range) + 1.0;
        let x = at.position[0];
        let from = self.viewers.partition_point(|v| v.1[0] < x - reach);
        !self.viewers[from..]
            .iter()
            .take_while(|v| v.1[0] <= x + reach)
            .any(|&(i, [vx, vz], view)| {
                let range = f64::from(range.min(view));
                let (dx, dz) = (vx - at.position[0], vz - at.position[2]);
                dx * dx + dz * dz <= range * range && !tracks(i)
            })
    }

    /// The notes of this pass replace the last one's: the entities it did not
    /// note left, or tick again.
    pub(super) fn finish(self) {
        let kept = self.kept;
        std::mem::swap(&mut kept.ids, &mut kept.next_ids);
        std::mem::swap(&mut kept.places, &mut kept.next_places);
        kept.pass = self.now;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn place(x: f64) -> Place {
        Place {
            position: [x, 64.0, 0.0],
            rotation: (0.0, 0.0, 0.0),
            on_ground: true,
            velocity: [0.0; 3],
        }
    }

    /// One pass `now` over `entities` (id, type, place), each answered with
    /// whether the player `i` tracks it; what each could be described as.
    fn pass(
        kept: &mut Kept,
        now: u32,
        entities: &[(i32, &str, Place)],
        tracks: impl Fn(i32, usize) -> bool,
    ) -> Vec<bool> {
        // One client at the origin, one 400 blocks away; both see 10 chunks.
        let viewers = vec![(1, [400.0, 0.0], 160), (0, [0.0, 0.0], 160)];
        let mut pass = IdlePass::new(kept, now, viewers, 100);
        let bare = (entities.iter())
            .map(|(id, kind, at)| pass.unchanged(*id, kind, at, |i| tracks(*id, i)))
            .collect();
        pass.finish();
        bare
    }

    /// An idle cow (tracked 10 chunks away) is described bare while it stays
    /// put and every client in its range tracks it; in full when a client
    /// there could pair with it, when it moved, and after a pass it was not in.
    #[test]
    fn an_idle_entity_is_bare_until_a_client_could_pair_or_it_moves() {
        let mut kept = Kept::default();
        let cow = |at| [(7, "minecraft:cow", at)];
        let at = place(100.0);
        // Its first pass describes it in full.
        assert_eq!(pass(&mut kept, 5, &cow(at), |_, _| true), [false]);
        // Tracked by the client in range; the far one can't pair.
        let tracked = |_, i| i == 0;
        assert_eq!(pass(&mut kept, 6, &cow(at), tracked), [true]);
        // The client in range doesn't track it yet: it could pair.
        assert_eq!(pass(&mut kept, 7, &cow(at), |_, _| false), [false]);
        // 100 blocks is past an item's 6 chunks: no client could pair.
        let item = [(7, "minecraft:item", at)];
        assert_eq!(pass(&mut kept, 8, &item, |_, _| false), [true]);
        // It moved.
        assert_eq!(pass(&mut kept, 9, &cow(place(100.5)), tracked), [false]);
        // A pass went by without it.
        assert_eq!(pass(&mut kept, 11, &cow(place(100.5)), tracked), [false]);
        // The client 400 blocks away is in range of a cow next to it.
        let far = [(7, "minecraft:cow", place(300.0))];
        pass(&mut kept, 12, &far, |_, _| true);
        assert_eq!(pass(&mut kept, 13, &far, |_, i| i == 0), [false]);
        assert_eq!(pass(&mut kept, 14, &far, |_, i| i == 1), [true]);
    }

    /// Each entity's note is found in the last pass's order, past the ones
    /// that left since and after one that joined.
    #[test]
    fn notes_are_found_in_the_last_pass_order() {
        let mut kept = Kept::default();
        let item = |id| (id, "minecraft:item", place(1000.0 + f64::from(id)));
        let all = |ids: &[i32]| ids.iter().map(|&id| item(id)).collect::<Vec<_>>();
        let first = all(&(1..=20).collect::<Vec<_>>());
        pass(&mut kept, 1, &first, |_, _| false);
        // 2 to 4 left, 30 joined in the middle, 12 to 14 left.
        let ids = [1, 5, 6, 30, 7, 8, 9, 10, 11, 15, 16, 17, 18, 19, 20];
        let bare = pass(&mut kept, 2, &all(&ids), |_, _| false);
        let full: Vec<i32> = ids
            .iter()
            .zip(&bare)
            .filter(|(_, b)| !**b)
            .map(|(id, _)| *id)
            .collect();
        assert_eq!(full, [30]);
        // One that moved out of order beyond the look-ahead is described in
        // full, which is always right, and the next pass finds it again.
        let moved = [20, 1, 5, 6, 7];
        let bare = pass(&mut kept, 3, &all(&moved), |_, _| false);
        assert_eq!(bare, [false, true, true, true, true]);
        assert_eq!(pass(&mut kept, 4, &all(&moved), |_, _| false), [true; 5]);
    }
}
