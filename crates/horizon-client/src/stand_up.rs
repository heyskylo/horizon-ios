//! Where an entity stands up beside a block: `AbstractBedBlock.findStandUpPosition`
//! and `DismountHelper.findSafeDismountLocation` for any entity type, shared by
//! players (waking up, respawning at a bed or anchor) and villagers waking up.
use std::collections::{HashMap, HashSet};

use crate::{
    blocks::BlockFacts,
    collisions::{SceneCollisions, no_collision_empty},
    world::World,
};
use horizon_core::{
    geometry::{Aabb, Axis, Vec3, VoxelShape, block_floor_height, border::BorderGeometry, trig},
    position::BlockPos,
};
use horizon_world::{
    block_collision::BlockCollisionRegistry,
    collision_context::CollisionContext,
    registry::{BlockRegistry, BlockStateId},
};

/// What `findSafeDismountLocation` reads of an `EntityType`: its
/// dimensions, whether it is the player, and `fireImmune`. No type that
/// stands up from a bed has an `immuneTo` block.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StandUpType {
    pub width: f32,
    pub height: f32,
    pub player: bool,
    pub fire_immune: bool,
}

impl StandUpType {
    /// `EntityTypes.PLAYER`: 0.6 by 1.8.
    pub const PLAYER: Self = Self {
        width: 0.6,
        height: 1.8,
        player: true,
        fire_immune: false,
    };
    /// `EntityTypes.VILLAGER`: 0.6 by 1.95.
    pub const VILLAGER: Self = Self {
        width: 0.6,
        height: 1.95,
        player: false,
        fire_immune: false,
    };
}

/// The level as the stand-up search reads it (a `CollisionGetter`).
pub struct StandUpLevel<'a, 'r> {
    pub world: &'a World,
    pub registry: &'a BlockRegistry,
    pub facts: &'a BlockFacts,
    pub collision: &'a BlockCollisionRegistry<'r>,
    pub block_tags: &'a HashMap<String, HashSet<String>>,
    /// The world border; None: none (every box is inside).
    pub border: Option<&'a BorderGeometry>,
}

impl StandUpLevel<'_, '_> {
    fn name(&self, p: [i32; 3]) -> &str {
        &self.facts.get(self.world.block(p[0], p[1], p[2])).name
    }
    fn tagged(&self, tag: &str, block: &str) -> bool {
        self.block_tags.get(tag).is_some_and(|s| s.contains(block))
    }
    fn prop(&self, state: BlockStateId, key: &str) -> Option<&str> {
        self.registry
            .state(state)
            .and_then(|s| s.properties.get(key))
            .map(String::as_str)
    }
    /// `BlockState.getCollisionShape(level, pos)` under the empty context.
    fn collision_shape(&self, p: [i32; 3]) -> VoxelShape {
        let mut scene = SceneCollisions {
            world: self.world,
            binding: self.collision,
            subject: None,
        };
        self.collision
            .collision_shape_with_context(
                &mut scene,
                self.world.block(p[0], p[1], p[2]),
                BlockPos::new(p[0], p[1], p[2]),
                &CollisionContext::empty(false),
            )
            .unwrap_or_else(|_| VoxelShape::empty())
    }
    /// `DismountHelper.nonClimbableShape`: no shape for climbable blocks
    /// and open trapdoors.
    pub fn non_climbable_shape(&self, p: [i32; 3]) -> VoxelShape {
        let state = self.world.block(p[0], p[1], p[2]);
        let name = self.name(p);
        let open_trapdoor = name.ends_with("_trapdoor") && self.prop(state, "open") == Some("true");
        if self.tagged("minecraft:climbable", name) || open_trapdoor {
            return VoxelShape::empty();
        }
        self.collision_shape(p)
    }
    /// `EntityType.isBlockDangerous`: `NodeEvaluator.isBurningBlock` unless
    /// fire immune, then the hurting plants and powder snow.
    pub fn dangerous(&self, ty: StandUpType, p: [i32; 3]) -> bool {
        let state = self.world.block(p[0], p[1], p[2]);
        let name = self.name(p);
        let burning = self.tagged("minecraft:fire", name)
            || matches!(
                name,
                "minecraft:lava" | "minecraft:magma_block" | "minecraft:lava_cauldron"
            )
            || (matches!(name, "minecraft:campfire" | "minecraft:soul_campfire")
                && self.prop(state, "lit") == Some("true"));
        (!ty.fire_immune && burning)
            || matches!(
                name,
                "minecraft:wither_rose"
                    | "minecraft:sweet_berry_bush"
                    | "minecraft:cactus"
                    | "minecraft:powder_snow"
            )
    }
    /// `WorldBorder.isWithinBounds(AABB)`: the max corner less `1.0E-5F`.
    fn within_border(&self, bounds: Aabb) -> bool {
        let Some(border) = self.border else {
            return true;
        };
        let inset = f64::from(1.0e-5f32);
        let (lo_x, lo_z) = (bounds.min(Axis::X), bounds.min(Axis::Z));
        let (hi_x, hi_z) = (bounds.max(Axis::X) - inset, bounds.max(Axis::Z) - inset);
        border.is_within_bounds(lo_x, lo_z, 0.0) && border.is_within_bounds(hi_x, hi_z, 0.0)
    }
    /// `DismountHelper.findSafeDismountLocation`: standing in `p` on its
    /// floor (or the block below's), with room for the type's box.
    pub fn safe_dismount_location(
        &self,
        ty: StandUpType,
        p: [i32; 3],
        check_dangerous: bool,
    ) -> Option<Vec3> {
        if check_dangerous && self.dangerous(ty, p) {
            return None;
        }
        let below = [p[0], p[1] - 1, p[2]];
        let shape = self.non_climbable_shape(p);
        let below_shape = self.non_climbable_shape(below);
        let floor = block_floor_height(&shape, || &below_shape);
        // DismountHelper.isBlockFloorValid.
        if floor.is_infinite() || floor >= 1.0 {
            return None;
        }
        if check_dangerous && floor <= 0.0 && self.dangerous(ty, below) {
            return None;
        }
        let position = Vec3::new(
            f64::from(p[0]) + 0.5,
            f64::from(p[1]) + floor,
            f64::from(p[2]) + 0.5,
        );
        // EntityDimensions.makeBoundingBox: the half width is a float.
        let half = f64::from(ty.width / 2.0);
        let bounds = Aabb::new(
            Vec3::new(position.x - half, position.y, position.z - half),
            Vec3::new(
                position.x + half,
                position.y + f64::from(ty.height),
                position.z + half,
            ),
        );
        let mut scene = SceneCollisions {
            world: self.world,
            binding: self.collision,
            subject: None,
        };
        if !no_collision_empty(&mut scene, bounds) {
            return None;
        }
        let invalid = |q: [i32; 3]| self.tagged("minecraft:invalid_spawn_inside", self.name(q));
        if ty.player && (invalid(p) || invalid([p[0], p[1] + 1, p[2]])) {
            return None;
        }
        self.within_border(bounds).then_some(position)
    }
    /// `AbstractBedBlock.findStandUpPosition` for a bed whose block is at
    /// `pos`, facing `forward` (a horizontal step), from `yaw`: beside the
    /// bed on the side the sleeper faced away from, around it, then on it;
    /// a safe spot first, else any spot (a bunk bed: around it, a block
    /// lower, then on it).
    pub fn bed_stand_up(
        &self,
        ty: StandUpType,
        pos: [i32; 3],
        forward: [i32; 3],
        yaw: f32,
    ) -> Option<Vec3> {
        // Direction.getClockWise.
        let right = [-forward[2], 0, forward[0]];
        // Direction.isFacingAngle.
        let radians = yaw * 0.017_453_292;
        let (dx, dz) = (
            -trig::sin(f64::from(radians)),
            trig::cos(f64::from(radians)),
        );
        let facing = right[0] as f32 * dx + right[2] as f32 * dz > 0.0;
        let side = if facing {
            [-right[0], 0, -right[2]]
        } else {
            right
        };
        let (f, s) = ([forward[0], forward[2]], [side[0], side[2]]);
        // bedSurroundStandUpOffsets and bedAboveStandUpOffsets.
        let surround = [
            [s[0], s[1]],
            [s[0] - f[0], s[1] - f[1]],
            [s[0] - f[0] * 2, s[1] - f[1] * 2],
            [-f[0] * 2, -f[1] * 2],
            [-s[0] - f[0] * 2, -s[1] - f[1] * 2],
            [-s[0] - f[0], -s[1] - f[1]],
            [-s[0], -s[1]],
            [-s[0] + f[0], -s[1] + f[1]],
            [f[0], f[1]],
            [s[0] + f[0], s[1] + f[1]],
        ];
        let above = [[0, 0], [-f[0], -f[1]]];
        // findStandUpPositionAtOffset.
        let at = |base: [i32; 3], offsets: &[[i32; 2]], check: bool| {
            offsets.iter().find_map(|o| {
                self.safe_dismount_location(ty, [base[0] + o[0], base[1], base[2] + o[1]], check)
            })
        };
        let below = [pos[0], pos[1] - 1, pos[2]];
        // isBunkBed: a bed below.
        if self.name(below).ends_with("_bed") {
            return at(pos, &surround, true)
                .or_else(|| at(below, &surround, true))
                .or_else(|| at(pos, &above, true))
                .or_else(|| at(pos, &surround, false))
                .or_else(|| at(below, &surround, false))
                .or_else(|| at(pos, &above, false));
        }
        let all: Vec<[i32; 2]> = surround.into_iter().chain(above).collect();
        at(pos, &all, true).or_else(|| at(pos, &all, false))
    }
}

/// `LivingEntity.stopSleeping`'s stand-up spot when `findStandUpPosition`
/// finds none: 0.1 above the block over the bed.
pub fn stand_up_fallback(bed: [i32; 3]) -> Vec3 {
    Vec3::new(
        f64::from(bed[0]) + 0.5,
        f64::from(bed[1] + 1) + 0.1,
        f64::from(bed[2]) + 0.5,
    )
}

/// A bed's `facing` property as a step (north when unset).
pub fn bed_forward(facing: Option<&str>) -> [i32; 3] {
    match facing {
        Some("south") => [0, 0, 1],
        Some("west") => [-1, 0, 0],
        Some("east") => [1, 0, 0],
        _ => [0, 0, -1],
    }
}
