//! A level's `ServerEntity`s by id (`ChunkMap.entityMap`), found without hashing on most ticks.
//!
//! A level tracks tens of thousands of entities, most of them items and mobs frozen past the
//! simulation distance, and looked each one up by id on every tick: one probe of a large table
//! for each, missing the cache nearly every time. The game describes its entities in the same
//! order every tick (its lists keep their order), so each tick notes the order it met them in,
//! and the next one finds each entity at or just past where it found the one before. Only an
//! entity that joined, or one met out of order, is looked up by id.

use super::ServerEntity;
use crate::fast_hash::FxHashMap;

/// How many entries past the last one found an entity is looked for: the entities that left
/// the level since are skipped over.
const LOOK_AHEAD: usize = 4;

#[derive(Default)]
pub(super) struct ServerEntities {
    /// The entities and their ids; None in a slot that is free.
    slots: Vec<Option<(i32, ServerEntity)>>,
    free: Vec<u32>,
    by_id: FxHashMap<i32, u32>,
    /// The last tick's entities in the order it met them (id, slot), this tick's so far, and
    /// where the next one is looked for.
    order: Vec<(i32, u32)>,
    next_order: Vec<(i32, u32)>,
    cursor: usize,
}

impl ServerEntities {
    pub(super) fn len(&self) -> usize {
        self.by_id.len()
    }

    pub(super) fn contains_key(&self, id: &i32) -> bool {
        self.by_id.contains_key(id)
    }

    pub(super) fn get(&self, id: &i32) -> Option<&ServerEntity> {
        self.by_id.get(id).map(|&slot| self.at(slot))
    }

    pub(super) fn at(&self, slot: u32) -> &ServerEntity {
        &self.slots[slot as usize].as_ref().expect("a live slot").1
    }

    pub(super) fn at_mut(&mut self, slot: u32) -> &mut ServerEntity {
        &mut self.slots[slot as usize].as_mut().expect("a live slot").1
    }

    /// Adds the entity `id` (not tracked yet), returning its slot.
    pub(super) fn insert(&mut self, id: i32, s: ServerEntity) -> u32 {
        debug_assert!(!self.by_id.contains_key(&id));
        let slot = match self.free.pop() {
            Some(slot) => {
                self.slots[slot as usize] = Some((id, s));
                slot
            }
            None => {
                self.slots.push(Some((id, s)));
                (self.slots.len() - 1) as u32
            }
        };
        self.by_id.insert(id, slot);
        slot
    }

    /// Starts meeting a tick's entities in order.
    pub(super) fn begin(&mut self) {
        self.cursor = 0;
        self.next_order.clear();
    }

    /// The slot of the entity `id`, met next in this tick's order, and whether it was found by
    /// id (not where the last tick's order had it). Notes it in this tick's order.
    pub(super) fn meet(&mut self, id: i32) -> Option<(u32, bool)> {
        let end = self.order.len().min(self.cursor + LOOK_AHEAD);
        let found = match (self.cursor..end).find(|&k| self.order[k].0 == id && self.holds(k, id)) {
            Some(k) => {
                self.cursor = k + 1;
                Some((self.order[k].1, false))
            }
            None => self.by_id.get(&id).map(|&slot| (slot, true)),
        };
        if let Some((slot, _)) = found {
            self.next_order.push((id, slot));
        }
        found
    }

    /// Whether the last tick's `k`-th slot still holds the entity `id`.
    fn holds(&self, k: usize, id: i32) -> bool {
        (self.slots[self.order[k].1 as usize].as_ref()).is_some_and(|(held, _)| *held == id)
    }

    /// Notes an entity met this tick that was not tracked before it.
    pub(super) fn met_new(&mut self, id: i32, slot: u32) {
        self.next_order.push((id, slot));
    }

    /// The next entity is looked for after the last tick's `index`-th: where the one just
    /// found by id was then.
    pub(super) fn resume_after(&mut self, index: usize) {
        self.cursor = index + 1;
    }

    /// This tick's order is the one the next tick looks in; the slot of the tick's
    /// `index`-th entity, until then.
    pub(super) fn end(&mut self) {
        std::mem::swap(&mut self.order, &mut self.next_order);
    }

    pub(super) fn slot_of_index(&self, index: usize) -> u32 {
        self.order[index].1
    }

    /// Keeps only the entities `keep` says to.
    pub(super) fn retain(&mut self, mut keep: impl FnMut(&ServerEntity) -> bool) {
        for (slot, entry) in self.slots.iter_mut().enumerate() {
            if let Some((id, s)) = entry
                && !keep(s)
            {
                self.by_id.remove(id);
                *entry = None;
                self.free.push(slot as u32);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::{EntityState, Riding};
    use super::*;

    fn entity(id: i32) -> ServerEntity {
        let e = EntityState {
            id,
            kind: "minecraft:item".into(),
            ..Default::default()
        };
        ServerEntity::new(&e, Some(1), &Riding::default())
    }

    /// One tick meeting `ids` in order: each one's slot and whether it was
    /// found by id; the new ones are added.
    fn tick(all: &mut ServerEntities, ids: &[i32]) -> Vec<(u32, bool)> {
        all.begin();
        let met = (ids.iter())
            .map(|&id| {
                all.meet(id).unwrap_or_else(|| {
                    let slot = all.insert(id, entity(id));
                    all.met_new(id, slot);
                    (slot, true)
                })
            })
            .collect();
        all.end();
        met
    }

    /// The entities met in last tick's order are found there, past the ones
    /// that left; the rest by id, each in its own slot, freed ones reused.
    #[test]
    fn entities_are_found_in_last_ticks_order_or_by_id() {
        let mut all = ServerEntities::default();
        let first = tick(&mut all, &[10, 11, 12, 13, 14, 15]);
        assert_eq!(
            first.iter().map(|m| m.0).collect::<Vec<_>>(),
            [0, 1, 2, 3, 4, 5]
        );
        // 11 and 12 left; 20 joined between 13 and 14.
        let met = tick(&mut all, &[10, 13, 20, 14, 15]);
        assert_eq!(
            met,
            [(0, false), (3, false), (6, true), (4, false), (5, false)]
        );
        // 15 out of order is found by id; the next ones in order again.
        let met = tick(&mut all, &[15, 10, 20]);
        assert_eq!(met, [(5, true), (0, false), (6, false)]);
        // Only those three are left in the level: the others' slots go to
        // the next ones to join.
        for (slot, _) in &met {
            all.at_mut(*slot).seen = 1;
        }
        all.retain(|s| s.seen == 1);
        assert_eq!(all.len(), 3);
        assert!(!all.contains_key(&13) && all.get(&20).is_some());
        let met = tick(&mut all, &[15, 10, 20, 30, 31, 32]);
        let slots: Vec<u32> = met.iter().map(|m| m.0).collect();
        assert_eq!(slots, [5, 0, 6, 4, 3, 2]);
        assert_eq!(all.slot_of_index(4), 3);
    }
}
