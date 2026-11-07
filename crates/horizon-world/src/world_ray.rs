//! BlockGetter clipping: ordered block/fluid queries and interaction-face override.
use crate::entity_obstruction::EntityIdentity;
use horizon_core::{
    geometry::{BlockHit, Direction, TraversalError, Vec3, VoxelShape, traverse_blocks},
    position::BlockPos,
};
use std::fmt;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RayBlockMode {
    Collider,
    Outline,
    Visual,
    FallDamageResetting,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RayFluidMode {
    None,
    SourceOnly,
    Any,
    Water,
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RayResult {
    pub hit: BlockHit,
    pub miss: bool,
}
#[derive(Debug)]
pub enum RayError<E> {
    Traversal(TraversalError),
    World(E),
}
impl<E: fmt::Display> fmt::Display for RayError<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Traversal(e) => e.fmt(f),
            Self::World(e) => e.fmt(f),
        }
    }
}
impl<E: std::error::Error + 'static> std::error::Error for RayError<E> {}
/// Block/shape providers retain the captured collision context for the query.
/// State handles must remain valid when later provider calls change the world.
pub trait RayWorld {
    type Error;
    type Block: Copy;
    type Fluid: Copy;
    fn block_state(&mut self, p: BlockPos) -> Result<Self::Block, Self::Error>;
    fn fluid_state(&mut self, p: BlockPos) -> Result<Self::Fluid, Self::Error>;
    fn block_shape(
        &mut self,
        state: Self::Block,
        p: BlockPos,
        mode: RayBlockMode,
    ) -> Result<VoxelShape, Self::Error>;
    fn interaction_shape(
        &mut self,
        state: Self::Block,
        p: BlockPos,
    ) -> Result<VoxelShape, Self::Error>;
    fn fluid_is_source(&mut self, state: Self::Fluid) -> Result<bool, Self::Error>;
    fn fluid_is_empty(&mut self, state: Self::Fluid) -> Result<bool, Self::Error>;
    fn fluid_is_water(&mut self, state: Self::Fluid) -> Result<bool, Self::Error>;
    fn fluid_shape(&mut self, state: Self::Fluid, p: BlockPos) -> Result<VoxelShape, Self::Error>;
}
pub fn clip_with_interaction<W: RayWorld>(
    world: &mut W,
    start: Vec3,
    end: Vec3,
    p: BlockPos,
    shape: &VoxelShape,
    state: W::Block,
) -> Result<Option<BlockHit>, W::Error> {
    let mut hit = shape.clip(start, end, p);
    if let Some(hit) = &mut hit
        && let Some(interaction) = world.interaction_shape(state, p)?.clip(start, end, p)
        && interaction.location.subtract(start).length_squared()
            < hit.location.subtract(start).length_squared()
    {
        hit.direction = interaction.direction;
    }
    Ok(hit)
}
fn pick_fluid<W: RayWorld>(
    world: &mut W,
    state: W::Fluid,
    mode: RayFluidMode,
) -> Result<bool, W::Error> {
    match mode {
        RayFluidMode::None => Ok(false),
        RayFluidMode::SourceOnly => world.fluid_is_source(state),
        RayFluidMode::Any => Ok(!world.fluid_is_empty(state)?),
        RayFluidMode::Water => world.fluid_is_water(state),
    }
}
/// Finite rays only, with an explicit traversal budget. Budget/provider failures
/// preserve preceding queries and return an error rather than a fabricated miss.
pub fn clip_world<W: RayWorld>(
    world: &mut W,
    start: Vec3,
    end: Vec3,
    blocks: RayBlockMode,
    fluids: RayFluidMode,
    max_visits: usize,
) -> Result<RayResult, RayError<W::Error>> {
    let result = traverse_blocks(start, end, max_visits, |p| {
        let mut query = || -> Result<Option<BlockHit>, W::Error> {
            let block = world.block_state(p)?;
            let fluid = world.fluid_state(p)?;
            let shape = world.block_shape(block, p, blocks)?;
            let solid = clip_with_interaction(world, start, end, p, &shape, block)?;
            let liquid = if pick_fluid(world, fluid, fluids)? {
                world.fluid_shape(fluid, p)?.clip(start, end, p)
            } else {
                None
            };
            let solid_distance = solid.map_or(f64::MAX, |h| start.distance_squared(h.location));
            let liquid_distance = liquid.map_or(f64::MAX, |h| start.distance_squared(h.location));
            Ok(if solid_distance <= liquid_distance {
                solid
            } else {
                liquid
            })
        };
        match query() {
            Ok(Some(hit)) => Some(Ok(hit)),
            Ok(None) => None,
            Err(e) => Some(Err(e)),
        }
    })
    .map_err(RayError::Traversal)?;
    match result {
        Some(hit) => Ok(RayResult {
            hit: hit.map_err(RayError::World)?,
            miss: false,
        }),
        None => Ok(RayResult {
            hit: BlockHit {
                location: end,
                direction: Direction::approximate_nearest(start.subtract(end)),
                block: BlockPos::new(
                    end.x.floor() as i32,
                    end.y.floor() as i32,
                    end.z.floor() as i32,
                ),
                inside: false,
            },
            miss: true,
        }),
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResetPortal {
    EndGateway,
    EndPortal,
    NetherPortal,
}
pub trait FallResetWorld {
    type Error;
    type Block: Copy;
    fn fall_reset_tag(&mut self, state: Self::Block) -> Result<bool, Self::Error>;
    fn entity_is_player(&mut self, entity: EntityIdentity) -> Result<bool, Self::Error>;
    fn is_portal(&mut self, state: Self::Block, portal: ResetPortal) -> Result<bool, Self::Error>;
    fn is_server_level(&self) -> bool;
    fn players_nether_portal_delay(&mut self) -> Result<i32, Self::Error>;
}
/// Supply an entity only when the native context is an EntityCollisionContext
/// containing one. Player testing is entity type identity, not a class check.
pub fn fall_reset_shape<W: FallResetWorld>(
    world: &mut W,
    state: W::Block,
    entity: Option<EntityIdentity>,
) -> Result<VoxelShape, W::Error> {
    if world.fall_reset_tag(state)? {
        return Ok(VoxelShape::block());
    }
    if let Some(entity) = entity
        && world.entity_is_player(entity)?
    {
        if world.is_portal(state, ResetPortal::EndGateway)?
            || world.is_portal(state, ResetPortal::EndPortal)?
        {
            return Ok(VoxelShape::block());
        }
        if world.is_server_level()
            && world.is_portal(state, ResetPortal::NetherPortal)?
            && world.players_nether_portal_delay()? == 0
        {
            return Ok(VoxelShape::block());
        }
    }
    Ok(VoxelShape::empty())
}
