//! Fluid states carried by the pinned vanilla block-state registry.
//! Flow updates, spreading and entity interaction are separate simulation work.
use crate::{
    chunk::BlockState,
    compression::invalid,
    registry::{BlockRegistry, BlockStateId},
};
use std::io;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum FluidKind {
    Empty,
    FlowingWater,
    Water,
    FlowingLava,
    Lava,
}
impl FluidKind {
    pub const ALL: [Self; 5] = [
        Self::Empty,
        Self::FlowingWater,
        Self::Water,
        Self::FlowingLava,
        Self::Lava,
    ];
    pub const fn name(self) -> &'static str {
        match self {
            Self::Empty => "minecraft:empty",
            Self::FlowingWater => "minecraft:flowing_water",
            Self::Water => "minecraft:water",
            Self::FlowingLava => "minecraft:flowing_lava",
            Self::Lava => "minecraft:lava",
        }
    }
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|kind| kind.name() == name)
    }
    pub const fn same_family(self, other: Self) -> bool {
        matches!(
            (self, other),
            (Self::Empty, Self::Empty)
                | (
                    Self::Water | Self::FlowingWater,
                    Self::Water | Self::FlowingWater
                )
                | (
                    Self::Lava | Self::FlowingLava,
                    Self::Lava | Self::FlowingLava
                )
        )
    }
    pub const fn mask(self) -> u8 {
        1 << self as u8
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FluidState {
    kind: FluidKind,
    amount: u8,
    falling: bool,
}
impl FluidState {
    /// Native fluid states require an object; properties decode leniently.
    pub fn from_json(json: &str) -> io::Result<Self> {
        let value: serde_json::Value =
            serde_json::from_str(json).map_err(|e| invalid(e.to_string()))?;
        let name = value
            .get("id")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| invalid("missing fluid identifier"))?;
        let (namespace, path) = name.split_once(':').unwrap_or(("minecraft", name));
        let name = format!(
            "{}:{path}",
            if namespace.is_empty() {
                "minecraft"
            } else {
                namespace
            }
        );
        let kind =
            FluidKind::from_name(&name).ok_or_else(|| invalid("unknown fluid identifier"))?;
        if kind == FluidKind::Empty {
            return Ok(Self::EMPTY);
        }
        let flowing = matches!(kind, FluidKind::FlowingWater | FluidKind::FlowingLava);
        let mut state = Self {
            kind,
            amount: if flowing { 1 } else { 8 },
            falling: true,
        };
        if let Some(properties) = value
            .get("properties")
            .and_then(serde_json::Value::as_object)
        {
            if let Some(value) = properties
                .get("falling")
                .and_then(serde_json::Value::as_str)
            {
                match value {
                    "true" => state.falling = true,
                    "false" => state.falling = false,
                    _ => {}
                }
            }
            if flowing
                && let Some(value) = properties
                    .get("level")
                    .and_then(serde_json::Value::as_str)
                    .and_then(crate::registry::java_integer)
                    .filter(|v| (1..=8).contains(v))
            {
                state.amount = value as u8;
            }
        }
        Ok(state)
    }
    pub fn to_json(self) -> io::Result<String> {
        let mut value = serde_json::json!({"id": self.kind.name()});
        if self.kind != FluidKind::Empty {
            let mut properties = serde_json::json!({"falling": self.falling.to_string()});
            if !self.is_source() {
                properties["level"] = self.amount.to_string().into();
            }
            value["properties"] = properties;
        }
        serde_json::to_string(&value).map_err(|e| invalid(e.to_string()))
    }
    /// A fluid state from its parts; the empty fluid ignores amount and falling.
    pub const fn new(kind: FluidKind, amount: u8, falling: bool) -> Self {
        if matches!(kind, FluidKind::Empty) {
            return Self::EMPTY;
        }
        Self {
            kind,
            amount,
            falling,
        }
    }
    pub const EMPTY: Self = Self {
        kind: FluidKind::Empty,
        amount: 0,
        falling: false,
    };
    pub const WATER: Self = Self {
        kind: FluidKind::Water,
        amount: 8,
        falling: false,
    };
    pub const fn kind(self) -> FluidKind {
        self.kind
    }
    pub const fn amount(self) -> u8 {
        self.amount
    }
    pub const fn falling(self) -> bool {
        self.falling
    }
    pub const fn is_source(self) -> bool {
        matches!(self.kind, FluidKind::Water | FluidKind::Lava)
    }
    pub const fn is_full(self) -> bool {
        self.amount == 8
    }
    pub fn height_at<W: FluidHeightWorld>(
        self,
        world: &mut W,
        position: [i32; 3],
    ) -> Result<f32, W::Error> {
        if self.kind == FluidKind::Empty {
            return Ok(0.0);
        }
        let above = [position[0], position[1].wrapping_add(1), position[2]];
        Ok(if self.kind.same_family(world.fluid(above)?.kind) {
            1.0
        } else {
            self.own_height()
        })
    }
    pub fn camera_height_at<W: FluidHeightWorld>(
        self,
        world: &mut W,
        position: [i32; 3],
    ) -> Result<f32, W::Error> {
        if self.is_source() {
            let above = [position[0], position[1].wrapping_add(1), position[2]];
            if world.sturdy_down(above)? {
                return Ok(1.0);
            }
        }
        self.height_at(world, position)
    }
    pub fn own_height(self) -> f32 {
        f32::from(self.amount) / 9.0
    }
    pub fn from_block(state: &BlockState) -> io::Result<Self> {
        let water = state.name == "minecraft:water";
        if water || state.name == "minecraft:lava" {
            let level = state
                .properties
                .get("level")
                .and_then(|v| v.parse::<u8>().ok())
                .filter(|v| *v <= 15)
                .ok_or_else(|| invalid("invalid liquid block level"))?;
            let kind = match (water, level == 0) {
                (true, true) => FluidKind::Water,
                (true, false) => FluidKind::FlowingWater,
                (false, true) => FluidKind::Lava,
                (false, false) => FluidKind::FlowingLava,
            };
            return Ok(Self {
                kind,
                amount: if level == 0 || level >= 8 {
                    8
                } else {
                    8 - level
                },
                falling: level >= 8,
            });
        }
        if matches!(
            state.name.as_str(),
            "minecraft:kelp"
                | "minecraft:kelp_plant"
                | "minecraft:seagrass"
                | "minecraft:tall_seagrass"
                | "minecraft:bubble_column"
        ) {
            return Ok(Self::WATER);
        }
        match state.properties.get("waterlogged").map(String::as_str) {
            Some("true") => {
                // The pinned copper-grate blocks use falling source water.
                let falling = matches!(
                    state.name.as_str(),
                    "minecraft:copper_grate"
                        | "minecraft:exposed_copper_grate"
                        | "minecraft:weathered_copper_grate"
                        | "minecraft:oxidized_copper_grate"
                        | "minecraft:waxed_copper_grate"
                        | "minecraft:waxed_exposed_copper_grate"
                        | "minecraft:waxed_weathered_copper_grate"
                        | "minecraft:waxed_oxidized_copper_grate"
                );
                Ok(Self {
                    falling,
                    ..Self::WATER
                })
            }
            None | Some("false") => Ok(Self::EMPTY),
            _ => Err(invalid("invalid waterlogged block property")),
        }
    }
}
/// Queries required by fluid surface height and camera immersion. The caller
/// owns block shape/support behavior and world-query failures.
pub trait FluidHeightWorld {
    type Error;
    fn fluid(&mut self, position: [i32; 3]) -> Result<FluidState, Self::Error>;
    fn sturdy_down(&mut self, position: [i32; 3]) -> Result<bool, Self::Error>;
}
#[derive(Debug)]
pub struct FluidStateTable(Vec<FluidState>);
impl FluidStateTable {
    pub fn new(registry: &BlockRegistry) -> io::Result<Self> {
        (0..registry.len())
            .map(|i| FluidState::from_block(registry.state(BlockStateId(i as u32)).unwrap()))
            .collect::<io::Result<Vec<_>>>()
            .map(Self)
    }
    pub fn get(&self, state: BlockStateId) -> Option<FluidState> {
        self.0.get(state.0 as usize).copied()
    }
}

/// Bound identities for converting fluid states into their legacy liquid blocks.
pub struct FluidBlockBindings {
    air: BlockStateId,
    water: [BlockStateId; 16],
    lava: [BlockStateId; 16],
}
impl FluidBlockBindings {
    pub fn new(registry: &BlockRegistry) -> io::Result<Self> {
        let levels = |name: &str| -> io::Result<[BlockStateId; 16]> {
            let default = registry
                .default_state(name)
                .ok_or_else(|| invalid("missing liquid block"))?;
            let mut state = registry.state(default).unwrap().clone();
            let mut result = [default; 16];
            for (level, id) in result.iter_mut().enumerate() {
                state.properties.insert("level".into(), level.to_string());
                *id = registry
                    .id(&state)
                    .ok_or_else(|| invalid("missing liquid block level"))?;
            }
            Ok(result)
        };
        Ok(Self {
            air: registry
                .default_state("minecraft:air")
                .ok_or_else(|| invalid("missing air block"))?,
            water: levels("minecraft:water")?,
            lava: levels("minecraft:lava")?,
        })
    }
    pub fn block(&self, state: FluidState) -> BlockStateId {
        if state.kind == FluidKind::Empty {
            return self.air;
        }
        let level = if state.is_source() {
            0
        } else {
            8 - state.amount.min(8) + if state.falling { 8 } else { 0 }
        } as usize;
        if state.kind.same_family(FluidKind::Water) {
            self.water[level]
        } else {
            self.lava[level]
        }
    }
}
