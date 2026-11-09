//! Moving-piston collision selection and interpolation over immutable state IDs.
//! Tick progression, entity pushing and world mutation belong to the world loop.
use crate::{
    compression::invalid,
    registry::{BlockRegistry, BlockStateId},
};
use horizon_core::geometry::{Direction, ShapeError, ShapeOp, Vec3, VoxelShape};
use std::{collections::BTreeMap, io};
#[derive(Clone, Copy, Debug)]
pub struct MovingPistonState {
    pub moved_state: BlockStateId,
    pub direction: Direction,
    pub extending: bool,
    pub source: bool,
    pub progress: f32,
    pub previous_progress: f32,
}
impl MovingPistonState {
    pub fn movement_direction(self) -> Direction {
        if self.extending {
            self.direction
        } else {
            self.direction.opposite()
        }
    }
    pub fn interpolated_progress(self, partial: f32) -> f32 {
        let partial = if partial > 1.0 { 1.0 } else { partial };
        self.previous_progress + partial * (self.progress - self.previous_progress)
    }
    pub fn interpolated_offset(self, partial: f32) -> [f32; 3] {
        self.offset(self.interpolated_progress(partial))
    }
    fn offset(self, progress: f32) -> [f32; 3] {
        let extended = if self.extending {
            progress - 1.0
        } else {
            1.0 - progress
        };
        self.direction.normal().map(|step| step as f32 * extended)
    }
}
pub trait PistonCollisionWorld {
    type Error;
    /// The no-entity-context BlockState collision query at the supplied position.
    fn collision_shape(
        &mut self,
        state: BlockStateId,
        position: [i32; 3],
    ) -> Result<VoxelShape, Self::Error>;
}
#[derive(Debug)]
pub enum PistonShapeError<E> {
    World(E),
    Shape(ShapeError),
    Registry(io::Error),
}
impl<E: std::fmt::Display> std::fmt::Display for PistonShapeError<E> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::World(e) => e.fmt(f),
            Self::Shape(e) => e.fmt(f),
            Self::Registry(e) => e.fmt(f),
        }
    }
}
impl<E: std::error::Error + 'static> std::error::Error for PistonShapeError<E> {}
struct Base {
    extended: BlockStateId,
    facing: Direction,
    sticky: bool,
}
pub struct PistonBlockBindings<'r> {
    registry: &'r BlockRegistry,
    bases: BTreeMap<BlockStateId, Base>,
    heads: [[[BlockStateId; 2]; 6]; 2],
}
const DIRECTION_NAMES: [&str; 6] = ["down", "up", "north", "south", "west", "east"];
impl<'r> PistonBlockBindings<'r> {
    pub fn new(registry: &'r BlockRegistry) -> io::Result<Self> {
        let mut bases = BTreeMap::new();
        for id in 0..registry.len() {
            let id = BlockStateId(id as u32);
            let state = registry.state(id).unwrap();
            if !matches!(
                state.name.as_str(),
                "minecraft:piston" | "minecraft:sticky_piston"
            ) {
                continue;
            }
            let facing = state
                .properties
                .get("facing")
                .and_then(|name| DIRECTION_NAMES.iter().position(|v| v == name))
                .ok_or_else(|| invalid("invalid piston facing"))?;
            let mut extended = state.clone();
            extended.properties.insert("extended".into(), "true".into());
            bases.insert(
                id,
                Base {
                    extended: registry
                        .id(&extended)
                        .ok_or_else(|| invalid("missing extended piston state"))?,
                    facing: Direction::ALL[facing],
                    sticky: state.name == "minecraft:sticky_piston",
                },
            );
        }
        let default = registry
            .default_state("minecraft:piston_head")
            .and_then(|id| registry.state(id))
            .ok_or_else(|| invalid("missing piston head"))?;
        let mut heads = [[[BlockStateId(0); 2]; 6]; 2];
        for (kind, type_heads) in heads.iter_mut().enumerate() {
            for (direction, variants) in type_heads.iter_mut().enumerate() {
                for (short, id) in variants.iter_mut().enumerate() {
                    let mut state = default.clone();
                    state.properties.insert(
                        "type".into(),
                        if kind == 0 { "normal" } else { "sticky" }.into(),
                    );
                    state
                        .properties
                        .insert("facing".into(), DIRECTION_NAMES[direction].into());
                    state
                        .properties
                        .insert("short".into(), (short != 0).to_string());
                    *id = registry
                        .id(&state)
                        .ok_or_else(|| invalid("missing piston head variant"))?;
                }
            }
        }
        Ok(Self {
            registry,
            bases,
            heads,
        })
    }
    fn validate(&self, state: BlockStateId) -> io::Result<()> {
        if self.registry.state(state).is_none() {
            Err(invalid("piston moved state outside registry"))
        } else {
            Ok(())
        }
    }
    /// The collision-related state used by the native entity-pushing path.
    /// Its sticky/short/facing rules differ from the entity's own collision head.
    pub fn collision_related_state(&self, piston: MovingPistonState) -> io::Result<BlockStateId> {
        self.validate(piston.moved_state)?;
        if !piston.extending
            && piston.source
            && let Some(base) = self.bases.get(&piston.moved_state)
        {
            return Ok(self.heads[usize::from(base.sticky)][base.facing as usize]
                [usize::from(piston.progress > 0.25)]);
        }
        Ok(piston.moved_state)
    }
    pub fn collision_shape<W: PistonCollisionWorld>(
        &self,
        world: &mut W,
        position: [i32; 3],
        piston: MovingPistonState,
        noclip: Option<Direction>,
    ) -> Result<VoxelShape, PistonShapeError<W::Error>> {
        self.validate(piston.moved_state)
            .map_err(PistonShapeError::Registry)?;
        let base = if !piston.extending && piston.source {
            if let Some(base) = self.bases.get(&piston.moved_state) {
                world
                    .collision_shape(base.extended, position)
                    .map_err(PistonShapeError::World)?
            } else {
                VoxelShape::empty()
            }
        } else {
            VoxelShape::empty()
        };
        // The stationary base is queried before this native noclip shortcut.
        if piston.progress < 1.0 && noclip == Some(piston.movement_direction()) {
            return Ok(base);
        }
        let state = if piston.source {
            let short = piston.extending != (1.0f32 - piston.progress < 0.25f32);
            self.heads[0][piston.direction as usize][usize::from(short)]
        } else {
            piston.moved_state
        };
        let offset = piston.offset(piston.progress).map(f64::from);
        let moving = world
            .collision_shape(state, position)
            .map_err(PistonShapeError::World)?
            .moved(Vec3::new(offset[0], offset[1], offset[2]));
        base.join(&moving, ShapeOp::OR)
            .map_err(PistonShapeError::Shape)
    }
}
