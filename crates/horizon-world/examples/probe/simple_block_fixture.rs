//! Ordered feature effects over a mutable registry-bound world.
use horizon_core::{
    geometry::{Direction, VoxelShape},
    random::{LegacyRandom, RandomSource, XoroshiroRandom},
};
use horizon_world::{
    block_collision::{BlockCollisionRegistry, DynamicCollisionWorld, ShulkerCollisionState},
    block_placement::BlockPlacementBindings,
    block_properties::BlockPropertiesRegistry,
    block_support::{BlockSupportRegistry, SupportType},
    block_survival::{BlockSurvivalRegistry, SurvivalDefinitions, SurvivalRule, SurvivalWorld},
    collision::CachedCollisionRegistry,
    dynamic_support::DynamicSupportWorld,
    fluid::{FluidKind, FluidState, FluidStateTable},
    heightmap::HeightmapType,
    nbt::{self, Limits, Tag},
    registry::{BlockRegistry, BlockStateId},
    worldgen::{
        anchor::WorldGenerationContext,
        biome::BiomeId,
        biome_temperature::{BiomeTemperature, BiomeTemperatureSettings, TemperatureModifier},
        biome_weather::BiomeWeatherBindings,
        block_feature::FeatureBlockBindings,
        feature::{FeatureBlocks, FeatureWorld},
        feature_catalog::FeatureCatalog,
        feature_graph::{FeatureDefinition, PlacedFeatureDefinition},
        placement::{PlacementContext, PlacementQueryError},
        simple_block::SimpleBlockFeature,
        state_provider::{StateProviderBindings, StateProviderCatalog},
        tags::TagDefinitions,
    },
};
use serde_json::Value;
use std::{
    collections::BTreeMap,
    fs,
    io::{self, BufRead, Write},
    path::Path,
    sync::Arc,
};
struct World<'a, 'r, W, R> {
    out: &'a mut W,
    random: R,
    states: BTreeMap<[i32; 3], BlockStateId>,
    fallback: BlockStateId,
    fluid: Option<BlockStateId>,
    fluids: &'a FluidStateTable,
    water_mask: horizon_world::fluid::FluidMask,
    survival: &'a BlockSurvivalRegistry<'r>,
    support: &'a BlockSupportRegistry<'r>,
    collision: &'a BlockCollisionRegistry<'r>,
    loot_container: bool,
    loot: BTreeMap<[i32; 3], (String, i64)>,
    calls: i32,
    fail: i32,
    reject: i32,
    brightness: i32,
    sea_level: i32,
    surface_height: i32,
    air_above: Option<i32>,
    biome: Option<(BiomeWeatherBindings, BiomeTemperature)>,
    air: BlockStateId,
    destroy_success: bool,
    gateway_entity: bool,
    gateways: BTreeMap<[i32; 3], ([i32; 3], bool)>,
}
impl<W: Write, R> World<'_, '_, W, R> {
    fn event(&mut self, text: std::fmt::Arguments<'_>) -> io::Result<i32> {
        writeln!(self.out, "{text}")?;
        let call = self.calls;
        self.calls += 1;
        if call == self.fail {
            return Err(io::Error::other("injected world failure"));
        }
        Ok(call)
    }
    fn at(&self, p: [i32; 3]) -> BlockStateId {
        self.states.get(&p).copied().unwrap_or_else(|| {
            if self.air_above.is_some_and(|y| p[1] > y) {
                self.air
            } else {
                self.fallback
            }
        })
    }
    fn get(&mut self, p: [i32; 3]) -> io::Result<BlockStateId> {
        self.event(format_args!("get {} {} {}", p[0], p[1], p[2]))?;
        let state = self.at(p);
        writeln!(self.out, "state {}", state.0)?;
        Ok(state)
    }
    fn entity(&mut self, p: [i32; 3]) -> io::Result<()> {
        self.event(format_args!("entity {} {} {}", p[0], p[1], p[2]))?;
        Ok(())
    }
}
impl<W: Write, R> DynamicSupportWorld for World<'_, '_, W, R> {
    type Error = io::Error;
    fn shulker_is_closed(&mut self, p: [i32; 3]) -> io::Result<Option<bool>> {
        self.entity(p)?;
        Ok(None)
    }
    fn moving_piston_shape(&mut self, p: [i32; 3]) -> io::Result<Option<VoxelShape>> {
        self.entity(p)?;
        Ok(None)
    }
}
impl<W: Write, R> DynamicCollisionWorld for World<'_, '_, W, R> {
    fn shulker_collision_state(
        &mut self,
        p: [i32; 3],
    ) -> io::Result<Option<ShulkerCollisionState>> {
        self.entity(p)?;
        Ok(None)
    }
}
impl<W: Write, R> SurvivalWorld for World<'_, '_, W, R> {
    fn block_state(&mut self, p: [i32; 3]) -> io::Result<BlockStateId> {
        self.get(p)
    }
    fn fluid_state(&mut self, p: [i32; 3]) -> io::Result<FluidState> {
        self.event(format_args!("fluid {} {} {}", p[0], p[1], p[2]))?;
        self.fluids
            .get(self.fluid.unwrap_or_else(|| self.at(p)))
            .ok_or_else(|| io::Error::other("unknown fluid"))
    }
    fn raw_brightness(&mut self, p: [i32; 3], sky_darken: i32) -> io::Result<i32> {
        self.event(format_args!(
            "light {} {} {} {sky_darken}",
            p[0], p[1], p[2]
        ))?;
        Ok(self.brightness)
    }
}
impl<W: Write, R> FeatureWorld for World<'_, '_, W, R> {
    type Error = io::Error;
    fn block(&mut self, p: [i32; 3]) -> io::Result<BlockStateId> {
        self.get(p)
    }
    fn set_block(&mut self, p: [i32; 3], state: BlockStateId, flags: i32) -> io::Result<bool> {
        let call = self.event(format_args!(
            "set {} {} {} {} {flags}",
            p[0], p[1], p[2], state.0
        ))?;
        let result = self.reject == 0 || call % self.reject != 0;
        if result {
            self.states.insert(p, state);
        }
        writeln!(self.out, "written {result}")?;
        Ok(result)
    }
}
fn query(error: impl std::fmt::Display) -> PlacementQueryError {
    PlacementQueryError::Failed(error.to_string())
}
impl<W: Write, R: RandomSource> World<'_, '_, W, R> {
    fn weather(
        &mut self,
        biome: BiomeId,
        p: [i32; 3],
        snow: bool,
        edge: bool,
    ) -> Result<bool, PlacementQueryError> {
        if biome != BiomeId(0) {
            return Err(PlacementQueryError::Unavailable);
        }
        let (bindings, mut temperature) =
            self.biome.take().ok_or(PlacementQueryError::Unavailable)?;
        let result = if snow {
            bindings.should_snow(self, &mut temperature, p)
        } else {
            bindings.should_freeze(self, &mut temperature, p, edge)
        };
        self.biome = Some((bindings, temperature));
        result
    }
}
impl<W: Write, R: RandomSource> PlacementContext for World<'_, '_, W, R> {
    fn biome_identity(&mut self, p: [i32; 3]) -> Result<BiomeId, PlacementQueryError> {
        self.event(format_args!("biome {} {} {}", p[0], p[1], p[2]))
            .map_err(query)?;
        if self.biome.is_none() {
            return Err(PlacementQueryError::Unavailable);
        }
        Ok(BiomeId(0))
    }
    fn state_collision_empty(
        &mut self,
        state: BlockStateId,
        p: [i32; 3],
    ) -> Result<bool, PlacementQueryError> {
        let collision = self.collision;
        collision
            .collision_shape(self, state, p)
            .map(|shape| shape.is_empty())
            .map_err(query)
    }
    fn set_block_entity_loot_table(
        &mut self,
        p: [i32; 3],
        table: &str,
        random: &mut impl RandomSource,
    ) -> Result<(), PlacementQueryError> {
        self.event(format_args!("entity {} {} {}", p[0], p[1], p[2]))
            .map_err(query)?;
        if self.loot_container {
            self.loot.insert(p, (table.into(), random.next_long()));
        }
        Ok(())
    }
    fn configure_end_gateway(
        &mut self,
        p: [i32; 3],
        exit: [i32; 3],
        exact: bool,
    ) -> Result<(), PlacementQueryError> {
        self.event(format_args!("entity {} {} {}", p[0], p[1], p[2]))
            .map_err(query)?;
        if self.gateway_entity {
            self.gateways.insert(p, (exit, exact));
            self.event(format_args!("gateway_changed {} {} {}", p[0], p[1], p[2]))
                .map_err(query)?;
        }
        Ok(())
    }
    fn biome_should_freeze(
        &mut self,
        biome: BiomeId,
        p: [i32; 3],
        edge: bool,
    ) -> Result<bool, PlacementQueryError> {
        self.weather(biome, p, false, edge)
    }
    fn biome_should_snow(
        &mut self,
        biome: BiomeId,
        p: [i32; 3],
    ) -> Result<bool, PlacementQueryError> {
        self.weather(biome, p, true, false)
    }
    fn destroy_block_without_entity(
        &mut self,
        p: [i32; 3],
        drops: bool,
    ) -> Result<bool, PlacementQueryError> {
        self.event(format_args!("destroy {} {} {} {drops}", p[0], p[1], p[2]))
            .map_err(query)?;
        if self.destroy_success {
            self.states.insert(p, self.air);
        }
        writeln!(self.out, "destroyed {}", self.destroy_success).map_err(query)?;
        Ok(self.destroy_success)
    }
    fn block_brightness(&mut self, p: [i32; 3]) -> Result<i32, PlacementQueryError> {
        self.event(format_args!("blocklight {} {} {}", p[0], p[1], p[2]))
            .map_err(query)?;
        Ok(self.brightness)
    }
    fn fluid_state(&mut self, p: [i32; 3]) -> Result<FluidState, PlacementQueryError> {
        SurvivalWorld::fluid_state(self, p).map_err(query)
    }
    fn sea_level(&mut self) -> Result<i32, PlacementQueryError> {
        Ok(self.sea_level)
    }

    fn schedule_fluid_tick(
        &mut self,
        p: [i32; 3],
        fluid: horizon_world::fluid::FluidKind,
        delay: i32,
    ) -> Result<(), PlacementQueryError> {
        self.event(format_args!(
            "fluidtick {} {} {} {} {delay}",
            p[0],
            p[1],
            p[2],
            fluid.name()
        ))
        .map_err(query)?;
        Ok(())
    }
    fn mark_postprocessing(&mut self, p: [i32; 3]) -> Result<(), PlacementQueryError> {
        self.event(format_args!("chunk {} {} {}", p[0], p[1], p[2]))
            .map_err(query)?;
        self.event(format_args!("post {} {} {}", p[0], p[1], p[2]))
            .map_err(query)?;
        Ok(())
    }
    fn water_at(&mut self, p: [i32; 3]) -> Result<bool, PlacementQueryError> {
        Ok(SurvivalWorld::fluid_state(self, p)
            .map_err(query)?
            .kind()
            .mask()
            & self.water_mask
            != 0)
    }
    fn attachment_at(
        &mut self,
        p: [i32; 3],
        direction: Direction,
    ) -> Result<bool, PlacementQueryError> {
        let survival = self.survival;
        survival.can_attach_to(self, p, direction).map_err(query)
    }
    fn next_world_boolean(&mut self) -> Result<bool, PlacementQueryError> {
        Ok(self.random.next_boolean())
    }
    fn schedule_block_tick(
        &mut self,
        p: [i32; 3],
        block: BlockStateId,
        delay: i32,
    ) -> Result<(), PlacementQueryError> {
        self.event(format_args!(
            "tick {} {} {} {} {delay}",
            p[0], p[1], p[2], block.0
        ))
        .map_err(query)?;
        Ok(())
    }
    fn generation(&self) -> WorldGenerationContext {
        WorldGenerationContext::new(-64, 384, 63)
    }
    fn min_y(&mut self) -> Result<i32, PlacementQueryError> {
        Ok(-64)
    }
    fn max_y(&mut self) -> Result<i32, PlacementQueryError> {
        Ok(319)
    }
    fn height(&mut self, kind: HeightmapType, x: i32, z: i32) -> Result<i32, PlacementQueryError> {
        self.event(format_args!("height {} {x} {z}", kind.name()))
            .map_err(query)?;
        Ok(self.surface_height)
    }
    fn block_state(&mut self, p: [i32; 3]) -> Result<BlockStateId, PlacementQueryError> {
        self.get(p).map_err(query)
    }
    fn block_survives(
        &mut self,
        state: BlockStateId,
        p: [i32; 3],
    ) -> Result<bool, PlacementQueryError> {
        let survival = self.survival;
        survival.can_survive(self, state, p).map_err(query)
    }
    fn face_sturdy(
        &mut self,
        p: [i32; 3],
        direction: Direction,
    ) -> Result<bool, PlacementQueryError> {
        let state = self.get(p).map_err(query)?;
        self.state_face_sturdy(state, p, direction)
    }
    fn state_face_sturdy(
        &mut self,
        state: BlockStateId,
        p: [i32; 3],
        direction: Direction,
    ) -> Result<bool, PlacementQueryError> {
        let support = self.support;
        support
            .is_face_sturdy(self, state, p, direction, SupportType::Full)
            .map_err(query)
    }
}
struct WeatherProbe {
    bindings: BiomeWeatherBindings,
    temperature: BiomeTemperature,
    snow: bool,
    require_edge: bool,
}
enum Feature {
    Weather(Box<WeatherProbe>),
    Map(SimpleBlockFeature),
    Graph(Box<FeatureDefinition>),
    Placed(Box<PlacedFeatureDefinition>),
}
#[allow(clippy::too_many_arguments)]
fn fixture<R: RandomSource>(
    f: &Value,
    mut random: R,
    world_random: R,
    providers: &StateProviderBindings<'_>,
    catalog: &StateProviderCatalog,
    placement: &BlockPlacementBindings<'_>,
    feature_bindings: &FeatureBlockBindings<'_>,
    feature_catalog: &FeatureCatalog,
    blocks: FeatureBlocks,
    survival: &BlockSurvivalRegistry<'_>,
    support: &BlockSupportRegistry<'_>,
    collision: &BlockCollisionRegistry<'_>,
    fluids: &FluidStateTable,
    water_mask: horizon_world::fluid::FluidMask,
    out: &mut impl Write,
) -> Result<(), Box<dyn std::error::Error>> {
    let json = f["definition"].as_str().unwrap();
    let feature = match f["mode"].as_str().unwrap_or("map") {
        "weather" => BiomeWeatherBindings::new(feature_bindings).map(|bindings| {
            Feature::Weather(Box::new(WeatherProbe {
                bindings,
                temperature: BiomeTemperature::new(BiomeTemperatureSettings {
                    base_temperature: f32::from_bits(
                        u32::from_str_radix(f["base"].as_str().unwrap(), 16).unwrap(),
                    ),
                    modifier: if f["modifier"].as_str() == Some("FROZEN") {
                        TemperatureModifier::Frozen
                    } else {
                        TemperatureModifier::None
                    },
                    has_precipitation: f["precipitation"].as_bool().unwrap(),
                    downfall: 0.0,
                }),
                snow: f["operation"].as_str() == Some("snow"),
                require_edge: f["edge"].as_bool().unwrap(),
            }))
        }),
        "map" => {
            SimpleBlockFeature::from_json(json, providers, catalog, placement).map(Feature::Map)
        }
        "feature" => feature_catalog
            .feature_with_blocks(json, feature_bindings)
            .map(|v| Feature::Graph(Box::new(v))),
        _ => feature_catalog
            .placed_feature_with_blocks(json, feature_bindings)
            .map(|v| Feature::Placed(Box::new(v))),
    };
    let biome = f
        .get("biome")
        .map(|value| -> Result<_, Box<dyn std::error::Error>> {
            Ok((
                BiomeWeatherBindings::new(feature_bindings)?,
                BiomeTemperature::new(BiomeTemperatureSettings {
                    base_temperature: f32::from_bits(u32::from_str_radix(
                        value["base"].as_str().unwrap(),
                        16,
                    )?),
                    modifier: if value["modifier"].as_str() == Some("FROZEN") {
                        TemperatureModifier::Frozen
                    } else {
                        TemperatureModifier::None
                    },
                    has_precipitation: value["precipitation"].as_bool().unwrap(),
                    downfall: 0.0,
                }),
            ))
        })
        .transpose()?;
    let mut world = World {
        collision,
        loot_container: f.get("loot_entity").and_then(Value::as_str) == Some("chest"),
        loot: BTreeMap::new(),
        gateway_entity: f.get("gateway_entity").and_then(Value::as_str) == Some("gateway"),
        gateways: BTreeMap::new(),
        biome,
        out,
        random: world_random,
        states: f["states"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| {
                (
                    std::array::from_fn(|i| v[i].as_i64().unwrap() as i32),
                    BlockStateId(v[3].as_u64().unwrap() as u32),
                )
            })
            .collect(),
        fallback: BlockStateId(f["fallback"].as_u64().unwrap() as u32),
        fluid: f
            .get("fluid")
            .map(|v| BlockStateId(v.as_u64().unwrap() as u32)),
        fluids,
        water_mask,
        survival,
        support,
        calls: 0,
        fail: f["fail"].as_i64().unwrap() as i32,
        reject: f["reject"].as_i64().unwrap() as i32,
        brightness: f["brightness"].as_i64().unwrap() as i32,
        sea_level: f.get("sea").and_then(Value::as_i64).unwrap_or(63) as i32,
        surface_height: f.get("height").and_then(Value::as_i64).unwrap_or(64) as i32,
        air_above: f.get("air_above").and_then(Value::as_i64).map(|v| v as i32),
        air: blocks.air,
        destroy_success: f
            .get("destroy_success")
            .and_then(Value::as_bool)
            .unwrap_or(true),
    };
    match feature {
        Err(_) => writeln!(world.out, "decode error")?,
        Ok(mut feature) => {
            writeln!(world.out, "decode ok")?;
            let origin = std::array::from_fn(|i| f["origin"][i].as_i64().unwrap() as i32);
            for _ in 0..f["repeats"].as_u64().unwrap() {
                world.calls = 0;
                let result = match &mut feature {
                    Feature::Weather(probe) => if probe.snow {
                        probe
                            .bindings
                            .should_snow(&mut world, &mut probe.temperature, origin)
                    } else {
                        probe.bindings.should_freeze(
                            &mut world,
                            &mut probe.temperature,
                            origin,
                            probe.require_edge,
                        )
                    }
                    .map_err(|_| ()),
                    Feature::Map(feature) => feature
                        .place(&mut world, &mut random, origin)
                        .map_err(|_| ()),
                    Feature::Graph(feature) => feature
                        .place(&mut world, &mut random, origin, blocks)
                        .map_err(|_| ()),
                    Feature::Placed(feature) => feature
                        .place(&mut world, &mut random, origin, blocks)
                        .map_err(|_| ()),
                };
                match result {
                    Ok(result) => writeln!(world.out, "place {result}")?,
                    Err(_) => writeln!(world.out, "place error")?,
                }
                writeln!(
                    world.out,
                    "next {} {}",
                    random.next_long(),
                    world.random.next_long()
                )?;
                writeln!(world.out, "calls {}", world.calls)?;
                for (p, state) in &world.states {
                    writeln!(world.out, "stored {} {} {} {}", p[0], p[1], p[2], state.0)?;
                }
                for (p, (table, seed)) in &world.loot {
                    writeln!(world.out, "loot {} {} {} {table} {seed}", p[0], p[1], p[2])?;
                }
                for (p, (exit, exact)) in &world.gateways {
                    writeln!(
                        world.out,
                        "gateway {} {} {} {} {} {} {exact}",
                        p[0], p[1], p[2], exit[0], exit[1], exit[2]
                    )?;
                }
                writeln!(world.out, "end")?;
            }
        }
    }
    writeln!(
        world.out,
        "tail {} {}",
        random.next_long(),
        world.random.next_long()
    )?;
    Ok(())
}
pub fn main() -> Result<(), Box<dyn std::error::Error>> {
    let arg = crate::args().nth(1).expect("private fixture directory");
    let dir = Path::new(&arg);
    let read = |name: &str| -> Result<Tag, Box<dyn std::error::Error>> {
        Ok(
            nbt::read_named(&fs::read(dir.join(name))?, Limits::default())?
                .0
                .tag,
        )
    };
    let registry = BlockRegistry::from_nbt(&read("block-registry.nbt")?)?;
    let collisions = CachedCollisionRegistry::from_nbt(&read("cached-collision.nbt")?, &registry)?;
    let properties =
        BlockPropertiesRegistry::from_nbt(&read("block-properties.nbt")?, &collisions)?;
    let support = Arc::new(BlockSupportRegistry::from_nbt(
        &read("cached-support.nbt")?,
        &registry,
    )?);
    let resources: BTreeMap<String, String> =
        serde_json::from_str(&fs::read_to_string(dir.join("tags.json"))?)?;
    let mut tags = TagDefinitions::default();
    for (name, text) in resources {
        tags.apply_json(&name, "vanilla", &text)?;
    }
    let tags = tags.build(|name, _| registry.default_state(name).map(|_| name.to_owned()))?;
    assert!(tags.missing.is_empty());
    let definitions: BTreeMap<String, String> =
        serde_json::from_str(&fs::read_to_string(dir.join("survival-rules.json"))?)?;
    let definitions = definitions
        .into_iter()
        .map(|(name, rule)| {
            use SurvivalRule::*;
            let rule = match rule.as_str() {
                value if value.starts_with("TagBelow:") => TagBelow(value[9..].to_owned()),
                "CactusFlower" => CactusFlower,
                "Seagrass" => Seagrass,
                "BubbleColumn" => BubbleColumn,
                "RedstoneWire" => RedstoneWire,
                "DoublePlant" => DoublePlant,
                "Door" => Door,
                "BigDripleaf" => BigDripleaf,
                "BigDripleafStem" => BigDripleafStem,
                "Farmland" => Farmland,
                "Cocoa" => Cocoa,
                "Crop" => Crop,
                "GrowingPlant" => GrowingPlant,
                "MangrovePropagule" => MangrovePropagule,
                "Speleothem" => Speleothem,
                "Scaffolding" => Scaffolding,
                "PistonHead" => PistonHead,
                "ChorusPlant" => ChorusPlant,
                "ChorusFlower" => ChorusFlower,
                "PitcherCrop" => PitcherCrop,
                "Multiface" => Multiface,
                "Vine" => Vine,
                "HangingMoss" => HangingMoss,
                "SnowLayer" => SnowLayer,
                "SeaPickle" => SeaPickle,
                "Fire" => Fire,
                "Cactus" => Cactus,
                "Path" => Path,
                "Mushroom" => Mushroom,
                "Bell" => Bell,
                "SugarCane" => SugarCane,
                "LilyPad" => LilyPad,
                "Frogspawn" => Frogspawn,
                "SporeBlossom" => SporeBlossom,
                "SmallDripleaf" => SmallDripleaf,
                "TallSeagrass" => TallSeagrass,
                "Always" => Always,
                "SolidBelow" => SolidBelow,
                "SolidBehind" => SolidBehind,
                "NonAirBelow" => NonAirBelow,
                "MossCarpet" => MossCarpet,
                "CenterBelow" => CenterBelow,
                "Lantern" => Lantern,
                "FullAbove" => FullAbove,
                "FullBehind" => FullBehind,
                "RigidOrCenterBelow" => RigidOrCenterBelow,
                "RigidBelow" => RigidBelow,
                "FullBelow" => FullBelow,
                "CenterAbove" => CenterAbove,
                "FaceAttached" => FaceAttached,
                "SupportsBamboo" => SupportsBamboo,
                _ => panic!("unknown rule {rule}"),
            };
            (name, rule)
        })
        .collect();
    let fluid_resources: BTreeMap<String, String> =
        serde_json::from_str(&fs::read_to_string(dir.join("fluid-tags.json"))?)?;
    let mut fluid_tags = TagDefinitions::default();
    for (name, text) in fluid_resources {
        fluid_tags.apply_json(&name, "vanilla", &text)?;
    }
    let fluid_tags =
        fluid_tags.build(|name, _| FluidKind::from_name(name).map(|_| name.to_owned()))?;
    assert!(fluid_tags.missing.is_empty());
    let fluids = FluidStateTable::new(&registry)?;
    let collision = Arc::new(BlockCollisionRegistry::new(Arc::new(collisions))?);
    let extra: Value = serde_json::from_str(&fs::read_to_string(
        dir.join("survival-extra-definitions.json"),
    )?)?;
    let definitions = SurvivalDefinitions {
        rules: definitions,
        ignition_odds: serde_json::from_value(extra["ignition_odds"].clone())?,
        fence_gates: serde_json::from_value(extra["fence_gates"].clone())?,
    };
    let survival = BlockSurvivalRegistry::new(
        &registry,
        &properties,
        support.clone(),
        collision.clone(),
        &definitions,
        &tags.tags,
        &fluid_tags.tags,
    )?;
    let facts: Value = serde_json::from_str(&fs::read_to_string(dir.join("facts.json"))?)?;
    let integers: std::collections::BTreeSet<(String, String)> = facts["integers"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| {
            (
                v[0].as_str().unwrap().to_owned(),
                v[1].as_str().unwrap().to_owned(),
            )
        })
        .collect();
    let providers = StateProviderBindings::new(&registry, integers.clone(), &tags.tags)?
        .with_block_properties(&properties)?
        .with_fluid_tags(&fluid_tags.tags)?;
    let resources: BTreeMap<String, String> =
        serde_json::from_str(&fs::read_to_string(dir.join("catalog.json"))?)?;
    let catalog = StateProviderCatalog::from_resources(&resources)?;
    let definitions: Value =
        serde_json::from_str(&fs::read_to_string(dir.join("placement-definitions.json"))?)?;
    let placement = BlockPlacementBindings::new(
        &registry,
        &properties,
        &serde_json::from_value(definitions["double_plants"].clone())?,
        &serde_json::from_value(definitions["mossy_carpets"].clone())?,
    )?;
    let feature_bindings =
        FeatureBlockBindings::with_tags(&registry, integers.clone(), &tags.tags)?
            .with_block_properties(&properties)?
            .with_fluid_tags(&fluid_tags.tags)?
            .with_state_providers(
                StateProviderBindings::new(&registry, integers, &tags.tags)?
                    .with_block_properties(&properties)?
                    .with_fluid_tags(&fluid_tags.tags)?,
                StateProviderCatalog::from_resources(&resources)?,
                BlockPlacementBindings::new(
                    &registry,
                    &properties,
                    &serde_json::from_value(definitions["double_plants"].clone())?,
                    &serde_json::from_value(definitions["mossy_carpets"].clone())?,
                )?,
            )?;
    let feature_resources: Value =
        serde_json::from_str(&fs::read_to_string(dir.join("feature-catalog.json"))?)?;
    let feature_catalog = FeatureCatalog::from_resources(
        &serde_json::from_value(feature_resources["features"].clone())?,
        &serde_json::from_value(feature_resources["placed"].clone())?,
        &TagDefinitions::default(),
    )?;
    let blocks = FeatureBlocks::from_registry(&registry)?;
    let water_mask = fluid_tags.tags["minecraft:water"]
        .iter()
        .fold(0, |mask, name| {
            mask | FluidKind::from_name(name).unwrap().mask()
        });
    let mut out = io::BufWriter::new(fs::File::create(dir.join("rust.txt"))?);
    for line in io::BufReader::new(fs::File::open(dir.join("inputs.txt"))?).lines() {
        let f: Value = serde_json::from_str(&line?)?;
        let seed = f["seed"].as_i64().unwrap();
        let world_seed = f["world_seed"].as_i64().unwrap();
        if f["legacy"].as_bool().unwrap() {
            fixture(
                &f,
                LegacyRandom::new(seed),
                LegacyRandom::new(world_seed),
                &providers,
                &catalog,
                &placement,
                &feature_bindings,
                &feature_catalog,
                blocks,
                &survival,
                &support,
                &collision,
                &fluids,
                water_mask,
                &mut out,
            )?;
        } else {
            fixture(
                &f,
                XoroshiroRandom::new(seed),
                XoroshiroRandom::new(world_seed),
                &providers,
                &catalog,
                &placement,
                &feature_bindings,
                &feature_catalog,
                blocks,
                &survival,
                &support,
                &collision,
                &fluids,
                water_mask,
                &mut out,
            )?;
        }
    }
    out.flush()?;
    Ok(())
}
