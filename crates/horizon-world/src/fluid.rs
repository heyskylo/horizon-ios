//! Fluid states carried by the pinned vanilla block-state registry.
//! Flow updates, spreading and entity interaction are separate simulation work.
use crate::{
    chunk::BlockState,
    compression::invalid,
    registry::{BlockRegistry, BlockStateId},
};
use std::io;

/// A fluid: its index in the fluid table, which is its `minecraft:fluid`
/// network id. Vanilla's five come first, in their registry order; the
/// constants keep the names the enum had, so code matches on them as before.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct FluidKind(u8);

/// The vanilla fluid a fluid behaves as: how it flows, what it does to
/// mobs and how it meets other fluids. A mod fluid copies water or lava
/// (#6301), as a content block copies a template block.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FluidTemplate {
    Empty,
    Water,
    Lava,
}

/// Numbers a fluid gives instead of its template's (NeoForge's
/// `BaseFlowingFluid.Properties`). Vanilla's give none: water and lava keep
/// their own rules, lava's faster ones in an ultrawarm dimension included.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FluidNumbers {
    /// `getTickDelay`: ticks between spreads.
    pub tick_delay: Option<i32>,
    /// `getDropOff`: levels lost per block.
    pub drop_off: Option<i32>,
    /// `getSlopeFindDistance`: how far a spread looks for a way down.
    pub slope_find_distance: Option<i32>,
    /// `canConvertToSource`: two sources make a third.
    pub convert_to_source: Option<bool>,
}

/// One row of the fluid table.
#[derive(Clone, Copy, Debug)]
pub struct FluidInfo {
    pub name: &'static str,
    pub template: FluidTemplate,
    /// Its family's source and flowing forms (the empty fluid is both).
    pub source: FluidKind,
    pub flowing: FluidKind,
    pub numbers: FluidNumbers,
}

/// Vanilla's fluids, in `minecraft:fluid` order.
const VANILLA: [FluidInfo; 5] = {
    const fn row(
        name: &'static str,
        template: FluidTemplate,
        source: FluidKind,
        flowing: FluidKind,
    ) -> FluidInfo {
        let numbers = FluidNumbers {
            tick_delay: None,
            drop_off: None,
            slope_find_distance: None,
            convert_to_source: None,
        };
        FluidInfo {
            name,
            template,
            source,
            flowing,
            numbers,
        }
    }
    let (empty, flowing_water, water, flowing_lava, lava) = (
        FluidKind(0),
        FluidKind(1),
        FluidKind(2),
        FluidKind(3),
        FluidKind(4),
    );
    [
        row("minecraft:empty", FluidTemplate::Empty, empty, empty),
        row(
            "minecraft:flowing_water",
            FluidTemplate::Water,
            water,
            flowing_water,
        ),
        row(
            "minecraft:water",
            FluidTemplate::Water,
            water,
            flowing_water,
        ),
        row(
            "minecraft:flowing_lava",
            FluidTemplate::Lava,
            lava,
            flowing_lava,
        ),
        row("minecraft:lava", FluidTemplate::Lava, lava, flowing_lava),
    ]
};

#[allow(non_upper_case_globals)]
impl FluidKind {
    pub const Empty: Self = Self(0);
    pub const FlowingWater: Self = Self(1);
    pub const Water: Self = Self(2);
    pub const FlowingLava: Self = Self(3);
    pub const Lava: Self = Self(4);
    /// Vanilla's fluids.
    pub const ALL: [Self; 5] = [
        Self::Empty,
        Self::FlowingWater,
        Self::Water,
        Self::FlowingLava,
        Self::Lava,
    ];
    /// Its row in the fluid table, and its network id.
    pub const fn index(self) -> usize {
        self.0 as usize
    }
    pub const fn info(self) -> &'static FluidInfo {
        &VANILLA[self.0 as usize]
    }
    pub const fn name(self) -> &'static str {
        self.info().name
    }
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|kind| kind.name() == name)
    }
    pub const fn template(self) -> FluidTemplate {
        self.info().template
    }
    /// The source form of its family (`minecraft:water` for flowing water).
    pub const fn source(self) -> Self {
        self.info().source
    }
    /// The flowing form of its family.
    pub const fn flowing(self) -> Self {
        self.info().flowing
    }
    pub const fn numbers(self) -> FluidNumbers {
        self.info().numbers
    }
    /// `Fluid.isSame`: the same fluid, flowing or not.
    pub const fn same_family(self, other: Self) -> bool {
        self.source().0 == other.source().0
    }
    /// Its bit in a fluid tag's mask.
    pub const fn mask(self) -> FluidMask {
        1 << self.0
    }
}

/// A set of fluids, one bit per [`FluidKind`] (fluid tags). Eight bits hold
/// vanilla's five; it widens when mod fluids join the table, with the
/// behaviour cache's version (its snapshots keep these masks).
pub type FluidMask = u8;

impl std::fmt::Debug for FluidKind {
    /// The enum's variant names, which logs and probes print.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        const NAMES: [&str; 5] = ["Empty", "FlowingWater", "Water", "FlowingLava", "Lava"];
        match NAMES.get(self.index()) {
            Some(name) => f.write_str(name),
            None => write!(f, "FluidKind({})", self.0),
        }
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
        let flowing = kind == kind.flowing();
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
        self.kind.0 != FluidKind::Empty.0 && self.kind.0 == self.kind.source().0
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

#[cfg(test)]
mod tests {
    use super::{FluidKind, FluidNumbers, FluidState, FluidTemplate};

    /// The table gives vanilla's fluids what the enum gave them: their
    /// `minecraft:fluid` ids and names, families, sources and no numbers of
    /// their own (water's and lava's rules stay in the flowing code).
    #[test]
    fn vanillas_fluids_keep_their_ids_families_and_rules() {
        let names = [
            "minecraft:empty",
            "minecraft:flowing_water",
            "minecraft:water",
            "minecraft:flowing_lava",
            "minecraft:lava",
        ];
        for (id, (kind, name)) in FluidKind::ALL.into_iter().zip(names).enumerate() {
            assert_eq!((kind.index(), kind.name()), (id, name));
            assert_eq!(FluidKind::from_name(name), Some(kind));
            assert_eq!(kind.numbers(), FluidNumbers::default());
            assert_eq!(kind.mask(), 1 << id);
        }
        use FluidKind as K;
        let rows = [
            (K::Empty, FluidTemplate::Empty, K::Empty, K::Empty),
            (
                K::FlowingWater,
                FluidTemplate::Water,
                K::Water,
                K::FlowingWater,
            ),
            (K::Water, FluidTemplate::Water, K::Water, K::FlowingWater),
            (K::FlowingLava, FluidTemplate::Lava, K::Lava, K::FlowingLava),
            (K::Lava, FluidTemplate::Lava, K::Lava, K::FlowingLava),
        ];
        for (kind, template, source, flowing) in rows {
            assert_eq!(
                (kind.template(), kind.source(), kind.flowing()),
                (template, source, flowing)
            );
        }
        assert!(K::Water.same_family(K::FlowingWater) && K::Lava.same_family(K::FlowingLava));
        assert!(!K::Water.same_family(K::Lava) && !K::Empty.same_family(K::Water));
        assert!(K::Empty.same_family(K::Empty));
        let sources: Vec<_> = K::ALL
            .into_iter()
            .filter(|k| FluidState::new(*k, 8, false).is_source())
            .collect();
        assert_eq!(sources, [K::Water, K::Lava]);
        // Logs and probes print the enum's names.
        assert_eq!(
            format!("{:?} {:?}", K::FlowingLava, K::Empty),
            "FlowingLava Empty"
        );
    }

    #[test]
    fn a_fluid_state_reads_its_flowing_form_from_the_table() {
        let flowing = FluidState::from_json(
            r#"{"id":"minecraft:flowing_lava","properties":{"level":"3","falling":"false"}}"#,
        )
        .unwrap();
        assert_eq!(
            (flowing.kind(), flowing.amount(), flowing.is_source()),
            (FluidKind::FlowingLava, 3, false)
        );
        let source =
            FluidState::from_json(r#"{"id":"minecraft:water","properties":{"level":"3"}}"#)
                .unwrap();
        assert_eq!((source.amount(), source.is_source()), (8, true));
    }
}
