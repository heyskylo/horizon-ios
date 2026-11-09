//! The block phases of a level's tick (`ServerLevel.tick`): scheduled ticks, random ticks in the
//! chunks the tickets simulate, block events and moving pistons (`tick_blocks`).
use super::*;

impl Game<'_> {
    /// ServerLevel.tick block phases: scheduled ticks, random ticks in the
    /// chunks the tickets simulate, block events and moving pistons.
    /// The length the block log had after the changed blocks went out
    /// (`ServerChunkCache.tick`'s broadcast), before the block events and the
    /// block entities ran.
    pub(super) fn tick_blocks(&mut self) -> Result<Option<usize>> {
        let mut scratch = std::mem::take(&mut self.tick_scratch);
        let result = self.tick_blocks_with_scratch(&mut scratch);
        self.tick_scratch = scratch;
        result
    }
    pub(super) fn tick_blocks_with_scratch(
        &mut self,
        scratch: &mut TickScratch,
    ) -> Result<Option<usize>> {
        let mut laps = horizon_client::spans::laps();
        // ServerChunkCache.tick: the tickets that time out.
        let world = &self.world;
        self.tickets
            .tick_portal(|ticket| portal_ticket_can_expire(world, ticket));
        self.sync_tick_chunks();
        self.start_ticking_chunks();
        laps.lap("tb_tickets");
        let levels = self.tickets.levels();
        let bounds = levels.random_tick_bounds();
        // Keep the world map's chunk order, including empty chunks
        // (tick_chunk_random still draws for precipitation in those).
        scratch.ticking.clear();
        scratch
            .ticking
            .extend(self.world.chunks.keys().copied().filter(|&(x, z)| {
                if let Some((min_x, min_z, max_x, max_z)) = bounds
                    && (x < min_x || x > max_x || z < min_z || z > max_z)
                {
                    return false;
                }
                levels.random_ticking(x, z) && ticking_chunk(&self.world, x, z)
            }));
        laps.lap("tb_chunks");
        // Entity.checkInsideBlocks for the plates, tripwires, detector rails and
        // buttons under the players (the host and the LAN guests), the minecarts
        // and the arrows (the mobs and other entities are added in `with_level`
        // below).
        scratch.inside.clear();
        let guests = self.lan_block_trigger_boxes().into_iter().map(|(_, b)| b);
        let boxes = self
            .player
            .iter()
            .filter(|_| !self.spectator())
            .map(|p| {
                let w = f64::from(p.dimensions.width) / 2.0;
                let h = f64::from(p.dimensions.height);
                (
                    [p.position.x - w, p.position.y, p.position.z - w],
                    [p.position.x + w, p.position.y + h, p.position.z + w],
                )
            })
            .chain(self.carts.iter().map(|c| {
                let p = c.position;
                (
                    [p.x - 0.49, p.y, p.z - 0.49],
                    [p.x + 0.49, p.y + 0.7, p.z + 0.49],
                )
            }))
            .chain(guests);
        for (lo, hi) in boxes {
            for x in (lo[0] + 1e-7).floor() as i32..=(hi[0] - 1e-7).floor() as i32 {
                for y in (lo[1] + 1e-7).floor() as i32..=(hi[1] - 1e-7).floor() as i32 {
                    for z in (lo[2] + 1e-7).floor() as i32..=(hi[2] - 1e-7).floor() as i32 {
                        scratch.inside.push(BlockPos::new(x, y, z));
                    }
                }
            }
        }
        // AbstractArrow.applyEffectsFromBlocks: the blocks every arrow passed
        // through this tick (a wooden button is pressed by one inside it), and
        // the ones a thrown trident is in.
        for p in self
            .creatures
            .projectiles
            .iter()
            .filter(|p| p.arrow.is_some())
        {
            scratch.inside.extend(horizon_client::arrows::blocks_inside(
                p.previous, p.position,
            ));
        }
        for t in &self.tridents {
            scratch.inside.extend(horizon_client::arrows::blocks_inside(
                t.position, t.position,
            ));
        }
        // BigDripleafBlock.entityInside also asks whether the player stands on
        // the ground and how high its feet are: the leaves its box's feet
        // layer is inside (the host and the LAN guests).
        scratch.standing.clear();
        let feet = self
            .player
            .iter()
            .filter(|_| !self.spectator())
            .map(|p| {
                (
                    [p.position.x, p.position.y, p.position.z],
                    p.ground.on_ground,
                    f64::from(p.dimensions.width) / 2.0,
                )
            })
            .chain(
                self.lan_guest_feet()
                    .into_iter()
                    .map(|(at, on_ground)| (at, on_ground, 0.3)),
            );
        for (at, on_ground, w) in feet {
            let y = (at[1] + 1e-7).floor() as i32;
            for x in (at[0] - w + 1e-7).floor() as i32..=(at[0] + w - 1e-7).floor() as i32 {
                for z in (at[2] - w + 1e-7).floor() as i32..=(at[2] + w - 1e-7).floor() as i32 {
                    scratch
                        .standing
                        .push((BlockPos::new(x, y, z), on_ground, at[1]));
                }
            }
        }
        // ServerLevel.tick ticks only the entities in entity-ticking chunks, and
        // applyEffectsFromBlocks runs in their tick: a mob or item past the
        // simulation distance presses, burns and slows on nothing.
        let entity_ticking = self.entity_ticking_area();
        let first_contact_drop = self.next_entity + 1;
        self.with_level(|b, level, ticks| {
            // LilyPadBlock.entityInside: a boat inside a lily pad breaks it.
            scratch.boat_cells.clear();
            for bounds in level.boat_trigger_boxes() {
                let (lo, hi) = (bounds.min_position(), bounds.max_position());
                for x in (lo.x + 1e-7).floor() as i32..=(hi.x - 1e-7).floor() as i32 {
                    for y in (lo.y + 1e-7).floor() as i32..=(hi.y - 1e-7).floor() as i32 {
                        for z in (lo.z + 1e-7).floor() as i32..=(hi.z - 1e-7).floor() as i32 {
                            scratch.boat_cells.push(BlockPos::new(x, y, z));
                        }
                    }
                }
            }
            for pos in scratch.boat_cells.drain(..) {
                b.boat_inside(level, ticks, pos);
            }
            // The mobs and the other entities (items, orbs, projectiles...) too.
            scratch.touched.clear();
            use entity::{Entity as _, for_each_entity};
            let mut touch = |bounds: Aabb| {
                let (lo, hi) = (bounds.min_position(), bounds.max_position());
                for x in (lo.x + 1e-7).floor() as i32..=(hi.x - 1e-7).floor() as i32 {
                    for y in (lo.y + 1e-7).floor() as i32..=(hi.y - 1e-7).floor() as i32 {
                        for z in (lo.z + 1e-7).floor() as i32..=(hi.z - 1e-7).floor() as i32 {
                            scratch.touched.push(BlockPos::new(x, y, z));
                        }
                    }
                }
            };
            for_each_entity!(level.entities, entity => {
                if entity.triggers_blocks() && entity.sweeps_blocks_inside() {
                    let at = entity.position();
                    if entity_ticking(at.x.floor() as i32, at.z.floor() as i32) {
                        touch(entity.bounds());
                    }
                }
            });
            for crystal in level.crystals {
                touch(crystal.bounds());
            }
            for &(bounds, applies) in &level.trigger_hooks {
                if applies {
                    touch(bounds);
                }
            }
            for &bounds in &level.trigger_extras {
                touch(bounds);
            }
            for pos in scratch.inside.drain(..) {
                b.entity_inside(level, ticks, pos);
            }
            // The touched blocks go once each, in (x, y, z) order. Only an
            // entity-sensing block does anything there, so the others are
            // passed over while no block has changed since they were looked
            // at (they are still not sensing), and only the few sensing ones
            // are sorted. Once a block changes, every later one goes, as the
            // whole sorted list would.
            let key = |p: &BlockPos| (p.x, p.y, p.z);
            scratch.sensing.clear();
            scratch.rest.clear();
            for pos in scratch.touched.drain(..) {
                if b.senses_entities(level.world.block(pos.x, pos.y, pos.z)) {
                    scratch.sensing.push(pos);
                } else {
                    scratch.rest.push(pos);
                }
            }
            scratch.sensing.sort_unstable_by_key(key);
            scratch.sensing.dedup();
            let looked = (
                level.world.revision,
                level.world.chunks.membership_revision(),
            );
            let mut i = 0;
            while i < scratch.sensing.len() {
                let pos = scratch.sensing[i];
                b.entity_inside(level, ticks, pos);
                i += 1;
                let now = (
                    level.world.revision,
                    level.world.chunks.membership_revision(),
                );
                if now != looked {
                    scratch.later.clear();
                    scratch.later.append(&mut scratch.rest);
                    scratch.later.extend(scratch.sensing.drain(i..));
                    scratch.later.retain(|p| key(p) > key(&pos));
                    scratch.later.sort_unstable_by_key(key);
                    scratch.later.dedup();
                    for pos in scratch.later.drain(..) {
                        b.entity_inside(level, ticks, pos);
                    }
                    break;
                }
            }
            for (pos, on_ground, feet_y) in scratch.standing.drain(..) {
                b.entity_on_big_dripleaf(level, ticks, pos, on_ground, feet_y);
            }
        });
        self.process_level_events()?;
        self.contact_drops = first_contact_drop..self.next_entity + 1;
        laps.lap("tb_entity_inside");
        // ServerLevel.tick: a Debug world runs no scheduled block or fluid ticks, and
        // ServerChunkCache.tickChunks no random ticks.
        let debug = self.world_debug;
        if !debug {
            self.tick_scheduled()?;
        }
        self.with_level(|b, level, ticks| {
            horizon_client::spans::time("random_ticks", || {
                for (x, z) in scratch.ticking.drain(..).filter(|_| !debug) {
                    // The last answer, asked from Y `from` at a world revision:
                    // the next ticking section and its mask. It stands for a
                    // later Y up to that section's until a block changes.
                    let mut last: Option<(u64, i32, Option<(i32, std::rc::Rc<_>)>)> = None;
                    let pos = horizon_core::position::ChunkPos::new(x, z);
                    b.tick_chunk_random_masked(level, ticks, pos, |level, from| {
                        let revision = level.world.revision;
                        let stands = last.as_ref().is_some_and(|(r, f, next)| {
                            *r == revision
                                && *f <= from
                                && next.as_ref().is_none_or(|(y, _)| from <= *y)
                        });
                        if !stands {
                            let next = level.world.chunks.get(&(x, z)).and_then(|chunk| {
                                level.random_tick_sections.next_masked(
                                    (x, z),
                                    chunk,
                                    level.facts,
                                    |id| b.random_tick_candidate(BlockStateId(id)),
                                    from,
                                )
                            });
                            last = Some((revision, from, next));
                        }
                        last.as_ref()
                            .and_then(|(_, _, next)| next.as_ref())
                            .map(|(y, mask)| (*y, Some(mask.clone())))
                    });
                }
            });
        });
        laps = horizon_client::spans::laps();
        let sent = self.lan_broadcast_changes();
        laps.lap("tb_broadcast");
        self.save_chunks_eagerly();
        self.with_level(|b, level, ticks| {
            b.run_block_events(level, ticks);
        });
        laps.lap("tb_block_events");
        self.process_level_events()?;
        laps.lap("tb_level_events");
        self.sync_moving_pistons();
        laps.lap("tb_pistons");
        Ok(sent)
    }
}
