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

/// Per level, each idle entity's place and the tracking pass it was last described in.
#[derive(Default)]
pub(super) struct IdleStates {
    levels: FxHashMap<String, FxHashMap<i32, (u32, Place)>>,
}

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
    kept: &'a mut FxHashMap<i32, (u32, Place)>,
    /// The level's tracking passes so far (`Hub::tracking_ticks`). An entity
    /// described one pass earlier was in the tracker's last pass.
    now: u32,
    /// The players the level's tracker runs for (`Hub::watches`: their
    /// index), where they are horizontally, and their view distance in blocks:
    /// the clients that could pair.
    viewers: Vec<(usize, [f64; 2], i32)>,
    /// `entity-broadcast-range-percentage`, as the tracker scales ranges.
    percent: i32,
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
        Some(IdlePass {
            kept,
            now,
            viewers,
            percent,
        })
    }
}

impl IdlePass<'_> {
    /// Whether the idle entity `id` of type `kind`, now at `at`, can be
    /// described bare: it was in the tracker's last pass at the same place,
    /// carries no passengers (whose ranges widen its own), and every client
    /// within its tracking range already tracks it. Notes it either way.
    pub(super) fn bare(&mut self, hub: &Hub, id: i32, kind: &str, at: &Place) -> bool {
        self.unchanged(id, kind, at, |i| hub.guests[i].tracker.is_tracking(id))
            && hub.riding.passengers(id).is_empty()
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
        // A new one reads as noted two passes ago.
        let now = self.now;
        let noted = self.kept.entry(id).or_insert((now.wrapping_sub(2), *at));
        let still = noted.0.wrapping_add(1) == now && noted.1 == *at;
        *noted = (now, *at);
        if !still {
            return false;
        }
        // The tracker's `sees`, but for the chunk it also asks the client to have.
        let range = self.percent * (i32::from(tracking::settings(kind).0) * 16) / 100;
        !self.viewers.iter().any(|&(i, [x, z], view)| {
            let range = f64::from(range.min(view));
            let (dx, dz) = (x - at.position[0], z - at.position[2]);
            dx * dx + dz * dz <= range * range && !tracks(i)
        })
    }

    /// Forget the entities this pass did not note: they left, or tick again.
    pub(super) fn finish(self) {
        let now = self.now;
        self.kept.retain(|_, (stamp, _)| *stamp == now);
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

    fn pass(kept: &mut FxHashMap<i32, (u32, Place)>, now: u32) -> IdlePass<'_> {
        // One client at the origin, one 400 blocks away; both see 10 chunks.
        IdlePass {
            kept,
            now,
            viewers: vec![(0, [0.0, 0.0], 160), (1, [400.0, 0.0], 160)],
            percent: 100,
        }
    }

    /// An idle cow (tracked 10 chunks away) is described bare while it stays
    /// put and every client in its range tracks it; in full when a client
    /// there could pair with it, when it moved, and after a pass it was not in.
    #[test]
    fn an_idle_entity_is_bare_until_a_client_could_pair_or_it_moves() {
        let mut kept = FxHashMap::default();
        let cow = "minecraft:cow";
        let at = place(100.0);
        // Its first pass describes it in full.
        assert!(!pass(&mut kept, 5).unchanged(7, cow, &at, |_| true));
        // Tracked by the client in range; the far one can't pair.
        let tracked = |i| i == 0;
        assert!(pass(&mut kept, 6).unchanged(7, cow, &at, tracked));
        // The client in range doesn't track it yet: it could pair.
        assert!(!pass(&mut kept, 7).unchanged(7, cow, &at, |_| false));
        // 100 blocks is past an item's 6 chunks: no client could pair.
        assert!(pass(&mut kept, 8).unchanged(7, "minecraft:item", &at, |_| false));
        // It moved.
        assert!(!pass(&mut kept, 9).unchanged(7, cow, &place(100.5), tracked));
        // A pass went by without it.
        assert!(!pass(&mut kept, 11).unchanged(7, cow, &place(100.5), tracked));
        // Only what a pass noted stays.
        pass(&mut kept, 12).finish();
        assert!(kept.is_empty());
    }
}
