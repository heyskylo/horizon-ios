//! BoatItem's server POV clip; deliberately separate from renderer/mining picks.
use super::{Fluid, Game};
use horizon_core::{
    geometry::{Aabb, ShapeOp, Vec3, VoxelShape},
    position::BlockPos,
};
use horizon_world::{
    registry::BlockStateId,
    world_ray::{RayBlockMode, RayFluidMode, RayWorld, clip_world},
};
use std::io;

struct BoatItemRay<'a, 'r>(&'a Game<'r>);
impl RayWorld for BoatItemRay<'_, '_> {
    type Error = io::Error;
    type Block = BlockStateId;
    type Fluid = Option<(f32, bool, Fluid)>;
    fn block_state(&mut self, p: BlockPos) -> io::Result<BlockStateId> {
        Ok(self.0.world.block(p.x, p.y, p.z))
    }
    fn fluid_state(&mut self, p: BlockPos) -> io::Result<Self::Fluid> {
        Ok(self
            .0
            .fluid_height_of(p.x, p.y, p.z, None)
            .map(|(h, source)| {
                (
                    h,
                    source,
                    self.0
                        .res
                        .facts()
                        .get(self.0.world.block(p.x, p.y, p.z))
                        .fluid,
                )
            }))
    }
    fn block_shape(
        &mut self,
        state: BlockStateId,
        p: BlockPos,
        _: RayBlockMode,
    ) -> io::Result<VoxelShape> {
        // LiquidBlock.getShape is empty. Waterlogged blocks retain their outline.
        if matches!(
            &*self.0.res.facts().get(state).name,
            "minecraft:water" | "minecraft:lava" | "minecraft:bubble_column"
        ) {
            return Ok(VoxelShape::empty());
        }
        let mut shape = VoxelShape::empty();
        for b in self.0.outline_boxes([p.x, p.y, p.z]) {
            let b: [f64; 6] = std::array::from_fn(|i| b[i] - f64::from([p.x, p.y, p.z][i % 3]));
            let part = VoxelShape::from_box(Aabb::new(
                Vec3::new(b[0], b[1], b[2]),
                Vec3::new(b[3], b[4], b[5]),
            ))
            .map_err(|e| io::Error::other(format!("{e:?}")))?;
            shape = shape
                .join(&part, ShapeOp::OR)
                .map_err(|e| io::Error::other(format!("{e:?}")))?;
        }
        Ok(shape)
    }
    fn interaction_shape(&mut self, _: BlockStateId, _: BlockPos) -> io::Result<VoxelShape> {
        // Pinned vanilla's registered interaction shapes are all empty;
        // interaction_shapes::InteractionShapes validates that same table.
        Ok(VoxelShape::empty())
    }
    fn fluid_is_source(&mut self, s: Self::Fluid) -> io::Result<bool> {
        Ok(s.is_some_and(|s| s.1))
    }
    fn fluid_is_empty(&mut self, s: Self::Fluid) -> io::Result<bool> {
        Ok(s.is_none())
    }
    fn fluid_is_water(&mut self, s: Self::Fluid) -> io::Result<bool> {
        Ok(s.is_some_and(|s| s.2 == Fluid::Water))
    }
    fn fluid_shape(&mut self, s: Self::Fluid, _: BlockPos) -> io::Result<VoxelShape> {
        match s {
            None => Ok(VoxelShape::empty()),
            Some((h, _, _)) => {
                VoxelShape::from_box(Aabb::new(Vec3::ZERO, Vec3::new(1.0, f64::from(h), 1.0)))
                    .map_err(|e| io::Error::other(format!("{e:?}")))
            }
        }
    }
}
impl Game<'_> {
    /// `Item.getPlayerPOVHitResult`: float view vector, OUTLINE and Fluid.ANY.
    pub(super) fn boat_item_hit(&self) -> Option<[f64; 3]> {
        let [x, y, z] = self.eye_position()?;
        let start = Vec3::new(x, y, z);
        let dir = horizon_world::view_direction::view_vector(self.pitch, self.yaw);
        let end = start.add_vector(dir.scale(self.reach()));
        let hit = clip_world(
            &mut BoatItemRay(self),
            start,
            end,
            RayBlockMode::Outline,
            RayFluidMode::Any,
            256,
        )
        .ok()?;
        (!hit.miss).then_some([hit.hit.location.x, hit.hit.location.y, hit.hit.location.z])
    }
}
