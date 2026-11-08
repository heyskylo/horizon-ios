//! End gateway structure placement and conditional block-entity configuration.
use super::{
    DecodeError, Object,
    block_feature::FeatureBlockBindings,
    feature::FeatureWorld,
    integer, optional,
    placement::{PlacementContext, PlacementQueryError},
    required,
};
use crate::registry::BlockStateId;
use serde_json::value::RawValue;

#[derive(Clone, Debug)]
pub struct EndGatewayFeature {
    exit: Option<[i32; 3]>,
    exact: bool,
    air: BlockStateId,
    bedrock: BlockStateId,
    gateway: BlockStateId,
}
#[derive(Debug)]
pub enum EndGatewayError<E> {
    World(E),
    Query(PlacementQueryError),
    WorkLimit,
}
impl<E: std::fmt::Display> std::fmt::Display for EndGatewayError<E> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::World(e) => e.fmt(f),
            Self::Query(e) => e.fmt(f),
            Self::WorkLimit => f.write_str("end gateway work limit exceeded"),
        }
    }
}
impl<E: std::error::Error + 'static> std::error::Error for EndGatewayError<E> {}
impl EndGatewayFeature {
    pub fn from_json(json: &str, bindings: &FeatureBlockBindings<'_>) -> Result<Self, DecodeError> {
        let object: Object = serde_json::from_str(json)?;
        let exact = serde_json::from_str(required(&object, "exact")?.get())?;
        let exit = optional(&object, "exit")
            .map(|raw| {
                let values: Vec<Box<RawValue>> = serde_json::from_str(raw.get())?;
                if values.len() != 3 {
                    return Err(DecodeError::Field("gateway exit"));
                }
                Ok([
                    integer(&values[0])?,
                    integer(&values[1])?,
                    integer(&values[2])?,
                ])
            })
            .transpose()?;
        let get = |name| {
            bindings
                .registry()
                .default_state(name)
                .ok_or_else(|| DecodeError::Registry(format!("unknown gateway block {name}")))
        };
        Ok(Self {
            exit,
            exact,
            air: get("minecraft:air")?,
            bedrock: get("minecraft:bedrock")?,
            gateway: get("minecraft:end_gateway")?,
        })
    }
    pub fn place<W: FeatureWorld + PlacementContext>(
        &self,
        world: &mut W,
        origin: [i32; 3],
    ) -> Result<bool, EndGatewayError<W::Error>> {
        let offset =
            |d: [i32; 3]| -> [i32; 3] { std::array::from_fn(|i| origin[i].wrapping_add(d[i])) };
        let a = offset([-1, -2, -1]);
        let b = offset([1, 2, 1]);
        let min: [i32; 3] = std::array::from_fn(|i| a[i].min(b[i]));
        let sizes: [i32; 3] =
            std::array::from_fn(|i| a[i].max(b[i]).wrapping_sub(min[i]).wrapping_add(1));
        let count = sizes[0].wrapping_mul(sizes[1]).wrapping_mul(sizes[2]);
        let mut index: i32 = 0;
        let mut remaining = 1_048_576;
        while index != count {
            if remaining == 0 {
                return Err(EndGatewayError::WorkLimit);
            }
            remaining -= 1;
            let quotient = index / sizes[0];
            let p = [
                min[0].wrapping_add(index % sizes[0]),
                min[1].wrapping_add(quotient % sizes[1]),
                min[2].wrapping_add(quotient / sizes[1]),
            ];
            index = index.wrapping_add(1);
            let x = p[0] == origin[0];
            let y = p[1] == origin[1];
            let z = p[2] == origin[2];
            let cap = p[1].wrapping_sub(origin[1]).wrapping_abs() == 2;
            let center = x && y && z;
            let state = if center {
                self.gateway
            } else if y {
                self.air
            } else if cap && x && z || !cap && (x || z) {
                self.bedrock
            } else {
                self.air
            };
            world
                .set_block(p, state, 3)
                .map_err(EndGatewayError::World)?;
            if center && let Some(exit) = self.exit {
                // Native code looks up the entity even when the write returned false.
                world
                    .configure_end_gateway(p, exit, self.exact)
                    .map_err(EndGatewayError::Query)?;
            }
        }
        Ok(true)
    }
}
