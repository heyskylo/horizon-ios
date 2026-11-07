//! Blocks driven by the overworld day timeline's environment attributes
//! (`gameplay/eyeblossom_open`, `gameplay/turtle_egg_hatch_chance`) and the
//! dried ghast's hydration ticks.
use super::{LevelAccess, LevelEvent, Sim, update::below};
use crate::registry::BlockStateId;
use horizon_core::{position::BlockPos, random::RandomSource};
use std::sync::Arc;

/// `BlockScanUtils.findBlocks` order: sections by x, z, y; blocks by y, z, x.
pub(crate) fn box_scan_order(min: BlockPos, max: BlockPos) -> Vec<BlockPos> {
    let mut out = Vec::new();
    for sx in (min.x >> 4)..=(max.x >> 4) {
        for sz in (min.z >> 4)..=(max.z >> 4) {
            for sy in (min.y >> 4)..=(max.y >> 4) {
                for y in (sy * 16).max(min.y)..=(sy * 16 + 15).min(max.y) {
                    for z in (sz * 16).max(min.z)..=(sz * 16 + 15).min(max.z) {
                        for x in (sx * 16).max(min.x)..=(sx * 16 + 15).min(max.x) {
                            out.push(BlockPos::new(x, y, z));
                        }
                    }
                }
            }
        }
    }
    out
}

impl<L: LevelAccess + ?Sized> Sim<'_, L> {
    fn day_ticks(&self) -> Option<i64> {
        self.level
            .dimension_type()
            .has_timeline("minecraft:day")
            .then(|| self.level.day_time().rem_euclid(24000))
    }
    /// `EnvironmentAttributes.EYEBLOSSOM_OPEN` (day timeline: open from 12600
    /// to 23400); None (TriState.DEFAULT) outside the overworld.
    fn eyeblossom_open(&self) -> Option<bool> {
        self.day_ticks().map(|t| (12600..23401).contains(&t))
    }
    /// `EnvironmentAttributes.TURTLE_EGG_HATCH_CHANCE`: 1.0 around dawn
    /// (21062..21905), otherwise the 0.002 default.
    fn turtle_hatch_chance(&self) -> f32 {
        match self.day_ticks() {
            Some(t) if (21062..21905).contains(&t) => 1.0,
            _ => 0.002,
        }
    }
    fn named_state(&self, name: &str) -> Option<BlockStateId> {
        self.b
            .table
            .block_by_name(name)
            .map(|b| self.b.default_of(b))
    }
    fn play(&mut self, pos: BlockPos, sound: &str, volume: f32, pitch: f32) {
        self.emit(LevelEvent::Sound {
            pos,
            sound: Arc::from(sound),
            volume,
            pitch,
        });
    }
    /// `EyeblossomBlock.Type.spawnTransformParticle`: four random draws.
    fn eyeblossom_particle_draws(&mut self) {
        for _ in 0..4 {
            self.random().next_double();
        }
    }
    /// `EyeblossomBlock.tryChangingState`; `long` picks the random-tick sound.
    pub(crate) fn eyeblossom_tick(&mut self, state: BlockStateId, pos: BlockPos, long: bool) {
        let b = self.b;
        let open = b.table.name(state) == "minecraft:open_eyeblossom";
        let Some(should_be_open) = self.eyeblossom_open() else {
            return;
        };
        if should_be_open == open {
            return;
        }
        let new_name = if open {
            "minecraft:closed_eyeblossom"
        } else {
            "minecraft:open_eyeblossom"
        };
        let Some(next) = self.named_state(new_name) else {
            return;
        };
        self.set_block_and_update(pos, next);
        self.game_event(pos, "minecraft:block_change");
        self.eyeblossom_particle_draws();
        let min = pos.offset(-3, -2, -3);
        let max = pos.offset(3, 2, 3);
        let block = b.block(state);
        for near in box_scan_order(min, max) {
            if self.state(near) != state {
                continue;
            }
            let (dx, dy, dz) = (near.x - pos.x, near.y - pos.y, near.z - pos.z);
            let distance = f64::from(dx * dx + dy * dy + dz * dz).sqrt();
            let (lo, hi) = ((distance * 5.0) as i32, (distance * 10.0) as i32);
            let delay = self
                .random()
                .next_int_between_inclusive(lo, hi)
                .unwrap_or(lo);
            self.schedule_tick(near, block, delay);
        }
        let sound = match (!open, long) {
            (true, true) => "minecraft:block.eyeblossom.open_long",
            (true, false) => "minecraft:block.eyeblossom.open",
            (false, true) => "minecraft:block.eyeblossom.close_long",
            (false, false) => "minecraft:block.eyeblossom.close",
        };
        self.play(pos, sound, 1.0, 1.0);
    }
    /// `FlowerPotBlock.randomTick` for potted eyeblossoms.
    pub(crate) fn potted_eyeblossom_tick(&mut self, state: BlockStateId, pos: BlockPos) {
        let name = self.b.table.name(state);
        let open = match name {
            "minecraft:potted_open_eyeblossom" => true,
            "minecraft:potted_closed_eyeblossom" => false,
            _ => return,
        };
        let Some(should_be_open) = self.eyeblossom_open() else {
            return;
        };
        if open == should_be_open {
            return;
        }
        let other = if open {
            "minecraft:potted_closed_eyeblossom"
        } else {
            "minecraft:potted_open_eyeblossom"
        };
        let Some(next) = self.named_state(other) else {
            return;
        };
        self.set_block_and_update(pos, next);
        self.eyeblossom_particle_draws();
        let sound = if open {
            "minecraft:block.eyeblossom.close_long"
        } else {
            "minecraft:block.eyeblossom.open_long"
        };
        self.play(pos, sound, 1.0, 1.0);
    }
    /// `TurtleEggBlock.randomTick`: crack, then hatch on sand.
    pub(crate) fn turtle_egg_tick(&mut self, state: BlockStateId, pos: BlockPos) {
        let b = self.b;
        let chance = self.turtle_hatch_chance();
        if !(chance > 0.0 && self.random().next_float() < chance) {
            return;
        }
        if !b
            .tag("minecraft:sand")
            .is_some_and(|t| b.in_set(self.state(below(pos)), t))
        {
            return;
        }
        let hatch = b.table.int(state, b.props.hatch);
        let pitch = 0.9 + self.random().next_float() * 0.2;
        if hatch < 2 {
            self.play(pos, "minecraft:entity.turtle.egg_crack", 0.7, pitch);
            self.set_block(pos, b.table.set_int(state, b.props.hatch, hatch + 1), 2);
            self.game_event(pos, "minecraft:block_change");
        } else {
            self.play(pos, "minecraft:entity.turtle.egg_hatch", 0.7, pitch);
            self.remove_block(pos, false);
            self.game_event(pos, "minecraft:block_destroy");
            for _ in 0..b.table.int(state, b.props.eggs) {
                self.level_event(2001, pos, state.0 as i32);
            }
            // Baby turtles are entities the caller spawns (one per egg).
            self.emit(LevelEvent::BlockEvent {
                pos,
                state,
                kind: "hatch_turtles",
            });
        }
    }
    /// `SnifferEggBlock.tick`: crack twice, then hatch a snifflet.
    pub(crate) fn sniffer_egg_tick(&mut self, state: BlockStateId, pos: BlockPos) {
        let b = self.b;
        let hatch = b.table.int(state, b.props.hatch);
        let pitch = 0.9 + self.random().next_float() * 0.2;
        if hatch < 2 {
            self.play(pos, "minecraft:block.sniffer_egg.crack", 0.7, pitch);
            self.set_block(pos, b.table.set_int(state, b.props.hatch, hatch + 1), 2);
        } else {
            self.play(pos, "minecraft:block.sniffer_egg.hatch", 0.7, pitch);
            self.destroy_block(pos, false, 512);
            // The snifflet is an entity the caller spawns.
            self.emit(LevelEvent::BlockEvent {
                pos,
                state,
                kind: "hatch_sniffer",
            });
        }
    }
    /// `DriedGhastBlock.tick`: hydrate while waterlogged, dry out otherwise.
    pub(crate) fn dried_ghast_tick(&mut self, state: BlockStateId, pos: BlockPos) {
        let b = self.b;
        let Some(key) = b.table.key_of("hydration") else {
            return;
        };
        let hydration = b.table.int(state, key);
        if b.table.bool(state, b.props.waterlogged) {
            if hydration < 3 {
                self.play(pos, "minecraft:block.dried_ghast.transition", 1.0, 1.0);
                self.set_block(pos, b.table.set_int(state, key, hydration + 1), 2);
                self.game_event(pos, "minecraft:block_change");
            } else {
                self.remove_block(pos, false);
                // The happy ghastling is an entity the caller spawns.
                self.emit(LevelEvent::BlockEvent {
                    pos,
                    state,
                    kind: "spawn_ghastling",
                });
            }
        } else if hydration > 0 {
            self.set_block(pos, b.table.set_int(state, key, hydration - 1), 2);
            self.game_event(pos, "minecraft:block_change");
        }
    }
    /// `DriedGhastBlock.randomTick`: keep a 5000-tick hydration tick scheduled.
    pub(crate) fn dried_ghast_random_tick(&mut self, state: BlockStateId, pos: BlockPos) {
        let b = self.b;
        let hydration = b
            .table
            .key_of("hydration")
            .map_or(0, |k| b.table.int(state, k));
        let block = b.block(state);
        if (b.table.bool(state, b.props.waterlogged) || hydration > 0)
            && !self.ticks.has_block_tick(pos, block)
        {
            self.schedule_tick(pos, block, 5000);
        }
    }
}
