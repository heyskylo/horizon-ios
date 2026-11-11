//! A bucket's aim and target: `getPlayerPOVHitResult(..., SOURCE_ONLY)` for an empty bucket (the block outline or a source fluid, whichever is nearer) and `BucketItem.use`'s place position.
use super::*;

impl Game<'_> {
    /// `getPlayerPOVHitResult(level, player, SOURCE_ONLY)` for an empty
    /// bucket: the block outline or a source fluid, whichever is nearer. A
    /// waterlogged block is a block hit (its outline is the block's, its
    /// fluid a source); flowing fluid has no shape to pick.
    pub(super) fn bucket_hit(&self) -> Option<Hit> {
        let origin = self.aim().map_or([0.0; 3], |(origin, _)| origin);
        let distance = |p: [f64; 3]| {
            (p[0] - origin[0]).powi(2) + (p[1] - origin[1]).powi(2) + (p[2] - origin[2]).powi(2)
        };
        match (self.pick(), self.pick_fluid()) {
            (Some(block), Some(fluid)) => Some(if distance(block.point) <= distance(fluid.point) {
                block
            } else {
                fluid
            }),
            (block, fluid) => block.or(fluid),
        }
    }

    /// `BucketItem.use`'s place position: a water bucket fills the clicked
    /// block itself when that block can hold it (`canBlockContainFluid`: a
    /// waterloggable stair, slab, fence, ..., but not a double slab, kelp or
    /// seagrass); every other bucket empties into the block the clicked face
    /// leads to. `SolidBucketItem` places like a block item, into a
    /// replaceable clicked block.
    pub(super) fn bucket_target(&self, content: BucketContent, hit: &Hit) -> [i32; 3] {
        let clicked = self.world.block(hit.pos[0], hit.pos[1], hit.pos[2]);
        let behaviours = self.res.behaviours();
        if content == BucketContent::Powder {
            if self.res.facts().get(clicked).replaceable {
                return hit.pos;
            }
        } else if content.is_water()
            && behaviours.is_liquid_container(clicked)
            && behaviours.can_place_liquid(
                clicked,
                horizon_world::fluid::FluidKind::Water,
                self.creative,
            )
        {
            return hit.pos;
        }
        hit.beside()
    }
}
