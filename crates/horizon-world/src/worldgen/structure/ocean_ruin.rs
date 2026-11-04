//! Ocean ruins (OceanRuinPieces).
use super::{
    BoundingBox, Mirror, Rotation, StructureResources, StructureTemplate,
    legacy::{PieceContext, Result, TemplateHandler, TemplatePiece},
    processor::ProcessorList,
};
use crate::{
    fluid::FluidKind,
    heightmap::HeightmapType,
    worldgen::{DecodeError, block_feature::FeatureBlockBindings},
};
use horizon_core::random::RandomSource;
use std::sync::Arc;

const WARM: [&str; 8] = [
    "warm_1", "warm_2", "warm_3", "warm_4", "warm_5", "warm_6", "warm_7", "warm_8",
];
const BIG_WARM: [&str; 4] = ["big_warm_4", "big_warm_5", "big_warm_6", "big_warm_7"];
const BRICK: [&str; 8] = [
    "brick_1", "brick_2", "brick_3", "brick_4", "brick_5", "brick_6", "brick_7", "brick_8",
];
const CRACKED: [&str; 8] = [
    "cracked_1",
    "cracked_2",
    "cracked_3",
    "cracked_4",
    "cracked_5",
    "cracked_6",
    "cracked_7",
    "cracked_8",
];
const MOSSY: [&str; 8] = [
    "mossy_1", "mossy_2", "mossy_3", "mossy_4", "mossy_5", "mossy_6", "mossy_7", "mossy_8",
];
const BIG_BRICK: [&str; 4] = ["big_brick_1", "big_brick_2", "big_brick_3", "big_brick_8"];
const BIG_CRACKED: [&str; 4] = [
    "big_cracked_1",
    "big_cracked_2",
    "big_cracked_3",
    "big_cracked_8",
];
const BIG_MOSSY: [&str; 4] = ["big_mossy_1", "big_mossy_2", "big_mossy_3", "big_mossy_8"];

/// Mth.nextInt(random, min, max).
pub fn next_int(random: &mut impl RandomSource, min: i32, max: i32) -> i32 {
    if min >= max {
        min
    } else {
        random.next_int_bound(max - min + 1).unwrap_or(0) + min
    }
}

pub struct OceanRuinConfig {
    pub warm: bool,
    pub large_probability: f32,
    pub cluster_probability: f32,
}
fn processors(
    resources: &StructureResources,
    bindings: &FeatureBlockBindings<'_>,
    integrity: f32,
    warm: bool,
) -> std::result::Result<Arc<ProcessorList>, DecodeError> {
    let (candidate, replacement, table) = if warm {
        (
            "minecraft:sand",
            "minecraft:suspicious_sand",
            "minecraft:archaeology/ocean_ruin_warm",
        )
    } else {
        (
            "minecraft:gravel",
            "minecraft:suspicious_gravel",
            "minecraft:archaeology/ocean_ruin_cold",
        )
    };
    let json = serde_json::json!({"processors": [
        {"processor_type": "minecraft:block_rot", "integrity": integrity},
        {"processor_type": "minecraft:block_ignore", "blocks": [{"id": "minecraft:structure_block"}, {"id": "minecraft:air"}]},
        {"processor_type": "minecraft:capped", "limit": 5, "delegate": {"processor_type": "minecraft:rule", "rules": [{
            "input_predicate": {"predicate_type": "minecraft:block_match", "block": candidate},
            "location_predicate": {"predicate_type": "minecraft:always_true"},
            "output_state": {"id": replacement},
            "block_entity_modifier": {"type": "minecraft:append_loot", "loot_table": table}
        }]}}
    ]});
    Ok(Arc::new(ProcessorList::decode(
        &json.to_string(),
        resources,
        bindings,
    )?))
}
#[allow(clippy::too_many_arguments)]
fn piece(
    resources: &StructureResources,
    bindings: &FeatureBlockBindings<'_>,
    name: &str,
    position: [i32; 3],
    rotation: Rotation,
    integrity: f32,
    warm: bool,
    large: bool,
) -> Option<TemplatePiece> {
    let name = format!("minecraft:underwater_ruin/{name}");
    Some(TemplatePiece::new(
        "minecraft:orp",
        &name,
        resources.template(&name)?,
        position,
        rotation,
        Mirror::None,
        [0; 3],
        processors(resources, bindings, integrity, warm).ok()?,
        true,
        Vec::new(),
        TemplateHandler::OceanRuin {
            warm,
            large,
            integrity,
        },
    ))
}
#[allow(clippy::too_many_arguments)]
fn add_piece(
    out: &mut Vec<TemplatePiece>,
    resources: &StructureResources,
    bindings: &FeatureBlockBindings<'_>,
    position: [i32; 3],
    rotation: Rotation,
    random: &mut impl RandomSource,
    config: &OceanRuinConfig,
    large: bool,
    integrity: f32,
) -> Option<()> {
    if config.warm {
        let names: &[&str] = if large { &BIG_WARM } else { &WARM };
        let name = names[random.next_int_bound(names.len() as i32).ok()? as usize];
        out.push(piece(
            resources, bindings, name, position, rotation, integrity, true, large,
        )?);
    } else {
        let (bricks, cracked, mossy): (&[&str], &[&str], &[&str]) = if large {
            (&BIG_BRICK, &BIG_CRACKED, &BIG_MOSSY)
        } else {
            (&BRICK, &CRACKED, &MOSSY)
        };
        let idx = random.next_int_bound(bricks.len() as i32).ok()? as usize;
        out.push(piece(
            resources,
            bindings,
            bricks[idx],
            position,
            rotation,
            integrity,
            false,
            large,
        )?);
        out.push(piece(
            resources,
            bindings,
            cracked[idx],
            position,
            rotation,
            0.7,
            false,
            large,
        )?);
        out.push(piece(
            resources, bindings, mossy[idx], position, rotation, 0.5, false, large,
        )?);
    }
    Some(())
}
/// OceanRuinPieces.addPieces.
pub fn pieces(
    resources: &StructureResources,
    bindings: &FeatureBlockBindings<'_>,
    position: [i32; 3],
    rotation: Rotation,
    random: &mut impl RandomSource,
    config: &OceanRuinConfig,
) -> Option<Vec<TemplatePiece>> {
    let mut out = Vec::new();
    let large = random.next_float() <= config.large_probability;
    let integrity = if large { 0.9 } else { 0.8 };
    add_piece(
        &mut out, resources, bindings, position, rotation, random, config, large, integrity,
    )?;
    if large && random.next_float() <= config.cluster_probability {
        let parent = [position[0], 90, position[2]];
        let corner = StructureTemplate::transform([15, 0, 15], Mirror::None, rotation, [0; 3]);
        let corner = [
            corner[0] + parent[0],
            corner[1] + parent[1],
            corner[2] + parent[2],
        ];
        let parent_box = BoundingBox::from_corners(parent, corner);
        let origin = [
            parent[0].min(corner[0]),
            parent[1],
            parent[2].min(corner[2]),
        ];
        let mut all = vec![
            [
                origin[0] - 16 + next_int(random, 1, 8),
                origin[1],
                origin[2] + 16 + next_int(random, 1, 7),
            ],
            [
                origin[0] - 16 + next_int(random, 1, 8),
                origin[1],
                origin[2] + next_int(random, 1, 7),
            ],
            [
                origin[0] - 16 + next_int(random, 1, 8),
                origin[1],
                origin[2] - 16 + next_int(random, 4, 8),
            ],
            [
                origin[0] + next_int(random, 1, 7),
                origin[1],
                origin[2] + 16 + next_int(random, 1, 7),
            ],
            [
                origin[0] + next_int(random, 1, 7),
                origin[1],
                origin[2] - 16 + next_int(random, 4, 6),
            ],
            [
                origin[0] + 16 + next_int(random, 1, 7),
                origin[1],
                origin[2] + 16 + next_int(random, 3, 8),
            ],
            [
                origin[0] + 16 + next_int(random, 1, 7),
                origin[1],
                origin[2] + next_int(random, 1, 7),
            ],
            [
                origin[0] + 16 + next_int(random, 1, 7),
                origin[1],
                origin[2] - 16 + next_int(random, 4, 8),
            ],
        ];
        let ruins = next_int(random, 4, 8);
        for _ in 0..ruins {
            if all.is_empty() {
                continue;
            }
            let idx = random.next_int_bound(all.len() as i32).ok()? as usize;
            let pos = all.remove(idx);
            let next_rotation = Rotation::random(random);
            let c = StructureTemplate::transform([5, 0, 6], Mirror::None, next_rotation, [0; 3]);
            let next_box =
                BoundingBox::from_corners(pos, [c[0] + pos[0], c[1] + pos[1], c[2] + pos[2]]);
            if !next_box.intersects(&parent_box) {
                add_piece(
                    &mut out,
                    resources,
                    bindings,
                    pos,
                    next_rotation,
                    random,
                    config,
                    false,
                    0.8,
                )?;
            }
        }
    }
    Some(out)
}

fn is_water(c: &mut PieceContext<'_>, pos: [i32; 3]) -> Result<bool> {
    Ok(matches!(
        c.world.fluid(pos)?.kind(),
        FluidKind::Water | FluidKind::FlowingWater
    ))
}
/// OceanRuinPiece.postProcess.
pub fn post_process(piece: &TemplatePiece, c: &mut PieceContext<'_>) -> Result<()> {
    let TemplateHandler::OceanRuin { large, .. } = piece.handler else {
        return Ok(());
    };
    let [x, _, z] = piece.position;
    let mut position = [x, c.world.height(HeightmapType::OceanFloorWg, x, z)?, z];
    let size = piece.template.size;
    let r = StructureTemplate::transform(
        [size[0] - 1, 0, size[2] - 1],
        Mirror::None,
        piece.rotation,
        [0; 3],
    );
    let corner = [r[0] + position[0], r[1] + position[1], r[2] + position[2]];
    // getHeight over the footprint.
    let mut new_y = position[1];
    let mut min_y = 512;
    let top_y = new_y - 1;
    let mut area = 0;
    let min_world = c.world.min_y()?;
    for px in position[0].min(corner[0])..=position[0].max(corner[0]) {
        for pz in position[2].min(corner[2])..=position[2].max(corner[2]) {
            let mut floor = position[1] - 1;
            loop {
                let state = c.world.block([px, floor, pz])?;
                let ice = c
                    .resources
                    .block_tags
                    .get("minecraft:ice")
                    .is_some_and(|t| t.contains(c.name(state)));
                let open = c.is_air(state) || is_water(c, [px, floor, pz])? || ice;
                if !(open && floor > min_world + 1) {
                    break;
                }
                floor -= 1;
            }
            min_y = min_y.min(floor);
            if floor < top_y - 2 {
                area += 1;
            }
        }
    }
    let width = (position[0] - corner[0]).abs();
    if top_y - min_y > 2 && area > width - 2 {
        new_y = min_y + 1;
    }
    position[1] = new_y;
    // OceanRuinPiece.postProcess keeps the height in templatePosition.
    *piece.state.lock().unwrap() = Some(new_y);
    if let Some(markers) = piece.place_at(c, position, &[])? {
        for (marker, pos) in markers {
            if marker == "chest" {
                let water = if is_water(c, pos)? { "true" } else { "false" };
                let chest = c.state_with("chest", &[("waterlogged", water)]);
                c.world.set(pos, chest, 2)?;
                let table = if large {
                    "minecraft:chests/underwater_ruin_big"
                } else {
                    "minecraft:chests/underwater_ruin_small"
                };
                c.loot_table(pos, "minecraft:chest", table)?;
            } else if marker == "drowned" {
                c.world.entity(super::legacy::spawn_nbt(
                    "minecraft:drowned",
                    pos,
                    true,
                    true,
                ))?;
                let sea = c.world.sea_level()?;
                let state = if pos[1] > sea {
                    c.state("air")
                } else {
                    c.state("water")
                };
                c.world.set(pos, state, 2)?;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        fluid::FluidState,
        nbt::{Compound, parse_snbt},
        registry::{BlockRegistry, BlockStateId},
        worldgen::{
            placement::PlacementQueryError,
            structure::{
                BoundingBox, StateTransforms,
                legacy::{PieceWorld, SolidRender, TemplatePiece},
                processor::ProcessorList,
            },
        },
    };
    use horizon_core::{position::ChunkPos, random::LegacyRandom};
    use std::{collections::BTreeMap, path::Path, sync::Mutex};

    /// Air, the water and the floor the piece is placed in, and the structure
    /// block that carries the piece's markers.
    fn registry() -> &'static BlockRegistry {
        let rows: Vec<serde_json::Value> = [
            "minecraft:air",
            "minecraft:cave_air",
            "minecraft:void_air",
            "minecraft:water",
            "minecraft:sandstone",
            "minecraft:structure_block",
        ]
        .into_iter()
        .map(|name| {
            serde_json::json!([name, name.ends_with("air"), false, "",
                {"id": name, "properties": {}}])
        })
        .collect();
        let defaults: serde_json::Map<String, serde_json::Value> = [
            "minecraft:air",
            "minecraft:cave_air",
            "minecraft:void_air",
            "minecraft:water",
            "minecraft:sandstone",
            "minecraft:structure_block",
        ]
        .iter()
        .enumerate()
        .map(|(i, name)| ((*name).to_owned(), serde_json::json!(i)))
        .collect();
        let facts = serde_json::json!({"states": rows, "defaults": defaults});
        let registry = BlockRegistry::from_facts_json(facts.to_string().as_bytes()).unwrap();
        Box::leak(Box::new(registry))
    }

    /// The piece world: a solid seabed at `height` everywhere, a sea level, and
    /// every `set` recorded in call order.
    struct World {
        registry: &'static BlockRegistry,
        floor: BlockStateId,
        height: i32,
        sea: i32,
        sets: Vec<([i32; 3], BlockStateId, i32)>,
    }
    impl World {
        fn new(registry: &'static BlockRegistry, sea: i32) -> Self {
            Self {
                registry,
                floor: registry.default_state("minecraft:sandstone").unwrap(),
                height: 77,
                sea,
                sets: Vec::new(),
            }
        }
        fn name_of(&self, state: BlockStateId) -> &str {
            self.registry.state(state).map(|s| s.name.as_str()).unwrap()
        }
    }
    impl PieceWorld for World {
        fn block(&mut self, _: [i32; 3]) -> Result<BlockStateId> {
            Ok(self.floor)
        }
        fn set(&mut self, pos: [i32; 3], state: BlockStateId, flags: i32) -> Result<bool> {
            self.sets.push((pos, state, flags));
            Ok(true)
        }
        fn fluid(&mut self, _: [i32; 3]) -> Result<FluidState> {
            Ok(FluidState::EMPTY)
        }
        fn height(&mut self, _: HeightmapType, _: i32, _: i32) -> Result<i32> {
            Ok(self.height)
        }
        fn min_y(&mut self) -> Result<i32> {
            Ok(-64)
        }
        fn max_y(&mut self) -> Result<i32> {
            Ok(384)
        }
        fn block_entity(&mut self, _: [i32; 3], _: Compound) -> Result<()> {
            Ok(())
        }
        fn entity(&mut self, _: Compound) -> Result<()> {
            Ok(())
        }
        fn post_process(&mut self, _: [i32; 3]) -> Result<()> {
            Ok(())
        }
        fn fluid_tick(&mut self, _: [i32; 3], _: crate::fluid::FluidKind, _: i32) -> Result<()> {
            Ok(())
        }
        fn seed(&mut self) -> Result<i64> {
            Ok(0)
        }
        fn level_boolean(&mut self) -> Result<bool> {
            Ok(false)
        }
        fn level_float(&mut self) -> Result<f32> {
            Ok(0.0)
        }
        fn level_int(&mut self, _: i32) -> Result<i32> {
            Ok(0)
        }
        fn sea_level(&mut self) -> Result<i32> {
            Ok(self.sea)
        }
        fn face_full(&mut self, _: [i32; 3], _: horizon_core::geometry::Direction) -> Result<bool> {
            Ok(false)
        }
        fn face_sturdy(
            &mut self,
            _: [i32; 3],
            _: horizon_core::geometry::Direction,
        ) -> Result<bool> {
            Ok(false)
        }
        fn center_sturdy(
            &mut self,
            _: [i32; 3],
            _: horizon_core::geometry::Direction,
        ) -> Result<bool> {
            Ok(false)
        }
        fn biome(&mut self, _: [i32; 3]) -> Result<crate::worldgen::biome::BiomeId> {
            Err(PlacementQueryError::Unavailable)
        }
        fn place_template(
            &mut self,
            _: &StructureResources,
            _: &StructureTemplate,
            _: [i32; 3],
            _: [i32; 3],
            _: &mut super::super::processor::PlaceSettings<'_>,
            _: &mut dyn super::super::processor::ProcessorRandom,
            _: i32,
        ) -> Result<bool> {
            Ok(true)
        }
    }
    /// No block draws as a solid block here.
    struct Blocks;
    impl SolidRender for Blocks {
        fn solid_render(&self, _: BlockStateId) -> bool {
            false
        }
    }

    /// One ocean ruin piece whose template holds a `drowned` data marker at
    /// its own origin.
    fn piece(registry: &BlockRegistry) -> TemplatePiece {
        let (tag, _) = parse_snbt(
            r#"{size:[1,1,1],palette:[{Name:"minecraft:structure_block"}],
                blocks:[{pos:[0,0,0],state:0,nbt:{mode:"data",metadata:"drowned"}}]}"#,
        )
        .unwrap();
        let template = StructureTemplate::from_nbt(&tag, registry, &StateTransforms::default());
        TemplatePiece {
            type_name: "minecraft:ocean_ruin",
            template_name: "ocean_ruin/cold/brick_1".into(),
            template: Arc::new(template),
            position: [8, 0, 8],
            rotation: Rotation::None,
            mirror: Mirror::None,
            pivot: [0; 3],
            processors: Arc::new(ProcessorList(Vec::new())),
            waterlogging: true,
            ignore: Vec::new(),
            bounding_box: BoundingBox::new([8, 0, 8], [8, 384, 8]),
            handler: TemplateHandler::OceanRuin {
                warm: false,
                large: false,
                integrity: 1.0,
            },
            state: Mutex::new(None),
        }
    }

    /// The drowned marker's block, with the piece's world at sea level `sea`.
    fn drowned_block(sea: i32) -> String {
        let registry = registry();
        let resources = StructureResources::new(
            registry,
            StateTransforms::default(),
            Path::new("/nonexistent"),
            BTreeMap::new(),
            &BTreeMap::new(),
        );
        let piece = piece(registry);
        let mut world = World::new(registry, sea);
        let mut blocks = Blocks;
        let mut random = LegacyRandom::new(0);
        {
            let mut c = PieceContext {
                world: &mut world,
                resources: &resources,
                blocks: &mut blocks,
                random: &mut random,
                chunk_box: BoundingBox::new([0, -64, 0], [16, 384, 16]),
                chunk: ChunkPos::new(0, 0),
                reference: [8, 0, 8],
            };
            post_process(&piece, &mut c).unwrap();
        }
        // The drowned marker sits at the piece's own height, 77: above the old
        // hardcoded sea level and below this world's.
        let (_, state, flags) = world
            .sets
            .iter()
            .find(|(pos, state, _)| {
                *pos == [8, 77, 8] && world.name_of(*state) != "minecraft:structure_block"
            })
            .map(|(pos, state, flags)| (*pos, *state, *flags))
            .expect("the drowned marker's block");
        assert_eq!(flags, 2);
        world.name_of(state).to_owned()
    }

    /// `OceanRuinPieces.OceanRuinPiece.postProcess` floods the drowned marker's
    /// block when it is at or below `level.getSeaLevel()`, and leaves air above.
    #[test]
    fn a_drowned_marker_is_water_at_or_below_the_sea_level() {
        assert_eq!(drowned_block(90), "minecraft:water");
        assert_eq!(drowned_block(77), "minecraft:water");
        assert_eq!(drowned_block(76), "minecraft:air");
    }
}
