//! Fall-reset rays bound to registered interactions, fluid states and live queries.
use crate::{
    compression::invalid,
    entity_obstruction::EntityIdentity,
    fluid::{FluidHeightWorld, FluidKind, FluidState},
    fluid_shape::FluidShapeCache,
    interaction_shapes::InteractionShapes,
    registry::BlockStateId,
    world_ray::{
        self, FallResetWorld, RayBlockMode, RayError, RayFluidMode, RayResult, RayWorld,
        ResetPortal,
    },
};
use horizon_core::{
    geometry::{Vec3, VoxelShape},
    position::BlockPos,
};
use std::{collections::BTreeSet, fmt, io};
#[derive(Clone, Copy)]
struct State {
    reset: bool,
    portal: Option<ResetPortal>,
    fluid: FluidState,
}
pub struct FallRayRegistry<'s, 'r> {
    interactions: &'s InteractionShapes<'r>,
    states: Vec<State>,
    water_mask: crate::fluid::FluidMask,
}
impl<'s, 'r> FallRayRegistry<'s, 'r> {
    /// Tags are resolved snapshots; build a new binding after a tag reload.
    pub fn new(
        interactions: &'s InteractionShapes<'r>,
        reset_blocks: &BTreeSet<String>,
        water_fluids: &BTreeSet<String>,
    ) -> io::Result<Self> {
        let registry = interactions.registry();
        if reset_blocks
            .iter()
            .any(|name| registry.default_state(name).is_none())
        {
            return Err(invalid("unknown fall-reset block tag member"));
        }
        let mut water_mask = 0;
        for name in water_fluids {
            water_mask |= FluidKind::from_name(name)
                .ok_or_else(|| invalid("unknown water fluid tag member"))?
                .mask();
        }
        let mut states = Vec::with_capacity(registry.len());
        for i in 0..registry.len() {
            let state = registry.state(BlockStateId(i as u32)).unwrap();
            states.push(State {
                reset: reset_blocks.contains(&state.name),
                portal: match state.name.as_str() {
                    "minecraft:end_gateway" => Some(ResetPortal::EndGateway),
                    "minecraft:end_portal" => Some(ResetPortal::EndPortal),
                    "minecraft:nether_portal" => Some(ResetPortal::NetherPortal),
                    _ => None,
                },
                fluid: FluidState::from_block(state)?,
            });
        }
        Ok(Self {
            interactions,
            states,
            water_mask,
        })
    }
    fn state(&self, id: BlockStateId) -> io::Result<State> {
        self.states
            .get(id.0 as usize)
            .copied()
            .ok_or_else(|| invalid("ray block state outside registry"))
    }
    pub fn fluid_for_block(&self, id: BlockStateId) -> io::Result<FluidState> {
        Ok(self.state(id)?.fluid)
    }
}
pub trait FallRayWorld {
    type Error;
    fn block_state(&mut self, p: BlockPos) -> Result<BlockStateId, Self::Error>;
    fn fluid_state(&mut self, p: BlockPos) -> Result<FluidState, Self::Error>;
    fn is_server_level(&self) -> bool;
    fn entity_is_player(&mut self, e: EntityIdentity) -> Result<bool, Self::Error>;
    fn players_nether_portal_delay(&mut self) -> Result<i32, Self::Error>;
}
#[derive(Debug)]
pub enum FallRayError<E> {
    Data(io::Error),
    World(E),
}
impl<E: fmt::Display> fmt::Display for FallRayError<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Data(e) => e.fmt(f),
            Self::World(e) => e.fmt(f),
        }
    }
}
impl<E: std::error::Error + 'static> std::error::Error for FallRayError<E> {}
struct Bound<'a, 's, 'r, W> {
    registry: &'a FallRayRegistry<'s, 'r>,
    cache: &'a mut FluidShapeCache,
    entity: Option<EntityIdentity>,
    world: &'a mut W,
}
impl<W: FallRayWorld> FallResetWorld for Bound<'_, '_, '_, W> {
    type Error = FallRayError<W::Error>;
    type Block = BlockStateId;
    fn fall_reset_tag(&mut self, s: BlockStateId) -> Result<bool, Self::Error> {
        Ok(self.registry.state(s).map_err(FallRayError::Data)?.reset)
    }
    fn entity_is_player(&mut self, e: EntityIdentity) -> Result<bool, Self::Error> {
        self.world.entity_is_player(e).map_err(FallRayError::World)
    }
    fn is_portal(&mut self, s: BlockStateId, p: ResetPortal) -> Result<bool, Self::Error> {
        Ok(self.registry.state(s).map_err(FallRayError::Data)?.portal == Some(p))
    }
    fn is_server_level(&self) -> bool {
        self.world.is_server_level()
    }
    fn players_nether_portal_delay(&mut self) -> Result<i32, Self::Error> {
        self.world
            .players_nether_portal_delay()
            .map_err(FallRayError::World)
    }
}
struct Height<'a, W>(&'a mut W);
impl<W: FallRayWorld> FluidHeightWorld for Height<'_, W> {
    type Error = FallRayError<W::Error>;
    fn fluid(&mut self, p: [i32; 3]) -> Result<FluidState, Self::Error> {
        self.0
            .fluid_state(BlockPos::new(p[0], p[1], p[2]))
            .map_err(FallRayError::World)
    }
    fn sturdy_down(&mut self, _: [i32; 3]) -> Result<bool, Self::Error> {
        Err(FallRayError::Data(invalid(
            "camera support is outside a fluid shape query",
        )))
    }
}
impl<W: FallRayWorld> RayWorld for Bound<'_, '_, '_, W> {
    type Error = FallRayError<W::Error>;
    type Block = BlockStateId;
    type Fluid = FluidState;
    fn block_state(&mut self, p: BlockPos) -> Result<BlockStateId, Self::Error> {
        let state = self.world.block_state(p).map_err(FallRayError::World)?;
        self.registry.state(state).map_err(FallRayError::Data)?;
        Ok(state)
    }
    fn fluid_state(&mut self, p: BlockPos) -> Result<FluidState, Self::Error> {
        self.world.fluid_state(p).map_err(FallRayError::World)
    }
    fn block_shape(
        &mut self,
        s: BlockStateId,
        _: BlockPos,
        mode: RayBlockMode,
    ) -> Result<VoxelShape, Self::Error> {
        if mode != RayBlockMode::FallDamageResetting {
            return Err(FallRayError::Data(invalid("fall ray block mode mismatch")));
        }
        let entity = self.entity;
        world_ray::fall_reset_shape(self, s, entity)
    }
    fn interaction_shape(
        &mut self,
        s: BlockStateId,
        _: BlockPos,
    ) -> Result<VoxelShape, Self::Error> {
        Ok(self
            .registry
            .interactions
            .shape(s)
            .map_err(FallRayError::Data)?
            .clone())
    }
    fn fluid_is_source(&mut self, s: FluidState) -> Result<bool, Self::Error> {
        Ok(s.is_source())
    }
    fn fluid_is_empty(&mut self, s: FluidState) -> Result<bool, Self::Error> {
        Ok(s.kind() == FluidKind::Empty)
    }
    fn fluid_is_water(&mut self, s: FluidState) -> Result<bool, Self::Error> {
        Ok(self.registry.water_mask & s.kind().mask() != 0)
    }
    fn fluid_shape(&mut self, s: FluidState, p: BlockPos) -> Result<VoxelShape, Self::Error> {
        self.cache.shape(&mut Height(self.world), s, p)
    }
}
/// `entity` represents the optional entity in a native EntityCollisionContext.
/// Keep the cache shared across calls to preserve native first-query behavior.
pub fn fall_reset_ray<W: FallRayWorld>(
    world: &mut W,
    registry: &FallRayRegistry<'_, '_>,
    cache: &mut FluidShapeCache,
    entity: Option<EntityIdentity>,
    from: Vec3,
    to: Vec3,
    max_visits: usize,
) -> Result<RayResult, RayError<FallRayError<W::Error>>> {
    world_ray::clip_world(
        &mut Bound {
            world,
            registry,
            cache,
            entity,
        },
        from,
        to,
        RayBlockMode::FallDamageResetting,
        RayFluidMode::Water,
        max_visits,
    )
}
