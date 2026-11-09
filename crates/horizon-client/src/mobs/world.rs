//! What mobs read from the level: block solidity, ray clipping for line of
//! sight (`ClipContext.Block.COLLIDER`), sky visibility, light-dependent
//! values and the positions of other entities.
use super::{MobWorld, Seen, Who, data};
use crate::{
    blocks::{BlockFacts, Fluid},
    collisions::SceneCollisions,
};
use horizon_core::{
    geometry::{Aabb, Vec3, VoxelShape, block_floor_height, traverse_blocks},
    position::BlockPos,
};
use horizon_world::{
    block_collision::{BlockCollisionRegistry, DynamicCollisionWorld},
    registry::BlockStateId,
};

/// Where a target entity is this tick.
#[derive(Clone, Copy, Debug)]
pub struct TargetInfo {
    pub position: Vec3,
    pub eye_y: f64,
    pub bounds: Aabb,
    /// `LivingEntity.getHitbox`: `bounds` cut below the seat of a rider.
    pub hitbox: Aabb,
    pub alive: bool,
    pub can_be_seen_as_enemy: bool,
    pub is_player: bool,
    pub creative: bool,
    /// `isPassenger`.
    pub passenger: bool,
}
thread_local! {
    static NEAR_BUF_POOL: std::cell::RefCell<Vec<Vec<u32>>> = const { std::cell::RefCell::new(Vec::new()) };
}

pub struct NearIter<'a> {
    others: &'a [Seen],
    indices: Option<Vec<u32>>,
    pos: usize,
    fallback: Option<std::slice::Iter<'a, Seen>>,
}

impl<'a> Iterator for NearIter<'a> {
    type Item = &'a Seen;

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        if let Some(indices) = &self.indices
            && self.pos < indices.len()
        {
            let idx = indices[self.pos];
            self.pos += 1;
            return Some(&self.others[idx as usize]);
        }
        if let Some(all) = &mut self.fallback {
            return all.next();
        }
        None
    }

    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        if let Some(indices) = &self.indices {
            let rem = indices.len().saturating_sub(self.pos);
            (rem, Some(rem))
        } else if let Some(all) = &self.fallback {
            all.size_hint()
        } else {
            (0, Some(0))
        }
    }
}

impl Drop for NearIter<'_> {
    fn drop(&mut self) {
        if let Some(mut buf) = self.indices.take() {
            buf.clear();
            NEAR_BUF_POOL.with(|pool| {
                if let Ok(mut pool) = pool.try_borrow_mut()
                    && pool.len() < 8
                {
                    pool.push(buf);
                }
            });
        }
    }
}

/// Chunk and section cache for raycast block traversals.
/// Caches the current section to avoid section index checks and hashmap lookups
/// for consecutive voxels in the same 16x16x16 section.
#[derive(Default)]
struct RaycastBlockCache<'b> {
    chunks: [Option<((i32, i32), Option<&'b crate::chunk::Chunk>)>; 4],
    section_key: (i32, i32, i32),
    section: Option<&'b crate::chunk::Section>,
    section_uniform_air: bool,
}

impl<'a> MobWorld<'a> {
    /// Creates a lightweight view for creature despawn evaluation without entity collections.
    pub fn despawn_view(&self) -> MobWorld<'a> {
        MobWorld {
            others: &[],
            others_grid: None,
            others_index: None,
            others_villagers: None,
            others_solid: None,
            player: None,
            teams: std::sync::Arc::clone(&self.teams),
            ..self.clone()
        }
    }
}

/// Single non-spectator player position helper for creature ticking despawn checks.
pub fn single_player_pos(players: impl IntoIterator<Item = (Vec3, bool)>) -> Option<Option<Vec3>> {
    let mut non_spec = players.into_iter().filter(|(_, spec)| !*spec);
    match (non_spec.next(), non_spec.next()) {
        (Some((pos, _)), None) => Some(Some(pos)),
        (None, _) => Some(None),
        _ => None,
    }
}

impl<'b> RaycastBlockCache<'b> {
    #[inline]
    fn new() -> Self {
        Self {
            chunks: [None, None, None, None],
            section_key: (i32::MIN, i32::MIN, i32::MIN),
            section: None,
            section_uniform_air: false,
        }
    }

    #[inline]
    fn block(&mut self, world: &'b crate::world::World, x: i32, y: i32, z: i32) -> BlockStateId {
        let (cx, cy, cz) = (x >> 4, y >> 4, z >> 4);
        if (cx, cy, cz) == self.section_key {
            if self.section_uniform_air {
                return world.air;
            }
            if let Some(sec) = self.section {
                return BlockStateId(sec.get(crate::chunk::index(x & 15, y, z & 15)));
            }
            return world.air;
        }

        self.section_key = (cx, cy, cz);
        let slot = ((cx as usize).wrapping_mul(5) ^ (cz as usize)) & 3;
        let chunk = match self.chunks[slot] {
            Some(((ccx, ccz), c)) if ccx == cx && ccz == cz => c,
            _ => {
                let c = world.chunks.get(&(cx, cz)).map(|c| c.as_ref());
                self.chunks[slot] = Some(((cx, cz), c));
                c
            }
        };

        match chunk {
            Some(c) => match c.section_index(y) {
                Some(s) => {
                    let sec = c.sections[s].as_ref();
                    self.section = Some(sec);
                    self.section_uniform_air = sec.is_uniform(world.air.0);
                    if self.section_uniform_air {
                        world.air
                    } else {
                        BlockStateId(sec.get(crate::chunk::index(x & 15, y, z & 15)))
                    }
                }
                None => {
                    self.section = None;
                    self.section_uniform_air = true;
                    world.air
                }
            },
            None => {
                self.section = None;
                self.section_uniform_air = true;
                world.air
            }
        }
    }
}

impl MobWorld<'_> {
    /// `Level.getEntity(uuid)`: the entity a UUID a mob stored names, while it
    /// is in the level. Nothing when this world keeps no entity UUIDs, or when
    /// nothing loaded answers to it.
    pub fn who_by_uuid(&self, uuid: [i32; 4]) -> Option<Who> {
        self.who_lookup?.who_by_uuid(uuid)
    }
    /// `LivingEntity.setPosToBed` / `AbstractBedBlock.getSleepHeight`: the
    /// same part selection and outline-height math as Game's bed route.
    pub(super) fn sleeping_position(&self, head: [i32; 3]) -> Option<Vec3> {
        use crate::bed_geometry::{SleepOn, pos_to_bed, shape_top, slept_on};
        let state = self.world.block(head[0], head[1], head[2]);
        let facts = self.facts.get(state);
        if !facts.has_class("AbstractBedBlock") {
            return None;
        }
        let part = self.prop(head[0], head[1], head[2], "part")?;
        if !matches!(part, "head" | "foot") {
            return None;
        }
        let facing = self.prop(head[0], head[1], head[2], "facing")?;
        if !matches!(facing, "north" | "south" | "east" | "west") {
            return None;
        }
        let forward = crate::stand_up::bed_forward(Some(facing));
        let foot = [head[0] - forward[0], head[1], head[2] - forward[2]];
        let on = slept_on(
            &facts.name,
            Some(part),
            Some((
                self.block_name(foot[0], foot[1], foot[2]),
                self.prop(foot[0], foot[1], foot[2], "part"),
            )),
        )?;
        let at = match on {
            SleepOn::Head => head,
            SleepOn::Foot => foot,
        };
        let outline = self.facts.get(self.world.block(at[0], at[1], at[2]));
        // Bed outlines are static. Without an offset context, do not invent
        // a position for a custom outline that requires one.
        if outline.outline_offset {
            return None;
        }
        let boxes: Vec<[f64; 6]> = outline
            .outline
            .iter()
            .map(|b| std::array::from_fn(|i| f64::from(at[i % 3]) + b[i]))
            .collect();
        shape_top(&boxes, f64::from(at[1])).map(|height| pos_to_bed(head, height))
    }
    /// The biome's base temperature at a position.
    pub fn temperature(&self, x: i32, y: i32, z: i32) -> f32 {
        self.world
            .chunks
            .get(&(x >> 4, z >> 4))
            .map_or(0.8, |c| self.biomes.get(c.biome(x, y, z)).temperature)
    }
    /// `Heightmap.Types.MOTION_BLOCKING`: above the highest blocking block.
    pub fn motion_blocking_height(&self, x: i32, z: i32) -> i32 {
        let top = self.world.top_y();
        (self.world.bottom_y()..top)
            .rev()
            .find(|&y| {
                let f = self.facts.get(self.world.block(x, y, z));
                f.full_collision || f.fluid != crate::blocks::Fluid::None
            })
            .map_or(self.world.bottom_y(), |y| y + 1)
    }
    /// BlockState.isSolid (the legacy solid flag).
    pub fn solid(&self, x: i32, y: i32, z: i32) -> bool {
        self.data.state(self.world.block(x, y, z)).flags & data::SOLID != 0
    }
    /// `BlockGetter.getBlockFloorHeight(pos)`: how high the block's collision
    /// shape stands, or how far into it the block below reaches up, minus one
    /// (negative infinity with neither).
    pub fn block_floor_height(&self, x: i32, y: i32, z: i32) -> f64 {
        let mut scene = SceneCollisions {
            world: self.world,
            binding: self.collision,
            subject: None,
        };
        let mut shape = |y: i32| {
            self.collision
                .collision_shape(&mut scene, self.world.block(x, y, z), [x, y, z])
                .unwrap_or_else(|_| VoxelShape::empty())
        };
        let here = shape(y);
        let below = shape(y - 1);
        block_floor_height(&here, || &below)
    }
    pub fn fluid(&self, x: i32, y: i32, z: i32) -> Fluid {
        self.facts.get(self.world.block(x, y, z)).fluid
    }
    pub fn block_name(&self, x: i32, y: i32, z: i32) -> &str {
        &self.facts.get(self.world.block(x, y, z)).name
    }
    /// A block state property's value.
    pub fn prop(&self, x: i32, y: i32, z: i32, key: &str) -> Option<&str> {
        self.registry
            .state(self.world.block(x, y, z))
            .and_then(|s| s.properties.get(key))
            .map(String::as_str)
    }
    pub fn has_tag(&self, tag: &str, block: &str) -> bool {
        self.block_tags.get(tag).is_some_and(|s| s.contains(block))
    }
    /// `findBlocksInBoxByManhattanDistance(at, range_xz, range_y).filterState(..).findFirst()`:
    /// the first block `matches` accepts (given its position and name), going
    /// out from `at` by Manhattan distance as `BlockPos.withinBoxByManhattanDistance` does.
    pub fn nearest_block(
        &self,
        at: [i32; 3],
        range_xz: i32,
        range_y: i32,
        matches: impl Fn([i32; 3], &str) -> bool,
    ) -> Option<[i32; 3]> {
        // Visiting order: distance, then x, then y, then +z before -z.
        let mut best: Option<((i32, i32, i32, bool), [i32; 3])> = None;
        for dx in -range_xz..=range_xz {
            for dy in -range_y..=range_y {
                for dz in -range_xz..=range_xz {
                    let key = (dx.abs() + dy.abs() + dz.abs(), dx, dy, dz < 0);
                    if best.is_some_and(|(k, _)| k <= key) {
                        continue;
                    }
                    let p = [at[0] + dx, at[1] + dy, at[2] + dz];
                    let state = self.world.block(p[0], p[1], p[2]);
                    if !self.facts.is_air(state) && matches(p, &self.facts.get(state).name) {
                        best = Some((key, p));
                    }
                }
            }
        }
        best.map(|(_, p)| p)
    }
    /// Level.clip with COLLIDER shapes (optionally stopping at any fluid): true on a miss.
    pub fn clear_between(&self, from: Vec3, to: Vec3, fluids_block: bool) -> bool {
        self.clear_between_for(None, from, to, fluids_block)
    }
    /// The same for an entity whose `getY()` is `feet_y`: `ClipContext` hands
    /// its `CollisionContext.of(entity)` to every block, so scaffolding is
    /// solid for a viewer above it and open for one below.
    fn clear_between_for(
        &self,
        feet_y: Option<f64>,
        from: Vec3,
        to: Vec3,
        fluids_block: bool,
    ) -> bool {
        let context = feet_y.map(horizon_world::collision_context::CollisionContext::position);
        let mut scene = SceneCollisions {
            world: self.world,
            binding: self.collision,
            subject: None,
        };
        let mut cache = RaycastBlockCache::new();
        let result = traverse_blocks(from, to, 512, |p: BlockPos| {
            let state = cache.block(self.world, p.x, p.y, p.z);
            if fluids_block && self.facts.get(state).fluid != Fluid::None {
                return Some(());
            }
            if cache.section_uniform_air || self.facts.is_air(state) {
                return None;
            }
            if !self.collision.may_collide(state) {
                return None;
            }
            // Scaffolding, liquids (a strider stands on lava) and powder snow
            // take the entity's context; every other state has its cached shape.
            let is_contextual = context.is_some() && self.collision.has_context_rule(state);
            if !is_contextual
                && let Ok(Some(shape)) = self.collision.cached_shapes().cached_shape(state)
            {
                return shape.clip(from, to, p).map(|_| ());
            }
            let shape = match &context {
                Some(context) => self
                    .collision
                    .collision_shape_with_context(&mut scene, state, p, context),
                None => self
                    .collision
                    .collision_shape(&mut scene, state, [p.x, p.y, p.z]),
            }
            .ok()?;
            shape.clip(from, to, p).map(|_| ())
        });
        matches!(result, Ok(None))
    }
    /// Level.clip with VISUAL shapes and no fluids: true on a miss.
    pub fn visual_clear_between(&self, from: Vec3, to: Vec3) -> bool {
        self.visual_clip(from, to) == Some(None)
    }
    /// Level.clip with VISUAL shapes and no fluids: the block the segment
    /// first hits, if it does.
    pub fn visual_hit(&self, from: Vec3, to: Vec3) -> Option<[i32; 3]> {
        self.visual_clip(from, to).flatten()
    }
    fn visual_clip(&self, from: Vec3, to: Vec3) -> Option<Option<[i32; 3]>> {
        let mut scene = SceneCollisions {
            world: self.world,
            binding: self.collision,
            subject: None,
        };
        let cache = std::cell::RefCell::new(RaycastBlockCache::new());
        let state = |x, y, z| cache.borrow_mut().block(self.world, x, y, z);
        visual_clip(self.facts, self.collision, &mut scene, &state, from, to)
    }
    /// Level.clip with COLLIDER shapes and no fluids: where the segment
    /// first hits a block, if it does.
    pub fn clip_block(&self, from: Vec3, to: Vec3) -> Option<Vec3> {
        let mut scene = SceneCollisions {
            world: self.world,
            binding: self.collision,
            subject: None,
        };
        let mut cache = RaycastBlockCache::new();
        let result = traverse_blocks(from, to, 512, |p: BlockPos| {
            let state = cache.block(self.world, p.x, p.y, p.z);
            if cache.section_uniform_air || self.facts.is_air(state) {
                return None;
            }
            if !self.collision.may_collide(state) {
                return None;
            }
            if let Ok(Some(shape)) = self.collision.cached_shapes().cached_shape(state) {
                return shape.clip(from, to, p).map(|hit| hit.location);
            }
            let shape = self
                .collision
                .collision_shape(&mut scene, state, [p.x, p.y, p.z])
                .ok()?;
            shape.clip(from, to, p).map(|hit| hit.location)
        });
        result.ok().flatten()
    }
    /// Level.clip with COLLIDER shapes and no fluids: the block the segment
    /// first hits, if it does.
    pub fn clip_block_at(&self, from: Vec3, to: Vec3) -> Option<[i32; 3]> {
        let mut scene = SceneCollisions {
            world: self.world,
            binding: self.collision,
            subject: None,
        };
        let mut cache = RaycastBlockCache::new();
        let result = traverse_blocks(from, to, 512, |p: BlockPos| {
            let state = cache.block(self.world, p.x, p.y, p.z);
            if cache.section_uniform_air || self.facts.is_air(state) {
                return None;
            }
            if !self.collision.may_collide(state) {
                return None;
            }
            if let Ok(Some(shape)) = self.collision.cached_shapes().cached_shape(state) {
                return shape.clip(from, to, p).map(|_| [p.x, p.y, p.z]);
            }
            let shape = self
                .collision
                .collision_shape(&mut scene, state, [p.x, p.y, p.z])
                .ok()?;
            shape.clip(from, to, p).map(|_| [p.x, p.y, p.z])
        });
        result.ok().flatten()
    }
    /// The bounds of the block's collision shape, relative to the block.
    pub fn collision_bounds(&self, p: [i32; 3]) -> Option<horizon_core::geometry::Aabb> {
        self.collision_bounds_of(self.world.block(p[0], p[1], p[2]), p)
    }
    /// The same for `state` standing at `p` (a block that was there).
    pub fn collision_bounds_of(
        &self,
        state: horizon_world::registry::BlockStateId,
        p: [i32; 3],
    ) -> Option<horizon_core::geometry::Aabb> {
        let mut scene = SceneCollisions {
            world: self.world,
            binding: self.collision,
            subject: None,
        };
        self.collision
            .collision_shape(&mut scene, state, p)
            .ok()?
            .bounds()
    }
    /// LivingEntity.hasLineOfSight: eye to eye within 128 blocks.
    pub fn line_of_sight(&self, from_eye: Vec3, to: Vec3) -> bool {
        self.line_of_sight_for(None, from_eye, to)
    }
    /// The same for a viewer standing at `feet_y` (its collision context).
    pub fn line_of_sight_for(&self, feet_y: Option<f64>, from_eye: Vec3, to: Vec3) -> bool {
        let d =
            (to.x - from_eye.x).powi(2) + (to.y - from_eye.y).powi(2) + (to.z - from_eye.z).powi(2);
        if d > 128.0 * 128.0 {
            return false;
        }
        self.clear_between_for(feet_y, from_eye, to, false)
    }
    /// Level.canSeeSky: sky light 15 at the position (no roof above).
    pub fn can_see_sky(&self, x: i32, y: i32, z: i32) -> bool {
        if !self.dimension_type.height.has_skylight {
            return false;
        }
        self.world.light(x, y, z) >> 4 == 15
    }
    /// Level.precipitationAt: raining, open to the sky, nothing motion
    /// blocking above, then the biome's precipitation at the position:
    /// `Some(true)` rain, `Some(false)` snow, `None` nothing.
    pub fn precipitation_at(&self, x: i32, y: i32, z: i32) -> Option<bool> {
        if !self.raining || !self.can_see_sky(x, y, z) || self.motion_blocking_height(x, z) > y {
            return None;
        }
        let chunk = self.world.chunks.get(&(x >> 4, z >> 4))?;
        let biome = self.biomes.get(chunk.biome(x, y, z));
        if !biome.precipitation {
            return None;
        }
        Some(biome.temperature_at(BlockPos::new(x, y, z), self.sea_level) >= 0.15)
    }
    /// Level.isRainingAt: the precipitation at the position is rain, not snow
    /// or none.
    pub fn raining_at(&self, x: i32, y: i32, z: i32) -> bool {
        self.precipitation_at(x, y, z) == Some(true)
    }
    /// Level.getLightLevelDependentMagicValue: the dimension's brightness curve
    /// at the raw (sky darkened) light level.
    pub fn brightness(&self, x: i32, y: i32, z: i32) -> f32 {
        let light = self.world.light(x, y, z);
        let sky = i32::from(light >> 4) - self.sky_darken();
        let level = sky.max(i32::from(light & 15)).clamp(0, 15) as f32;
        // DimensionType.getBrightness with ambient light 0 (overworld) / 0.1 (nether).
        let f = level / 15.0;
        let base = f / (4.0 - 3.0 * f);
        // The End's ambient light (0.25) is not applied here, as before.
        if self.level == horizon_core::LevelKey::NETHER {
            0.1 + base * 0.9
        } else {
            base
        }
    }
    pub fn sky_darken(&self) -> i32 {
        if self.daylight { 0 } else { 11 }
    }
    pub fn target_info(&self, who: Who) -> Option<TargetInfo> {
        if let Some((id, target)) = self.dragon
            && who == Who::Mob(id)
        {
            return Some(target);
        }
        match who {
            Who::Player => self.player.as_ref().map(|p| TargetInfo {
                position: p.position,
                eye_y: p.eye[1],
                bounds: p.bounds,
                // A riding player's seat is `Avatar.DEFAULT_VEHICLE_ATTACHMENT`
                // (0.6) above its feet.
                hitbox: if p.passenger {
                    let (lo, hi) = (p.bounds.min_position(), p.bounds.max_position());
                    let y = (p.position.y + f64::from(0.6_f32)).max(lo.y);
                    Aabb::new(Vec3::new(lo.x, y, lo.z), hi)
                } else {
                    p.bounds
                },
                alive: p.alive,
                can_be_seen_as_enemy: true,
                is_player: true,
                creative: p.creative,
                passenger: p.passenger,
            }),
            Who::Mob(id) => self.seen(id).map(|s| TargetInfo {
                position: s.position,
                eye_y: s.eye_y,
                bounds: s.bounds,
                hitbox: s.hitbox,
                alive: s.alive,
                can_be_seen_as_enemy: !s.playing_dead,
                is_player: false,
                creative: false,
                passenger: s.passenger,
            }),
        }
    }
    /// The other mobs whose boxes may meet `area` (callers test the boxes),
    /// in snapshot order: through the grid when there is one, else all.
    pub fn others_near(&self, area: Aabb) -> NearIter<'_> {
        let mut buf = NEAR_BUF_POOL.with(|pool| {
            pool.try_borrow_mut()
                .ok()
                .and_then(|mut p| p.pop())
                .unwrap_or_default()
        });
        let indexed = self
            .others_grid
            .is_some_and(|grid| grid.candidates(area, &mut buf));
        if indexed {
            NearIter {
                others: self.others,
                indices: Some(buf),
                pos: 0,
                fallback: None,
            }
        } else {
            buf.clear();
            NEAR_BUF_POOL.with(|pool| {
                if let Ok(mut pool) = pool.try_borrow_mut()
                    && pool.len() < 8
                {
                    pool.push(buf);
                }
            });
            NearIter {
                others: self.others,
                indices: None,
                pos: 0,
                fallback: Some(self.others.iter()),
            }
        }
    }
    pub fn seen(&self, id: u32) -> Option<&Seen> {
        let own = match self.others_index {
            Some(index) => index
                .get(&id)
                .and_then(|&i| self.others.get(i as usize))
                .filter(|s| s.id == id),
            None => self.others.iter().find(|s| s.id == id),
        };
        own.or_else(|| self.foreign?.seen(id))
    }
    /// Living mobs of the given species within `range` of `from`, nearest first.
    pub fn nearby<'s>(
        &'s self,
        from: Vec3,
        range: f64,
        pred: impl Fn(&Seen) -> bool + 's,
    ) -> impl Iterator<Item = &'s Seen> + 's {
        // A position within `range` puts the box's bottom face in this area
        // (one block wider, as the box test is strict).
        let r = range + 1.0;
        let area = Aabb::new(
            Vec3::new(from.x - r, from.y - r, from.z - r),
            Vec3::new(from.x + r, from.y + r, from.z + r),
        );
        let mut v: Vec<(f64, &Seen)> = self
            .others_near(area)
            .filter(|s| {
                s.alive
                    && (s.position.x - from.x).abs() <= range
                    && (s.position.y - from.y).abs() <= range
                    && (s.position.z - from.z).abs() <= range
            })
            .filter(move |s| pred(s))
            .map(|s| {
                let d = (s.position.x - from.x).powi(2)
                    + (s.position.y - from.y).powi(2)
                    + (s.position.z - from.z).powi(2);
                (d, s)
            })
            .collect();
        v.sort_by(|a, b| a.0.total_cmp(&b.0));
        v.into_iter().map(|(_, s)| s)
    }
}

/// Level.clip with VISUAL shapes and no fluids: the block the segment first
/// hits, None inside when it hits nothing and outside when the segment is
/// too long to walk. Glass and copper grates (`TransparentBlock`) and bars and panes
/// (`IronBarsBlock`) have no visual shape; mud, soul sand and powder snow
/// look like full blocks.
pub fn visual_clip<W: DynamicCollisionWorld>(
    facts: &BlockFacts,
    collision: &BlockCollisionRegistry,
    scene: &mut W,
    state: &dyn Fn(i32, i32, i32) -> BlockStateId,
    from: Vec3,
    to: Vec3,
) -> Option<Option<[i32; 3]>> {
    let full = Aabb::new(Vec3::ZERO, Vec3::new(1.0, 1.0, 1.0));
    let result = traverse_blocks(from, to, 512, |p: BlockPos| {
        let id = state(p.x, p.y, p.z);
        if facts.is_air(id) {
            return None;
        }
        let name = facts.get(id).name.trim_start_matches("minecraft:");
        if name.ends_with("glass")
            || name.ends_with("glass_pane")
            || name.ends_with("_bars")
            || name.ends_with("copper_grate")
        {
            return None;
        }
        if matches!(name, "mud" | "soul_sand" | "powder_snow") {
            return full
                .move_by(Vec3::new(f64::from(p.x), f64::from(p.y), f64::from(p.z)))
                .clip(from, to)
                .map(|_| [p.x, p.y, p.z]);
        }
        if let Ok(Some(shape)) = collision.cached_shapes().cached_shape(id) {
            return shape.clip(from, to, p).map(|_| [p.x, p.y, p.z]);
        }
        let shape = collision.collision_shape(scene, id, [p.x, p.y, p.z]).ok()?;
        shape.clip(from, to, p).map(|_| [p.x, p.y, p.z])
    });
    result.ok()
}

/// Runs `f` with an empty overworld (`others` its mobs) and the mob types,
/// both from the extracted vanilla data; false, without running it, when
/// that data is missing.
#[cfg(test)]
pub(crate) fn with_test_world(
    others: &[Seen],
    f: impl FnOnce(&MobWorld<'_>, &super::MobTypes),
) -> bool {
    with_built_test_world(others, |_, _| {}, f)
}
/// `with_test_world` with `build` placing blocks first.
#[cfg(test)]
pub(crate) fn with_built_test_world(
    others: &[Seen],
    build: impl FnOnce(&mut crate::world::World, &horizon_world::registry::BlockRegistry),
    f: impl FnOnce(&MobWorld<'_>, &super::MobTypes),
) -> bool {
    with_built_test_world_rules(others, true, true, build, f)
}
/// `with_built_test_world` with the `mob_griefing` game rule the world reports.
#[cfg(test)]
pub(crate) fn with_built_test_world_rules(
    others: &[Seen],
    mob_griefing: bool,
    mob_drops: bool,
    build: impl FnOnce(&mut crate::world::World, &horizon_world::registry::BlockRegistry),
    f: impl FnOnce(&MobWorld<'_>, &super::MobTypes),
) -> bool {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../.local/horizon-client")
        .join(horizon_core::VANILLA_VERSION);
    with_resource_test_world(&root, others, mob_griefing, mob_drops, build, f)
}
/// The same mob world loaded from a content mod's resource root.
#[cfg(test)]
pub(crate) fn with_resource_test_world(
    root: &std::path::Path,
    others: &[Seen],
    mob_griefing: bool,
    mob_drops: bool,
    build: impl FnOnce(&mut crate::world::World, &horizon_world::registry::BlockRegistry),
    f: impl FnOnce(&MobWorld<'_>, &super::MobTypes),
) -> bool {
    if !root.join("entity-types.json").exists() || !root.join("movement-properties.nbt").exists() {
        return false;
    }
    let assets = crate::assets::Assets::load(root).unwrap();
    let registry = assets.registry;
    let facts = BlockFacts::load(root, registry).unwrap();
    let motion = crate::motion::MotionResources::load(root, &assets).unwrap();
    let data = super::MobData::load(root, &facts);
    let types = super::MobTypes::load(root).unwrap();
    let biomes = crate::biome::Biomes::load(root).unwrap();
    let items = crate::items::Items::load(
        root,
        registry,
        &facts,
        &horizon_world::loot::PackOverlay::NONE,
    )
    .unwrap();
    let collision = std::sync::Arc::new(crate::collisions::load(root, registry).unwrap());
    let suffocation =
        crate::mobs::suffocation::bind(collision.clone(), &motion.causes_suffocation).unwrap();
    let air = registry.default_state("minecraft:air").unwrap();
    let mut world = crate::world::World::new(air, 0);
    build(&mut world, registry);
    let lid_openers = std::collections::HashMap::new();
    let w = MobWorld {
        teams: Default::default(),
        world: &world,
        registry,
        facts: &facts,
        fluid_tables: &motion.fluids,
        movement: &motion.blocks,
        collision: &collision,
        suffocation: &suffocation,
        data: &data,
        player: None,
        controlling_player_movement: None,
        nearest_player: None,
        others,
        dragon: None,
        leash_boats: &[],
        who_lookup: None,
        foreign: None,
        others_grid: None,
        others_index: None,
        others_villagers: None,
        others_solid: None,
        daylight: true,
        monsters_burn: false,
        game_time: 1000,
        day_time: 18000,
        raining: false,
        thundering: false,
        level: horizon_core::LevelKey::OVERWORLD,
        block_tags: &items.block_tags,
        landing: &motion.landing,
        biomes: &biomes,
        mob_griefing,
        universal_anger: false,
        mob_drops,
        max_entity_cramming: 24,
        difficulty: super::spawning::LocalDifficulty::new(2, 18000, 0, 0.0),
        dimension_type: horizon_world::dimension_type::DimensionFacts::overworld(),
        world_seed: 0,
        sea_level: 63,
        items: Some(&items),
        lid_openers: &lid_openers,
        mod_effects: None,
        border: None,
    };
    f(&w, &types);
    true
}
/// A player standing at `position` in survival, as mobs see one.
#[cfg(test)]
pub(crate) fn test_player(position: Vec3) -> super::PlayerView {
    super::PlayerView {
        position,
        bounds: Aabb::new(
            Vec3::new(position.x - 0.3, position.y, position.z - 0.3),
            Vec3::new(position.x + 0.3, position.y + 1.8, position.z + 0.3),
        ),
        eye: [position.x, position.y + 1.62, position.z],
        alive: true,
        creative: false,
        sneaking: false,
        swimming: false,
        look: [0.0; 3],
        main_hand: None,
        off_hand: None,
        head: None,
        chest: None,
        legs: None,
        feet: None,
        armor_cover: 0.0,
        invisible: false,
        spectator: false,
        gold_armor: false,
        hurt_by: None,
        last_hurt: None,
        velocity: Vec3::ZERO,
        yaw: 0.0,
        sprinting: false,
        riding: false,
        hero: false,
        effects: Vec::new(),
        uuid: [0; 4],
        allies: Default::default(),
        remote: false,
        sleeping: false,
        sleep_timer: 0,
        passenger: false,
        riding_vehicle: None,
        vehicle: None,
    }
}

#[cfg(test)]
mod scaffolding_sight_tests {
    use super::*;

    /// ScaffoldingBlock.getCollisionShape: a viewer below the block sees
    /// through it, one above does not (issue 9607).
    #[test]
    fn scaffolding_blocks_sight_only_from_above() {
        let ran = super::with_built_test_world(
            &[],
            |world, registry| {
                world.chunks.insert(
                    (0, 0),
                    std::sync::Arc::new(crate::chunk::Chunk::empty(0, 0)),
                );
                let scaffolding = registry.default_state("minecraft:scaffolding").unwrap();
                world.set(0, 65, 0, scaffolding).unwrap();
            },
            |w, _| {
                let below = Vec3::new(0.5, 64.0 + 1.62, 0.5);
                let above = Vec3::new(0.5, 66.0 + 1.62, 0.5);
                assert!(w.line_of_sight_for(Some(64.0), below, above));
                assert!(!w.line_of_sight_for(Some(66.0), above, below));
            },
        );
        let _ = ran;
    }
}
