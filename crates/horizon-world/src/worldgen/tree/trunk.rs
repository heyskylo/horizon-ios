//! Trunk placers (TrunkPlacer subclasses), in native random-draw order.
use super::{
    FoliageAttachment, Layer, Run, TreeDecoder, TreeFeature, TreeResult, kind, read,
    sample_optional, sample_state, unit_float,
};
use crate::registry::BlockStateId;
use crate::worldgen::{
    DecodeError, Object,
    feature::FeatureWorld,
    int_range, optional,
    placement::PlacementContext,
    required,
    tree_blocks::{self as tb, Axis, TreeBlocks},
    value_provider::IntProvider,
};
use horizon_core::{geometry::Direction, random::RandomSource};
use serde_json::value::RawValue;
use std::{collections::BTreeSet, sync::Arc};

#[derive(Clone, Debug)]
enum Kind {
    Straight,
    Forking,
    Giant,
    MegaJungle,
    DarkOak,
    Fancy,
    Bending {
        min_height_for_leaves: i32,
        bend_length: IntProvider,
    },
    UpwardsBranching {
        extra_branch_steps: IntProvider,
        branch_probability: f32,
        extra_branch_length: IntProvider,
        can_grow_through: Arc<BTreeSet<BlockStateId>>,
    },
    Cherry {
        branch_count: IntProvider,
        branch_horizontal_length: IntProvider,
        branch_start: [i32; 2],
        branch_end_offset: IntProvider,
    },
    Poplar {
        trunk_height_above_branches: IntProvider,
        branch_amount: IntProvider,
    },
}
#[derive(Clone, Debug)]
pub(super) struct TrunkPlacer {
    base_height: i32,
    height_rand_a: i32,
    height_rand_b: i32,
    kind: Kind,
}

fn random_horizontal(
    random: &mut impl RandomSource,
) -> Result<Direction, horizon_core::random::InvalidBound> {
    Ok(tb::HORIZONTAL[random.next_int_bound(4)? as usize])
}

impl TrunkPlacer {
    pub(super) fn decode(
        raw: &RawValue,
        decoder: &mut TreeDecoder<'_, '_>,
    ) -> Result<Self, DecodeError> {
        let object: Object = serde_json::from_str(raw.get())?;
        let base_height = int_range(required(&object, "base_height")?, "base_height", 0, 32)?;
        let height_rand_a = int_range(required(&object, "height_rand_a")?, "height_rand_a", 0, 24)?;
        let height_rand_b = int_range(required(&object, "height_rand_b")?, "height_rand_b", 0, 24)?;
        let kind = match kind(&object)?.as_str() {
            "minecraft:straight_trunk_placer" => Kind::Straight,
            "minecraft:forking_trunk_placer" => Kind::Forking,
            "minecraft:giant_trunk_placer" => Kind::Giant,
            "minecraft:mega_jungle_trunk_placer" => Kind::MegaJungle,
            "minecraft:dark_oak_trunk_placer" => Kind::DarkOak,
            "minecraft:fancy_trunk_placer" => Kind::Fancy,
            "minecraft:bending_trunk_placer" => Kind::Bending {
                min_height_for_leaves: optional(&object, "min_height_for_leaves")
                    .map(|raw| int_range(raw, "min_height_for_leaves", 1, i32::MAX))
                    .transpose()?
                    .unwrap_or(1),
                bend_length: decoder.int_provider(required(&object, "bend_length")?, 1, 64)?,
            },
            "minecraft:upwards_branching_trunk_placer" => Kind::UpwardsBranching {
                extra_branch_steps: decoder.int_provider(
                    required(&object, "extra_branch_steps")?,
                    1,
                    i32::MAX,
                )?,
                branch_probability: unit_float(
                    required(&object, "place_branch_per_log_probability")?,
                    "place_branch_per_log_probability",
                )?,
                extra_branch_length: decoder.int_provider(
                    required(&object, "extra_branch_length")?,
                    0,
                    i32::MAX,
                )?,
                can_grow_through: decoder.block_set(required(&object, "can_grow_through")?)?,
            },
            "minecraft:cherry_trunk_placer" => {
                let start: Object =
                    serde_json::from_str(required(&object, "branch_start_offset_from_top")?.get())?;
                if let Some(kind) = optional(&start, "type")
                    && serde_json::from_str::<String>(kind.get())? != "minecraft:uniform"
                {
                    return Err(DecodeError::Field("cherry branch start type"));
                }
                let min = int_range(required(&start, "min_inclusive")?, "branch start", -16, 0)?;
                let max = int_range(required(&start, "max_inclusive")?, "branch start", -16, 0)?;
                if max - min < 1 {
                    return Err(DecodeError::Field("cherry branch start variation"));
                }
                Kind::Cherry {
                    branch_count: decoder.int_provider(required(&object, "branch_count")?, 1, 3)?,
                    branch_horizontal_length: decoder.int_provider(
                        required(&object, "branch_horizontal_length")?,
                        2,
                        16,
                    )?,
                    branch_start: [min, max],
                    branch_end_offset: decoder.int_provider(
                        required(&object, "branch_end_offset_from_top")?,
                        -16,
                        16,
                    )?,
                }
            }
            "minecraft:poplar_trunk_placer" => Kind::Poplar {
                trunk_height_above_branches: decoder.int_provider(
                    required(&object, "trunk_height_above_branches")?,
                    0,
                    8,
                )?,
                branch_amount: decoder.int_provider(required(&object, "branch_amount")?, 1, 4)?,
            },
            _ => return Err(DecodeError::Field("trunk placer type")),
        };
        Ok(Self {
            base_height,
            height_rand_a,
            height_rand_b,
            kind,
        })
    }
    pub(super) fn tree_height<E>(&self, random: &mut impl RandomSource) -> TreeResult<i32, E> {
        let a = random.next_int_bound(self.height_rand_a + 1)?;
        let b = random.next_int_bound(self.height_rand_b + 1)?;
        Ok(self.base_height + a + b)
    }
    /// Virtual TrunkPlacer.validTreePos.
    fn valid_tree_pos(&self, blocks: &TreeBlocks, state: BlockStateId) -> bool {
        blocks.valid_tree_pos(state)
            || matches!(&self.kind, Kind::UpwardsBranching { can_grow_through, .. } if can_grow_through.contains(&state))
    }
    pub(super) fn is_free(&self, blocks: &TreeBlocks, state: BlockStateId) -> bool {
        self.valid_tree_pos(blocks, state) || blocks.has(state, tb::LOGS)
    }
    fn place_log<W: FeatureWorld + PlacementContext, R: RandomSource>(
        &self,
        run: &mut Run<'_, W, R>,
        tree: &TreeFeature,
        position: [i32; 3],
        axis: Option<Axis>,
    ) -> TreeResult<bool, W::Error> {
        let here = read(run.world, position)?;
        if !self.valid_tree_pos(&tree.blocks, here) {
            return Ok(false);
        }
        let mut state = sample_state(&tree.trunk_provider, run.world, run.random, position)?;
        if let Some(axis) = axis {
            state = tree.blocks.try_axis(state, axis);
        }
        run.set(Layer::Trunk, position, state)?;
        Ok(true)
    }
    fn place_log_if_free<W: FeatureWorld + PlacementContext, R: RandomSource>(
        &self,
        run: &mut Run<'_, W, R>,
        tree: &TreeFeature,
        position: [i32; 3],
    ) -> TreeResult<(), W::Error> {
        let here = read(run.world, position)?;
        if self.is_free(&tree.blocks, here) {
            self.place_log(run, tree, position, None)?;
        }
        Ok(())
    }
    fn place_below_trunk<W: FeatureWorld + PlacementContext, R: RandomSource>(
        run: &mut Run<'_, W, R>,
        tree: &TreeFeature,
        position: [i32; 3],
    ) -> TreeResult<(), W::Error> {
        if let Some(state) =
            sample_optional(&tree.below_trunk_provider, run.world, run.random, position)?
        {
            run.set(Layer::Trunk, position, state)?;
        }
        Ok(())
    }
    pub(super) fn place_trunk<W: FeatureWorld + PlacementContext, R: RandomSource>(
        &self,
        run: &mut Run<'_, W, R>,
        tree: &TreeFeature,
        tree_height: i32,
        origin: [i32; 3],
    ) -> TreeResult<Vec<FoliageAttachment>, W::Error> {
        let below = tb::offset(origin, 0, -1, 0);
        match &self.kind {
            Kind::Straight => {
                Self::place_below_trunk(run, tree, below)?;
                for y in 0..tree_height {
                    self.place_log(run, tree, tb::offset(origin, 0, y, 0), None)?;
                }
                Ok(vec![FoliageAttachment::new(
                    tb::offset(origin, 0, tree_height, 0),
                    0,
                    false,
                )])
            }
            Kind::Forking => self.forking(run, tree, tree_height, origin),
            Kind::Giant => self.giant(run, tree, tree_height, origin),
            Kind::MegaJungle => {
                let mut attachments = self.giant(run, tree, tree_height, origin)?;
                let mut branch_height = tree_height - 2 - run.random.next_int_bound(4)?;
                while branch_height > tree_height / 2 {
                    let angle = run.random.next_float() * (std::f64::consts::PI * 2.0) as f32;
                    let (mut bx, mut bz) = (0, 0);
                    for b in 0..5 {
                        bx = (1.5f32
                            + horizon_core::geometry::trig::cos(f64::from(angle)) * b as f32)
                            as i32;
                        bz = (1.5f32
                            + horizon_core::geometry::trig::sin(f64::from(angle)) * b as f32)
                            as i32;
                        let p = tb::offset(origin, bx, branch_height - 3 + b / 2, bz);
                        self.place_log(run, tree, p, None)?;
                    }
                    attachments.push(FoliageAttachment::new(
                        tb::offset(origin, bx, branch_height, bz),
                        -2,
                        false,
                    ));
                    branch_height -= 2 + run.random.next_int_bound(4)?;
                }
                Ok(attachments)
            }
            Kind::DarkOak => self.dark_oak(run, tree, tree_height, origin),
            Kind::Fancy => self.fancy(run, tree, tree_height, origin),
            Kind::Bending {
                min_height_for_leaves,
                bend_length,
            } => {
                let direction = random_horizontal(run.random)?;
                let log_height = tree_height - 1;
                let mut pos = origin;
                Self::place_below_trunk(run, tree, below)?;
                let mut points = Vec::new();
                for i in 0..=log_height {
                    if i + 1 >= log_height + run.random.next_int_bound(2)? {
                        pos = tb::relative(pos, direction, 1);
                    }
                    if tree.blocks.valid_tree_pos(read(run.world, pos)?) {
                        self.place_log(run, tree, pos, None)?;
                    }
                    if i >= *min_height_for_leaves {
                        points.push(FoliageAttachment::new(pos, 0, false));
                    }
                    pos = tb::offset(pos, 0, 1, 0);
                }
                let length = bend_length.sample(run.random)?;
                for _ in 0..=length {
                    if tree.blocks.valid_tree_pos(read(run.world, pos)?) {
                        self.place_log(run, tree, pos, None)?;
                    }
                    points.push(FoliageAttachment::new(pos, 0, false));
                    pos = tb::relative(pos, direction, 1);
                }
                Ok(points)
            }
            Kind::UpwardsBranching {
                extra_branch_steps,
                branch_probability,
                extra_branch_length,
                ..
            } => {
                let mut attachments = Vec::new();
                for height in 0..tree_height {
                    let current = origin[1].wrapping_add(height);
                    let log = [origin[0], current, origin[2]];
                    if self.place_log(run, tree, log, None)?
                        && height < tree_height - 1
                        && run.random.next_float() < *branch_probability
                    {
                        let direction = random_horizontal(run.random)?;
                        let length = extra_branch_length.sample(run.random)?;
                        let branch_pos =
                            0.max(length - extra_branch_length.sample(run.random)? - 1);
                        let steps = extra_branch_steps.sample(run.random)?;
                        self.upwards_branch(
                            run,
                            tree,
                            tree_height,
                            &mut attachments,
                            log,
                            current,
                            direction,
                            branch_pos,
                            steps,
                        )?;
                    }
                    if height == tree_height - 1 {
                        attachments.push(FoliageAttachment::new(
                            [origin[0], current.wrapping_add(1), origin[2]],
                            0,
                            false,
                        ));
                    }
                }
                Ok(attachments)
            }
            Kind::Cherry { .. } => self.cherry(run, tree, tree_height, origin),
            Kind::Poplar {
                trunk_height_above_branches,
                branch_amount,
            } => {
                Self::place_below_trunk(run, tree, below)?;
                let up_to_branches =
                    tree_height - trunk_height_above_branches.sample(run.random)?;
                for y in 0..tree_height {
                    let log = tb::offset(origin, 0, y, 0);
                    self.place_log(run, tree, log, None)?;
                    let mut all = Direction::ALL;
                    tb::shuffle(&mut all, run.random)?;
                    let directions: Vec<Direction> = all
                        .into_iter()
                        .filter(|d| !matches!(d, Direction::Up | Direction::Down))
                        .collect();
                    if up_to_branches - 1 == y {
                        let branches = branch_amount.sample(run.random)?;
                        for direction in directions.iter().take(branches.max(0) as usize) {
                            self.place_log(
                                run,
                                tree,
                                tb::relative(log, *direction, 1),
                                Some(Axis::of(*direction)),
                            )?;
                        }
                    }
                }
                Ok(vec![FoliageAttachment::new(
                    tb::offset(origin, 0, up_to_branches, 0),
                    0,
                    false,
                )])
            }
        }
    }

    fn forking<W: FeatureWorld + PlacementContext, R: RandomSource>(
        &self,
        run: &mut Run<'_, W, R>,
        tree: &TreeFeature,
        tree_height: i32,
        origin: [i32; 3],
    ) -> TreeResult<Vec<FoliageAttachment>, W::Error> {
        Self::place_below_trunk(run, tree, tb::offset(origin, 0, -1, 0))?;
        let mut attachments = Vec::new();
        let lean = random_horizontal(run.random)?;
        let lean_height = tree_height - run.random.next_int_bound(4)? - 1;
        let mut lean_steps = 3 - run.random.next_int_bound(3)?;
        let (mut tx, mut tz) = (origin[0], origin[2]);
        let mut ey = None;
        for yo in 0..tree_height {
            let yy = origin[1].wrapping_add(yo);
            if yo >= lean_height && lean_steps > 0 {
                let n = lean.normal();
                tx = tx.wrapping_add(n[0]);
                tz = tz.wrapping_add(n[2]);
                lean_steps -= 1;
            }
            if self.place_log(run, tree, [tx, yy, tz], None)? {
                ey = Some(yy.wrapping_add(1));
            }
        }
        if let Some(ey) = ey {
            attachments.push(FoliageAttachment::new([tx, ey, tz], 1, false));
        }
        tx = origin[0];
        tz = origin[2];
        let branch = random_horizontal(run.random)?;
        if branch != lean {
            let branch_pos = lean_height - run.random.next_int_bound(2)? - 1;
            let mut steps = 1 + run.random.next_int_bound(3)?;
            let mut ey = None;
            let mut yo = branch_pos;
            while yo < tree_height && steps > 0 {
                if yo >= 1 {
                    let yy = origin[1].wrapping_add(yo);
                    let n = branch.normal();
                    tx = tx.wrapping_add(n[0]);
                    tz = tz.wrapping_add(n[2]);
                    if self.place_log(run, tree, [tx, yy, tz], None)? {
                        ey = Some(yy.wrapping_add(1));
                    }
                }
                yo += 1;
                steps -= 1;
            }
            if let Some(ey) = ey {
                attachments.push(FoliageAttachment::new([tx, ey, tz], 0, false));
            }
        }
        Ok(attachments)
    }

    fn giant<W: FeatureWorld + PlacementContext, R: RandomSource>(
        &self,
        run: &mut Run<'_, W, R>,
        tree: &TreeFeature,
        tree_height: i32,
        origin: [i32; 3],
    ) -> TreeResult<Vec<FoliageAttachment>, W::Error> {
        let below = tb::offset(origin, 0, -1, 0);
        Self::place_below_trunk(run, tree, below)?;
        Self::place_below_trunk(run, tree, tb::offset(below, 1, 0, 0))?;
        Self::place_below_trunk(run, tree, tb::offset(below, 0, 0, 1))?;
        Self::place_below_trunk(run, tree, tb::offset(below, 1, 0, 1))?;
        for hh in 0..tree_height {
            self.place_log_if_free(run, tree, tb::offset(origin, 0, hh, 0))?;
            if hh < tree_height - 1 {
                self.place_log_if_free(run, tree, tb::offset(origin, 1, hh, 0))?;
                self.place_log_if_free(run, tree, tb::offset(origin, 1, hh, 1))?;
                self.place_log_if_free(run, tree, tb::offset(origin, 0, hh, 1))?;
            }
        }
        Ok(vec![FoliageAttachment::new(
            tb::offset(origin, 0, tree_height, 0),
            0,
            true,
        )])
    }

    fn dark_oak<W: FeatureWorld + PlacementContext, R: RandomSource>(
        &self,
        run: &mut Run<'_, W, R>,
        tree: &TreeFeature,
        tree_height: i32,
        origin: [i32; 3],
    ) -> TreeResult<Vec<FoliageAttachment>, W::Error> {
        let mut attachments = Vec::new();
        let below = tb::offset(origin, 0, -1, 0);
        Self::place_below_trunk(run, tree, below)?;
        Self::place_below_trunk(run, tree, tb::offset(below, 1, 0, 0))?;
        Self::place_below_trunk(run, tree, tb::offset(below, 0, 0, 1))?;
        Self::place_below_trunk(run, tree, tb::offset(below, 1, 0, 1))?;
        let lean = random_horizontal(run.random)?;
        let lean_height = tree_height - run.random.next_int_bound(4)?;
        let mut lean_steps = 2 - run.random.next_int_bound(3)?;
        let [x, y, z] = origin;
        let (mut tx, mut tz) = (x, z);
        let ey = y.wrapping_add(tree_height).wrapping_sub(1);
        for dy in 0..tree_height {
            if dy >= lean_height && lean_steps > 0 {
                let n = lean.normal();
                tx = tx.wrapping_add(n[0]);
                tz = tz.wrapping_add(n[2]);
                lean_steps -= 1;
            }
            let p = [tx, y.wrapping_add(dy), tz];
            let here = read(run.world, p)?;
            if tree.blocks.has(here, tb::AIR | tb::LEAVES) {
                self.place_log(run, tree, p, None)?;
                self.place_log(run, tree, tb::offset(p, 1, 0, 0), None)?;
                self.place_log(run, tree, tb::offset(p, 0, 0, 1), None)?;
                self.place_log(run, tree, tb::offset(p, 1, 0, 1), None)?;
            }
        }
        attachments.push(FoliageAttachment::new([tx, ey, tz], 0, true));
        for ox in -1..=2 {
            for oz in -1..=2 {
                if (!(0..=1).contains(&ox) || !(0..=1).contains(&oz))
                    && run.random.next_int_bound(3)? <= 0
                {
                    let length = run.random.next_int_bound(3)? + 2;
                    for branch_y in 0..length {
                        self.place_log(
                            run,
                            tree,
                            [
                                x.wrapping_add(ox),
                                ey.wrapping_sub(branch_y).wrapping_sub(1),
                                z.wrapping_add(oz),
                            ],
                            None,
                        )?;
                    }
                    attachments.push(FoliageAttachment::new(
                        [x.wrapping_add(ox), ey, z.wrapping_add(oz)],
                        0,
                        false,
                    ));
                }
            }
        }
        Ok(attachments)
    }

    #[allow(clippy::too_many_arguments)]
    fn upwards_branch<W: FeatureWorld + PlacementContext, R: RandomSource>(
        &self,
        run: &mut Run<'_, W, R>,
        tree: &TreeFeature,
        tree_height: i32,
        attachments: &mut Vec<FoliageAttachment>,
        log: [i32; 3],
        current_height: i32,
        direction: Direction,
        branch_pos: i32,
        mut steps: i32,
    ) -> TreeResult<(), W::Error> {
        let mut height_along = current_height.wrapping_add(branch_pos);
        let (mut lx, mut lz) = (log[0], log[2]);
        let mut index = branch_pos;
        let n = direction.normal();
        while index < tree_height && steps > 0 {
            if index >= 1 {
                let placement = current_height.wrapping_add(index);
                lx = lx.wrapping_add(n[0]);
                lz = lz.wrapping_add(n[2]);
                height_along = placement;
                let log = [lx, placement, lz];
                if self.place_log(run, tree, log, None)? {
                    height_along = height_along.wrapping_add(1);
                }
                attachments.push(FoliageAttachment::new(log, 0, false));
            }
            index += 1;
            steps -= 1;
        }
        if height_along.wrapping_sub(current_height) > 1 {
            let foliage = [lx, height_along, lz];
            attachments.push(FoliageAttachment::new(foliage, 0, false));
            attachments.push(FoliageAttachment::new(
                tb::offset(foliage, 0, -2, 0),
                0,
                false,
            ));
        }
        Ok(())
    }

    fn cherry<W: FeatureWorld + PlacementContext, R: RandomSource>(
        &self,
        run: &mut Run<'_, W, R>,
        tree: &TreeFeature,
        tree_height: i32,
        origin: [i32; 3],
    ) -> TreeResult<Vec<FoliageAttachment>, W::Error> {
        let Kind::Cherry {
            branch_count,
            branch_horizontal_length,
            branch_start: [start_min, start_max],
            branch_end_offset,
        } = &self.kind
        else {
            unreachable!("cherry trunk kind");
        };
        Self::place_below_trunk(run, tree, tb::offset(origin, 0, -1, 0))?;
        let first = 0.max(
            tree_height - 1
                + run
                    .random
                    .next_int_between_inclusive(*start_min, *start_max)?,
        );
        let mut second = 0.max(
            tree_height - 1
                + run
                    .random
                    .next_int_between_inclusive(*start_min, start_max - 1)?,
        );
        if second >= first {
            second += 1;
        }
        let count = branch_count.sample(run.random)?;
        let middle = count == 3;
        let both = count >= 2;
        let trunk_height = if middle {
            tree_height
        } else if both {
            first.max(second) + 1
        } else {
            first + 1
        };
        for y in 0..trunk_height {
            self.place_log(run, tree, tb::offset(origin, 0, y, 0), None)?;
        }
        let mut attachments = Vec::new();
        if middle {
            attachments.push(FoliageAttachment::new(
                tb::offset(origin, 0, trunk_height, 0),
                0,
                false,
            ));
        }
        let direction = random_horizontal(run.random)?;
        let sideways = Axis::of(direction);
        let generate = |this: &Self,
                        run: &mut Run<'_, W, R>,
                        branch_direction: Direction,
                        offset: i32,
                        middle_continues: bool|
         -> TreeResult<FoliageAttachment, W::Error> {
            let mut log = tb::offset(origin, 0, offset, 0);
            let end_offset = tree_height - 1 + branch_end_offset.sample(run.random)?;
            let extend = middle_continues || end_offset < offset;
            let distance = branch_horizontal_length.sample(run.random)? + i32::from(extend);
            let end = tb::offset(
                tb::relative(origin, branch_direction, distance),
                0,
                end_offset,
                0,
            );
            let steps = if extend { 2 } else { 1 };
            for _ in 0..steps {
                log = tb::relative(log, branch_direction, 1);
                this.place_log(run, tree, log, Some(sideways))?;
            }
            let vertical = if end[1] > log[1] {
                Direction::Up
            } else {
                Direction::Down
            };
            loop {
                let distance = tb::dist_manhattan(log, end);
                if distance == 0 {
                    return Ok(FoliageAttachment::new(tb::offset(end, 0, 1, 0), 0, false));
                }
                let chance = end[1].wrapping_sub(log[1]).wrapping_abs() as f32 / distance as f32;
                let grow_vertically = run.random.next_float() < chance;
                log = tb::relative(
                    log,
                    if grow_vertically {
                        vertical
                    } else {
                        branch_direction
                    },
                    1,
                );
                this.place_log(
                    run,
                    tree,
                    log,
                    if grow_vertically {
                        None
                    } else {
                        Some(sideways)
                    },
                )?;
            }
        };
        attachments.push(generate(
            self,
            run,
            direction,
            first,
            first < trunk_height - 1,
        )?);
        if both {
            attachments.push(generate(
                self,
                run,
                direction.opposite(),
                second,
                second < trunk_height - 1,
            )?);
        }
        Ok(attachments)
    }

    fn fancy<W: FeatureWorld + PlacementContext, R: RandomSource>(
        &self,
        run: &mut Run<'_, W, R>,
        tree: &TreeFeature,
        tree_height: i32,
        origin: [i32; 3],
    ) -> TreeResult<Vec<FoliageAttachment>, W::Error> {
        let height = tree_height + 2;
        let trunk_height = (f64::from(height) * 0.618).floor() as i32;
        Self::place_below_trunk(run, tree, tb::offset(origin, 0, -1, 0))?;
        let clusters_per_y = 1.min(
            (1.382 + horizon_core::jmath::pow(1.0 * f64::from(height) / 13.0, 2.0)).floor() as i32,
        );
        let trunk_top = origin[1].wrapping_add(trunk_height);
        let mut relative_y = height - 5;
        // (attachment position, branch base y)
        let mut coords = vec![(tb::offset(origin, 0, relative_y, 0), trunk_top)];
        while relative_y >= 0 {
            let shape = tree_shape(height, relative_y);
            // tree_shape never returns NaN.
            if shape >= 0.0 {
                for _ in 0..clusters_per_y {
                    let radius =
                        1.0 * f64::from(shape) * (f64::from(run.random.next_float()) + 0.328);
                    let angle = f64::from(run.random.next_float() * 2.0f32) * std::f64::consts::PI;
                    let x = radius * horizon_core::jmath::sin(angle) + 0.5;
                    let z = radius * horizon_core::jmath::cos(angle) + 0.5;
                    let check_start =
                        tb::offset(origin, x.floor() as i32, relative_y - 1, z.floor() as i32);
                    let check_end = tb::offset(check_start, 0, 5, 0);
                    if self.make_limb(run, tree, check_start, check_end, false)? {
                        let dx = origin[0].wrapping_sub(check_start[0]);
                        let dz = origin[2].wrapping_sub(check_start[2]);
                        let branch_height = f64::from(check_start[1])
                            - f64::from(dx.wrapping_mul(dx).wrapping_add(dz.wrapping_mul(dz)))
                                .sqrt()
                                * 0.381;
                        let branch_top = if branch_height > f64::from(trunk_top) {
                            trunk_top
                        } else {
                            branch_height as i32
                        };
                        let base = [origin[0], branch_top, origin[2]];
                        if self.make_limb(run, tree, base, check_start, false)? {
                            coords.push((check_start, base[1]));
                        }
                    }
                }
            }
            relative_y -= 1;
        }
        self.make_limb(
            run,
            tree,
            origin,
            tb::offset(origin, 0, trunk_height, 0),
            true,
        )?;
        for (end, branch_base) in &coords {
            let base = [origin[0], *branch_base, origin[2]];
            if base != *end && trim_branches(height, branch_base.wrapping_sub(origin[1])) {
                self.make_limb(run, tree, base, *end, true)?;
            }
        }
        Ok(coords
            .into_iter()
            .filter(|(_, base)| trim_branches(height, base.wrapping_sub(origin[1])))
            .map(|(p, _)| FoliageAttachment::new(p, 0, false))
            .collect())
    }

    fn make_limb<W: FeatureWorld + PlacementContext, R: RandomSource>(
        &self,
        run: &mut Run<'_, W, R>,
        tree: &TreeFeature,
        start: [i32; 3],
        end: [i32; 3],
        place: bool,
    ) -> TreeResult<bool, W::Error> {
        if !place && start == end {
            return Ok(true);
        }
        let delta: [i32; 3] = std::array::from_fn(|i| end[i].wrapping_sub(start[i]));
        let steps = delta[0]
            .wrapping_abs()
            .max(delta[1].wrapping_abs().max(delta[2].wrapping_abs()));
        let step: [f32; 3] = delta.map(|d| d as f32 / steps as f32);
        for i in 0..=steps {
            let p: [i32; 3] = std::array::from_fn(|axis| {
                start[axis].wrapping_add((0.5f32 + i as f32 * step[axis]).floor() as i32)
            });
            if place {
                let axis = log_axis(start, p);
                self.place_log(run, tree, p, Some(axis))?;
            } else if !self.is_free(&tree.blocks, read(run.world, p)?) {
                return Ok(false);
            }
        }
        Ok(true)
    }
}

fn log_axis(start: [i32; 3], p: [i32; 3]) -> Axis {
    let xdiff = p[0].wrapping_sub(start[0]).wrapping_abs();
    let zdiff = p[2].wrapping_sub(start[2]).wrapping_abs();
    let max = xdiff.max(zdiff);
    if max > 0 {
        if xdiff == max { Axis::X } else { Axis::Z }
    } else {
        Axis::Y
    }
}
fn trim_branches(height: i32, local_y: i32) -> bool {
    f64::from(local_y) >= f64::from(height) * 0.2
}
fn tree_shape(height: i32, y: i32) -> f32 {
    if (y as f32) < height as f32 * 0.3f32 {
        return -1.0;
    }
    let radius = height as f32 / 2.0;
    let adjacent = radius - y as f32;
    let mut distance = (radius * radius - adjacent * adjacent).sqrt();
    if adjacent == 0.0 {
        distance = radius;
    } else if adjacent.abs() >= radius {
        return 0.0;
    }
    distance * 0.5
}
