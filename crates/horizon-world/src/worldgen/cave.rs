//! Underground structures expressed as features: lakes, monster rooms
//! (dungeons) and geodes.
use super::{
    DecodeError, Object,
    block_feature::FeatureBlockBindings,
    block_predicate::BlockPredicate,
    double_range,
    feature::FeatureWorld,
    feature_graph::FeatureExecutionError,
    int_range, optional,
    placement::{PlacementContext, PlacementQueryError},
    required,
    state_provider::BlockStateProvider,
    tree::{TreeDecoder, TreeResult, kind, read, sample_state, write},
    tree_blocks::{self as tb, TreeBlocks},
    value_provider::IntProvider,
};
use crate::registry::BlockStateId;
use horizon_core::{
    geometry::Direction,
    noise::{Noise, NormalNoise},
    random::{LegacyRandom, RandomSource},
};
use std::{collections::BTreeSet, sync::Arc};

fn query<E>(error: PlacementQueryError) -> FeatureExecutionError<E> {
    FeatureExecutionError::Query(error)
}
fn state<E>(blocks: &TreeBlocks, name: &str) -> TreeResult<BlockStateId, E> {
    blocks
        .default_state(name)
        .ok_or(FeatureExecutionError::InvalidState)
}
/// Feature.safeSetBlock.
fn safe_set<W: FeatureWorld>(
    world: &mut W,
    p: [i32; 3],
    value: BlockStateId,
    cannot_replace: impl Fn(BlockStateId) -> bool,
) -> TreeResult<(), W::Error> {
    if !cannot_replace(read(world, p)?) {
        write(world, p, value, 2)?;
    }
    Ok(())
}
/// Feature.markAboveForPostProcessing.
fn mark_above<W: FeatureWorld + PlacementContext>(
    world: &mut W,
    blocks: &TreeBlocks,
    p: [i32; 3],
) -> TreeResult<(), W::Error> {
    let mut above = p;
    for _ in 0..2 {
        above = tb::offset(above, 0, 1, 0);
        if blocks.is_air(read(world, above)?) {
            return Ok(());
        }
        world.mark_postprocessing(above).map_err(query)?;
    }
    Ok(())
}

#[derive(Clone, Debug)]
pub struct LakeFeature {
    fluid: BlockStateProvider,
    barrier: BlockStateProvider,
    can_place: BlockPredicate,
    can_replace_air_or_fluid: BlockPredicate,
    can_replace_barrier: BlockPredicate,
    blocks: Arc<TreeBlocks>,
}
impl LakeFeature {
    pub(crate) fn decode(
        json: &str,
        bindings: &FeatureBlockBindings<'_>,
        depth: usize,
        remaining: &mut usize,
    ) -> Result<Self, DecodeError> {
        let object: Object = serde_json::from_str(json)?;
        if kind(&object)? != "minecraft:lake" {
            return Err(DecodeError::Field("lake type"));
        }
        let blocks = bindings.tree_blocks()?;
        let mut decoder = TreeDecoder {
            bindings,
            depth,
            remaining,
        };
        let fluid = decoder.provider(required(&object, "fluid")?)?;
        let barrier = decoder.provider(required(&object, "barrier")?)?;
        let mut predicate = |name: &'static str| {
            BlockPredicate::decode_with_blocks(
                required(&object, name)?.get(),
                depth + 1,
                decoder.remaining,
                Some(bindings),
            )
        };
        Ok(Self {
            fluid,
            barrier,
            can_place: predicate("can_place_feature")?,
            can_replace_air_or_fluid: predicate("can_replace_with_air_or_fluid")?,
            can_replace_barrier: predicate("can_replace_with_barrier")?,
            blocks,
        })
    }
    pub(crate) fn place<W: FeatureWorld + PlacementContext, R: RandomSource>(
        &self,
        world: &mut W,
        random: &mut R,
        origin: [i32; 3],
    ) -> TreeResult<bool, W::Error> {
        if origin[1] <= world.min_y().map_err(query)?.wrapping_add(4) {
            return Ok(false);
        }
        let origin = tb::offset(origin, -8, -4, -8);
        // Vanilla's 16x16x8 grid, one byte per column (bit y).
        let mut grid = [0u8; 256];
        let spots = random.next_int_bound(4)? + 4;
        for _ in 0..spots {
            let xr = random.next_double() * 6.0 + 3.0;
            let yr = random.next_double() * 4.0 + 2.0;
            let zr = random.next_double() * 6.0 + 3.0;
            let xp = random.next_double() * (16.0 - xr - 2.0) + 1.0 + xr / 2.0;
            let yp = random.next_double() * (8.0 - yr - 4.0) + 2.0 + yr / 2.0;
            let zp = random.next_double() * (16.0 - zr - 2.0) + 1.0 + zr / 2.0;
            // Each axis term once per coordinate: the same expressions and
            // sum as vanilla's, without three divisions per cell.
            let square = |v: i32, p: f64, r: f64| {
                let d = (f64::from(v) - p) / (r / 2.0);
                d * d
            };
            let xs: [f64; 16] = std::array::from_fn(|v| square(v as i32, xp, xr));
            let ys: [f64; 8] = std::array::from_fn(|v| square(v as i32, yp, yr));
            let zs: [f64; 16] = std::array::from_fn(|v| square(v as i32, zp, zr));
            for xx in 1..15 {
                for zz in 1..15 {
                    for yy in 1..7 {
                        if xs[xx] + ys[yy] + zs[zz] < 1.0 {
                            grid[xx * 16 + zz] |= 1 << yy;
                        }
                    }
                }
            }
        }
        // Vanilla's edge test (not in the lake, a lake neighbour within the
        // grid) for a whole column at once.
        let edges: [u8; 256] = std::array::from_fn(|i| {
            let (x, z) = (i / 16, i % 16);
            let c = grid[i];
            let mut near = (c << 1) | (c >> 1);
            if x < 15 {
                near |= grid[i + 16];
            }
            if x > 0 {
                near |= grid[i - 16];
            }
            if z < 15 {
                near |= grid[i + 1];
            }
            if z > 0 {
                near |= grid[i - 1];
            }
            !c & near
        });
        // The Ys of a column's set bits, ascending (vanilla's Y loop order).
        let ys = |bits: u8| (0..8).filter(move |y| bits >> y & 1 != 0);
        let fluid = sample_state(&self.fluid, world, random, origin)?;
        for xx in 0..16 {
            for zz in 0..16 {
                for yy in ys(edges[(xx * 16 + zz) as usize]) {
                    let p = tb::offset(origin, xx, yy, zz);
                    let state = read(world, p)?;
                    if yy >= 4 && self.blocks.has(state, tb::LIQUID) {
                        return Ok(false);
                    }
                    if yy < 4 && !self.blocks.has(state, tb::SOLID) && state != fluid {
                        return Ok(false);
                    }
                    if !self.can_place.test(world, p).map_err(query)? {
                        return Ok(false);
                    }
                }
            }
        }
        let cave_air = state(&self.blocks, "minecraft:cave_air")?;
        for xx in 0..16 {
            for zz in 0..16 {
                for yy in ys(grid[(xx * 16 + zz) as usize]) {
                    let p = tb::offset(origin, xx, yy, zz);
                    if self
                        .can_replace_air_or_fluid
                        .test(world, p)
                        .map_err(query)?
                    {
                        let air = yy >= 4;
                        write(world, p, if air { cave_air } else { fluid }, 2)?;
                        if air {
                            world.schedule_block_tick(p, cave_air, 0).map_err(query)?;
                            mark_above(world, &self.blocks, p)?;
                        }
                    }
                }
            }
        }
        let barrier = sample_state(&self.barrier, world, random, origin)?;
        if !self.blocks.is_air(barrier) {
            for xx in 0..16 {
                for zz in 0..16 {
                    for yy in ys(edges[(xx * 16 + zz) as usize]) {
                        if yy < 4 || random.next_int_bound(2)? != 0 {
                            let p = tb::offset(origin, xx, yy, zz);
                            let state = read(world, p)?;
                            if self.blocks.has(state, tb::SOLID)
                                && self.can_replace_barrier.test(world, p).map_err(query)?
                            {
                                write(world, p, barrier, 2)?;
                                mark_above(world, &self.blocks, p)?;
                            }
                        }
                    }
                }
            }
        }
        if self.blocks.has(fluid, tb::WATER_FLUID) {
            let ice = state(&self.blocks, "minecraft:ice")?;
            for xx in 0..16 {
                for zz in 0..16 {
                    let p = tb::offset(origin, xx, 4, zz);
                    let biome = world.biome_identity(p).map_err(query)?;
                    if world.biome_should_freeze(biome, p, false).map_err(query)?
                        && self
                            .can_replace_air_or_fluid
                            .test(world, p)
                            .map_err(query)?
                    {
                        write(world, p, ice, 2)?;
                    }
                }
            }
        }
        Ok(true)
    }
}

const DUNGEON_MOBS: [&str; 4] = [
    "minecraft:skeleton",
    "minecraft:zombie",
    "minecraft:zombie",
    "minecraft:spider",
];
/// Initial data for a fresh dungeon spawner, preserving the generation
/// sink's fields and insertion order. The entity was already selected.
pub(crate) fn dungeon_spawner_nbt(entity: &str) -> crate::nbt::Compound {
    use crate::nbt::Tag;
    let mut mob = crate::nbt::Compound::new();
    mob.insert("id".into(), Tag::String(entity.into()));
    let mut data = crate::nbt::Compound::new();
    data.insert("entity".into(), Tag::Compound(mob));
    let mut nbt = crate::nbt::Compound::new();
    nbt.insert("id".into(), Tag::String("minecraft:mob_spawner".into()));
    nbt.insert("SpawnData".into(), Tag::Compound(data));
    nbt.insert("SpawnPotentials".into(), Tag::List(Vec::new()));
    nbt
}
#[derive(Clone, Debug)]
pub struct MonsterRoomFeature {
    blocks: Arc<TreeBlocks>,
}
impl MonsterRoomFeature {
    pub(crate) fn new(bindings: &FeatureBlockBindings<'_>) -> Result<Self, DecodeError> {
        Ok(Self {
            blocks: bindings.tree_blocks()?,
        })
    }
    fn solid<W: FeatureWorld>(&self, world: &mut W, p: [i32; 3]) -> TreeResult<bool, W::Error> {
        Ok(self.blocks.has(read(world, p)?, tb::SOLID))
    }
    pub(crate) fn place<W: FeatureWorld + PlacementContext, R: RandomSource>(
        &self,
        world: &mut W,
        random: &mut R,
        origin: [i32; 3],
    ) -> TreeResult<bool, W::Error> {
        let blocks = &*self.blocks;
        let cannot = |s: BlockStateId| blocks.has(s, tb::FEATURES_CANNOT_REPLACE);
        let xr = random.next_int_bound(2)? + 2;
        let (min_x, max_x) = (-xr - 1, xr + 1);
        let zr = random.next_int_bound(2)? + 2;
        let (min_z, max_z) = (-zr - 1, zr + 1);
        let mut holes = 0;
        for dx in min_x..=max_x {
            for dy in -1..=4 {
                for dz in min_z..=max_z {
                    let p = tb::offset(origin, dx, dy, dz);
                    let solid = self.solid(world, p)?;
                    if (dy == -1 || dy == 4) && !solid {
                        return Ok(false);
                    }
                    if (dx == min_x || dx == max_x || dz == min_z || dz == max_z)
                        && dy == 0
                        && blocks.is_air(read(world, p)?)
                        && blocks.is_air(read(world, tb::offset(p, 0, 1, 0))?)
                    {
                        holes += 1;
                    }
                }
            }
        }
        if !(1..=5).contains(&holes) {
            return Ok(false);
        }
        let cave_air = state(blocks, "minecraft:cave_air")?;
        let cobble = state(blocks, "minecraft:cobblestone")?;
        let mossy = state(blocks, "minecraft:mossy_cobblestone")?;
        let min_y = world.min_y().map_err(query)?;
        for dx in min_x..=max_x {
            for dy in (-1..=3).rev() {
                for dz in min_z..=max_z {
                    let p = tb::offset(origin, dx, dy, dz);
                    let wall = read(world, p)?;
                    if dx == min_x
                        || dy == -1
                        || dz == min_z
                        || dx == max_x
                        || dy == 4
                        || dz == max_z
                    {
                        if p[1] >= min_y && !self.solid(world, tb::offset(p, 0, -1, 0))? {
                            write(world, p, cave_air, 2)?;
                        } else if blocks.has(wall, tb::SOLID) && !blocks.has(wall, tb::CHEST) {
                            let value = if dy == -1 && random.next_int_bound(4)? != 0 {
                                mossy
                            } else {
                                cobble
                            };
                            safe_set(world, p, value, cannot)?;
                        }
                    } else if !blocks.has(wall, tb::CHEST | tb::SPAWNER) {
                        safe_set(world, p, cave_air, cannot)?;
                    }
                }
            }
        }
        let chest = state(blocks, "minecraft:chest")?;
        for _ in 0..2 {
            for _ in 0..3 {
                let x = origin[0]
                    .wrapping_add(random.next_int_bound(xr * 2 + 1)?)
                    .wrapping_sub(xr);
                let z = origin[2]
                    .wrapping_add(random.next_int_bound(zr * 2 + 1)?)
                    .wrapping_sub(zr);
                let p = [x, origin[1], z];
                if !blocks.is_air(read(world, p)?) {
                    continue;
                }
                let mut walls = 0;
                for direction in tb::HORIZONTAL {
                    if self.solid(world, tb::relative(p, direction, 1))? {
                        walls += 1;
                    }
                }
                if walls == 1 {
                    let oriented = self.reorient(world, p, chest)?;
                    safe_set(world, p, oriented, cannot)?;
                    world
                        .set_block_entity_loot_table(p, "minecraft:chests/simple_dungeon", random)
                        .map_err(query)?;
                    break;
                }
            }
        }
        let spawner = state(blocks, "minecraft:spawner")?;
        safe_set(world, origin, spawner, cannot)?;
        if read(world, origin)? == spawner {
            let mob = DUNGEON_MOBS[random.next_int_bound(4)? as usize];
            world.configure_spawner(origin, mob).map_err(query)?;
        }
        Ok(true)
    }
    /// StructurePiece.reorient for the chest's horizontal facing.
    fn reorient<W: FeatureWorld>(
        &self,
        world: &mut W,
        p: [i32; 3],
        chest: BlockStateId,
    ) -> TreeResult<BlockStateId, W::Error> {
        let blocks = &*self.blocks;
        let mut solid_neighbor = None;
        for direction in tb::HORIZONTAL {
            let state = read(world, tb::relative(p, direction, 1))?;
            if blocks.has(state, tb::CHEST) {
                return Ok(chest);
            }
            if blocks.has(state, tb::SOLID_RENDER) {
                if solid_neighbor.is_some() {
                    solid_neighbor = None;
                    break;
                }
                solid_neighbor = Some(direction);
            }
        }
        let facing = |d: Direction| {
            blocks
                .with(chest, "facing", tb::direction_name(d))
                .ok_or(FeatureExecutionError::InvalidState)
        };
        if let Some(direction) = solid_neighbor {
            return facing(direction.opposite());
        }
        let mut lock = blocks
            .properties
            .value(chest, "facing")
            .and_then(|v| tb::decode_direction(v).ok())
            .ok_or(FeatureExecutionError::InvalidState)?;
        let mut rel = tb::relative(p, lock, 1);
        if blocks.has(read(world, rel)?, tb::SOLID_RENDER) {
            lock = lock.opposite();
            rel = tb::relative(p, lock, 1);
        }
        if blocks.has(read(world, rel)?, tb::SOLID_RENDER) {
            lock = tb::clockwise(lock);
            rel = tb::relative(p, lock, 1);
        }
        if blocks.has(read(world, rel)?, tb::SOLID_RENDER) {
            lock = lock.opposite();
        }
        facing(lock)
    }
}

#[derive(Clone, Debug)]
pub struct GeodeFeature {
    filling: BlockStateProvider,
    inner: BlockStateProvider,
    alternate_inner: BlockStateProvider,
    middle: BlockStateProvider,
    outer: BlockStateProvider,
    inner_placements: Vec<BlockStateId>,
    cannot_replace: Arc<BTreeSet<BlockStateId>>,
    invalid_blocks: Arc<BTreeSet<BlockStateId>>,
    layers: [f64; 4],
    crack_chance: f64,
    base_crack_size: f64,
    crack_point_offset: i32,
    potential_chance: f64,
    alternate_chance: f64,
    require_alternate: bool,
    outer_wall_distance: IntProvider,
    distribution_points: IntProvider,
    point_offset: IntProvider,
    min_gen_offset: i32,
    max_gen_offset: i32,
    noise_multiplier: f64,
    invalid_threshold: i32,
    noise: NormalNoise,
    blocks: Arc<TreeBlocks>,
}
fn chance(object: &Object, name: &'static str, default: f64) -> Result<f64, DecodeError> {
    optional(object, name)
        .map(|raw| double_range(raw, name, 0.0, 1.0))
        .transpose()
        .map(|v| v.unwrap_or(default))
}
impl GeodeFeature {
    pub(crate) fn decode(
        json: &str,
        bindings: &FeatureBlockBindings<'_>,
        depth: usize,
        remaining: &mut usize,
    ) -> Result<Self, DecodeError> {
        let object: Object = serde_json::from_str(json)?;
        if kind(&object)? != "minecraft:geode" {
            return Err(DecodeError::Field("geode type"));
        }
        let blocks = bindings.tree_blocks()?;
        let mut decoder = TreeDecoder {
            bindings,
            depth,
            remaining,
        };
        let settings: Object = serde_json::from_str(required(&object, "blocks")?.get())?;
        let placements: Vec<Box<serde_json::value::RawValue>> =
            serde_json::from_str(required(&settings, "inner_placements")?.get())?;
        if placements.is_empty() || placements.len() > 8192 {
            return Err(DecodeError::Field("geode inner placements"));
        }
        let inner_placements = placements
            .iter()
            .map(|raw| {
                bindings
                    .codec
                    .decode(raw.get(), false)
                    .map_err(|e| DecodeError::Registry(e.to_string()))
            })
            .collect::<Result<_, _>>()?;
        let layers: Object = serde_json::from_str(required(&object, "layers")?.get())?;
        let layer = |name: &'static str, default: f64| {
            optional(&layers, name)
                .map(|raw| double_range(raw, name, 0.01, 50.0))
                .transpose()
                .map(|v| v.unwrap_or(default))
        };
        let crack: Object = serde_json::from_str(required(&object, "crack")?.get())?;
        let mut provider =
            |name: &'static str, default: [i32; 2], low: i32, high: i32| match optional(
                &object, name,
            ) {
                Some(raw) => decoder.int_provider(raw, low, high),
                None => Ok(IntProvider::Uniform {
                    min: default[0],
                    max: default[1],
                }),
            };
        let outer_wall_distance = provider("outer_wall_distance", [4, 5], 1, 20)?;
        let distribution_points = provider("distribution_points", [3, 4], 1, 20)?;
        let point_offset = provider("point_offset", [1, 2], 0, 10)?;
        let int_or = |name: &'static str, default: i32| {
            optional(&object, name)
                .map(super::integer)
                .transpose()
                .map(|v| v.unwrap_or(default))
        };
        Ok(Self {
            filling: decoder.provider(required(&settings, "filling_provider")?)?,
            inner: decoder.provider(required(&settings, "inner_layer_provider")?)?,
            alternate_inner: decoder
                .provider(required(&settings, "alternate_inner_layer_provider")?)?,
            middle: decoder.provider(required(&settings, "middle_layer_provider")?)?,
            outer: decoder.provider(required(&settings, "outer_layer_provider")?)?,
            inner_placements,
            cannot_replace: decoder.block_set(required(&settings, "cannot_replace")?)?,
            invalid_blocks: decoder.block_set(required(&settings, "invalid_blocks")?)?,
            layers: [
                layer("filling", 1.7)?,
                layer("inner_layer", 2.2)?,
                layer("middle_layer", 3.2)?,
                layer("outer_layer", 4.2)?,
            ],
            crack_chance: chance(&crack, "generate_crack_chance", 1.0)?,
            base_crack_size: optional(&crack, "base_crack_size")
                .map(|raw| double_range(raw, "base_crack_size", 0.0, 5.0))
                .transpose()?
                .unwrap_or(2.0),
            crack_point_offset: optional(&crack, "crack_point_offset")
                .map(|raw| int_range(raw, "crack_point_offset", 0, 10))
                .transpose()?
                .unwrap_or(2),
            potential_chance: chance(&object, "use_potential_placements_chance", 0.35)?,
            alternate_chance: chance(&object, "use_alternate_layer0_chance", 0.0)?,
            require_alternate: optional(&object, "placements_require_layer0_alternate")
                .map(|raw| serde_json::from_str::<bool>(raw.get()))
                .transpose()?
                .unwrap_or(true),
            outer_wall_distance,
            distribution_points,
            point_offset,
            min_gen_offset: int_or("min_gen_offset", -16)?,
            max_gen_offset: int_or("max_gen_offset", 16)?,
            noise_multiplier: chance(&object, "noise_multiplier", 0.05)?,
            invalid_threshold: super::integer(required(&object, "invalid_blocks_threshold")?)?,
            noise: NormalNoise::create_parity(-4, &[1.0])?,
            blocks,
        })
    }
    pub(crate) fn place<W: FeatureWorld + PlacementContext, R: RandomSource>(
        &self,
        world: &mut W,
        random: &mut R,
        origin: [i32; 3],
    ) -> TreeResult<bool, W::Error> {
        let blocks = &*self.blocks;
        let cannot = |s: BlockStateId| self.cannot_replace.contains(&s);
        let num_points = self.distribution_points.sample(random)?;
        let seed = world.world_seed().map_err(query)?;
        let noise = self
            .noise
            .create(&mut LegacyRandom::new(seed))
            .map_err(|_| FeatureExecutionError::InvalidState)?;
        let crack_adjust = f64::from(num_points) / f64::from(self.outer_wall_distance.bounds()[1]);
        let [filling, inner, middle, outer] = self.layers;
        let inner_air = 1.0 / filling.sqrt();
        let innermost = 1.0 / (inner + crack_adjust).sqrt();
        let inner_crust = 1.0 / (middle + crack_adjust).sqrt();
        let outer_crust = 1.0 / (outer + crack_adjust).sqrt();
        let crack_size = 1.0
            / (self.base_crack_size
                + random.next_double() / 2.0
                + if num_points > 3 { crack_adjust } else { 0.0 })
            .sqrt();
        let crack = f64::from(random.next_float()) < self.crack_chance;
        let mut invalid = 0;
        let mut points = Vec::new();
        for _ in 0..num_points {
            let x = self.outer_wall_distance.sample(random)?;
            let y = self.outer_wall_distance.sample(random)?;
            let z = self.outer_wall_distance.sample(random)?;
            let p = tb::offset(origin, x, y, z);
            let state = read(world, p)?;
            if blocks.is_air(state) || self.invalid_blocks.contains(&state) {
                invalid += 1;
                if invalid > self.invalid_threshold {
                    return Ok(false);
                }
            }
            points.push((p, self.point_offset.sample(random)?));
        }
        let mut crack_points = Vec::new();
        if crack {
            let index = random.next_int_bound(4)?;
            let o = num_points * 2 + 1;
            let (cx, cz) = match index {
                0 => (o, 0),
                1 => (0, o),
                2 => (o, o),
                _ => (0, 0),
            };
            for y in [7, 5, 1] {
                crack_points.push(tb::offset(origin, cx, y, cz));
            }
        }
        let dist_sqr = |a: [i32; 3], b: [i32; 3]| -> f64 {
            (0..3)
                .map(|i| {
                    let d = f64::from(a[i]) - f64::from(b[i]);
                    d * d
                })
                .sum()
        };
        let mut potential = Vec::new();
        let air_state = blocks
            .default_state("minecraft:air")
            .ok_or(FeatureExecutionError::InvalidState)?;
        let (lo, hi) = (self.min_gen_offset, self.max_gen_offset);
        // BlockPos.betweenClosed iterates x fastest, then y, then z.
        for dz in lo..=hi {
            for dy in lo..=hi {
                for dx in lo..=hi {
                    let p = tb::offset(origin, dx, dy, dz);
                    let noise_offset =
                        f64::from(noise.get3(f64::from(p[0]), f64::from(p[1]), f64::from(p[2])))
                            * self.noise_multiplier;
                    let mut shell = 0.0;
                    for (point, offset) in &points {
                        shell +=
                            1.0 / (dist_sqr(p, *point) + f64::from(*offset)).sqrt() + noise_offset;
                    }
                    if shell < outer_crust {
                        continue;
                    }
                    if shell >= inner_air {
                        let value = sample_state(&self.filling, world, random, p)?;
                        safe_set(world, p, value, cannot)?;
                        continue;
                    }
                    let mut crack_sum = 0.0;
                    for point in &crack_points {
                        crack_sum += 1.0
                            / (dist_sqr(p, *point) + f64::from(self.crack_point_offset)).sqrt()
                            + noise_offset;
                    }
                    if crack && crack_sum >= crack_size {
                        safe_set(world, p, air_state, cannot)?;
                        for direction in Direction::ALL {
                            let q = tb::relative(p, direction, 1);
                            let fluid = world.fluid_state(q).map_err(query)?;
                            if fluid.kind() != crate::fluid::FluidKind::Empty {
                                world
                                    .schedule_fluid_tick(q, fluid.kind(), 0)
                                    .map_err(query)?;
                            }
                        }
                    } else if shell >= innermost {
                        let alternate = f64::from(random.next_float()) < self.alternate_chance;
                        let provider = if alternate {
                            &self.alternate_inner
                        } else {
                            &self.inner
                        };
                        let value = sample_state(provider, world, random, p)?;
                        safe_set(world, p, value, cannot)?;
                        if (!self.require_alternate || alternate)
                            && f64::from(random.next_float()) < self.potential_chance
                        {
                            potential.push(p);
                        }
                    } else if shell >= inner_crust {
                        let value = sample_state(&self.middle, world, random, p)?;
                        safe_set(world, p, value, cannot)?;
                    } else if shell >= outer_crust {
                        let value = sample_state(&self.outer, world, random, p)?;
                        safe_set(world, p, value, cannot)?;
                    }
                }
            }
        }
        for crystal in potential {
            let index = random.next_int_bound(self.inner_placements.len() as i32)? as usize;
            let mut state = self.inner_placements[index];
            for direction in Direction::ALL {
                if blocks
                    .properties
                    .values(state, "facing")
                    .is_some_and(|v| v.len() == 6)
                {
                    state = blocks
                        .with(state, "facing", tb::direction_name(direction))
                        .ok_or(FeatureExecutionError::InvalidState)?;
                }
                let target = tb::relative(crystal, direction, 1);
                let existing = read(world, target)?;
                if blocks.has(state, tb::WATERLOGGABLE) {
                    state = blocks.waterlogged(state, blocks.has(existing, tb::FLUID_SOURCE));
                }
                // BuddingAmethystBlock.canClusterGrowAtState.
                if blocks.is_air(existing)
                    || (blocks.has(existing, tb::WATER_BLOCK)
                        && blocks.has(existing, tb::FLUID_FULL))
                {
                    safe_set(world, target, state, cannot)?;
                    break;
                }
            }
        }
        Ok(true)
    }
}

#[cfg(test)]
mod dungeon_spawner_data_tests {
    use super::*;
    #[test]
    fn extracted_generation_builder_preserves_exact_payload_and_order() {
        use crate::nbt::{Limits, Tag, write_network};
        for entity in [
            "minecraft:skeleton",
            "minecraft:zombie",
            "minecraft:spider",
            "example:bound_entity",
        ] {
            let expected = {
                let mut mob = crate::nbt::Compound::new();
                mob.insert("id".into(), Tag::String(entity.into()));
                let mut data = crate::nbt::Compound::new();
                data.insert("entity".into(), Tag::Compound(mob));
                let mut nbt = crate::nbt::Compound::new();
                nbt.insert("id".into(), Tag::String("minecraft:mob_spawner".into()));
                nbt.insert("SpawnData".into(), Tag::Compound(data));
                nbt.insert("SpawnPotentials".into(), Tag::List(Vec::new()));

                nbt
            };
            let actual = dungeon_spawner_nbt(entity);
            assert_eq!(
                write_network(&Tag::Compound(actual), Limits::default()).unwrap(),
                write_network(&Tag::Compound(expected), Limits::default()).unwrap()
            );
        }
    }
}
