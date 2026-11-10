//! Per-level mutable simulation state: scheduled block and fluid ticks plus the
//! in-flight neighbour-update queue and the random-tick position generator.
use super::{states::BlockId, update::NeighborUpdater};
use crate::fluid::FluidKind;
use horizon_core::{
    position::{BlockPos, ChunkPos},
    ticks::{
        ChunkTicks, LevelTicks, ScheduleResult, ScheduledTick, TickError, TickPriority, TickTypeId,
    },
};
use horizon_core::fast_hash::FxHashMap;

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
    pub block_events: Vec<super::pistons::BlockEventData>,
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
            block_events: Vec::new(),
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
            assert_eq!(ticks.moving_piston_slot(at(i)), scan(ticks, at(i)), "pos {i}");
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
        assert!(ticks.moving_pistons.windows(2).all(|w| w[0].1.id < w[1].1.id));

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
