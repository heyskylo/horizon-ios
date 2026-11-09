//! Initial block-survival rules over version-bound definitions and world queries.
//! Unsupported definitions remain explicit; survival is not inferred from shape.
use crate::{
    block_collision::{BlockCollisionRegistry, CollisionQueryError, DynamicCollisionWorld},
    block_properties::{BlockPropertiesRegistry, BlockProperty, BoundBlockProperty},
    block_support::{BlockSupportRegistry, SupportQueryError, SupportType},
    compression::invalid,
    fluid::{FluidKind, FluidState},
    registry::{BlockRegistry, BlockStateId},
};
use horizon_core::geometry::Direction;
use std::{
    collections::{BTreeMap, BTreeSet},
    io,
    sync::Arc,
};
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SurvivalRule {
    Fire,
    Cactus,
    Path,
    Mushroom,
    Multiface,
    Vine,
    HangingMoss,
    SnowLayer,
    SeaPickle,
    Bell,
    GrowingPlant,
    MangrovePropagule,
    Speleothem,
    Scaffolding,
    PistonHead,
    ChorusPlant,
    ChorusFlower,
    PitcherCrop,
    SugarCane,
    LilyPad,
    Frogspawn,
    SporeBlossom,
    SmallDripleaf,
    TallSeagrass,
    BigDripleaf,
    BigDripleafStem,
    Farmland,
    Cocoa,
    Crop,
    DoublePlant,
    Door,
    TagBelow(String),
    CactusFlower,
    Seagrass,
    BubbleColumn,
    RedstoneWire,
    Always,
    SolidBelow,
    SolidBehind,
    NonAirBelow,
    MossCarpet,
    CenterBelow,
    Lantern,
    FullAbove,
    FullBehind,
    RigidOrCenterBelow,
    RigidBelow,
    FullBelow,
    CenterAbove,
    FaceAttached,
    SupportsBamboo,
}
/// Version-specific definition inputs, independent of evaluated survival results.
#[derive(Clone, Debug, Default)]
pub struct SurvivalDefinitions {
    pub rules: BTreeMap<String, SurvivalRule>,
    pub ignition_odds: BTreeMap<String, i32>,
    pub fence_gates: BTreeSet<String>,
}
#[derive(Clone, Copy)]
enum Rule {
    Fire,
    Cactus,
    Path,
    Mushroom,
    Multiface(u8),
    Vine(u8),
    HangingMoss,
    SnowLayer,
    SeaPickle,
    GrowingPlant {
        head: BlockStateId,
        body: BlockStateId,
        direction: Direction,
        kelp: bool,
    },
    MangrovePropagule(bool),
    Speleothem(Direction),
    Scaffolding,
    PistonHead {
        direction: Direction,
        sticky: bool,
    },
    ChorusPlant,
    ChorusFlower,
    PitcherCrop(bool),
    SugarCane,
    LilyPad,
    Frogspawn,
    SporeBlossom,
    SmallDripleaf(bool),
    TallSeagrass(bool),
    BigDripleaf,
    BigDripleafStem,
    Farmland,
    Cocoa(Direction),
    Crop,
    DoublePlant(bool),
    Door(bool),
    TagBelow(usize),
    CactusFlower,
    Seagrass,
    BubbleColumn,
    RedstoneWire,
    Always,
    Solid(Direction),
    NonAirBelow,
    MossCarpet(bool),
    Support {
        face: Direction,
        kind: SupportType,
        center_restriction: bool,
    },
    RigidOrCenterBelow,
    SupportsBamboo,
}
pub trait SurvivalWorld: DynamicCollisionWorld {
    fn fluid_state(&mut self, position: [i32; 3]) -> Result<FluidState, Self::Error>;
    fn raw_brightness(&mut self, position: [i32; 3], sky_darken: i32) -> Result<i32, Self::Error>;
    fn block_state(&mut self, position: [i32; 3]) -> Result<BlockStateId, Self::Error>;
}
#[derive(Debug)]
pub enum SurvivalError<E> {
    World(E),
    Support(SupportQueryError<E>),
    Collision(CollisionQueryError<E>),
    Registry(io::Error),
    Unsupported(BlockStateId),
}
impl<E: std::fmt::Display> std::fmt::Display for SurvivalError<E> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::World(e) => e.fmt(f),
            Self::Support(e) => e.fmt(f),
            Self::Collision(e) => e.fmt(f),
            Self::Registry(e) => e.fmt(f),
            Self::Unsupported(id) => write!(f, "unsupported block survival state {}", id.0),
        }
    }
}
impl<E: std::error::Error + 'static> std::error::Error for SurvivalError<E> {}
pub struct BlockSurvivalRegistry<'r> {
    registry: &'r BlockRegistry,
    support: Arc<BlockSupportRegistry<'r>>,
    collision: Arc<BlockCollisionRegistry<'r>>,
    solid: BoundBlockProperty,
    liquid: BoundBlockProperty,
    solid_render: BoundBlockProperty,
    flammable: BTreeSet<BlockStateId>,
    fence_gates: BTreeSet<BlockStateId>,
    cactus_support: BTreeSet<BlockStateId>,
    mushroom_light_override: BTreeSet<BlockStateId>,
    rules: Vec<Option<Rule>>,
    unstable_bottom_center: BTreeSet<BlockStateId>,
    supports_bamboo: BTreeSet<BlockStateId>,
    bound_tags: Vec<BTreeSet<BlockStateId>>,
    cactus_flower: BTreeSet<BlockStateId>,
    no_seagrass: BTreeSet<BlockStateId>,
    vegetation: BTreeSet<BlockStateId>,
    dripleaf: BTreeSet<BlockStateId>,
    maintains_farmland: BTreeSet<BlockStateId>,
    cocoa: BTreeSet<BlockStateId>,
    crops: BTreeSet<BlockStateId>,
    no_snow: BTreeSet<BlockStateId>,
    snow_override: BTreeSet<BlockStateId>,
    mangrove: BTreeSet<BlockStateId>,
    hanging_mangrove: BTreeSet<BlockStateId>,
    no_kelp: BTreeSet<BlockStateId>,
    chorus_plant: BTreeSet<BlockStateId>,
    chorus_flower: BTreeSet<BlockStateId>,
    lily_pad: BTreeSet<BlockStateId>,
    frogspawn: BTreeSet<BlockStateId>,
    small_dripleaf: BTreeSet<BlockStateId>,
    sugar_cane: BTreeSet<BlockStateId>,
    sugar_cane_adjacent: BTreeSet<BlockStateId>,
    fluid_tags: [crate::fluid::FluidMask; 5],
    bubble_up: BTreeSet<BlockStateId>,
    bubble_down: BTreeSet<BlockStateId>,
}
impl<'r> BlockSurvivalRegistry<'r> {
    pub fn new(
        registry: &'r BlockRegistry,
        properties: &BlockPropertiesRegistry<'r>,
        support: Arc<BlockSupportRegistry<'r>>,
        collision: Arc<BlockCollisionRegistry<'r>>,
        definitions: &SurvivalDefinitions,
        tags: &BTreeMap<String, Vec<String>>,
        fluid_tags: &BTreeMap<String, Vec<String>>,
    ) -> io::Result<Self> {
        if !std::ptr::eq(registry, properties.registry())
            || !std::ptr::eq(registry, support.registry())
            || !std::ptr::eq(registry, collision.registry())
        {
            return Err(invalid("survival inputs use different registries"));
        }
        for name in definitions
            .rules
            .keys()
            .chain(definitions.ignition_odds.keys())
            .chain(definitions.fence_gates.iter())
        {
            if registry.default_state(name).is_none() {
                return Err(invalid("unknown block survival definition"));
            }
        }
        let mut rules = Vec::with_capacity(registry.len());
        let mut tag_names = Vec::<String>::new();
        let mut tag_indices = BTreeMap::<String, usize>::new();
        for id in 0..registry.len() {
            let state = registry.state(BlockStateId(id as u32)).unwrap();
            let Some(rule) = definitions.rules.get(&state.name) else {
                rules.push(None);
                continue;
            };
            let property = |name: &str| {
                state
                    .properties
                    .get(name)
                    .map(String::as_str)
                    .ok_or_else(|| invalid("missing survival property"))
            };
            let facing = || -> io::Result<Direction> {
                match property("facing")? {
                    "down" => Ok(Direction::Down),
                    "up" => Ok(Direction::Up),
                    "north" => Ok(Direction::North),
                    "south" => Ok(Direction::South),
                    "west" => Ok(Direction::West),
                    "east" => Ok(Direction::East),
                    _ => Err(invalid("invalid survival facing")),
                }
            };
            let boolean = |name: &str| -> io::Result<bool> {
                match property(name)? {
                    "true" => Ok(true),
                    "false" => Ok(false),
                    _ => Err(invalid("invalid survival boolean")),
                }
            };
            let upper_half = || -> io::Result<bool> {
                match property("half")? {
                    "upper" => Ok(true),
                    "lower" => Ok(false),
                    _ => Err(invalid("invalid survival half")),
                }
            };
            let full = |face| Rule::Support {
                face,
                kind: SupportType::Full,
                center_restriction: false,
            };
            let faces = || -> io::Result<u8> {
                let mut mask = 0;
                for direction in Direction::ALL {
                    if state.properties.contains_key(direction_name(direction))
                        && boolean(direction_name(direction))?
                    {
                        mask |= 1 << direction as u8;
                    }
                }
                Ok(mask)
            };
            rules.push(Some(match rule {
                SurvivalRule::Fire => Rule::Fire,
                SurvivalRule::Cactus => Rule::Cactus,
                SurvivalRule::Path => Rule::Path,
                SurvivalRule::Mushroom => Rule::Mushroom,
                SurvivalRule::Multiface => Rule::Multiface(faces()?),
                SurvivalRule::Vine => Rule::Vine(faces()?),
                SurvivalRule::HangingMoss => Rule::HangingMoss,
                SurvivalRule::SnowLayer => Rule::SnowLayer,
                SurvivalRule::SeaPickle => Rule::SeaPickle,
                SurvivalRule::Bell => {
                    let (face, kind, center_restriction) = match property("attachment")? {
                        "floor" => (Direction::Up, SupportType::Full, false),
                        "ceiling" => (Direction::Down, SupportType::Center, true),
                        "single_wall" | "double_wall" => {
                            (facing()?.opposite(), SupportType::Full, false)
                        }
                        _ => return Err(invalid("invalid bell attachment")),
                    };
                    Rule::Support {
                        face,
                        kind,
                        center_restriction,
                    }
                }
                SurvivalRule::GrowingPlant => {
                    let (head, body, direction, kelp) = match state.name.as_str() {
                        "minecraft:kelp" | "minecraft:kelp_plant" => (
                            "minecraft:kelp",
                            "minecraft:kelp_plant",
                            Direction::Up,
                            true,
                        ),
                        "minecraft:cave_vines" | "minecraft:cave_vines_plant" => (
                            "minecraft:cave_vines",
                            "minecraft:cave_vines_plant",
                            Direction::Down,
                            false,
                        ),
                        "minecraft:twisting_vines" | "minecraft:twisting_vines_plant" => (
                            "minecraft:twisting_vines",
                            "minecraft:twisting_vines_plant",
                            Direction::Up,
                            false,
                        ),
                        "minecraft:weeping_vines" | "minecraft:weeping_vines_plant" => (
                            "minecraft:weeping_vines",
                            "minecraft:weeping_vines_plant",
                            Direction::Down,
                            false,
                        ),
                        _ => return Err(invalid("unknown growing plant family")),
                    };
                    Rule::GrowingPlant {
                        head: registry
                            .default_state(head)
                            .ok_or_else(|| invalid("missing growing plant head"))?,
                        body: registry
                            .default_state(body)
                            .ok_or_else(|| invalid("missing growing plant body"))?,
                        direction,
                        kelp,
                    }
                }
                SurvivalRule::MangrovePropagule => Rule::MangrovePropagule(boolean("hanging")?),
                SurvivalRule::Speleothem => {
                    Rule::Speleothem(match property("vertical_direction")? {
                        "up" => Direction::Up,
                        "down" => Direction::Down,
                        _ => return Err(invalid("invalid speleothem direction")),
                    })
                }
                SurvivalRule::Scaffolding => Rule::Scaffolding,
                SurvivalRule::PistonHead => Rule::PistonHead {
                    direction: facing()?,
                    sticky: match property("type")? {
                        "normal" => false,
                        "sticky" => true,
                        _ => return Err(invalid("invalid piston head type")),
                    },
                },
                SurvivalRule::ChorusPlant => Rule::ChorusPlant,
                SurvivalRule::ChorusFlower => Rule::ChorusFlower,
                SurvivalRule::PitcherCrop => Rule::PitcherCrop(upper_half()?),
                SurvivalRule::SugarCane => Rule::SugarCane,
                SurvivalRule::LilyPad => Rule::LilyPad,
                SurvivalRule::Frogspawn => Rule::Frogspawn,
                SurvivalRule::SporeBlossom => Rule::SporeBlossom,
                SurvivalRule::SmallDripleaf => Rule::SmallDripleaf(upper_half()?),
                SurvivalRule::TallSeagrass => Rule::TallSeagrass(upper_half()?),
                SurvivalRule::BigDripleaf => Rule::BigDripleaf,
                SurvivalRule::BigDripleafStem => Rule::BigDripleafStem,
                SurvivalRule::Farmland => Rule::Farmland,
                SurvivalRule::Cocoa => Rule::Cocoa(facing()?),
                SurvivalRule::Crop => Rule::Crop,
                SurvivalRule::DoublePlant => Rule::DoublePlant(upper_half()?),
                SurvivalRule::Door => Rule::Door(upper_half()?),
                SurvivalRule::TagBelow(name) => {
                    Rule::TagBelow(*tag_indices.entry(name.clone()).or_insert_with(|| {
                        let index = tag_names.len();
                        tag_names.push(name.clone());
                        index
                    }))
                }
                SurvivalRule::CactusFlower => Rule::CactusFlower,
                SurvivalRule::Seagrass => Rule::Seagrass,
                SurvivalRule::BubbleColumn => Rule::BubbleColumn,
                SurvivalRule::RedstoneWire => Rule::RedstoneWire,
                SurvivalRule::Always => Rule::Always,
                SurvivalRule::SolidBelow => Rule::Solid(Direction::Down),
                SurvivalRule::SolidBehind => Rule::Solid(facing()?.opposite()),
                SurvivalRule::NonAirBelow => Rule::NonAirBelow,
                SurvivalRule::MossCarpet => Rule::MossCarpet(boolean("bottom")?),
                SurvivalRule::CenterBelow => Rule::Support {
                    face: Direction::Up,
                    kind: SupportType::Center,
                    center_restriction: true,
                },
                SurvivalRule::Lantern => Rule::Support {
                    face: if boolean("hanging")? {
                        Direction::Down
                    } else {
                        Direction::Up
                    },
                    kind: SupportType::Center,
                    center_restriction: true,
                },
                SurvivalRule::FullAbove => full(Direction::Down),
                SurvivalRule::FullBehind => full(facing()?),
                SurvivalRule::RigidOrCenterBelow => Rule::RigidOrCenterBelow,
                SurvivalRule::RigidBelow => Rule::Support {
                    face: Direction::Up,
                    kind: SupportType::Rigid,
                    center_restriction: false,
                },
                SurvivalRule::FullBelow => full(Direction::Up),
                SurvivalRule::CenterAbove => Rule::Support {
                    face: Direction::Down,
                    kind: SupportType::Center,
                    center_restriction: false,
                },
                SurvivalRule::FaceAttached => full(match property("face")? {
                    "floor" => Direction::Up,
                    "ceiling" => Direction::Down,
                    "wall" => facing()?,
                    _ => return Err(invalid("invalid attachment face")),
                }),
                SurvivalRule::SupportsBamboo => Rule::SupportsBamboo,
            }));
        }
        let tag_states = |name: &str| -> io::Result<BTreeSet<BlockStateId>> {
            let names = tags
                .get(name)
                .into_iter()
                .flatten()
                .collect::<BTreeSet<_>>();
            for name in &names {
                if registry.default_state(name).is_none() {
                    return Err(invalid("unknown survival tag member"));
                }
            }
            Ok((0..registry.len())
                .map(|i| BlockStateId(i as u32))
                .filter(|id| names.contains(&registry.state(*id).unwrap().name))
                .collect())
        };
        let fluid_tag = |name: &str| -> io::Result<crate::fluid::FluidMask> {
            fluid_tags
                .get(name)
                .into_iter()
                .flatten()
                .try_fold(0, |mask, name| {
                    Ok(mask
                        | FluidKind::from_name(name)
                            .ok_or_else(|| invalid("unknown survival fluid tag member"))?
                            .mask())
                })
        };
        Ok(Self {
            registry,
            support,
            collision,
            solid: properties.bind(BlockProperty::Solid),
            liquid: properties.bind(BlockProperty::Liquid),
            solid_render: properties.bind(BlockProperty::SolidRender),
            flammable: (0..registry.len())
                .map(|i| BlockStateId(i as u32))
                .filter(|id| {
                    let state = registry.state(*id).unwrap();
                    definitions
                        .ignition_odds
                        .get(&state.name)
                        .is_some_and(|value| *value > 0)
                        && state
                            .properties
                            .get("waterlogged")
                            .is_none_or(|value| value != "true")
                })
                .collect(),
            fence_gates: (0..registry.len())
                .map(|i| BlockStateId(i as u32))
                .filter(|id| {
                    definitions
                        .fence_gates
                        .contains(&registry.state(*id).unwrap().name)
                })
                .collect(),
            cactus_support: tag_states("minecraft:supports_cactus")?,
            mushroom_light_override: tag_states("minecraft:overrides_mushroom_light_requirement")?,
            rules,
            unstable_bottom_center: tag_states("minecraft:unstable_bottom_center")?,
            supports_bamboo: tag_states("minecraft:supports_bamboo")?,
            bound_tags: tag_names
                .iter()
                .map(|name| tag_states(name))
                .collect::<io::Result<_>>()?,
            cactus_flower: tag_states("minecraft:support_override_cactus_flower")?,
            no_seagrass: tag_states("minecraft:cannot_support_seagrass")?,
            vegetation: tag_states("minecraft:supports_vegetation")?,
            dripleaf: tag_states("minecraft:supports_big_dripleaf")?,
            maintains_farmland: tag_states("minecraft:maintains_farmland")?,
            cocoa: tag_states("minecraft:supports_cocoa")?,
            crops: tag_states("minecraft:supports_crops")?,
            no_snow: tag_states("minecraft:cannot_support_snow_layer")?,
            snow_override: tag_states("minecraft:support_override_snow_layer")?,
            mangrove: tag_states("minecraft:supports_mangrove_propagule")?,
            hanging_mangrove: tag_states("minecraft:supports_hanging_mangrove_propagule")?,
            no_kelp: tag_states("minecraft:cannot_support_kelp")?,
            chorus_plant: tag_states("minecraft:supports_chorus_plant")?,
            chorus_flower: tag_states("minecraft:supports_chorus_flower")?,
            lily_pad: tag_states("minecraft:supports_lily_pad")?,
            frogspawn: tag_states("minecraft:supports_frogspawn")?,
            small_dripleaf: tag_states("minecraft:supports_small_dripleaf")?,
            sugar_cane: tag_states("minecraft:supports_sugar_cane")?,
            sugar_cane_adjacent: tag_states("minecraft:supports_sugar_cane_adjacently")?,
            fluid_tags: [
                fluid_tag("minecraft:water")?,
                fluid_tag("minecraft:supports_lily_pad")?,
                fluid_tag("minecraft:supports_frogspawn")?,
                fluid_tag("minecraft:supports_sugar_cane_adjacently")?,
                fluid_tag("minecraft:lava")?,
            ],
            bubble_up: tag_states("minecraft:enables_bubble_column_push_up")?,
            bubble_down: tag_states("minecraft:enables_bubble_column_drag_down")?,
        })
    }
    fn read<W: SurvivalWorld>(
        &self,
        world: &mut W,
        position: [i32; 3],
    ) -> Result<BlockStateId, SurvivalError<W::Error>> {
        let state = world.block_state(position).map_err(SurvivalError::World)?;
        if self.registry.state(state).is_none() {
            return Err(SurvivalError::Registry(invalid(
                "world survival state outside registry",
            )));
        }
        Ok(state)
    }
    fn support<W: SurvivalWorld>(
        &self,
        world: &mut W,
        position: [i32; 3],
        face: Direction,
        kind: SupportType,
        center_restriction: bool,
    ) -> Result<bool, SurvivalError<W::Error>> {
        let state = self.read(world, position)?;
        if center_restriction
            && face == Direction::Down
            && self.unstable_bottom_center.contains(&state)
        {
            return Ok(false);
        }
        self.support
            .is_face_sturdy(world, state, position, face, kind)
            .map_err(SurvivalError::Support)
    }
    fn is_air(&self, state: BlockStateId) -> bool {
        matches!(
            self.registry.state(state).unwrap().name.as_str(),
            "minecraft:air" | "minecraft:cave_air" | "minecraft:void_air"
        )
    }
    fn face_property(&self, state: BlockStateId, direction: Direction) -> bool {
        self.registry
            .state(state)
            .unwrap()
            .properties
            .get(direction_name(direction))
            .is_some_and(|value| value == "true")
    }
    /// Native multiface attachment at a candidate position, testing support
    /// first and then empty-context collision on the neighbor's opposite face.
    pub fn can_attach_to<W: SurvivalWorld>(
        &self,
        world: &mut W,
        position: [i32; 3],
        direction: Direction,
    ) -> Result<bool, SurvivalError<W::Error>> {
        let step = direction.normal();
        let at = std::array::from_fn(|i| position[i].wrapping_add(step[i]));
        let neighbor = self.read(world, at)?;
        self.can_attach(world, neighbor, at, direction)
    }
    fn can_attach<W: SurvivalWorld>(
        &self,
        world: &mut W,
        neighbor: BlockStateId,
        at: [i32; 3],
        direction: Direction,
    ) -> Result<bool, SurvivalError<W::Error>> {
        let face = direction.opposite();
        if self
            .support
            .is_face_sturdy(world, neighbor, at, face, SupportType::Full)
            .map_err(SurvivalError::Support)?
        {
            return Ok(true);
        }
        let shape = self
            .collision
            .collision_shape(world, neighbor, at)
            .map_err(SurvivalError::Collision)?;
        SupportType::Full
            .is_supporting(&shape, face)
            .map_err(|error| SurvivalError::Registry(invalid(error.to_string())))
    }
    fn is_named(&self, state: BlockStateId, name: &str) -> bool {
        self.registry.state(state).unwrap().name == name
    }
    fn same_block(&self, first: BlockStateId, second: BlockStateId) -> bool {
        self.registry.state(first).unwrap().name == self.registry.state(second).unwrap().name
    }
    fn scaffold_distance(&self, state: BlockStateId) -> io::Result<Option<i32>> {
        if !self.is_named(state, "minecraft:scaffolding") {
            return Ok(None);
        }
        self.registry
            .state(state)
            .unwrap()
            .properties
            .get("distance")
            .and_then(|v| v.parse::<i32>().ok())
            .filter(|v| (0..=7).contains(v))
            .map(Some)
            .ok_or_else(|| invalid("invalid scaffolding distance"))
    }
    fn is_lower_half(&self, below: BlockStateId, candidate: BlockStateId) -> bool {
        let neighbor = self.registry.state(below).unwrap();
        neighbor.name == self.registry.state(candidate).unwrap().name
            && neighbor
                .properties
                .get("half")
                .is_some_and(|v| v == "lower")
    }
    fn fluid_matches(&self, tag: usize, fluid: FluidState) -> bool {
        self.fluid_tags[tag] & fluid.kind().mask() != 0
    }
    pub fn can_survive<W: SurvivalWorld>(
        &self,
        world: &mut W,
        state: BlockStateId,
        position: [i32; 3],
    ) -> Result<bool, SurvivalError<W::Error>> {
        let rule = self
            .rules
            .get(state.0 as usize)
            .ok_or_else(|| SurvivalError::Registry(invalid("survival candidate outside registry")))?
            .ok_or(SurvivalError::Unsupported(state))?;
        match rule {
            Rule::Fire => {
                let below = relative(position, Direction::Down);
                let neighbor = self.read(world, below)?;
                if self
                    .support
                    .is_face_sturdy(world, neighbor, below, Direction::Up, SupportType::Full)
                    .map_err(SurvivalError::Support)?
                {
                    return Ok(true);
                }
                for direction in Direction::ALL {
                    let neighbor = self.read(world, relative(position, direction))?;
                    if self.flammable.contains(&neighbor) {
                        return Ok(true);
                    }
                }
                Ok(false)
            }
            Rule::Cactus => {
                for direction in [
                    Direction::North,
                    Direction::East,
                    Direction::South,
                    Direction::West,
                ] {
                    let at = relative(position, direction);
                    let neighbor = self.read(world, at)?;
                    if self.solid.contains(neighbor)
                        || self
                            .fluid_matches(4, world.fluid_state(at).map_err(SurvivalError::World)?)
                    {
                        return Ok(false);
                    }
                }
                let neighbor = self.read(world, relative(position, Direction::Down))?;
                if !self.same_block(neighbor, state) && !self.cactus_support.contains(&neighbor) {
                    return Ok(false);
                }
                let above = self.read(world, relative(position, Direction::Up))?;
                Ok(!self.liquid.contains(above))
            }
            Rule::Path => {
                let above = self.read(world, relative(position, Direction::Up))?;
                Ok(!self.solid.contains(above) || self.fence_gates.contains(&above))
            }
            Rule::Mushroom => {
                let below = self.read(world, relative(position, Direction::Down))?;
                if self.mushroom_light_override.contains(&below) {
                    return Ok(true);
                }
                Ok(world
                    .raw_brightness(position, 0)
                    .map_err(SurvivalError::World)?
                    < 13
                    && self.solid_render.contains(below))
            }
            Rule::Multiface(faces) => {
                for direction in Direction::ALL {
                    if faces & (1 << direction as u8) != 0 {
                        let at = relative(position, direction);
                        let neighbor = self.read(world, at)?;
                        if !self.can_attach(world, neighbor, at, direction)? {
                            return Ok(false);
                        }
                    }
                }
                Ok(faces != 0)
            }
            Rule::HangingMoss => {
                let at = relative(position, Direction::Up);
                let neighbor = self.read(world, at)?;
                Ok(self.can_attach(world, neighbor, at, Direction::Up)?
                    || self.same_block(neighbor, state))
            }
            Rule::Vine(faces) => {
                let above = relative(position, Direction::Up);
                let mut any = false;
                let mut cached_above = None;
                if faces & (1 << Direction::Up as u8) != 0 {
                    let neighbor = self.read(world, above)?;
                    // getUpdatedState passes DOWN here, unlike canSupportAtFace.
                    any = self.can_attach(world, neighbor, above, Direction::Down)?;
                }
                for direction in [
                    Direction::North,
                    Direction::East,
                    Direction::South,
                    Direction::West,
                ] {
                    if faces & (1 << direction as u8) == 0 {
                        continue;
                    }
                    let at = relative(position, direction);
                    let neighbor = self.read(world, at)?;
                    let mut valid = self.can_attach(world, neighbor, at, direction)?;
                    if !valid {
                        let neighbor = self.read(world, above)?;
                        valid = self.same_block(neighbor, state)
                            && self.face_property(neighbor, direction);
                    }
                    if !valid {
                        let neighbor = match cached_above {
                            Some(neighbor) => neighbor,
                            None => {
                                let neighbor = self.read(world, above)?;
                                cached_above = Some(neighbor);
                                neighbor
                            }
                        };
                        valid = self.same_block(neighbor, state)
                            && self.face_property(neighbor, direction);
                    }
                    any |= valid;
                }
                Ok(any)
            }
            Rule::SnowLayer => {
                let at = relative(position, Direction::Down);
                let neighbor = self.read(world, at)?;
                if self.no_snow.contains(&neighbor) {
                    return Ok(false);
                }
                if self.snow_override.contains(&neighbor) {
                    return Ok(true);
                }
                let shape = self
                    .collision
                    .collision_shape(world, neighbor, at)
                    .map_err(SurvivalError::Collision)?;
                Ok(SupportType::Full
                    .is_supporting(&shape, Direction::Up)
                    .map_err(|error| SurvivalError::Registry(invalid(error.to_string())))?
                    || (self.same_block(neighbor, state)
                        && self
                            .registry
                            .state(neighbor)
                            .unwrap()
                            .properties
                            .get("layers")
                            .is_some_and(|value| value == "8")))
            }
            Rule::SeaPickle => {
                let at = relative(position, Direction::Down);
                let neighbor = self.read(world, at)?;
                let shape = self
                    .collision
                    .collision_shape(world, neighbor, at)
                    .map_err(SurvivalError::Collision)?;
                Ok(!shape
                    .face_shape(Direction::Up)
                    .map_err(|error| SurvivalError::Registry(invalid(error.to_string())))?
                    .is_empty()
                    || self
                        .support
                        .is_face_sturdy(world, neighbor, at, Direction::Up, SupportType::Full)
                        .map_err(SurvivalError::Support)?)
            }
            Rule::GrowingPlant {
                head,
                body,
                direction,
                kelp,
            } => {
                let at = relative(position, direction.opposite());
                let neighbor = self.read(world, at)?;
                if kelp && self.no_kelp.contains(&neighbor) {
                    return Ok(false);
                }
                Ok(self.same_block(neighbor, head)
                    || self.same_block(neighbor, body)
                    || self
                        .support
                        .is_face_sturdy(world, neighbor, at, direction, SupportType::Full)
                        .map_err(SurvivalError::Support)?)
            }
            Rule::MangrovePropagule(hanging) => {
                let neighbor = self.read(
                    world,
                    relative(
                        position,
                        if hanging {
                            Direction::Up
                        } else {
                            Direction::Down
                        },
                    ),
                )?;
                Ok(if hanging {
                    self.hanging_mangrove.contains(&neighbor)
                } else {
                    self.mangrove.contains(&neighbor)
                })
            }
            Rule::Speleothem(direction) => {
                let at = relative(position, direction.opposite());
                let neighbor = self.read(world, at)?;
                Ok(self
                    .support
                    .is_face_sturdy(world, neighbor, at, direction, SupportType::Full)
                    .map_err(SurvivalError::Support)?
                    || (self.same_block(neighbor, state)
                        && self
                            .registry
                            .state(neighbor)
                            .unwrap()
                            .properties
                            .get("vertical_direction")
                            == self
                                .registry
                                .state(state)
                                .unwrap()
                                .properties
                                .get("vertical_direction")))
            }
            Rule::Scaffolding => {
                let below = relative(position, Direction::Down);
                let neighbor = self.read(world, below)?;
                let mut distance = if let Some(distance) = self
                    .scaffold_distance(neighbor)
                    .map_err(SurvivalError::Registry)?
                {
                    distance
                } else {
                    if self
                        .support
                        .is_face_sturdy(world, neighbor, below, Direction::Up, SupportType::Full)
                        .map_err(SurvivalError::Support)?
                    {
                        return Ok(true);
                    }
                    7
                };
                for direction in [
                    Direction::North,
                    Direction::East,
                    Direction::South,
                    Direction::West,
                ] {
                    let neighbor = self.read(world, relative(position, direction))?;
                    if let Some(adjacent) = self
                        .scaffold_distance(neighbor)
                        .map_err(SurvivalError::Registry)?
                    {
                        distance = distance.min(adjacent + 1);
                        if distance == 1 {
                            break;
                        }
                    }
                }
                Ok(distance < 7)
            }
            Rule::PistonHead { direction, sticky } => {
                let neighbor = self.read(world, relative(position, direction.opposite()))?;
                let definition = self.registry.state(neighbor).unwrap();
                let base = if sticky {
                    "minecraft:sticky_piston"
                } else {
                    "minecraft:piston"
                };
                Ok(((definition.name == base
                    && definition
                        .properties
                        .get("extended")
                        .is_some_and(|v| v == "true"))
                    || definition.name == "minecraft:moving_piston")
                    && definition.properties.get("facing")
                        == self.registry.state(state).unwrap().properties.get("facing"))
            }
            Rule::ChorusPlant => {
                let below = self.read(world, relative(position, Direction::Down))?;
                let above = self.read(world, relative(position, Direction::Up))?;
                let crowded = !self.is_air(above) && !self.is_air(below);
                for direction in [
                    Direction::North,
                    Direction::East,
                    Direction::South,
                    Direction::West,
                ] {
                    let at = relative(position, direction);
                    let neighbor = self.read(world, at)?;
                    if self.same_block(neighbor, state) {
                        if crowded {
                            return Ok(false);
                        }
                        let support = self.read(world, relative(at, Direction::Down))?;
                        if self.same_block(support, state) || self.chorus_plant.contains(&support) {
                            return Ok(true);
                        }
                    }
                }
                Ok(self.same_block(below, state) || self.chorus_plant.contains(&below))
            }
            Rule::ChorusFlower => {
                let below = self.read(world, relative(position, Direction::Down))?;
                if self.is_named(below, "minecraft:chorus_plant")
                    || self.chorus_flower.contains(&below)
                {
                    return Ok(true);
                }
                if !self.is_air(below) {
                    return Ok(false);
                }
                let mut found = false;
                for direction in [
                    Direction::North,
                    Direction::East,
                    Direction::South,
                    Direction::West,
                ] {
                    let neighbor = self.read(world, relative(position, direction))?;
                    if self.is_named(neighbor, "minecraft:chorus_plant") {
                        if found {
                            return Ok(false);
                        }
                        found = true;
                    } else if !self.is_air(neighbor) {
                        return Ok(false);
                    }
                }
                Ok(found)
            }
            Rule::PitcherCrop(upper) => {
                if !upper
                    && world
                        .raw_brightness(position, 0)
                        .map_err(SurvivalError::World)?
                        < 8
                {
                    return Ok(false);
                }
                let below = self.read(world, relative(position, Direction::Down))?;
                Ok(if upper {
                    self.is_lower_half(below, state)
                } else {
                    self.crops.contains(&below)
                })
            }
            Rule::SugarCane => {
                let below = relative(position, Direction::Down);
                let neighbor = self.read(world, below)?;
                if self.registry.state(neighbor).unwrap().name
                    == self.registry.state(state).unwrap().name
                {
                    return Ok(true);
                }
                if !self.sugar_cane.contains(&neighbor) {
                    return Ok(false);
                }
                for direction in [
                    Direction::North,
                    Direction::East,
                    Direction::South,
                    Direction::West,
                ] {
                    let at = relative(below, direction);
                    let adjacent = self.read(world, at)?;
                    let fluid = world.fluid_state(at).map_err(SurvivalError::World)?;
                    if self.fluid_matches(3, fluid) || self.sugar_cane_adjacent.contains(&adjacent)
                    {
                        return Ok(true);
                    }
                }
                Ok(false)
            }
            Rule::LilyPad => {
                let below = relative(position, Direction::Down);
                let neighbor = self.read(world, below)?;
                let fluid_below = world.fluid_state(below).map_err(SurvivalError::World)?;
                let fluid_here = world.fluid_state(position).map_err(SurvivalError::World)?;
                Ok(
                    (self.fluid_matches(1, fluid_below) || self.lily_pad.contains(&neighbor))
                        && fluid_here.kind() == FluidKind::Empty,
                )
            }
            Rule::Frogspawn => {
                let below = relative(position, Direction::Down);
                let fluid_below = world.fluid_state(below).map_err(SurvivalError::World)?;
                let fluid_here = world.fluid_state(position).map_err(SurvivalError::World)?;
                Ok((self.fluid_matches(2, fluid_below)
                    || self.frogspawn.contains(&self.read(world, below)?))
                    && fluid_here.kind() == FluidKind::Empty)
            }
            Rule::SporeBlossom => Ok(self.support(
                world,
                relative(position, Direction::Up),
                Direction::Down,
                SupportType::Center,
                true,
            )? && !self.fluid_matches(
                0,
                world.fluid_state(position).map_err(SurvivalError::World)?,
            )),
            Rule::SmallDripleaf(upper) => {
                let neighbor = self.read(world, relative(position, Direction::Down))?;
                if upper {
                    return Ok(self.is_lower_half(neighbor, state));
                }
                Ok(self.small_dripleaf.contains(&neighbor)
                    || (world
                        .fluid_state(position)
                        .map_err(SurvivalError::World)?
                        .kind()
                        == FluidKind::Water
                        && self.vegetation.contains(&neighbor)))
            }
            Rule::TallSeagrass(upper) => {
                let below = relative(position, Direction::Down);
                if upper {
                    return Ok(self.is_lower_half(self.read(world, below)?, state));
                }
                let fluid = world.fluid_state(position).map_err(SurvivalError::World)?;
                let neighbor = self.read(world, below)?;
                Ok(self
                    .support
                    .is_face_sturdy(world, neighbor, below, Direction::Up, SupportType::Full)
                    .map_err(SurvivalError::Support)?
                    && !self.no_seagrass.contains(&neighbor)
                    && self.fluid_matches(0, fluid)
                    && fluid.is_full())
            }
            Rule::BigDripleaf => {
                let below = self.read(world, relative(position, Direction::Down))?;
                Ok(matches!(
                    self.registry.state(below).unwrap().name.as_str(),
                    "minecraft:big_dripleaf" | "minecraft:big_dripleaf_stem"
                ) || self.dripleaf.contains(&below))
            }
            Rule::BigDripleafStem => {
                let below = self.read(world, relative(position, Direction::Down))?;
                let above = self.read(world, relative(position, Direction::Up))?;
                Ok(
                    (self.registry.state(below).unwrap().name == "minecraft:big_dripleaf_stem"
                        || self.dripleaf.contains(&below))
                        && matches!(
                            self.registry.state(above).unwrap().name.as_str(),
                            "minecraft:big_dripleaf" | "minecraft:big_dripleaf_stem"
                        ),
                )
            }
            Rule::Farmland => {
                let above = relative(position, Direction::Up);
                Ok(!self.solid.contains(self.read(world, above)?)
                    || self.maintains_farmland.contains(&self.read(world, above)?))
            }
            Rule::Cocoa(direction) => Ok(self
                .cocoa
                .contains(&self.read(world, relative(position, direction))?)),
            Rule::Crop => Ok(world
                .raw_brightness(position, 0)
                .map_err(SurvivalError::World)?
                >= 8
                && self
                    .crops
                    .contains(&self.read(world, relative(position, Direction::Down))?)),
            Rule::DoublePlant(upper) => {
                let below = self.read(world, relative(position, Direction::Down))?;
                if !upper {
                    return Ok(self.vegetation.contains(&below));
                }
                let neighbor = self.registry.state(below).unwrap();
                Ok(neighbor.name == self.registry.state(state).unwrap().name
                    && neighbor
                        .properties
                        .get("half")
                        .is_some_and(|v| v == "lower"))
            }
            Rule::Door(upper) => {
                let below = relative(position, Direction::Down);
                let neighbor = self.read(world, below)?;
                if upper {
                    return Ok(self.registry.state(neighbor).unwrap().name
                        == self.registry.state(state).unwrap().name);
                }
                self.support
                    .is_face_sturdy(world, neighbor, below, Direction::Up, SupportType::Full)
                    .map_err(SurvivalError::Support)
            }
            Rule::TagBelow(index) => Ok(self.bound_tags[index]
                .contains(&self.read(world, relative(position, Direction::Down))?)),
            Rule::CactusFlower => {
                let below = relative(position, Direction::Down);
                self.read(world, below)?;
                let neighbor = self.read(world, below)?;
                Ok(self.cactus_flower.contains(&neighbor)
                    || self
                        .support
                        .is_face_sturdy(world, neighbor, below, Direction::Up, SupportType::Center)
                        .map_err(SurvivalError::Support)?)
            }
            Rule::Seagrass => {
                let below = relative(position, Direction::Down);
                let neighbor = self.read(world, below)?;
                Ok(self
                    .support
                    .is_face_sturdy(world, neighbor, below, Direction::Up, SupportType::Full)
                    .map_err(SurvivalError::Support)?
                    && !self.no_seagrass.contains(&neighbor))
            }
            Rule::BubbleColumn => {
                let below = self.read(world, relative(position, Direction::Down))?;
                Ok(self.registry.state(below).unwrap().name
                    == self.registry.state(state).unwrap().name
                    || self.bubble_up.contains(&below)
                    || self.bubble_down.contains(&below))
            }
            Rule::RedstoneWire => {
                let below = relative(position, Direction::Down);
                let neighbor = self.read(world, below)?;
                Ok(self
                    .support
                    .is_face_sturdy(world, neighbor, below, Direction::Up, SupportType::Full)
                    .map_err(SurvivalError::Support)?
                    || self.registry.state(neighbor).unwrap().name == "minecraft:hopper")
            }
            Rule::Always => Ok(true),
            Rule::Solid(direction) => Ok(self
                .solid
                .contains(self.read(world, relative(position, direction))?)),
            Rule::NonAirBelow => {
                Ok(!self.is_air(self.read(world, relative(position, Direction::Down))?))
            }
            Rule::MossCarpet(base) => {
                let below = self.read(world, relative(position, Direction::Down))?;
                if base {
                    return Ok(!self.is_air(below));
                }
                let below = self.registry.state(below).unwrap();
                Ok(below.name == self.registry.state(state).unwrap().name
                    && below.properties.get("bottom").is_some_and(|v| v == "true"))
            }
            Rule::Support {
                face,
                kind,
                center_restriction,
            } => self.support(
                world,
                relative(position, face.opposite()),
                face,
                kind,
                center_restriction,
            ),
            Rule::RigidOrCenterBelow => {
                let below = relative(position, Direction::Down);
                Ok(
                    self.support(world, below, Direction::Up, SupportType::Rigid, false)?
                        || self.support(world, below, Direction::Up, SupportType::Center, true)?,
                )
            }
            Rule::SupportsBamboo => Ok(self
                .supports_bamboo
                .contains(&self.read(world, relative(position, Direction::Down))?)),
        }
    }
}
fn relative(position: [i32; 3], direction: Direction) -> [i32; 3] {
    let normal = direction.normal();
    std::array::from_fn(|i| position[i].wrapping_add(normal[i]))
}

fn direction_name(direction: Direction) -> &'static str {
    match direction {
        Direction::Down => "down",
        Direction::Up => "up",
        Direction::North => "north",
        Direction::South => "south",
        Direction::West => "west",
        Direction::East => "east",
    }
}
