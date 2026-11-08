//! Ruined portals (RuinedPortalStructure, RuinedPortalPiece).
use super::{
    Mirror, Rotation, StructureResources,
    legacy::{
        HORIZONTAL, PieceContext, Result, TemplateHandler, TemplatePiece, direction_name, offset,
        opposite,
    },
    processor::ProcessorList,
    start::StartContext,
};
use crate::{
    heightmap::HeightmapType, registry::BlockStateId, worldgen::block_feature::FeatureBlockBindings,
};
use horizon_core::{geometry::Direction, position::ChunkPos, random::RandomSource};
use serde_json::{Value, json};
use std::sync::Arc;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Vertical {
    OnLandSurface,
    PartlyBuried,
    OnOceanFloor,
    InMountain,
    Underground,
    InNether,
}
impl Vertical {
    /// `RuinedPortalPiece.VerticalPlacement.getSerializedName`.
    pub(super) fn saved_name(self) -> &'static str {
        match self {
            Self::OnLandSurface => "on_land_surface",
            Self::PartlyBuried => "partly_buried",
            Self::OnOceanFloor => "on_ocean_floor",
            Self::InMountain => "in_mountain",
            Self::Underground => "underground",
            Self::InNether => "in_nether",
        }
    }
    fn heightmap(self) -> HeightmapType {
        if self == Self::OnOceanFloor {
            HeightmapType::OceanFloorWg
        } else {
            HeightmapType::WorldSurfaceWg
        }
    }
}
#[derive(Clone, Debug)]
pub struct Setup {
    placement: Vertical,
    air_pocket: f32,
    mossiness: f32,
    overgrown: bool,
    vines: bool,
    can_be_cold: bool,
    blackstone: bool,
    weight: f32,
}
impl Setup {
    pub fn decode(v: &Value) -> Self {
        let float = |k: &str| v[k].as_f64().unwrap_or(0.) as f32;
        let flag = |k: &str| v[k].as_bool().unwrap_or(false);
        Self {
            placement: match v["placement"].as_str().unwrap_or_default() {
                "partly_buried" => Vertical::PartlyBuried,
                "on_ocean_floor" => Vertical::OnOceanFloor,
                "in_mountain" => Vertical::InMountain,
                "underground" => Vertical::Underground,
                "in_nether" => Vertical::InNether,
                _ => Vertical::OnLandSurface,
            },
            air_pocket: float("air_pocket_probability"),
            mossiness: float("mossiness"),
            overgrown: flag("overgrown"),
            vines: flag("vines"),
            can_be_cold: flag("can_be_cold"),
            blackstone: flag("replace_with_blackstone"),
            weight: float("weight"),
        }
    }
}
#[derive(Clone, Copy, Debug)]
pub struct PortalProps {
    pub(super) placement: Vertical,
    pub(super) mossiness: f32,
    pub(super) air_pocket: bool,
    pub(super) blackstone: bool,
    pub(super) cold: bool,
    pub(super) overgrown: bool,
    pub(super) vines: bool,
}

fn between(random: &mut impl RandomSource, min: i32, max: i32) -> i32 {
    random.next_int_bound(max - min + 1).unwrap_or(0) + min
}
fn within(random: &mut impl RandomSource, min: i32, max: i32) -> i32 {
    if min < max {
        between(random, min, max)
    } else {
        max
    }
}

/// RuinedPortalStructure.findGenerationPoint: the start position and piece.
pub fn start(
    context: &mut impl StartContext,
    resources: &StructureResources,
    bindings: &FeatureBlockBindings<'_>,
    random: &mut impl RandomSource,
    chunk: ChunkPos,
    setups: &[Setup],
) -> Option<([i32; 3], TemplatePiece)> {
    let setup = if setups.len() > 1 {
        let total: f32 = setups.iter().map(|s| s.weight).sum();
        let mut pick = random.next_float();
        setups.iter().find(|s| {
            pick -= s.weight / total;
            pick < 0.
        })?
    } else {
        setups.first()?
    };
    let air_pocket = match setup.air_pocket {
        0. => false,
        1. => true,
        p => random.next_float() < p,
    };
    let name = if random.next_float() < 0.05 {
        format!(
            "minecraft:ruined_portal/giant_portal_{}",
            random.next_int_bound(3).ok()? + 1
        )
    } else {
        format!(
            "minecraft:ruined_portal/portal_{}",
            random.next_int_bound(10).ok()? + 1
        )
    };
    let template = resources.template(&name)?;
    let rotation = Rotation::random(random);
    let mirror = if random.next_float() < 0.5 {
        Mirror::None
    } else {
        Mirror::FrontBack
    };
    let pivot = [template.size[0] / 2, 0, template.size[2] / 2];
    let base = [chunk.x * 16, 0, chunk.z * 16];
    let b = template.bounding_box(base, rotation, pivot, mirror);
    let center = b.center();
    let surface = context.base_height(center[0], center[2], setup.placement.heightmap()) - 1;
    let span = b.span()[1];
    let min_y = context.min_y() + 15;
    let new_y = match setup.placement {
        Vertical::InNether => {
            if air_pocket {
                between(random, 32, 100)
            } else if random.next_float() < 0.5 {
                between(random, 27, 29)
            } else {
                between(random, 29, 100)
            }
        }
        Vertical::InMountain => within(random, 70, surface - span),
        Vertical::Underground => within(random, min_y, surface - span),
        Vertical::PartlyBuried => surface - span + between(random, 2, 8),
        _ => surface,
    };
    let kind = setup.placement.heightmap();
    let columns: Vec<Vec<bool>> = [
        [b.min[0], b.min[2]],
        [b.max[0], b.min[2]],
        [b.min[0], b.max[2]],
        [b.max[0], b.max[2]],
    ]
    .into_iter()
    .map(|[x, z]| context.base_column(x, z, kind))
    .collect();
    let world_min = context.min_y();
    let mut y = new_y;
    'search: while y > min_y {
        let mut solid = 0;
        for column in &columns {
            let index = y - world_min;
            if index >= 0 && column.get(index as usize).copied().unwrap_or(false) {
                solid += 1;
                if solid == 3 {
                    break 'search;
                }
            }
        }
        y -= 1;
    }
    let origin = [base[0], y, base[2]];
    let cold = setup.can_be_cold && {
        let biome = context.noise_biome(origin.map(|v| v >> 2));
        context.cold_enough_to_snow(biome, origin)
    };
    let props = PortalProps {
        placement: setup.placement,
        mossiness: setup.mossiness,
        air_pocket,
        blackstone: setup.blackstone,
        cold,
        overgrown: setup.overgrown,
        vines: setup.vines,
    };
    let processors = processors(resources, bindings, props)?;
    let mut ignore = vec!["minecraft:structure_block".to_owned()];
    if !air_pocket {
        ignore.push("minecraft:air".into());
    }
    let piece = TemplatePiece::new(
        "minecraft:rupo",
        &name,
        template,
        origin,
        rotation,
        mirror,
        pivot,
        processors,
        true,
        ignore,
        TemplateHandler::RuinedPortal(props),
    );
    Some((origin, piece))
}

/// `RuinedPortalPiece.makeSettings`: rebuild the same processor chain on load and generation.
pub(super) fn processors(
    resources: &StructureResources,
    bindings: &FeatureBlockBindings<'_>,
    props: PortalProps,
) -> Option<Arc<ProcessorList>> {
    let rule = |source: &str, probability: Option<f32>, target: &str| {
        let input = match probability {
            Some(p) => {
                json!({"predicate_type": "minecraft:random_block_match", "block": source, "probability": p})
            }
            None => json!({"predicate_type": "minecraft:block_match", "block": source}),
        };
        json!({"input_predicate": input, "location_predicate": {"predicate_type": "minecraft:always_true"}, "output_state": target})
    };
    let mut rules = vec![rule("minecraft:gold_block", Some(0.3), "minecraft:air")];
    rules.push(if props.placement == Vertical::OnOceanFloor {
        rule("minecraft:lava", None, "minecraft:magma_block")
    } else if props.cold {
        rule("minecraft:lava", None, "minecraft:netherrack")
    } else {
        rule("minecraft:lava", Some(0.2), "minecraft:magma_block")
    });
    if !props.cold {
        rules.push(rule(
            "minecraft:netherrack",
            Some(0.07),
            "minecraft:magma_block",
        ));
    }
    let mut list = vec![
        json!({"processor_type": "minecraft:rule", "rules": rules}),
        json!({"processor_type": "minecraft:block_age", "mossiness": props.mossiness}),
        json!({"processor_type": "minecraft:protected_blocks", "value": "#minecraft:features_cannot_replace"}),
        json!({"processor_type": "minecraft:lava_submerged_block"}),
    ];
    if props.blackstone {
        list.push(json!({"processor_type": "minecraft:blackstone_replace"}));
    }
    let processors = ProcessorList::decode(
        &json!({ "processors": list }).to_string(),
        resources,
        bindings,
    )
    .ok()?;
    Some(Arc::new(processors))
}

const NETHERRACK_BY_DISTANCE: [f32; 14] = [
    1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 0.9, 0.9, 0.8, 0.7, 0.6, 0.4, 0.2,
];
fn replaceable(c: &PieceContext<'_>, props: PortalProps, state: BlockStateId) -> bool {
    let name = c.name(state);
    name != "minecraft:air"
        && name != "minecraft:obsidian"
        && !c
            .resources
            .block_tags
            .get("minecraft:features_cannot_replace")
            .is_some_and(|t| t.contains(name))
        && (props.placement == Vertical::InNether || name != "minecraft:lava")
}
fn netherrack_or_magma(c: &mut PieceContext<'_>, props: PortalProps, pos: [i32; 3]) -> Result<()> {
    let state = c.world.block(pos)?;
    if replaceable(c, props, state) {
        let block = if !props.cold && c.random.float() < 0.07 {
            "magma_block"
        } else {
            "netherrack"
        };
        c.world.set(pos, c.state(block), 3)?;
    }
    Ok(())
}
fn drip_column(c: &mut PieceContext<'_>, props: PortalProps, mut pos: [i32; 3]) -> Result<()> {
    netherrack_or_magma(c, props, pos)?;
    let mut remaining = 8;
    while remaining > 0 && c.random.float() < 0.5 {
        pos[1] -= 1;
        remaining -= 1;
        netherrack_or_magma(c, props, pos)?;
    }
    Ok(())
}
fn leaves_above(c: &mut PieceContext<'_>, pos: [i32; 3]) -> Result<()> {
    if c.random.float() < 0.5 && c.is_block(pos, "minecraft:netherrack")? {
        let above = [pos[0], pos[1] + 1, pos[2]];
        let state = c.world.block(above)?;
        if c.is_air(state) {
            let leaves = c.state_with("jungle_leaves", &[("persistent", "true")]);
            c.world.set(above, leaves, 3)?;
        }
    }
    Ok(())
}
fn vines(c: &mut PieceContext<'_>, pos: [i32; 3]) -> Result<()> {
    let state = c.world.block(pos)?;
    if c.is_air(state) || c.name(state) == "minecraft:vine" {
        return Ok(());
    }
    let direction = random_horizontal_dyn(c);
    let neighbour = offset(pos, direction, 1);
    let n = c.world.block(neighbour)?;
    if c.is_air(n) && c.world.face_full(pos, direction)? {
        let vine = c.state_with("vine", &[(direction_name(opposite(direction)), "true")]);
        c.world.set(neighbour, vine, 3)?;
    }
    Ok(())
}
fn random_horizontal_dyn(c: &mut PieceContext<'_>) -> Direction {
    HORIZONTAL[c.random.int(4) as usize]
}
/// RuinedPortalPiece.postProcess.
pub fn post_process(piece: &TemplatePiece, c: &mut PieceContext<'_>) -> Result<()> {
    let TemplateHandler::RuinedPortal(props) = piece.handler else {
        return Ok(());
    };
    let b = piece.bounding_box;
    if !c.chunk_box.is_inside(b.center()) {
        return Ok(());
    }
    c.chunk_box.encapsulate(&b);
    piece.place_at(c, piece.position, &[])?;
    // spreadNetherrack
    let follow = matches!(
        props.placement,
        Vertical::OnLandSurface | Vertical::OnOceanFloor
    );
    let [cx, _, cz] = b.center();
    let max = NETHERRACK_BY_DISTANCE.len() as i32;
    let [sx, _, sz] = b.span();
    let adjustment = c.random.int((8 - (sx + sz) / 2 / 2).max(1));
    for x in cx - max..=cx + max {
        for z in cz - max..=cz + max {
            let distance = (x - cx).abs() + (z - cz).abs();
            let adjusted = (distance + adjustment).max(0);
            if adjusted >= max {
                continue;
            }
            if c.random.double() < f64::from(NETHERRACK_BY_DISTANCE[adjusted as usize]) {
                let surface = c.world.height(props.placement.heightmap(), x, z)? - 1;
                let y = if follow {
                    surface
                } else {
                    b.min[1].min(surface)
                };
                let pos = [x, y, z];
                let state = c.world.block(pos)?;
                if (y - b.min[1]).abs() <= 3 && replaceable(c, props, state) {
                    netherrack_or_magma(c, props, pos)?;
                    if props.overgrown {
                        leaves_above(c, pos)?;
                    }
                    drip_column(c, props, [x, y - 1, z])?;
                }
            }
        }
    }
    // addNetherrackDripColumnsBelowPortal
    for x in b.min[0] + 1..b.max[0] {
        for z in b.min[2] + 1..b.max[2] {
            if c.is_block([x, b.min[1], z], "minecraft:netherrack")? {
                drip_column(c, props, [x, b.min[1] - 1, z])?;
            }
        }
    }
    if props.vines || props.overgrown {
        for z in b.min[2]..=b.max[2] {
            for y in b.min[1]..=b.max[1] {
                for x in b.min[0]..=b.max[0] {
                    if props.vines {
                        vines(c, [x, y, z])?;
                    }
                    if props.overgrown {
                        leaves_above(c, [x, y, z])?;
                    }
                }
            }
        }
    }
    Ok(())
}

impl PortalProps {
    /// `RuinedPortalPiece`'s `VerticalPlacement` and `Properties` tags.
    pub fn save(&self) -> [(&'static str, crate::nbt::Tag); 2] {
        use crate::nbt::Tag;
        let flag = |b: bool| Tag::Byte(i8::from(b));
        [
            (
                "VerticalPlacement",
                Tag::String(self.placement.saved_name().into()),
            ),
            (
                "Properties",
                Tag::compound([
                    ("cold", flag(self.cold)),
                    ("mossiness", Tag::Float(self.mossiness.to_bits())),
                    ("air_pocket", flag(self.air_pocket)),
                    ("overgrown", flag(self.overgrown)),
                    ("vines", flag(self.vines)),
                    ("replace_with_blackstone", flag(self.blackstone)),
                ]),
            ),
        ]
    }
}
