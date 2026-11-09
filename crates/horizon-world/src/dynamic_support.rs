//! Uncached support-shape selection. Block entities are queried through snapshots;
//! their lifecycle and active moving-piston collision generation are separate.
use crate::{block_support::SupportType, chunk::BlockState, compression::invalid};
use horizon_core::{
    geometry::{Aabb, Direction, ShapeError, ShapeOp, Vec3, VoxelShape},
    position::BlockPos,
    random::position_seed,
};
use std::{io, sync::OnceLock};

pub trait DynamicSupportWorld {
    type Error;
    /// None represents a missing block entity or a different block-entity type.
    fn shulker_is_closed(&mut self, position: [i32; 3]) -> Result<Option<bool>, Self::Error>;
    /// Invoke the active moving-piston entity's collision calculation when it
    /// exists. None represents a missing entity or a different entity type.
    fn moving_piston_shape(
        &mut self,
        position: [i32; 3],
    ) -> Result<Option<VoxelShape>, Self::Error>;
}
#[derive(Debug)]
pub enum DynamicSupportError<E> {
    World(E),
    Shape(ShapeError),
}
impl<E: std::fmt::Display> std::fmt::Display for DynamicSupportError<E> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::World(e) => e.fmt(f),
            Self::Shape(e) => e.fmt(f),
        }
    }
}
impl<E: std::error::Error + 'static> std::error::Error for DynamicSupportError<E> {}
#[derive(Clone, Debug)]
pub struct DynamicSupport {
    kind: Kind,
}
#[derive(Clone, Debug)]
enum Kind {
    Bamboo,
    Speleothem(usize),
    Scaffolding,
    PowderSnow,
    Shulker(Direction),
    MovingPiston,
}
impl DynamicSupport {
    /// Select behavior from a validated registry descriptor. None requests a
    /// different implementation (usually the immutable cached-support branch).
    pub fn from_state(state: &BlockState) -> io::Result<Option<Self>> {
        let property = |name: &str| {
            state
                .properties
                .get(name)
                .map(String::as_str)
                .ok_or_else(|| invalid("missing dynamic support property"))
        };
        let kind = match state.name.as_str() {
            "minecraft:bamboo" => Kind::Bamboo,
            "minecraft:pointed_dripstone" | "minecraft:sulfur_spike" => {
                let down = match property("vertical_direction")? {
                    "down" => true,
                    "up" => false,
                    _ => return Err(invalid("invalid speleothem direction")),
                };
                let shape = match property("thickness")? {
                    "tip_merge" => 0,
                    "tip" => {
                        if down {
                            2
                        } else {
                            1
                        }
                    }
                    "frustum" => 3,
                    "middle" => 4,
                    "base" => 5,
                    _ => return Err(invalid("invalid speleothem thickness")),
                };
                Kind::Speleothem(shape)
            }
            "minecraft:scaffolding" => Kind::Scaffolding,
            "minecraft:powder_snow" => Kind::PowderSnow,
            "minecraft:moving_piston" => Kind::MovingPiston,
            name if shulker(name) => Kind::Shulker(match property("facing")? {
                "down" => Direction::Down,
                "up" => Direction::Up,
                "north" => Direction::North,
                "south" => Direction::South,
                "west" => Direction::West,
                "east" => Direction::East,
                _ => return Err(invalid("invalid shulker facing")),
            }),
            _ => return Ok(None),
        };
        Ok(Some(Self { kind }))
    }
    pub fn offset(&self, position: [i32; 3]) -> Vec3 {
        let maximum = match self.kind {
            Kind::Bamboo => 0.25f32,
            Kind::Speleothem(_) => 0.125f32,
            _ => return Vec3::ZERO,
        };
        let seed = position_seed(BlockPos::new(position[0], 0, position[2]));
        let component = |shift: u32| {
            let fraction = ((seed >> shift) & 15i64) as f32 / 15.0f32;
            ((f64::from(fraction) - 0.5) * 0.5).clamp(-f64::from(maximum), f64::from(maximum))
        };
        Vec3::new(component(0), 0.0, component(8))
    }
    pub fn shape<W: DynamicSupportWorld>(
        &self,
        world: &mut W,
        position: [i32; 3],
    ) -> Result<VoxelShape, DynamicSupportError<W::Error>> {
        Ok(match self.kind {
            Kind::Bamboo => {
                static SHAPE: OnceLock<VoxelShape> = OnceLock::new();
                SHAPE
                    .get_or_init(|| column(3.0, 0.0, 16.0))
                    .moved(self.offset(position))
            }
            Kind::Speleothem(index) => {
                static SHAPES: OnceLock<[VoxelShape; 6]> = OnceLock::new();
                SHAPES.get_or_init(|| {
                    [
                        column(6.0, 0.0, 16.0),
                        column(6.0, 0.0, 11.0),
                        column(6.0, 5.0, 16.0),
                        column(8.0, 0.0, 16.0),
                        column(10.0, 0.0, 16.0),
                        column(12.0, 0.0, 16.0),
                    ]
                })[index]
                    .moved(self.offset(position))
            }
            Kind::Scaffolding => scaffolding_stable_shape(),
            Kind::PowderSnow => VoxelShape::empty(),
            Kind::Shulker(facing) => {
                if world
                    .shulker_is_closed(position)
                    .map_err(DynamicSupportError::World)?
                    == Some(false)
                {
                    static SHAPES: OnceLock<[VoxelShape; 6]> = OnceLock::new();
                    SHAPES.get_or_init(|| {
                        Direction::ALL.map(|direction| {
                            let mut min = [0.0; 3];
                            let mut max = [1.0; 3];
                            for (i, n) in direction.normal().into_iter().enumerate() {
                                if n < 0 {
                                    max[i] = 0.0625;
                                } else if n > 0 {
                                    min[i] = 0.9375;
                                }
                            }
                            box_shape(min, max)
                        })
                    })[facing.opposite() as usize]
                        .clone()
                } else {
                    VoxelShape::block()
                }
            }
            Kind::MovingPiston => world
                .moving_piston_shape(position)
                .map_err(DynamicSupportError::World)?
                .unwrap_or_else(VoxelShape::empty),
        })
    }
    pub fn collision_shape<W: crate::block_collision::DynamicCollisionWorld>(
        &self,
        world: &mut W,
        position: [i32; 3],
    ) -> Result<VoxelShape, DynamicSupportError<W::Error>> {
        if let Kind::Shulker(direction) = self.kind {
            return match world
                .shulker_collision_state(position)
                .map_err(DynamicSupportError::World)?
            {
                Some(snapshot) => VoxelShape::from_box(snapshot.bounding_box(direction))
                    .map_err(DynamicSupportError::Shape),
                None => Ok(VoxelShape::block()),
            };
        }
        // Empty collision context uses the stable scaffolding shape, empty
        // powder snow, and the same bamboo/speleothem/piston geometry as support.
        self.shape(world, position)
    }
    pub fn is_face_sturdy<W: DynamicSupportWorld>(
        &self,
        world: &mut W,
        position: [i32; 3],
        direction: Direction,
        support: SupportType,
    ) -> Result<bool, DynamicSupportError<W::Error>> {
        support
            .is_supporting(&self.shape(world, position)?, direction)
            .map_err(DynamicSupportError::Shape)
    }
}
/// Shared no-context support and context-selected stable scaffold geometry.
pub(crate) fn scaffolding_stable_shape() -> VoxelShape {
    scaffolding_stable_shape_ref().clone()
}
/// Borrowed form of [`scaffolding_stable_shape`] for hot paths that must not
/// touch the shape's reference counts.
pub(crate) fn scaffolding_stable_shape_ref() -> &'static VoxelShape {
    static SHAPE: OnceLock<VoxelShape> = OnceLock::new();
    SHAPE.get_or_init(|| {
        let mut legs = VoxelShape::empty();
        for [x, z] in [[0.0, 0.0], [0.875, 0.0], [0.875, 0.875], [0.0, 0.875]] {
            let leg = box_shape([x, 0.0, z], [x + 0.125, 1.0, z + 0.125]);
            legs = legs.join(&leg, ShapeOp::OR).expect("bounded scaffold legs");
        }
        column(16.0, 14.0, 16.0)
            .join(&legs, ShapeOp::OR)
            .expect("bounded scaffold support")
    })
}

fn box_shape(min: [f64; 3], max: [f64; 3]) -> VoxelShape {
    VoxelShape::from_box(Aabb::new(
        Vec3::new(min[0], min[1], min[2]),
        Vec3::new(max[0], max[1], max[2]),
    ))
    .expect("bounded support shape")
}
fn column(width: f64, min_y: f64, max_y: f64) -> VoxelShape {
    let min = (8.0 - width / 2.0) / 16.0;
    let max = (8.0 + width / 2.0) / 16.0;
    box_shape([min, min_y / 16.0, min], [max, max_y / 16.0, max])
}
fn shulker(name: &str) -> bool {
    matches!(
        name,
        "minecraft:shulker_box"
            | "minecraft:white_shulker_box"
            | "minecraft:orange_shulker_box"
            | "minecraft:magenta_shulker_box"
            | "minecraft:light_blue_shulker_box"
            | "minecraft:yellow_shulker_box"
            | "minecraft:lime_shulker_box"
            | "minecraft:pink_shulker_box"
            | "minecraft:gray_shulker_box"
            | "minecraft:light_gray_shulker_box"
            | "minecraft:cyan_shulker_box"
            | "minecraft:purple_shulker_box"
            | "minecraft:blue_shulker_box"
            | "minecraft:brown_shulker_box"
            | "minecraft:green_shulker_box"
            | "minecraft:red_shulker_box"
            | "minecraft:black_shulker_box"
    )
}
