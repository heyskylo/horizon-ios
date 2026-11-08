//! Vegetation and cave-decoration features: vegetation patches (plain and
//! waterlogged), coral trees/claws, huge mushrooms and multiface growth.
use super::{
    DecodeError, Object,
    block_feature::FeatureBlockBindings,
    block_predicate::BlockPredicate,
    feature::FeatureWorld,
    feature_graph::{FeatureExecutionError, PlacedFeatureDefinition},
    int_range, optional,
    placement::PlacementContext,
    required,
    state_provider::BlockStateProvider,
    tree::{TreeDecoder, TreeResult, kind, read, sample_state, unit_float, write},
    tree_blocks::{self as tb, PosSet, TreeBlocks},
    value_provider::IntProvider,
};
use crate::registry::BlockStateId;
use horizon_core::{geometry::Direction, random::RandomSource};
use std::{collections::BTreeSet, sync::Arc};

/// Places a nested placed feature with the caller's run budget.
pub(crate) type PlacedPlacer<'a, W, R> = dyn FnMut(
        &PlacedFeatureDefinition,
        &mut W,
        &mut R,
        [i32; 3],
    ) -> TreeResult<bool, <W as FeatureWorld>::Error>
    + 'a;
pub(crate) type PlacedDecoder<'a> =
    dyn Fn(&str, usize, &mut usize) -> Result<PlacedFeatureDefinition, DecodeError> + 'a;

fn query<E>(error: super::placement::PlacementQueryError) -> FeatureExecutionError<E> {
    FeatureExecutionError::Query(error)
}

#[derive(Clone, Debug)]
pub struct VegetationPatchFeature {
    replaceable: Arc<BTreeSet<BlockStateId>>,
    ground: BlockStateProvider,
    vegetation: Box<PlacedFeatureDefinition>,
    /// CaveSurface direction: DOWN for floor, UP for ceiling.
    inwards: Direction,
    depth: IntProvider,
    extra_bottom: f32,
    vertical_range: i32,
    vegetation_chance: f32,
    xz_radius: IntProvider,
    extra_edge: f32,
    waterlogged: bool,
    blocks: Arc<TreeBlocks>,
}
impl VegetationPatchFeature {
    pub(crate) fn decode(
        json: &str,
        bindings: &FeatureBlockBindings<'_>,
        depth: usize,
        remaining: &mut usize,
        nested: &PlacedDecoder<'_>,
    ) -> Result<Self, DecodeError> {
        let object: Object = serde_json::from_str(json)?;
        let waterlogged = match kind(&object)?.as_str() {
            "minecraft:vegetation_patch" => false,
            "minecraft:waterlogged_vegetation_patch" => true,
            _ => return Err(DecodeError::Field("vegetation patch type")),
        };
        let blocks = bindings.tree_blocks()?;
        let mut decoder = TreeDecoder {
            bindings,
            depth,
            remaining,
        };
        let surface: String = serde_json::from_str(required(&object, "surface")?.get())?;
        let inwards = match surface.as_str() {
            "floor" => Direction::Down,
            "ceiling" => Direction::Up,
            _ => return Err(DecodeError::Field("cave surface")),
        };
        let vegetation = nested(
            required(&object, "vegetation_feature")?.get(),
            depth + 1,
            decoder.remaining,
        )?;
        Ok(Self {
            replaceable: decoder.block_set(required(&object, "replaceable")?)?,
            ground: decoder.provider(required(&object, "ground_state")?)?,
            vegetation: Box::new(vegetation),
            inwards,
            depth: decoder.int_provider(required(&object, "depth")?, 1, 128)?,
            extra_bottom: unit_float(
                required(&object, "extra_bottom_block_chance")?,
                "extra_bottom_block_chance",
            )?,
            vertical_range: int_range(
                required(&object, "vertical_range")?,
                "vertical_range",
                1,
                256,
            )?,
            vegetation_chance: unit_float(
                required(&object, "vegetation_chance")?,
                "vegetation_chance",
            )?,
            xz_radius: IntProvider::decode(
                required(&object, "xz_radius")?,
                depth + 1,
                decoder.remaining,
            )?,
            extra_edge: unit_float(
                required(&object, "extra_edge_column_chance")?,
                "extra_edge_column_chance",
            )?,
            waterlogged,
            blocks,
        })
    }
    pub(crate) fn place<W: FeatureWorld + PlacementContext, R: RandomSource>(
        &self,
        world: &mut W,
        random: &mut R,
        origin: [i32; 3],
        nested: &mut PlacedPlacer<'_, W, R>,
    ) -> TreeResult<bool, W::Error> {
        let x_radius = self.xz_radius.sample(random)?.wrapping_add(1);
        let z_radius = self.xz_radius.sample(random)?.wrapping_add(1);
        let mut surface = self.ground_patch(world, random, origin, x_radius, z_radius)?;
        if self.waterlogged {
            let mut water = PosSet::default();
            for p in surface.iter() {
                let mut exposed = false;
                for direction in [
                    Direction::North,
                    Direction::East,
                    Direction::South,
                    Direction::West,
                    Direction::Down,
                ] {
                    let q = tb::relative(p, direction, 1);
                    let state = read(world, q)?;
                    if !world
                        .state_face_sturdy(state, q, direction.opposite())
                        .map_err(query)?
                    {
                        exposed = true;
                        break;
                    }
                }
                if !exposed {
                    water.insert(p);
                }
            }
            let water_state = self
                .blocks
                .default_state("minecraft:water")
                .ok_or(FeatureExecutionError::InvalidState)?;
            for p in water.iter() {
                write(world, p, water_state, 2)?;
            }
            surface = water;
        }
        for p in surface.iter() {
            if self.vegetation_chance > 0.0 && random.next_float() < self.vegetation_chance {
                self.place_vegetation(world, random, p, nested)?;
            }
        }
        Ok(!surface.is_empty())
    }
    fn place_vegetation<W: FeatureWorld + PlacementContext, R: RandomSource>(
        &self,
        world: &mut W,
        random: &mut R,
        position: [i32; 3],
        nested: &mut PlacedPlacer<'_, W, R>,
    ) -> TreeResult<bool, W::Error> {
        let outwards = self.inwards.opposite();
        if !self.waterlogged {
            return nested(
                &self.vegetation,
                world,
                random,
                tb::relative(position, outwards, 1),
            );
        }
        // WaterloggedVegetationPatchFeature passes placementPos.below() to super.
        let below = tb::offset(position, 0, -1, 0);
        if nested(
            &self.vegetation,
            world,
            random,
            tb::relative(below, outwards, 1),
        )? {
            let placed = read(world, position)?;
            if self.blocks.has(placed, tb::WATERLOGGABLE)
                && self.blocks.properties.value(placed, "waterlogged") == Some("false")
            {
                write(world, position, self.blocks.waterlogged(placed, true), 2)?;
            }
            return Ok(true);
        }
        Ok(false)
    }
    fn ground_patch<W: FeatureWorld + PlacementContext, R: RandomSource>(
        &self,
        world: &mut W,
        random: &mut R,
        origin: [i32; 3],
        x_radius: i32,
        z_radius: i32,
    ) -> TreeResult<PosSet, W::Error> {
        let inwards = self.inwards;
        let outwards = inwards.opposite();
        let mut surface = PosSet::default();
        let mut dx = x_radius.wrapping_neg();
        while dx <= x_radius {
            let x_edge = dx == x_radius.wrapping_neg() || dx == x_radius;
            let mut dz = z_radius.wrapping_neg();
            while dz <= z_radius {
                let z_edge = dz == z_radius.wrapping_neg() || dz == z_radius;
                let edge = x_edge || z_edge;
                let corner = x_edge && z_edge;
                let edge_not_corner = edge && !corner;
                if !corner
                    && (!edge_not_corner
                        || (self.extra_edge != 0.0 && random.next_float() <= self.extra_edge))
                {
                    let mut pos = tb::offset(origin, dx, 0, dz);
                    let mut offset = 0;
                    while self.blocks.is_air(read(world, pos)?) && offset < self.vertical_range {
                        pos = tb::relative(pos, inwards, 1);
                        offset += 1;
                    }
                    let mut offset = 0;
                    while !self.blocks.is_air(read(world, pos)?) && offset < self.vertical_range {
                        pos = tb::relative(pos, outwards, 1);
                        offset += 1;
                    }
                    let below = tb::relative(pos, inwards, 1);
                    let below_state = read(world, below)?;
                    if self.blocks.is_air(read(world, pos)?)
                        && world
                            .state_face_sturdy(below_state, below, outwards)
                            .map_err(query)?
                    {
                        let depth = self.depth.sample(random)?.wrapping_add(i32::from(
                            self.extra_bottom > 0.0 && random.next_float() < self.extra_bottom,
                        ));
                        if self.place_ground(world, random, below, depth)? {
                            surface.insert(below);
                        }
                    }
                }
                dz += 1;
            }
            dx += 1;
        }
        Ok(surface)
    }
    fn place_ground<W: FeatureWorld + PlacementContext, R: RandomSource>(
        &self,
        world: &mut W,
        random: &mut R,
        mut below: [i32; 3],
        depth: i32,
    ) -> TreeResult<bool, W::Error> {
        for i in 0..depth {
            let state = sample_state(&self.ground, world, random, below)?;
            let existing = read(world, below)?;
            if !self.blocks.same_block(state, existing) {
                if !self.replaceable.contains(&existing) {
                    return Ok(i != 0);
                }
                write(world, below, state, 2)?;
                below = tb::relative(below, self.inwards, 1);
            }
        }
        Ok(true)
    }
}

#[derive(Clone, Debug)]
pub struct CoralFeature {
    claw: bool,
    feature: Box<PlacedFeatureDefinition>,
}
impl CoralFeature {
    pub(crate) fn decode(
        json: &str,
        depth: usize,
        remaining: &mut usize,
        nested: &PlacedDecoder<'_>,
    ) -> Result<Self, DecodeError> {
        let object: Object = serde_json::from_str(json)?;
        let claw = match kind(&object)?.as_str() {
            "minecraft:coral_tree" => false,
            "minecraft:coral_claw" => true,
            _ => return Err(DecodeError::Field("coral feature type")),
        };
        Ok(Self {
            claw,
            feature: Box::new(nested(
                required(&object, "feature")?.get(),
                depth + 1,
                remaining,
            )?),
        })
    }
    pub(crate) fn place<W: FeatureWorld + PlacementContext, R: RandomSource>(
        &self,
        world: &mut W,
        random: &mut R,
        origin: [i32; 3],
        nested: &mut PlacedPlacer<'_, W, R>,
    ) -> TreeResult<bool, W::Error> {
        let feature = &*self.feature;
        if !self.claw {
            let mut pos = origin;
            let trunk = random.next_int_bound(3)? + 1;
            for _ in 0..trunk {
                if !nested(feature, world, random, pos)? {
                    return Ok(true);
                }
                pos = tb::offset(pos, 0, 1, 0);
            }
            let top = pos;
            let branches = random.next_int_bound(3)? + 2;
            let mut directions = tb::HORIZONTAL;
            tb::shuffle(&mut directions, random)?;
            for &direction in directions.iter().take(branches as usize) {
                let mut pos = tb::relative(top, direction, 1);
                let height = random.next_int_bound(5)? + 2;
                let mut segment = 0;
                let mut j = 0;
                while j < height && nested(feature, world, random, pos)? {
                    segment += 1;
                    pos = tb::offset(pos, 0, 1, 0);
                    if j == 0 || (segment >= 2 && random.next_float() < 0.25) {
                        pos = tb::relative(pos, direction, 1);
                        segment = 0;
                    }
                    j += 1;
                }
            }
            return Ok(true);
        }
        if !nested(feature, world, random, origin)? {
            return Ok(false);
        }
        let claw = tb::HORIZONTAL[random.next_int_bound(4)? as usize];
        let branches = random.next_int_bound(2)? + 2;
        let mut possible = [claw, tb::clockwise(claw), counter_clockwise(claw)];
        tb::shuffle(&mut possible, random)?;
        for &branch in possible.iter().take(branches as usize) {
            let mut pos = origin;
            let sideways = random.next_int_bound(2)? + 1;
            pos = tb::relative(pos, branch, 1);
            let (segment, inway) = if branch == claw {
                (claw, random.next_int_bound(3)? + 2)
            } else {
                pos = tb::offset(pos, 0, 1, 0);
                let options = [branch, Direction::Up];
                let segment = options[random.next_int_bound(2)? as usize];
                (segment, random.next_int_bound(3)? + 3)
            };
            let mut i = 0;
            while i < sideways && nested(feature, world, random, pos)? {
                pos = tb::relative(pos, segment, 1);
                i += 1;
            }
            pos = tb::relative(pos, segment.opposite(), 1);
            pos = tb::offset(pos, 0, 1, 0);
            for _ in 0..inway {
                pos = tb::relative(pos, claw, 1);
                if !nested(feature, world, random, pos)? {
                    break;
                }
                if random.next_float() < 0.25 {
                    pos = tb::offset(pos, 0, 1, 0);
                }
            }
        }
        Ok(true)
    }
}
pub(crate) fn counter_clockwise(direction: Direction) -> Direction {
    match direction {
        Direction::North => Direction::West,
        Direction::East => Direction::North,
        Direction::South => Direction::East,
        Direction::West => Direction::South,
        other => other,
    }
}

#[derive(Clone, Debug)]
pub struct HugeMushroomFeature {
    red: bool,
    cap: BlockStateProvider,
    stem: BlockStateProvider,
    foliage_radius: i32,
    can_place_on: BlockPredicate,
    blocks: Arc<TreeBlocks>,
}
impl HugeMushroomFeature {
    pub(crate) fn decode(
        json: &str,
        bindings: &FeatureBlockBindings<'_>,
        depth: usize,
        remaining: &mut usize,
    ) -> Result<Self, DecodeError> {
        let object: Object = serde_json::from_str(json)?;
        let red = match kind(&object)?.as_str() {
            "minecraft:huge_red_mushroom" => true,
            "minecraft:huge_brown_mushroom" => false,
            _ => return Err(DecodeError::Field("huge mushroom type")),
        };
        let blocks = bindings.tree_blocks()?;
        let mut decoder = TreeDecoder {
            bindings,
            depth,
            remaining,
        };
        let cap = decoder.provider(required(&object, "cap_provider")?)?;
        let stem = decoder.provider(required(&object, "stem_provider")?)?;
        let foliage_radius = optional(&object, "foliage_radius")
            .map(super::integer)
            .transpose()?
            .unwrap_or(2);
        let can_place_on = BlockPredicate::decode_with_blocks(
            required(&object, "can_place_on")?.get(),
            depth + 1,
            decoder.remaining,
            Some(bindings),
        )?;
        Ok(Self {
            red,
            cap,
            stem,
            foliage_radius,
            can_place_on,
            blocks,
        })
    }
    fn radius_for_height(&self, tree_height: i32, leaf_radius: i32, yo: i32) -> i32 {
        if self.red {
            if (yo < tree_height && yo >= tree_height - 3) || yo == tree_height {
                leaf_radius
            } else {
                0
            }
        } else if yo <= 3 {
            0
        } else {
            leaf_radius
        }
    }
    fn place_block<W: FeatureWorld>(
        &self,
        world: &mut W,
        p: [i32; 3],
        state: BlockStateId,
    ) -> TreeResult<(), W::Error> {
        let current = read(world, p)?;
        if self.blocks.has(current, tb::AIR | tb::MUSHROOM_REPLACEABLE) {
            write(world, p, state, 3)?;
        }
        Ok(())
    }
    fn faces(&self, state: BlockStateId, faces: &[(&str, bool)]) -> BlockStateId {
        if faces
            .iter()
            .all(|(name, _)| self.blocks.properties.value(state, name).is_some())
        {
            faces.iter().fold(state, |state, (name, value)| {
                self.blocks
                    .with(state, name, if *value { "true" } else { "false" })
                    .unwrap_or(state)
            })
        } else {
            state
        }
    }
    pub(crate) fn place<W: FeatureWorld + PlacementContext, R: RandomSource>(
        &self,
        world: &mut W,
        random: &mut R,
        origin: [i32; 3],
    ) -> TreeResult<bool, W::Error> {
        let mut height = random.next_int_bound(3)? + 4;
        if random.next_int_bound(12)? == 0 {
            height *= 2;
        }
        let y = origin[1];
        let min = world.min_y().map_err(query)?;
        let max = world.max_y().map_err(query)?;
        if y < min.wrapping_add(1) || y.wrapping_add(height).wrapping_add(1) > max {
            return Ok(false);
        }
        if !self
            .can_place_on
            .test(world, tb::offset(origin, 0, -1, 0))
            .map_err(query)?
        {
            return Ok(false);
        }
        for dy in 0..=height {
            let radius = self.radius_for_height(-1, self.foliage_radius, dy);
            for dx in -radius..=radius {
                for dz in -radius..=radius {
                    let state = read(world, tb::offset(origin, dx, dy, dz))?;
                    if !self.blocks.has(state, tb::AIR | tb::LEAVES) {
                        return Ok(false);
                    }
                }
            }
        }
        let r = self.foliage_radius;
        if self.red {
            for dy in height - 3..=height {
                let radius = if dy < height { r } else { r - 1 };
                let center = r - 2;
                for dx in -radius..=radius {
                    for dz in -radius..=radius {
                        let x_edge = dx == -radius || dx == radius;
                        let z_edge = dz == -radius || dz == radius;
                        if dy >= height || x_edge != z_edge {
                            let p = tb::offset(origin, dx, dy, dz);
                            let state = sample_state(&self.cap, world, random, origin)?;
                            let state = self.faces(
                                state,
                                &[
                                    ("west", dx < -center),
                                    ("east", dx > center),
                                    ("north", dz < -center),
                                    ("south", dz > center),
                                    ("up", dy >= height - 1),
                                ],
                            );
                            self.place_block(world, p, state)?;
                        }
                    }
                }
            }
        } else {
            for dx in -r..=r {
                for dz in -r..=r {
                    let (min_x, max_x, min_z, max_z) = (dx == -r, dx == r, dz == -r, dz == r);
                    let x_edge = min_x || max_x;
                    let z_edge = min_z || max_z;
                    if !x_edge || !z_edge {
                        let p = tb::offset(origin, dx, height, dz);
                        let west = min_x || (z_edge && dx == 1 - r);
                        let east = max_x || (z_edge && dx == r - 1);
                        let north = min_z || (x_edge && dz == 1 - r);
                        let south = max_z || (x_edge && dz == r - 1);
                        let state = sample_state(&self.cap, world, random, origin)?;
                        let state = self.faces(
                            state,
                            &[
                                ("west", west),
                                ("east", east),
                                ("north", north),
                                ("south", south),
                            ],
                        );
                        self.place_block(world, p, state)?;
                    }
                }
            }
        }
        for dy in 0..height {
            let state = sample_state(&self.stem, world, random, origin)?;
            self.place_block(world, tb::offset(origin, 0, dy, 0), state)?;
        }
        Ok(true)
    }
}

#[derive(Clone, Debug)]
pub struct MultifaceGrowthFeature {
    block: BlockStateId,
    sculk: bool,
    search_range: i32,
    directions: Vec<Direction>,
    chance_of_spreading: f32,
    can_be_placed_on: Arc<BTreeSet<BlockStateId>>,
    blocks: Arc<TreeBlocks>,
}
#[derive(Clone, Copy)]
enum Spread {
    SamePosition,
    SamePlane,
    WrapAround,
}
impl MultifaceGrowthFeature {
    pub(crate) fn decode(
        json: &str,
        bindings: &FeatureBlockBindings<'_>,
    ) -> Result<Self, DecodeError> {
        let object: Object = serde_json::from_str(json)?;
        if kind(&object)? != "minecraft:multiface_growth" {
            return Err(DecodeError::Field("multiface growth type"));
        }
        let blocks = bindings.tree_blocks()?;
        let name = super::density::identifier(&serde_json::from_str::<String>(
            required(&object, "block")?.get(),
        )?)?;
        // The 26.3 MultifaceSpreadeableBlock subclasses.
        let sculk = match name.as_str() {
            "minecraft:glow_lichen" => false,
            "minecraft:sculk_vein" => true,
            _ => return Err(DecodeError::Field("multiface spreadeable block")),
        };
        let block = blocks
            .default_state(&name)
            .ok_or(DecodeError::Field("multiface block"))?;
        let flag = |key: &str| -> Result<bool, DecodeError> {
            Ok(optional(&object, key)
                .map(|raw| serde_json::from_str::<bool>(raw.get()))
                .transpose()?
                .unwrap_or(false))
        };
        let mut directions = Vec::new();
        if flag("can_place_on_ceiling")? {
            directions.push(Direction::Up);
        }
        if flag("can_place_on_floor")? {
            directions.push(Direction::Down);
        }
        if flag("can_place_on_wall")? {
            directions.extend(tb::HORIZONTAL);
        }
        Ok(Self {
            block,
            sculk,
            search_range: optional(&object, "search_range")
                .map(|raw| int_range(raw, "search_range", 1, 64))
                .transpose()?
                .unwrap_or(10),
            directions,
            chance_of_spreading: optional(&object, "chance_of_spreading")
                .map(|raw| unit_float(raw, "chance_of_spreading"))
                .transpose()?
                .unwrap_or(0.5),
            can_be_placed_on: super::block_predicate::PredicateBlockResolver::predicate_block_set(
                bindings,
                required(&object, "can_be_placed_on")?.get(),
            )?,
            blocks,
        })
    }
    fn air_or_water(&self, state: BlockStateId) -> bool {
        self.blocks.has(state, tb::AIR | tb::WATER_BLOCK)
    }
    fn is_block(&self, state: BlockStateId) -> bool {
        self.blocks.same_block(state, self.block)
    }
    fn has_face(&self, state: BlockStateId, face: Direction) -> bool {
        self.blocks
            .properties
            .value(state, tb::direction_name(face))
            == Some("true")
    }
    pub(crate) fn place<W: FeatureWorld + PlacementContext, R: RandomSource>(
        &self,
        world: &mut W,
        random: &mut R,
        origin: [i32; 3],
    ) -> TreeResult<bool, W::Error> {
        let here = read(world, origin)?;
        if !self.air_or_water(here) {
            return Ok(false);
        }
        debug_assert!(self.directions.len() <= Direction::ALL.len());
        let direction_count = self.directions.len();
        let mut directions = [Direction::Up; Direction::ALL.len()];
        directions[..direction_count].copy_from_slice(&self.directions);
        tb::shuffle(&mut directions[..direction_count], random)?;
        let here = read(world, origin)?;
        if self.place_growth(world, random, origin, here, &directions[..direction_count])? {
            return Ok(true);
        }
        for &search in &directions[..direction_count] {
            let mut placement = [Direction::Up; Direction::ALL.len()];
            let mut placement_count = 0;
            for &direction in &self.directions {
                if direction != search.opposite() {
                    placement[placement_count] = direction;
                    placement_count += 1;
                }
            }
            tb::shuffle(&mut placement[..placement_count], random)?;
            for _ in 0..self.search_range {
                // Native setWithOffset(origin, ...) revisits the first neighbor.
                let pos = tb::relative(origin, search, 1);
                let state = read(world, pos)?;
                if !self.air_or_water(state) && !self.is_block(state) {
                    break;
                }
                if self.place_growth(world, random, pos, state, &placement[..placement_count])? {
                    return Ok(true);
                }
            }
        }
        Ok(false)
    }
    fn place_growth<W: FeatureWorld + PlacementContext, R: RandomSource>(
        &self,
        world: &mut W,
        random: &mut R,
        pos: [i32; 3],
        old: BlockStateId,
        directions: &[Direction],
    ) -> TreeResult<bool, W::Error> {
        for &direction in directions {
            let neighbor = read(world, tb::relative(pos, direction, 1))?;
            if self.can_be_placed_on.contains(&neighbor) {
                let Some(state) = self.state_for_placement(world, old, pos, direction)? else {
                    return Ok(false);
                };
                write(world, pos, state, 3)?;
                world.mark_postprocessing(pos).map_err(query)?;
                if random.next_float() < self.chance_of_spreading {
                    let mut all = Direction::ALL;
                    tb::shuffle(&mut all, random)?;
                    for spread in all {
                        if self.spread(world, state, pos, direction, spread)? {
                            break;
                        }
                    }
                }
                return Ok(true);
            }
        }
        Ok(false)
    }
    fn valid_for_placement<W: FeatureWorld + PlacementContext>(
        &self,
        world: &mut W,
        old: BlockStateId,
        pos: [i32; 3],
        direction: Direction,
    ) -> TreeResult<bool, W::Error> {
        if self.is_block(old) && self.has_face(old, direction) {
            return Ok(false);
        }
        world.attachment_at(pos, direction).map_err(query)
    }
    fn state_for_placement<W: FeatureWorld + PlacementContext>(
        &self,
        world: &mut W,
        old: BlockStateId,
        pos: [i32; 3],
        direction: Direction,
    ) -> TreeResult<Option<BlockStateId>, W::Error> {
        if !self.valid_for_placement(world, old, pos, direction)? {
            return Ok(None);
        }
        let state = if self.is_block(old) {
            old
        } else if self.blocks.has(old, tb::WATER_SOURCE) {
            self.blocks.waterlogged(self.block, true)
        } else {
            self.block
        };
        Ok(Some(
            self.blocks
                .with(state, tb::direction_name(direction), "true")
                .ok_or(FeatureExecutionError::InvalidState)?,
        ))
    }
    fn spread_types(&self) -> [Spread; 3] {
        [Spread::SamePosition, Spread::SamePlane, Spread::WrapAround]
    }
    /// spreadFromFaceTowardDirection with postProcess = true.
    fn spread<W: FeatureWorld + PlacementContext>(
        &self,
        world: &mut W,
        state: BlockStateId,
        pos: [i32; 3],
        from: Direction,
        direction: Direction,
    ) -> TreeResult<bool, W::Error> {
        if tb::Axis::of(direction) == tb::Axis::of(from) {
            return Ok(false);
        }
        let other_source = self.sculk && !self.blocks.has(state, tb::SCULK_VEIN);
        if !(other_source || (self.has_face(state, from) && !self.has_face(state, direction))) {
            return Ok(false);
        }
        for kind in self.spread_types() {
            let (target, face) = match kind {
                Spread::SamePosition => (pos, direction),
                Spread::SamePlane => (tb::relative(pos, direction, 1), from),
                Spread::WrapAround => (
                    tb::relative(tb::relative(pos, direction, 1), from, 1),
                    direction.opposite(),
                ),
            };
            if self.can_spread_into(world, pos, target, face)? {
                let old = read(world, target)?;
                let Some(new) = self.state_for_placement(world, old, target, face)? else {
                    return Ok(false);
                };
                world.mark_postprocessing(target).map_err(query)?;
                return write(world, target, new, 2);
            }
        }
        Ok(false)
    }
    fn can_spread_into<W: FeatureWorld + PlacementContext>(
        &self,
        world: &mut W,
        source: [i32; 3],
        target: [i32; 3],
        face: Direction,
    ) -> TreeResult<bool, W::Error> {
        let existing = read(world, target)?;
        let replaceable = if self.sculk {
            let against = read(world, tb::relative(target, face, 1))?;
            if self.blocks.has(against, tb::SCULK_BLOCKER) {
                false
            } else {
                let mut blocked = false;
                if tb::dist_manhattan(source, target) == 2 {
                    let neighbor = tb::relative(source, face.opposite(), 1);
                    let state = read(world, neighbor)?;
                    blocked = world
                        .state_face_sturdy(state, neighbor, face)
                        .map_err(query)?;
                }
                if blocked
                    || (self.blocks.has(existing, tb::ANY_FLUID)
                        && !self.blocks.has(existing, tb::WATER_SOURCE))
                    || self.blocks.has(existing, tb::FIRE)
                {
                    false
                } else {
                    self.blocks.has(existing, tb::REPLACEABLE) || self.default_replaceable(existing)
                }
            }
        } else {
            self.default_replaceable(existing)
        };
        Ok(replaceable && self.valid_for_placement(world, existing, target, face)?)
    }
    fn default_replaceable(&self, existing: BlockStateId) -> bool {
        self.blocks.is_air(existing)
            || self.is_block(existing)
            || (self.blocks.has(existing, tb::WATER_BLOCK)
                && self.blocks.has(existing, tb::WATER_SOURCE))
    }
}
