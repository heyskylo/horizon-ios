//! Tree decorators (TreeDecorator subclasses), in native random-draw order.
use super::{
    Layer, NestedPlacer, Run, TreeDecoder, TreeResult, kind, read, sample_optional, sample_state,
    unit_float,
};
use crate::heightmap::HeightmapType;
use crate::registry::BlockStateId;
use crate::worldgen::{
    DecodeError, Object,
    feature::FeatureWorld,
    feature_graph::{FeatureDefinition, FeatureExecutionError},
    int_range, optional,
    placement::PlacementContext,
    required,
    state_provider::BlockStateProvider,
    tree_blocks::{self as tb, TreeBlocks},
};
use horizon_core::{geometry::Direction, random::RandomSource};
use serde_json::value::RawValue;
use std::collections::BTreeSet;

/// Catalog-injected configured feature for the pale-moss ground patch, which
/// vanilla looks up by registry key during decoration.
pub(crate) const GROUND_FEATURE_KEY: &str = "horizon:pale_moss_patch";

/// Decorator context lists: set iteration order, stably sorted by Y.
pub(crate) struct Lists {
    pub logs: Vec<[i32; 3]>,
    pub leaves: Vec<[i32; 3]>,
    pub roots: Vec<[i32; 3]>,
}
impl Lists {
    pub(crate) fn new(
        mut logs: Vec<[i32; 3]>,
        mut leaves: Vec<[i32; 3]>,
        mut roots: Vec<[i32; 3]>,
    ) -> Self {
        logs.sort_by_key(|p| p[1]);
        leaves.sort_by_key(|p| p[1]);
        roots.sort_by_key(|p| p[1]);
        Self {
            logs,
            leaves,
            roots,
        }
    }
    /// TreeFeature.getLowestTrunkOrRootOfTree.
    fn lowest(&self) -> Vec<[i32; 3]> {
        if self.roots.is_empty() {
            self.logs.clone()
        } else if !self.logs.is_empty() && self.roots[0][1] == self.logs[0][1] {
            let mut all = self.logs.clone();
            all.extend(&self.roots);
            all
        } else {
            self.roots.clone()
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) enum TreeDecorator {
    TrunkVine,
    LeaveVine(f32),
    Cocoa(f32),
    Beehive(f32),
    AlterGround(BlockStateProvider),
    AttachedToLeaves {
        probability: f32,
        exclusion_xz: i32,
        exclusion_y: i32,
        provider: BlockStateProvider,
        required_empty: i32,
        directions: Vec<Direction>,
    },
    AttachedToLogs {
        probability: f32,
        provider: BlockStateProvider,
        directions: Vec<Direction>,
    },
    CreakingHeart(f32),
    PaleMoss {
        leaves: f32,
        trunk: f32,
        ground: f32,
        patch: Option<Box<FeatureDefinition>>,
    },
    PlaceOnGround {
        tries: i32,
        radius: i32,
        height: i32,
        provider: BlockStateProvider,
    },
    ShelfMushroom(f32),
}

fn directions(raw: &RawValue) -> Result<Vec<Direction>, DecodeError> {
    let names: Vec<String> = serde_json::from_str(raw.get())?;
    if names.is_empty() || names.len() > 64 {
        return Err(DecodeError::Field("decorator directions"));
    }
    names.iter().map(|n| tb::decode_direction(n)).collect()
}

pub(crate) type NestedDecoder<'a> =
    dyn Fn(&str, usize, &mut usize) -> Result<FeatureDefinition, DecodeError> + 'a;

impl TreeDecorator {
    pub(crate) fn decode_list(
        raw: &RawValue,
        decoder: &mut TreeDecoder<'_, '_>,
        ground: Option<&RawValue>,
        nested: &NestedDecoder<'_>,
    ) -> Result<Vec<Self>, DecodeError> {
        let values: Vec<Box<RawValue>> = serde_json::from_str(raw.get())?;
        if values.len() > 256 {
            return Err(DecodeError::Field("tree decorator count"));
        }
        values
            .iter()
            .map(|raw| Self::decode(raw, decoder, ground, nested))
            .collect()
    }
    fn decode(
        raw: &RawValue,
        decoder: &mut TreeDecoder<'_, '_>,
        ground: Option<&RawValue>,
        nested: &NestedDecoder<'_>,
    ) -> Result<Self, DecodeError> {
        let object: Object = serde_json::from_str(raw.get())?;
        let probability = || unit_float(required(&object, "probability")?, "probability");
        Ok(match kind(&object)?.as_str() {
            "minecraft:trunk_vine" => Self::TrunkVine,
            "minecraft:leave_vine" => Self::LeaveVine(probability()?),
            "minecraft:cocoa" => Self::Cocoa(probability()?),
            "minecraft:beehive" => Self::Beehive(probability()?),
            "minecraft:creaking_heart" => Self::CreakingHeart(probability()?),
            "minecraft:shelf_mushroom" => Self::ShelfMushroom(probability()?),
            "minecraft:alter_ground" => {
                Self::AlterGround(decoder.provider(required(&object, "provider")?)?)
            }
            "minecraft:attached_to_leaves" => Self::AttachedToLeaves {
                probability: probability()?,
                exclusion_xz: int_range(
                    required(&object, "exclusion_radius_xz")?,
                    "exclusion_radius_xz",
                    0,
                    16,
                )?,
                exclusion_y: int_range(
                    required(&object, "exclusion_radius_y")?,
                    "exclusion_radius_y",
                    0,
                    16,
                )?,
                provider: decoder.provider(required(&object, "block_provider")?)?,
                required_empty: int_range(
                    required(&object, "required_empty_blocks")?,
                    "required_empty_blocks",
                    1,
                    16,
                )?,
                directions: directions(required(&object, "directions")?)?,
            },
            "minecraft:attached_to_logs" => Self::AttachedToLogs {
                probability: probability()?,
                provider: decoder.provider(required(&object, "block_provider")?)?,
                directions: directions(required(&object, "directions")?)?,
            },
            "minecraft:pale_moss" => Self::PaleMoss {
                leaves: unit_float(
                    required(&object, "leaves_probability")?,
                    "leaves_probability",
                )?,
                trunk: unit_float(required(&object, "trunk_probability")?, "trunk_probability")?,
                ground: unit_float(
                    required(&object, "ground_probability")?,
                    "ground_probability",
                )?,
                patch: ground
                    .map(|raw| nested(raw.get(), decoder.depth + 1, decoder.remaining))
                    .transpose()?
                    .map(Box::new),
            },
            "minecraft:place_on_ground" => Self::PlaceOnGround {
                tries: optional(&object, "tries")
                    .map(|raw| int_range(raw, "tries", 1, i32::MAX))
                    .transpose()?
                    .unwrap_or(128),
                radius: optional(&object, "radius")
                    .map(|raw| int_range(raw, "radius", 0, i32::MAX))
                    .transpose()?
                    .unwrap_or(2),
                height: optional(&object, "height")
                    .map(|raw| int_range(raw, "height", 0, i32::MAX))
                    .transpose()?
                    .unwrap_or(1),
                provider: decoder.provider(required(&object, "block_state_provider")?)?,
            },
            _ => return Err(DecodeError::Field("tree decorator type")),
        })
    }

    pub(crate) fn place<W: FeatureWorld + PlacementContext, R: RandomSource>(
        &self,
        run: &mut Run<'_, W, R>,
        blocks: &TreeBlocks,
        lists: &Lists,
        nested: &mut NestedPlacer<'_, W, R>,
    ) -> TreeResult<(), W::Error> {
        let mut d = Decorate { run, blocks };
        match self {
            Self::TrunkVine => {
                for &pos in &lists.logs {
                    for (direction, face) in [
                        (Direction::West, "east"),
                        (Direction::East, "west"),
                        (Direction::North, "south"),
                        (Direction::South, "north"),
                    ] {
                        if d.run.random.next_int_bound(3)? > 0 {
                            let p = tb::relative(pos, direction, 1);
                            if d.is_air(p)? {
                                d.place_vine(p, face)?;
                            }
                        }
                    }
                }
            }
            Self::LeaveVine(probability) => {
                for &pos in &lists.leaves {
                    for (direction, face) in [
                        (Direction::West, "east"),
                        (Direction::East, "west"),
                        (Direction::North, "south"),
                        (Direction::South, "north"),
                    ] {
                        if d.run.random.next_float() < *probability {
                            let p = tb::relative(pos, direction, 1);
                            if d.is_air(p)? {
                                d.place_vine(p, face)?;
                                let mut below = tb::offset(p, 0, -1, 0);
                                let mut remaining = 4;
                                while d.is_air(below)? && remaining > 0 {
                                    d.place_vine(below, face)?;
                                    below = tb::offset(below, 0, -1, 0);
                                    remaining -= 1;
                                }
                            }
                        }
                    }
                }
            }
            Self::Cocoa(probability) => {
                if d.run.random.next_float() >= *probability {
                    return Ok(());
                }
                let Some(first) = lists.logs.first() else {
                    return Ok(());
                };
                let tree_y = first[1];
                for &pos in lists.logs.iter().filter(|p| p[1].wrapping_sub(tree_y) <= 2) {
                    for direction in tb::HORIZONTAL {
                        if d.run.random.next_float() <= 0.25 {
                            let n = direction.opposite().normal();
                            let p = tb::offset(pos, n[0], 0, n[2]);
                            if d.is_air(p)? {
                                let age = d.run.random.next_int_bound(3)?;
                                let state = d.default("minecraft:cocoa")?;
                                let state = d.with(state, "age", &age.to_string())?;
                                let state =
                                    d.with(state, "facing", tb::direction_name(direction))?;
                                d.set(p, state)?;
                            }
                        }
                    }
                }
            }
            Self::Beehive(probability) => d.beehive(lists, *probability)?,
            Self::AlterGround(provider) => {
                let positions = lists.lowest();
                let Some(first) = positions.first() else {
                    return Ok(());
                };
                let min_y = first[1];
                for &pos in positions.iter().filter(|p| p[1] == min_y) {
                    d.circle(provider, tb::offset(pos, -1, 0, -1))?;
                    d.circle(provider, tb::offset(pos, 2, 0, -1))?;
                    d.circle(provider, tb::offset(pos, -1, 0, 2))?;
                    d.circle(provider, tb::offset(pos, 2, 0, 2))?;
                    for _ in 0..5 {
                        let placement = d.run.random.next_int_bound(64)?;
                        let (xx, zz) = (placement % 8, placement / 8);
                        if xx == 0 || xx == 7 || zz == 0 || zz == 7 {
                            d.circle(provider, tb::offset(pos, -3 + xx, 0, -3 + zz))?;
                        }
                    }
                }
            }
            Self::AttachedToLeaves {
                probability,
                exclusion_xz,
                exclusion_y,
                provider,
                required_empty,
                directions,
            } => {
                let mut blacklist = BTreeSet::new();
                let mut leaves = lists.leaves.clone();
                tb::shuffle(&mut leaves, d.run.random)?;
                for leaf in leaves {
                    let direction =
                        directions[d.run.random.next_int_bound(directions.len() as i32)? as usize];
                    let p = tb::relative(leaf, direction, 1);
                    if blacklist.contains(&p) || d.run.random.next_float() >= *probability {
                        continue;
                    }
                    let mut empty = true;
                    for i in 1..=*required_empty {
                        if !d.is_air(tb::relative(leaf, direction, i))? {
                            empty = false;
                            break;
                        }
                    }
                    if !empty {
                        continue;
                    }
                    for dx in -exclusion_xz..=*exclusion_xz {
                        for dy in -exclusion_y..=*exclusion_y {
                            for dz in -exclusion_xz..=*exclusion_xz {
                                blacklist.insert(tb::offset(p, dx, dy, dz));
                            }
                        }
                    }
                    let state = sample_state(provider, d.run.world, d.run.random, p)?;
                    d.set(p, state)?;
                }
            }
            Self::AttachedToLogs {
                probability,
                provider,
                directions,
            } => {
                let mut logs = lists.logs.clone();
                tb::shuffle(&mut logs, d.run.random)?;
                for log in logs {
                    let direction =
                        directions[d.run.random.next_int_bound(directions.len() as i32)? as usize];
                    let p = tb::relative(log, direction, 1);
                    if d.run.random.next_float() <= *probability && d.is_air(p)? {
                        let state = sample_state(provider, d.run.world, d.run.random, p)?;
                        d.set(p, state)?;
                    }
                }
            }
            Self::CreakingHeart(probability) => {
                if lists.logs.is_empty() || d.run.random.next_float() >= *probability {
                    return Ok(());
                }
                let mut candidates = lists.logs.clone();
                tb::shuffle(&mut candidates, d.run.random)?;
                for pos in candidates {
                    let mut enclosed = true;
                    for direction in Direction::ALL {
                        let state = read(d.run.world, tb::relative(pos, direction, 1))?;
                        if !blocks.has(state, tb::LOGS) {
                            enclosed = false;
                            break;
                        }
                    }
                    if enclosed {
                        let state = d.default("minecraft:creaking_heart")?;
                        let state = d.with(state, "creaking_heart_state", "dormant")?;
                        let state = d.with(state, "natural", "true")?;
                        d.set(pos, state)?;
                        break;
                    }
                }
            }
            Self::PaleMoss {
                leaves,
                trunk,
                ground,
                patch,
            } => {
                let mut logs = lists.logs.clone();
                tb::shuffle(&mut logs, d.run.random)?;
                let Some(origin) = logs
                    .iter()
                    .copied()
                    .reduce(|best, p| if p[1] < best[1] { p } else { best })
                else {
                    return Ok(());
                };
                if d.run.random.next_float() < *ground
                    && let Some(patch) = patch
                {
                    nested(
                        patch,
                        d.run.world,
                        d.run.random,
                        tb::offset(origin, 0, 1, 0),
                    )?;
                }
                for (positions, chance) in [(&lists.logs, *trunk), (&lists.leaves, *leaves)] {
                    for &pos in positions {
                        if d.run.random.next_float() < chance {
                            let down = tb::offset(pos, 0, -1, 0);
                            if d.is_air(down)? {
                                d.moss_hanger(down)?;
                            }
                        }
                    }
                }
            }
            Self::PlaceOnGround {
                tries,
                radius,
                height,
                provider,
            } => {
                let positions = lists.lowest();
                let Some(&origin) = positions.first() else {
                    return Ok(());
                };
                let min_y = origin[1];
                let (mut min_x, mut max_x, mut min_z, mut max_z) =
                    (origin[0], origin[0], origin[2], origin[2]);
                for p in positions.iter().filter(|p| p[1] == min_y) {
                    min_x = min_x.min(p[0]);
                    max_x = max_x.max(p[0]);
                    min_z = min_z.min(p[2]);
                    max_z = max_z.max(p[2]);
                }
                let low = [
                    min_x.wrapping_sub(*radius),
                    min_y.wrapping_sub(*height),
                    min_z.wrapping_sub(*radius),
                ];
                let high = [
                    max_x.wrapping_add(*radius),
                    min_y.wrapping_add(*height),
                    max_z.wrapping_add(*radius),
                ];
                for _ in 0..*tries {
                    let p = [
                        d.run.random.next_int_between_inclusive(low[0], high[0])?,
                        d.run.random.next_int_between_inclusive(low[1], high[1])?,
                        d.run.random.next_int_between_inclusive(low[2], high[2])?,
                    ];
                    let above = tb::offset(p, 0, 1, 0);
                    let above_state = read(d.run.world, above)?;
                    if !blocks.has(above_state, tb::AIR | tb::VINE) {
                        continue;
                    }
                    if !blocks.has(read(d.run.world, p)?, tb::SOLID_RENDER) {
                        continue;
                    }
                    let surface = d
                        .run
                        .world
                        .height(HeightmapType::MotionBlockingNoLeaves, p[0], p[2])
                        .map_err(FeatureExecutionError::Query)?;
                    if surface <= above[1] {
                        let state = sample_state(provider, d.run.world, d.run.random, above)?;
                        d.set(above, state)?;
                    }
                }
            }
            Self::ShelfMushroom(probability) => d.shelf_mushroom(lists, *probability)?,
        }
        Ok(())
    }
}

struct Decorate<'a, 'b, 'w, W, R> {
    run: &'a mut Run<'w, W, R>,
    blocks: &'b TreeBlocks,
}
impl<W: FeatureWorld + PlacementContext, R: RandomSource> Decorate<'_, '_, '_, W, R> {
    fn is_air(&mut self, p: [i32; 3]) -> TreeResult<bool, W::Error> {
        Ok(self.blocks.is_air(read(self.run.world, p)?))
    }
    fn has(&mut self, p: [i32; 3], flag: u32) -> TreeResult<bool, W::Error> {
        Ok(self.blocks.has(read(self.run.world, p)?, flag))
    }
    fn set(&mut self, p: [i32; 3], state: BlockStateId) -> TreeResult<(), W::Error> {
        self.run.set(Layer::Decoration, p, state)
    }
    fn default(&self, name: &str) -> TreeResult<BlockStateId, W::Error> {
        self.blocks
            .default_state(name)
            .ok_or(FeatureExecutionError::InvalidState)
    }
    fn with(
        &self,
        state: BlockStateId,
        name: &str,
        value: &str,
    ) -> TreeResult<BlockStateId, W::Error> {
        self.blocks
            .with(state, name, value)
            .ok_or(FeatureExecutionError::InvalidState)
    }
    fn place_vine(&mut self, p: [i32; 3], face: &str) -> TreeResult<(), W::Error> {
        let vine = self.default("minecraft:vine")?;
        let state = self.with(vine, face, "true")?;
        self.set(p, state)
    }
    fn circle(
        &mut self,
        provider: &BlockStateProvider,
        center: [i32; 3],
    ) -> TreeResult<(), W::Error> {
        for xx in -2i32..=2 {
            for zz in -2i32..=2 {
                if xx.abs() != 2 || zz.abs() != 2 {
                    let column = tb::offset(center, xx, 0, zz);
                    for dy in (-3..=2).rev() {
                        let cursor = tb::offset(column, 0, dy, 0);
                        if let Some(state) =
                            sample_optional(provider, self.run.world, self.run.random, cursor)?
                        {
                            self.set(cursor, state)?;
                            break;
                        }
                        if !self.is_air(cursor)? && dy < 0 {
                            break;
                        }
                    }
                }
            }
        }
        Ok(())
    }
    fn moss_hanger(&mut self, mut p: [i32; 3]) -> TreeResult<(), W::Error> {
        let moss = self.default("minecraft:pale_hanging_moss")?;
        let middle = self.with(moss, "tip", "false")?;
        let tip = self.with(moss, "tip", "true")?;
        while self.is_air(tb::offset(p, 0, -1, 0))? && self.run.random.next_float() >= 0.5 {
            self.set(p, middle)?;
            p = tb::offset(p, 0, -1, 0);
        }
        self.set(p, tip)
    }
    fn beehive(&mut self, lists: &Lists, probability: f32) -> TreeResult<(), W::Error> {
        let (Some(first_log), Some(last_log)) = (lists.logs.first(), lists.logs.last()) else {
            return Ok(());
        };
        if self.run.random.next_float() >= probability {
            return Ok(());
        }
        let hive_y = match lists.leaves.first() {
            Some(leaf) => (leaf[1] - 1).max(first_log[1] + 1),
            None => (first_log[1] + 1 + self.run.random.next_int_bound(3)?).min(last_log[1]),
        };
        let mut placements: Vec<[i32; 3]> = lists
            .logs
            .iter()
            .filter(|p| p[1] == hive_y)
            .flat_map(|p| {
                [Direction::East, Direction::South, Direction::West].map(|d| tb::relative(*p, d, 1))
            })
            .collect();
        if placements.is_empty() {
            return Ok(());
        }
        tb::shuffle(&mut placements, self.run.random)?;
        for p in placements {
            if self.is_air(p)? && self.is_air(tb::relative(p, Direction::South, 1))? {
                let nest = self.default("minecraft:bee_nest")?;
                let nest = self.with(nest, "facing", "south")?;
                self.set(p, nest)?;
                // Occupants exist only when the write produced the nest block entity.
                if read(self.run.world, p)? == nest {
                    let count = 2 + self.run.random.next_int_bound(2)?;
                    let mut ticks = Vec::with_capacity(count as usize);
                    for _ in 0..count {
                        ticks.push(self.run.random.next_int_bound(599)?);
                    }
                    self.run
                        .world
                        .store_beehive_occupants(p, &ticks)
                        .map_err(FeatureExecutionError::Query)?;
                }
                break;
            }
        }
        Ok(())
    }
    fn shelf_mushroom(&mut self, lists: &Lists, probability: f32) -> TreeResult<(), W::Error> {
        if self.run.random.next_float() >= probability {
            return Ok(());
        }
        let (Some(first), Some(last)) = (lists.logs.first(), lists.logs.last()) else {
            return Ok(());
        };
        if first[1] == last[1] {
            let facings = if first[0] != last[0] {
                [Direction::North, Direction::South]
            } else {
                [Direction::East, Direction::West]
            };
            for &log in &lists.logs {
                for facing in facings {
                    if self.run.random.next_float() <= 0.25 {
                        let p = mushroom_pos(log, facing);
                        if self.mushroom_replaceable(p)?
                            && !self.adjacent_mushroom(p)?
                            && !self.adjacent_mushroom(log)?
                        {
                            self.place_mushroom(p, facing)?;
                        }
                    }
                }
            }
        } else {
            let a = tb::HORIZONTAL[self.run.random.next_int_bound(4)? as usize];
            let facings = [a, tb::clockwise(a)];
            let base = first[1];
            for &log in &lists.logs {
                let dy = log[1].wrapping_sub(base);
                if !(1..=4).contains(&dy) {
                    continue;
                }
                for facing in facings {
                    if self.run.random.next_float() <= 0.25 {
                        let p = mushroom_pos(log, facing);
                        if self.mushroom_replaceable(p)?
                            && !self.has(tb::offset(p, 0, -1, 0), tb::SHELF_MUSHROOM)?
                        {
                            self.place_mushroom(p, facing)?;
                            break;
                        }
                    }
                }
            }
        }
        Ok(())
    }
    fn mushroom_replaceable(&mut self, p: [i32; 3]) -> TreeResult<bool, W::Error> {
        if !self.has(p, tb::REPLACEABLE)? {
            return Ok(false);
        }
        for q in [
            p,
            tb::offset(p, 1, 0, 0),
            tb::offset(p, -1, 0, 0),
            tb::offset(p, 0, 0, -1),
            tb::offset(p, 0, 0, 1),
        ] {
            if self.has(q, tb::WATER_BLOCK)? {
                return Ok(false);
            }
        }
        Ok(true)
    }
    fn adjacent_mushroom(&mut self, p: [i32; 3]) -> TreeResult<bool, W::Error> {
        for direction in tb::HORIZONTAL {
            if self.has(tb::relative(p, direction, 1), tb::SHELF_MUSHROOM)? {
                return Ok(true);
            }
        }
        Ok(false)
    }
    fn place_mushroom(&mut self, p: [i32; 3], facing: Direction) -> TreeResult<(), W::Error> {
        let age = self.run.random.next_int_bound(2)?;
        let state = self.default("minecraft:shelf_mushroom")?;
        let state = self.with(state, "age", &age.to_string())?;
        let state = self.with(state, "facing", tb::direction_name(facing))?;
        self.set(p, state)
    }
}
fn mushroom_pos(log: [i32; 3], facing: Direction) -> [i32; 3] {
    let n = facing.normal();
    tb::offset(log, n[0], 0, n[2])
}

/// The nest's block entity after `BeehiveDecorator` stored its bees
/// (`Occupant.create(ticks)`: a bee, with 600 minimum ticks in the hive).
pub(crate) fn beehive_occupants(ticks_in_hive: &[i32]) -> crate::nbt::Compound {
    use crate::nbt::{Compound, Tag};
    let bees = ticks_in_hive
        .iter()
        .map(|&ticks| {
            let mut entity = Compound::new();
            entity.insert("id".into(), Tag::String("minecraft:bee".into()));
            let mut bee = Compound::new();
            bee.insert("entity_data".into(), Tag::Compound(entity));
            bee.insert("ticks_in_hive".into(), Tag::Int(ticks));
            bee.insert("min_ticks_in_hive".into(), Tag::Int(600));
            Tag::Compound(bee)
        })
        .collect();
    let mut nbt = Compound::new();
    nbt.insert("id".into(), Tag::String("minecraft:beehive".into()));
    nbt.insert("bees".into(), Tag::List(bees));
    nbt
}
