//! Per-level mutable simulation state: scheduled block and fluid ticks plus the
//! in-flight neighbour-update queue and the random-tick position generator.
use super::{states::BlockId, update::NeighborUpdater};
use crate::fluid::FluidKind;
use horizon_core::fast_hash::FxHashMap;
use horizon_core::{
    position::{BlockPos, ChunkPos},
    ticks::{
        ChunkTicks, LevelTicks, ScheduleResult, ScheduledTick, TickError, TickPriority, TickTypeId,
    },
};

/// Owned by the caller next to its chunk storage. Chunks must be registered
/// with [`Ticks::add_chunk`] before ticks can be scheduled in them, mirroring
/// vanilla's `LevelTicks.allContainers` (scheduling elsewhere is dropped).
pub struct Ticks {
    /// Block ticks keyed by `BlockId` (vanilla Block registry ID).
    pub blocks: LevelTicks,
    /// Fluid ticks keyed by vanilla Fluid registry ID (`FluidKind as u32`).
    pub fluids: LevelTicks,
    sub_tick: i64,
    /// `Level.randValue`, the LCG behind `getBlockRandomPos`.
    pub random_tick_value: i32,
    pub(crate) updater: NeighborUpdater,
    /// `ComparatorBlockEntity.output` per comparator position (block-entity
    /// state owned by the level; persist it with the chunk if desired).
    pub comparator_outputs: std::collections::HashMap<BlockPos, i32>,
    /// `RedstoneTorchBlock.RECENT_TOGGLES` (position, game time).
    pub torch_toggles: Vec<(BlockPos, i64)>,
    /// Moving-piston block entities in ticker order (render and collide
    /// moving blocks from these; see `piston_shape`).
    pub moving_pistons: Vec<(BlockPos, super::pistons::MovingPiston)>,
    /// Position to id hint for `moving_pistons`, see `moving_piston_slot`.
    pub(crate) moving_piston_ids: FxHashMap<BlockPos, u64>,
    /// The pushes this tick's moving-piston block entities asked for: each
    /// piston with the progress it is about to reach, in the order they ticked
    /// (`PistonMovingBlockEntity.tick`'s `moveCollidedEntities` call). The
    /// caller moves entities by these; `Ticks` has no entities of its own.
    pub piston_pushes: Vec<(BlockPos, super::pistons::MovingPiston, f32)>,
    pub(crate) next_block_entity_id: u64,
    /// `ServerLevel.blockEvents`, run by `Behaviours::run_block_events`.
    /// Changed only through [`Ticks::queue_block_event`], [`Ticks::pop_block_event`]
    /// and [`Ticks::retain_block_events`], which keep `block_event_keys` in step.
    pub block_events: std::collections::VecDeque<super::pistons::BlockEventData>,
    /// The events in `block_events`, for the O(1) dedupe in `queue_block_event`.
    block_event_keys: horizon_core::fast_hash::FxHashSet<super::pistons::BlockEventData>,
    /// `ServerLevel.isHandlingTick` (true while scheduled ticks run).
    pub(crate) handling_tick: bool,
    /// Worldgen shape updates collect the ticks they schedule here, for the
    /// proto chunks of their region, instead of scheduling them.
    pub(crate) capture: Option<Vec<CapturedTick>>,
}
/// A tick scheduled while `Ticks::capture` is set.
#[derive(Clone, Copy, Debug)]
pub(crate) struct CapturedTick {
    pub fluid: bool,
    /// `BlockId` for block ticks, `FluidKind` for fluid ticks.
    pub kind: u32,
    pub pos: BlockPos,
    pub delay: i32,
}
impl Default for Ticks {
    fn default() -> Self {
        Self::new(0)
    }
}
impl Ticks {
    /// `random_tick_seed` seeds `Level.randValue` (vanilla draws it from a
    /// thread-local random at level construction).
    pub fn new(random_tick_seed: i32) -> Self {
        Self {
            blocks: LevelTicks::default(),
            fluids: LevelTicks::default(),
            sub_tick: 0,
            random_tick_value: random_tick_seed,
            updater: NeighborUpdater::default(),
            comparator_outputs: Default::default(),
            torch_toggles: Vec::new(),
            moving_pistons: Vec::new(),
            moving_piston_ids: FxHashMap::default(),
            piston_pushes: Vec::new(),
            next_block_entity_id: 0,
            block_events: Default::default(),
            block_event_keys: Default::default(),
            handling_tick: false,
            capture: None,
        }
    }
    /// Register a loaded chunk's saved block and fluid ticks.
    pub fn add_chunk(
        &mut self,
        pos: ChunkPos,
        blocks: ChunkTicks,
        fluids: ChunkTicks,
    ) -> Result<(), TickError> {
        self.blocks.add_chunk(pos, blocks)?;
        self.fluids.add_chunk(pos, fluids)
    }
    /// A moving-piston block entity loaded with its chunk, as a new ticker.
    pub fn load_moving_piston(&mut self, pos: BlockPos, piston: super::pistons::MovingPiston) {
        self.add_moving_piston(pos, piston);
    }
    /// Replaces the ticker at `pos` (if any) by `piston`, as a new block entity
    /// with the next id, at the end of the ticker order.
    pub(crate) fn add_moving_piston(
        &mut self,
        pos: BlockPos,
        mut piston: super::pistons::MovingPiston,
    ) {
        self.take_moving_piston(pos);
        self.next_block_entity_id += 1;
        piston.id = self.next_block_entity_id;
        self.moving_pistons.push((pos, piston));
        self.moving_piston_ids.insert(pos, piston.id);
    }
    /// Removes the ticker at `pos`; the other tickers keep their order.
    pub(crate) fn take_moving_piston(
        &mut self,
        pos: BlockPos,
    ) -> Option<super::pistons::MovingPiston> {
        let slot = self.moving_piston_slot(pos)?;
        self.moving_piston_ids.remove(&pos);
        Some(self.moving_pistons.remove(slot).1)
    }
    /// Slot of the ticker at `pos` in `moving_pistons`. Every in-crate writer
    /// appends a new, higher id, so the Vec is sorted by id and the hint finds
    /// the slot by binary search. The hint is trusted only when that slot
    /// holds `pos`; a Vec edited from outside falls back to a scan.
    pub(crate) fn moving_piston_slot(&self, pos: BlockPos) -> Option<usize> {
        if let Some(&id) = self.moving_piston_ids.get(&pos)
            && let Ok(slot) = self.moving_pistons.binary_search_by_key(&id, |(_, m)| m.id)
            && self.moving_pistons[slot].0 == pos
        {
            return Some(slot);
        }
        self.moving_pistons.iter().position(|(p, _)| *p == pos)
    }
    /// Whether the ticker `id` is still the one at `pos`.
    pub(crate) fn moving_piston_has(&self, pos: BlockPos, id: u64) -> bool {
        self.moving_piston_slot(pos)
            .is_some_and(|slot| self.moving_pistons[slot].1.id == id)
    }
    /// `ServerLevel.blockEvent`'s dedupe: queue `event` at the back unless the
    /// same event is already queued. Returns whether it was queued.
    pub fn queue_block_event(&mut self, event: super::pistons::BlockEventData) -> bool {
        if self.block_event_keys.insert(event) {
            self.block_events.push_back(event);
            true
        } else {
            false
        }
    }
    /// Take the front block event off the queue (`ServerLevel.runBlockEvents`).
    pub fn pop_block_event(&mut self) -> Option<super::pistons::BlockEventData> {
        let event = self.block_events.pop_front()?;
        self.block_event_keys.remove(&event);
        Some(event)
    }
    /// Keep only the queued block events `keep` accepts (a test area's clear).
    pub fn retain_block_events(
        &mut self,
        keep: impl FnMut(&super::pistons::BlockEventData) -> bool,
    ) {
        self.block_events.retain(keep);
        self.block_event_keys = self.block_events.iter().copied().collect();
    }
    /// Register a chunk without saved ticks.
    pub fn add_empty_chunk(&mut self, pos: ChunkPos) -> Result<(), TickError> {
        self.add_chunk(pos, ChunkTicks::default(), ChunkTicks::default())
    }
    pub fn remove_chunk(&mut self, pos: ChunkPos) -> (Option<ChunkTicks>, Option<ChunkTicks>) {
        (self.blocks.remove_chunk(pos), self.fluids.remove_chunk(pos))
    }
    /// `ServerLevel.nextSubTickCount`, shared by block and fluid ticks.
    pub fn next_sub_tick(&mut self) -> i64 {
        let value = self.sub_tick;
        self.sub_tick = self.sub_tick.wrapping_add(1);
        value
    }
    pub fn schedule_block(
        &mut self,
        game_time: i64,
        pos: BlockPos,
        block: BlockId,
        delay: i32,
        priority: TickPriority,
    ) -> ScheduleResult {
        if let Some(captured) = &mut self.capture {
            captured.push(CapturedTick {
                fluid: false,
                kind: u32::from(block.0),
                pos,
                delay,
            });
            return ScheduleResult::Added;
        }
        let tick = ScheduledTick {
            kind: TickTypeId(u32::from(block.0)),
            pos,
            trigger_tick: game_time.wrapping_add(i64::from(delay)),
            priority,
            sub_tick_order: self.next_sub_tick(),
        };
        self.blocks.schedule(tick)
    }
    pub fn schedule_fluid(
        &mut self,
        game_time: i64,
        pos: BlockPos,
        fluid: FluidKind,
        delay: i32,
    ) -> ScheduleResult {
        if let Some(captured) = &mut self.capture {
            captured.push(CapturedTick {
                fluid: true,
                kind: fluid.index() as u32,
                pos,
                delay,
            });
            return ScheduleResult::Added;
        }
        let tick = ScheduledTick {
            kind: TickTypeId(fluid.index() as u32),
            pos,
            trigger_tick: game_time.wrapping_add(i64::from(delay)),
            priority: TickPriority::Normal,
            sub_tick_order: self.next_sub_tick(),
        };
        self.fluids.schedule(tick)
    }
    pub fn has_block_tick(&self, pos: BlockPos, block: BlockId) -> bool {
        self.blocks
            .has_scheduled(pos, TickTypeId(u32::from(block.0)))
    }
    pub fn will_block_tick_this_tick(&mut self, pos: BlockPos, block: BlockId) -> bool {
        self.blocks
            .will_tick_this_tick(pos, TickTypeId(u32::from(block.0)))
    }
    /// `Level.getBlockRandomPos`.
    pub fn block_random_pos(&mut self, x: i32, y: i32, z: i32, y_mask: i32) -> BlockPos {
        self.random_tick_value = self
            .random_tick_value
            .wrapping_mul(3)
            .wrapping_add(1_013_904_223);
        let value = self.random_tick_value >> 2;
        BlockPos::new(
            x + (value & 15),
            y + ((value >> 16) & y_mask),
            z + ((value >> 8) & 15),
        )
    }
}

#[cfg(test)]
mod block_event_queue_tests {
    use super::Ticks;
    use crate::level::{pistons::BlockEventData, states::BlockId};
    use horizon_core::position::BlockPos;

    fn event(i: i32) -> BlockEventData {
        BlockEventData {
            pos: BlockPos::new(i % 37, i % 11, i / 37),
            block: BlockId((i % 5) as u16),
            param_a: i % 3,
            param_b: 0,
        }
    }

    /// The queue must behave like the old `Vec` (`contains` + `push`,
    /// `remove(0)`) under pushes, duplicates, pops with re-queues during a
    /// run, deferred events put back after the run, and retains.
    #[test]
    fn queue_matches_the_vec_model() {
        let mut ticks = Ticks::new(0);
        let mut model: Vec<BlockEventData> = Vec::new();
        let mut rng = 0x2545_f491_4f6c_dd1du64;
        let mut next = move || {
            rng ^= rng << 13;
            rng ^= rng >> 7;
            rng ^= rng << 17;
            rng
        };
        for round in 0..300 {
            for _ in 0..next() % 40 {
                let e = event((next() % 300) as i32);
                let queued = !model.contains(&e);
                if queued {
                    model.push(e);
                }
                assert_eq!(ticks.queue_block_event(e), queued, "round {round}");
            }
            let mut deferred = Vec::new();
            while !model.is_empty() {
                let old = model.remove(0);
                assert_eq!(ticks.pop_block_event(), Some(old), "round {round}");
                match next() % 6 {
                    0 => deferred.push(old),
                    1 => {
                        // Re-queued during its own run: goes to the back.
                        let queued = !model.contains(&old);
                        if queued {
                            model.push(old);
                        }
                        assert_eq!(ticks.queue_block_event(old), queued);
                    }
                    _ => {}
                }
            }
            for e in deferred {
                let queued = !model.contains(&e);
                if queued {
                    model.push(e);
                }
                assert_eq!(ticks.queue_block_event(e), queued, "reschedule");
            }
            if round % 17 == 0 {
                model.retain(|e| e.pos.x % 2 == 0);
                ticks.retain_block_events(|e| e.pos.x % 2 == 0);
            }
            assert!(ticks.block_events.iter().eq(model.iter()), "round {round}");
        }
        while !model.is_empty() {
            assert_eq!(ticks.pop_block_event(), Some(model.remove(0)));
        }
        assert_eq!(ticks.pop_block_event(), None);
    }

    #[test]
    fn distinct_events_pop_in_queue_order() {
        let mut ticks = Ticks::new(0);
        for i in 0..5000 {
            assert!(ticks.queue_block_event(event(i)));
        }
        assert!(!ticks.queue_block_event(event(4999)), "duplicate dropped");
        assert_eq!(ticks.block_events.len(), 5000);
        for i in 0..5000 {
            assert_eq!(ticks.pop_block_event(), Some(event(i)));
        }
        assert_eq!(ticks.pop_block_event(), None);
        assert!(
            ticks.queue_block_event(event(0)),
            "popped events queue again"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::Ticks;
    use crate::{level::pistons::MovingPiston, registry::BlockStateId};
    use horizon_core::{geometry::Direction, position::BlockPos};

    fn piston(progress: f32) -> MovingPiston {
        MovingPiston {
            moved_state: BlockStateId(1),
            direction: Direction::East,
            extending: true,
            source: false,
            progress,
            progress_o: 0.0,
            last_ticked: 0,
            death_ticks: 0,
            id: 0,
        }
    }

    fn at(i: i32) -> BlockPos {
        BlockPos::new(i, 64, -i)
    }

    /// The linear scan the index replaces: the answer must always match it.
    fn scan(ticks: &Ticks, pos: BlockPos) -> Option<usize> {
        ticks.moving_pistons.iter().position(|(p, _)| *p == pos)
    }

    fn assert_index_matches_scan(ticks: &Ticks) {
        for i in 0..1000 {
            assert_eq!(
                ticks.moving_piston_slot(at(i)),
                scan(ticks, at(i)),
                "pos {i}"
            );
        }
    }

    #[test]
    fn moving_piston_index_matches_a_scan_through_writes_and_external_edits() {
        let mut ticks = Ticks::default();
        for i in 0..500 {
            ticks.load_moving_piston(at(i), piston(0.5));
        }
        for i in (0..500).step_by(3) {
            ticks.add_moving_piston(at(i), piston(0.0));
        }
        for i in (0..500).step_by(7) {
            assert!(ticks.take_moving_piston(at(i)).is_some());
        }
        assert_index_matches_scan(&ticks);
        assert!(ticks.take_moving_piston(at(999)).is_none());
        // Tickers stay in ascending id order, the order the tick loop walks.
        assert!(
            ticks
                .moving_pistons
                .windows(2)
                .all(|w| w[0].1.id < w[1].1.id)
        );

        // Edits from outside the index (a direct push, a reversed Vec) must
        // still resolve to the scan's answer.
        ticks.moving_pistons.push((at(900), piston(0.0)));
        assert_index_matches_scan(&ticks);
        ticks.moving_pistons.reverse();
        assert_index_matches_scan(&ticks);
        assert!(ticks.moving_piston_has(at(900), 0));
        let id = ticks.moving_pistons[scan(&ticks, at(2)).unwrap()].1.id;
        assert!(ticks.moving_piston_has(at(2), id));
        assert!(!ticks.moving_piston_has(at(2), id + 1));
    }
}
