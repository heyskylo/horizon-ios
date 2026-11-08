//! Water-column floor scans and randomized magma replacement in occluded cells.
use super::{
    DecodeError, Object, block_feature::FeatureBlockBindings,
    block_predicate::PredicateBlockResolver, feature::FeatureWorld, int_range, number_text,
    required,
};
use crate::{block_properties::BoundOcclusionFaces, registry::BlockStateId};
use horizon_core::{geometry::Direction, random::RandomSource};
use std::{collections::BTreeSet, sync::Arc};
#[derive(Clone, Debug)]
pub struct UnderwaterMagmaFeature {
    search: i32,
    radius: i32,
    probability: f32,
    water: Arc<BTreeSet<BlockStateId>>,
    air: [BlockStateId; 3],
    magma: BlockStateId,
    faces: BoundOcclusionFaces,
}
#[derive(Debug)]
pub enum UnderwaterMagmaError<E> {
    World(E),
    Registry(std::io::Error),
    InvalidColumn,
    WorkLimit,
}
impl<E: std::fmt::Display> std::fmt::Display for UnderwaterMagmaError<E> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::World(e) => e.fmt(f),
            Self::Registry(e) => e.fmt(f),
            Self::InvalidColumn => f.write_str("negative scanned column height"),
            Self::WorkLimit => f.write_str("magma placement work limit exceeded"),
        }
    }
}
impl<E: std::error::Error + 'static> std::error::Error for UnderwaterMagmaError<E> {}
impl UnderwaterMagmaFeature {
    pub fn from_json(json: &str, bindings: &FeatureBlockBindings<'_>) -> Result<Self, DecodeError> {
        let object: Object = serde_json::from_str(json)?;
        let probability = number_text(required(
            &object,
            "placement_probability_per_valid_position",
        )?)?
        .parse::<f64>()
        .map_err(|_| DecodeError::Field("magma probability"))? as f32;
        if !(0.0..=1.0).contains(&probability) || probability.is_sign_negative() {
            return Err(DecodeError::Field("magma probability"));
        }
        Ok(Self {
            search: int_range(
                required(&object, "floor_search_range")?,
                "magma floor range",
                0,
                512,
            )?,
            radius: int_range(
                required(&object, "placement_radius_around_floor")?,
                "magma radius",
                0,
                64,
            )?,
            probability,
            water: bindings.predicate_block_set(r#""minecraft:water""#)?,
            air: bindings.air,
            magma: bindings
                .registry()
                .default_state("minecraft:magma_block")
                .ok_or(DecodeError::Field("magma block"))?,
            faces: bindings
                .occlusion_faces
                .clone()
                .ok_or(DecodeError::Field("magma occlusion bindings"))?,
        })
    }
    pub fn place<W: FeatureWorld>(
        &self,
        world: &mut W,
        random: &mut impl RandomSource,
        origin: [i32; 3],
    ) -> Result<bool, UnderwaterMagmaError<W::Error>> {
        if !self
            .water
            .contains(&world.block(origin).map_err(UnderwaterMagmaError::World)?)
        {
            return Ok(false);
        }
        let ceiling = self.scan(world, origin, 1)?;
        let floor = self.scan(world, origin, -1)?;
        if let (Some(floor), Some(ceiling)) = (floor, ceiling)
            && ceiling.wrapping_sub(floor).wrapping_sub(1) < 0
        {
            return Err(UnderwaterMagmaError::InvalidColumn);
        }
        let Some(y) = floor else {
            return Ok(false);
        };
        let center = [origin[0], y, origin[2]];
        let a: [i32; 3] = std::array::from_fn(|i| center[i].wrapping_sub(self.radius));
        let b: [i32; 3] = std::array::from_fn(|i| center[i].wrapping_add(self.radius));
        let min: [i32; 3] = std::array::from_fn(|i| a[i].min(b[i]));
        let sizes: [i32; 3] =
            std::array::from_fn(|i| a[i].max(b[i]).wrapping_sub(min[i]).wrapping_add(1));
        let count = sizes[0].wrapping_mul(sizes[1]).wrapping_mul(sizes[2]);
        let mut index: i32 = 0;
        let mut any = false;
        while index != count {
            // Includes the full valid radius-64 cube (2,146,689 positions).
            if index == 8_388_608 {
                return Err(UnderwaterMagmaError::WorkLimit);
            }
            let quotient = index / sizes[0];
            let p = [
                min[0].wrapping_add(index % sizes[0]),
                min[1].wrapping_add(quotient % sizes[1]),
                min[2].wrapping_add(quotient / sizes[1]),
            ];
            index += 1;
            if random.next_float() < self.probability && self.valid(world, p)? {
                world
                    .set_block(p, self.magma, 2)
                    .map_err(UnderwaterMagmaError::World)?;
                any = true;
            }
        }
        Ok(any)
    }
    fn scan<W: FeatureWorld>(
        &self,
        world: &mut W,
        mut p: [i32; 3],
        step: i32,
    ) -> Result<Option<i32>, UnderwaterMagmaError<W::Error>> {
        let mut distance = 1;
        while distance < self.search {
            if !self
                .water
                .contains(&world.block(p).map_err(UnderwaterMagmaError::World)?)
            {
                break;
            }
            p[1] = p[1].wrapping_add(step);
            distance += 1;
        }
        Ok((!self
            .water
            .contains(&world.block(p).map_err(UnderwaterMagmaError::World)?))
        .then_some(p[1]))
    }
    fn valid<W: FeatureWorld>(
        &self,
        world: &mut W,
        p: [i32; 3],
    ) -> Result<bool, UnderwaterMagmaError<W::Error>> {
        let state = world.block(p).map_err(UnderwaterMagmaError::World)?;
        if self.water.contains(&state) || self.air.contains(&state) {
            return Ok(false);
        }
        if !self.occluded(world, [p[0], p[1].wrapping_sub(1), p[2]], Direction::Up)? {
            return Ok(false);
        }
        for direction in [
            Direction::North,
            Direction::East,
            Direction::South,
            Direction::West,
        ] {
            let step = direction.normal();
            let at = std::array::from_fn(|i| p[i].wrapping_add(step[i]));
            if !self.occluded(world, at, direction.opposite())? {
                return Ok(false);
            }
        }
        Ok(true)
    }
    fn occluded<W: FeatureWorld>(
        &self,
        world: &mut W,
        p: [i32; 3],
        direction: Direction,
    ) -> Result<bool, UnderwaterMagmaError<W::Error>> {
        let state = world.block(p).map_err(UnderwaterMagmaError::World)?;
        self.faces
            .is_full(state, direction)
            .map_err(UnderwaterMagmaError::Registry)
    }
}
