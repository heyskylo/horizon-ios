//! Registry-bound block classifications and property transitions shared by tree,
//! fallen-tree and other vegetation features. Built once per binding snapshot.
use super::{
    DecodeError, block_feature::FeatureBlockBindings, state_provider::properties::StateProperties,
};
use crate::{
    block_properties::BlockProperty,
    fluid::FluidKind,
    registry::{BlockRegistry, BlockStateId},
};
use horizon_core::geometry::Direction;
use std::{collections::BTreeMap, sync::Arc};

pub(crate) const AIR: u32 = 1;
pub(crate) const LEAVES: u32 = 1 << 1;
pub(crate) const REPLACEABLE_BY_TREES: u32 = 1 << 2;
pub(crate) const LOGS: u32 = 1 << 3;
pub(crate) const PREVENTS_LEAF_DECAY: u32 = 1 << 4;
pub(crate) const VINE: u32 = 1 << 5;
pub(crate) const WATER_BLOCK: u32 = 1 << 6;
pub(crate) const WATER_SOURCE: u32 = 1 << 7;
pub(crate) const WATER_FLUID: u32 = 1 << 8;
pub(crate) const REPLACEABLE: u32 = 1 << 9;
pub(crate) const SOLID_RENDER: u32 = 1 << 10;
pub(crate) const SHELF_MUSHROOM: u32 = 1 << 11;
pub(crate) const PERSISTENT: u32 = 1 << 12;
pub(crate) const WATERLOGGABLE: u32 = 1 << 13;
pub(crate) const SOLID: u32 = 1 << 14;
pub(crate) const LAVA_FLUID: u32 = 1 << 15;
pub(crate) const ANY_FLUID: u32 = 1 << 16;
pub(crate) const FULL_AXIS: u32 = 1 << 17;
pub(crate) const MUSHROOM_REPLACEABLE: u32 = 1 << 18;
pub(crate) const FIRE: u32 = 1 << 19;
pub(crate) const SCULK_BLOCKER: u32 = 1 << 20;
pub(crate) const SCULK_VEIN: u32 = 1 << 21;
pub(crate) const LIQUID: u32 = 1 << 22;
pub(crate) const FEATURES_CANNOT_REPLACE: u32 = 1 << 23;
pub(crate) const CHEST: u32 = 1 << 24;
pub(crate) const SPAWNER: u32 = 1 << 25;
/// Any source fluid state (water or lava), including waterlogged blocks.
pub(crate) const FLUID_SOURCE: u32 = 1 << 26;
/// Fluid amount 8 (sources and falling fluids).
pub(crate) const FLUID_FULL: u32 = 1 << 27;
pub(crate) const BASE_STONE_OVERWORLD: u32 = 1 << 28;

/// Immutable per-state facts. Missing tags or blocks are empty classifications.
pub struct TreeBlocks {
    flags: Vec<u32>,
    /// Native LeavesBlock DISTANCE (1..=7) values; 0 when absent.
    distance: Vec<u8>,
    /// Block identity (index of the block name) per state.
    block: Vec<u32>,
    pub(crate) properties: Arc<StateProperties>,
    defaults: BTreeMap<String, BlockStateId>,
}
impl std::fmt::Debug for TreeBlocks {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TreeBlocks")
            .field("states", &self.flags.len())
            .finish_non_exhaustive()
    }
}
const DEFAULTS: [&str; 20] = [
    "minecraft:cobblestone",
    "minecraft:mossy_cobblestone",
    "minecraft:chest",
    "minecraft:spawner",
    "minecraft:ice",
    "minecraft:packed_ice",
    "minecraft:glow_lichen",
    "minecraft:sculk_vein",
    "minecraft:vine",
    "minecraft:cocoa",
    "minecraft:bee_nest",
    "minecraft:creaking_heart",
    "minecraft:pale_hanging_moss",
    "minecraft:shelf_mushroom",
    "minecraft:water",
    "minecraft:lava",
    "minecraft:air",
    "minecraft:cave_air",
    "minecraft:dirt",
    "minecraft:mud",
];
impl TreeBlocks {
    pub(crate) fn build(bindings: &FeatureBlockBindings<'_>) -> Result<Self, DecodeError> {
        let registry: &BlockRegistry = bindings.registry();
        let (providers, _, _) = bindings
            .simple_blocks
            .as_ref()
            .ok_or(DecodeError::Field("tree state provider bindings"))?;
        let properties = providers.properties()?;
        let replaceable = super::block_predicate::PredicateBlockResolver::predicate_property(
            bindings,
            BlockProperty::Replaceable,
        )?;
        let solid_render = super::block_predicate::PredicateBlockResolver::predicate_property(
            bindings,
            BlockProperty::SolidRender,
        )?;
        let solid = super::block_predicate::PredicateBlockResolver::predicate_property(
            bindings,
            BlockProperty::Solid,
        )?;
        let liquid = super::block_predicate::PredicateBlockResolver::predicate_property(
            bindings,
            BlockProperty::Liquid,
        )?;
        let mut flags = vec![0u32; registry.len()];
        let mut distance = vec![0u8; registry.len()];
        let mark_tag = |tag: &str, flag: u32, flags: &mut Vec<u32>| {
            if let Some(states) = bindings.tags.get(tag) {
                for state in states.iter() {
                    flags[state.0 as usize] |= flag;
                }
            }
        };
        mark_tag("minecraft:leaves", LEAVES, &mut flags);
        mark_tag(
            "minecraft:replaceable_by_trees",
            REPLACEABLE_BY_TREES,
            &mut flags,
        );
        mark_tag("minecraft:logs", LOGS, &mut flags);
        mark_tag(
            "minecraft:prevents_nearby_leaf_decay",
            PREVENTS_LEAF_DECAY,
            &mut flags,
        );
        mark_tag(
            "minecraft:replaceable_by_mushrooms",
            MUSHROOM_REPLACEABLE,
            &mut flags,
        );
        mark_tag("minecraft:fire", FIRE, &mut flags);
        mark_tag(
            "minecraft:base_stone_overworld",
            BASE_STONE_OVERWORLD,
            &mut flags,
        );
        mark_tag(
            "minecraft:features_cannot_replace",
            FEATURES_CANNOT_REPLACE,
            &mut flags,
        );
        for (name, flag) in [
            ("minecraft:vine", VINE),
            ("minecraft:water", WATER_BLOCK),
            ("minecraft:shelf_mushroom", SHELF_MUSHROOM),
            ("minecraft:sculk", SCULK_BLOCKER),
            ("minecraft:sculk_catalyst", SCULK_BLOCKER),
            ("minecraft:moving_piston", SCULK_BLOCKER),
            ("minecraft:sculk_vein", SCULK_VEIN),
            ("minecraft:chest", CHEST),
            ("minecraft:spawner", SPAWNER),
        ] {
            if let Some(states) = bindings.blocks.get(name) {
                for state in states.iter() {
                    flags[state.0 as usize] |= flag;
                }
            }
        }
        for air in bindings.air {
            flags[air.0 as usize] |= AIR;
        }
        for index in 0..registry.len() {
            let id = BlockStateId(index as u32);
            let flag = &mut flags[index];
            if replaceable.contains(id) {
                *flag |= REPLACEABLE;
            }
            if solid_render.contains(id) {
                *flag |= SOLID_RENDER;
            }
            if solid.contains(id) {
                *flag |= SOLID;
            }
            if liquid.contains(id) {
                *flag |= LIQUID;
            }
            if let Some(fluid) = bindings.fluids.state(id) {
                match fluid.kind() {
                    FluidKind::Water => *flag |= WATER_SOURCE | WATER_FLUID | ANY_FLUID,
                    FluidKind::FlowingWater => *flag |= WATER_FLUID | ANY_FLUID,
                    FluidKind::Lava | FluidKind::FlowingLava => *flag |= LAVA_FLUID | ANY_FLUID,
                    FluidKind::Empty => {}
                }
                if fluid.is_source() {
                    *flag |= FLUID_SOURCE;
                }
                if fluid.amount() == 8 {
                    *flag |= FLUID_FULL;
                }
            }
            if properties.value(id, "persistent") == Some("true") {
                *flag |= PERSISTENT;
            }
            if properties
                .values(id, "waterlogged")
                .is_some_and(|v| v.len() == 2)
            {
                *flag |= WATERLOGGABLE;
            }
            if properties.values(id, "axis").is_some_and(|values| {
                values.len() == 3
                    && ["x", "y", "z"]
                        .iter()
                        .all(|a| values.iter().any(|v| v == a))
            }) {
                *flag |= FULL_AXIS;
            }
            if let Some(values) = properties.values(id, "distance") {
                let native = values.len() == 7
                    && (1..=7).all(|n| values.iter().any(|v| *v == n.to_string()));
                if native {
                    distance[index] = properties
                        .value(id, "distance")
                        .and_then(|v| v.parse().ok())
                        .unwrap_or(0);
                }
            }
        }
        let mut names = BTreeMap::new();
        let block = (0..registry.len())
            .map(|i| {
                let name = &registry.state(BlockStateId(i as u32)).unwrap().name;
                let next = names.len() as u32;
                *names.entry(name.clone()).or_insert(next)
            })
            .collect();
        let defaults = DEFAULTS
            .iter()
            .filter_map(|name| {
                registry
                    .default_state(name)
                    .map(|id| ((*name).to_owned(), id))
            })
            .collect();
        Ok(Self {
            flags,
            distance,
            block,
            properties,
            defaults,
        })
    }
    pub(crate) fn has(&self, state: BlockStateId, flag: u32) -> bool {
        self.flags
            .get(state.0 as usize)
            .is_some_and(|value| value & flag != 0)
    }
    /// BlockState.is(Block): same block, any properties.
    pub(crate) fn same_block(&self, a: BlockStateId, b: BlockStateId) -> bool {
        self.block.get(a.0 as usize) == self.block.get(b.0 as usize)
    }
    pub(crate) fn is_air(&self, state: BlockStateId) -> bool {
        self.has(state, AIR)
    }
    /// TreeFeature.validTreePos: air or replaceable by trees.
    pub(crate) fn valid_tree_pos(&self, state: BlockStateId) -> bool {
        self.has(state, AIR | REPLACEABLE_BY_TREES)
    }
    /// LeavesBlock.getOptionalDistanceAt.
    pub(crate) fn leaf_distance(&self, state: BlockStateId) -> Option<i32> {
        if self.has(state, PREVENTS_LEAF_DECAY) {
            return Some(0);
        }
        match self.distance.get(state.0 as usize) {
            Some(0) | None => None,
            Some(d) => Some(i32::from(*d)),
        }
    }
    pub(crate) fn default_state(&self, name: &str) -> Option<BlockStateId> {
        self.defaults.get(name).copied()
    }
    pub(crate) fn with(
        &self,
        state: BlockStateId,
        name: &str,
        value: &str,
    ) -> Option<BlockStateId> {
        self.properties.with_value(state, name, value)
    }
    /// BlockState.trySetValue(RotatedPillarBlock.AXIS, axis).
    pub(crate) fn try_axis(&self, state: BlockStateId, axis: Axis) -> BlockStateId {
        if self.has(state, FULL_AXIS) {
            self.with(state, "axis", axis.name()).unwrap_or(state)
        } else {
            state
        }
    }
    /// Set WATERLOGGED when the state has that property; otherwise unchanged.
    pub(crate) fn waterlogged(&self, state: BlockStateId, value: bool) -> BlockStateId {
        if self.has(state, WATERLOGGABLE) {
            self.with(state, "waterlogged", if value { "true" } else { "false" })
                .unwrap_or(state)
        } else {
            state
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Axis {
    X,
    Y,
    Z,
}
impl Axis {
    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::X => "x",
            Self::Y => "y",
            Self::Z => "z",
        }
    }
    pub(crate) fn of(direction: Direction) -> Self {
        match direction {
            Direction::West | Direction::East => Self::X,
            Direction::Down | Direction::Up => Self::Y,
            Direction::North | Direction::South => Self::Z,
        }
    }
}

/// Direction.Plane.HORIZONTAL order.
pub(crate) const HORIZONTAL: [Direction; 4] = [
    Direction::North,
    Direction::East,
    Direction::South,
    Direction::West,
];
pub(crate) fn clockwise(direction: Direction) -> Direction {
    match direction {
        Direction::North => Direction::East,
        Direction::East => Direction::South,
        Direction::South => Direction::West,
        Direction::West => Direction::North,
        other => other,
    }
}
pub(crate) fn direction_name(direction: Direction) -> &'static str {
    match direction {
        Direction::Down => "down",
        Direction::Up => "up",
        Direction::North => "north",
        Direction::South => "south",
        Direction::West => "west",
        Direction::East => "east",
    }
}
pub(crate) fn decode_direction(name: &str) -> Result<Direction, DecodeError> {
    Direction::ALL
        .into_iter()
        .find(|d| direction_name(*d) == name)
        .ok_or(DecodeError::Field("direction"))
}
pub(crate) fn offset(p: [i32; 3], dx: i32, dy: i32, dz: i32) -> [i32; 3] {
    [
        p[0].wrapping_add(dx),
        p[1].wrapping_add(dy),
        p[2].wrapping_add(dz),
    ]
}
pub(crate) fn relative(p: [i32; 3], direction: Direction, distance: i32) -> [i32; 3] {
    let n = direction.normal();
    offset(
        p,
        n[0].wrapping_mul(distance),
        n[1].wrapping_mul(distance),
        n[2].wrapping_mul(distance),
    )
}
/// Vec3i.distManhattan, including its float accumulation.
pub(crate) fn dist_manhattan(a: [i32; 3], b: [i32; 3]) -> i32 {
    let d = |i: usize| b[i].wrapping_sub(a[i]).wrapping_abs() as f32;
    (d(0) + d(1) + d(2)) as i32
}

/// Java HashSet<BlockPos> with native bucket iteration order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct JavaPos(pub [i32; 3]);
impl crate::java_hash_map::JavaKey for JavaPos {
    fn java_hash(&self) -> i32 {
        let [x, y, z] = self.0;
        y.wrapping_add(z.wrapping_mul(31))
            .wrapping_mul(31)
            .wrapping_add(x)
    }
    fn tie_break(&self, other: &Self) -> std::cmp::Ordering {
        // BlockPos is not self-Comparable for HashMap; vanilla falls back to
        // identity hashes, which are not reproducible. Any total order works.
        let key = |p: &Self| (p.0[1], p.0[2], p.0[0]);
        match key(self).cmp(&key(other)) {
            std::cmp::Ordering::Equal => std::cmp::Ordering::Less,
            ordering => ordering,
        }
    }
}
#[derive(Default)]
pub(crate) struct PosSet(crate::java_hash_map::JavaHashMap<JavaPos, ()>);
impl PosSet {
    pub(crate) fn insert(&mut self, p: [i32; 3]) -> bool {
        self.0.put(JavaPos(p), ()).is_none()
    }
    pub(crate) fn contains(&self, p: [i32; 3]) -> bool {
        self.0.get(&JavaPos(p)).is_some()
    }
    pub(crate) fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
    /// Iteration order of the native set without materializing a position list.
    pub(crate) fn iter(&self) -> impl Iterator<Item = [i32; 3]> + '_ {
        self.0.entries().map(|(key, _)| key.0)
    }
    /// Iteration order of the native set.
    pub(crate) fn to_vec(&self) -> Vec<[i32; 3]> {
        self.0.entries().map(|(k, _)| k.0).collect()
    }
    /// iterator().next() followed by iterator.remove().
    pub(crate) fn pop_first(&mut self) -> Option<[i32; 3]> {
        let mut cursor = self.0.cursor();
        let first = self.0.next_entry(&mut cursor).ok().flatten()?.0.0;
        self.0.remove_current(&mut cursor).ok()?;
        Some(first)
    }
}

/// Util.shuffle (Fisher-Yates from the end with nextInt(i)).
pub(crate) fn shuffle<T>(
    values: &mut [T],
    random: &mut impl horizon_core::random::RandomSource,
) -> Result<(), horizon_core::random::InvalidBound> {
    for i in (2..=values.len()).rev() {
        let swap = random.next_int_bound(i as i32)? as usize;
        values.swap(i - 1, swap);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use horizon_core::random::{LegacyRandom, RandomSource};
    #[test]
    fn pos_set_iterates_in_java_hash_bucket_order() {
        let mut set = PosSet::default();
        let inserted = [[0, 64, 1], [1, 64, 0], [0, 65, 0], [0, 64, 0]];
        for p in inserted {
            set.insert(p);
        }
        // BlockPos.hashCode spread by h ^ (h >>> 16) into 16 buckets; ties keep
        // insertion order within a bucket.
        let mut expected = inserted.to_vec();
        expected.sort_by_key(|p| {
            let h = crate::java_hash_map::JavaKey::java_hash(&JavaPos(*p)) as u32;
            (h ^ (h >> 16)) & 15
        });
        assert_eq!(set.to_vec(), expected);
        assert_eq!(set.pop_first(), Some(expected[0]));
        assert!(!set.contains(expected[0]));
    }
    #[test]
    fn fisher_yates_matches_native_draw_order() {
        let mut values = [0, 1, 2, 3];
        shuffle(&mut values, &mut LegacyRandom::new(42)).unwrap();
        let mut reference = [0, 1, 2, 3];
        let mut random = LegacyRandom::new(42);
        for i in (2..=4).rev() {
            let j = random.next_int_bound(i).unwrap() as usize;
            reference.swap(i as usize - 1, j);
        }
        assert_eq!(values, reference);
    }
}
