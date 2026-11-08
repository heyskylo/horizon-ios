//! The template subclasses' `CompoundTag` constructors from `StructurePieceType`.
use super::{
    Mirror, Rotation, StructureResources,
    end_city::EndCityPiece,
    legacy::{LegacyPiece, TemplateHandler, TemplatePiece},
    nether_fossil::NetherFossilPiece,
    ocean_ruin,
    processor::ProcessorList,
    ruined_portal::{self, PortalProps, Vertical},
    simple::igloo_pivot,
};
use crate::{
    datafix::dynamic::{Number, as_bool_opt, as_float, as_number},
    nbt::Tag,
    worldgen::block_feature::FeatureBlockBindings,
};
use std::sync::Arc;

fn text(tag: &Tag, key: &str) -> Option<String> {
    match tag.get(key)? {
        Tag::String(s) => Some(s.to_string_lossy()),
        _ => None,
    }
}
fn int(tag: &Tag, key: &str) -> i32 {
    // `CompoundTag.getIntOr`: `Mth.floor` of a float or double tag, the low 32 bits of the others.
    match tag.get(key).and_then(as_number) {
        Some(Number::Float(v)) => {
            let i = v as i32;
            if v < i as f32 { i.wrapping_sub(1) } else { i }
        }
        Some(Number::Double(v)) => {
            let i = v as i32;
            if v < f64::from(i) {
                i.wrapping_sub(1)
            } else {
                i
            }
        }
        Some(n) => n.int_value(),
        None => 0,
    }
}
fn flag(tag: &Tag, key: &str) -> bool {
    int(tag, key) as i8 != 0
}

fn mirror(tag: &Tag, key: &str) -> Option<Mirror> {
    [Mirror::None, Mirror::LeftRight, Mirror::FrontBack]
        .into_iter()
        .find(|m| Some(m.saved_name()) == text(tag, key).as_deref())
}

/// `TemplateStructurePiece(StructurePieceType, CompoundTag, ...)` and its subclasses' tag
/// constructors (`ShipwreckPieces.ShipwreckPiece`, `OceanRuinPieces.OceanRuinPiece.create`...):
/// the box is rebuilt and the orientation reset to NORTH, and each subclass rebuilds its
/// settings, rather than trusting the saved `BB`/`O`.
pub(crate) fn read(
    id: &str,
    tag: &Tag,
    resources: &StructureResources,
    bindings: &FeatureBlockBindings<'_>,
) -> Option<Box<dyn LegacyPiece>> {
    let type_name = match id {
        "shipwreck" => "minecraft:shipwreck",
        "orp" => "minecraft:orp",
        "iglu" => "minecraft:iglu",
        "rupo" => "minecraft:rupo",
        "wmp" => "minecraft:wmp",
        "nefos" => "minecraft:nefos",
        "ecp" => "minecraft:ecp",
        _ => return None,
    };
    let name = text(tag, "Template").unwrap_or_default();
    let name = match id {
        "wmp" => format!("minecraft:woodland_mansion/{name}"),
        "ecp" => format!("minecraft:end_city/{name}"),
        _ => crate::worldgen::tags::identifier(&name).ok()?,
    };
    let name = crate::worldgen::tags::identifier(&name).ok()?;
    let template = resources.template(&name)?;
    let rotation_key = if id == "rupo" { "Rotation" } else { "Rot" };
    let rotation = Rotation::ALL
        .into_iter()
        .find(|r| Some(r.saved_name()) == text(tag, rotation_key).as_deref())?;
    let mut mirror_value = Mirror::None;
    let mut pivot = [0; 3];
    let mut processors = Arc::new(ProcessorList::default());
    let mut ignore = vec!["minecraft:structure_block".into()];
    let handler = match id {
        "shipwreck" => {
            pivot = [4, 0, 15];
            ignore.push("minecraft:air".into());
            TemplateHandler::Shipwreck {
                beached: flag(tag, "isBeached"),
            }
        }
        "iglu" => {
            if !matches!(
                name.as_str(),
                "minecraft:igloo/top" | "minecraft:igloo/middle" | "minecraft:igloo/bottom"
            ) {
                return None;
            }
            pivot = igloo_pivot(&name);
            TemplateHandler::Igloo
        }
        "orp" => {
            let warm = match text(tag, "BiomeType")?.as_str() {
                "WARM" => true,
                "COLD" => false,
                _ => return None,
            };
            let integrity = as_float(tag.get("Integrity"), 0.0);
            processors = ocean_ruin::processors(resources, bindings, integrity, warm).ok()?;
            ignore.clear();
            TemplateHandler::OceanRuin {
                warm,
                large: flag(tag, "IsLarge"),
                integrity,
            }
        }
        "rupo" => {
            mirror_value = mirror(tag, "Mirror")?;
            pivot = [template.size[0] / 2, 0, template.size[2] / 2];
            let placement = [
                Vertical::OnLandSurface,
                Vertical::PartlyBuried,
                Vertical::OnOceanFloor,
                Vertical::InMountain,
                Vertical::Underground,
                Vertical::InNether,
            ]
            .into_iter()
            .find(|v| Some(v.saved_name()) == text(tag, "VerticalPlacement").as_deref())?;
            let p = tag.get("Properties")?;
            let mossiness = as_number(p.get("mossiness")?)?.float_value();
            let property_flag = |key| as_bool_opt(p.get(key));
            let props = PortalProps {
                placement,
                mossiness,
                air_pocket: property_flag("air_pocket")?,
                blackstone: property_flag("replace_with_blackstone")?,
                cold: property_flag("cold")?,
                overgrown: property_flag("overgrown")?,
                vines: property_flag("vines")?,
            };
            processors = ruined_portal::processors(resources, bindings, props)?;
            if !props.air_pocket {
                ignore.push("minecraft:air".into());
            }
            TemplateHandler::RuinedPortal(props)
        }
        "wmp" => {
            mirror_value = mirror(tag, "Mi")?;
            TemplateHandler::Mansion
        }
        _ => {
            if id == "nefos" || !flag(tag, "OW") {
                ignore.push("minecraft:air".into());
            }
            TemplateHandler::Plain
        }
    };
    let mut piece = TemplatePiece::new(
        type_name,
        &name,
        template,
        [int(tag, "TPX"), int(tag, "TPY"), int(tag, "TPZ")],
        rotation,
        mirror_value,
        pivot,
        processors,
        id != "iglu",
        ignore,
        handler,
    );
    if id == "shipwreck" && flag(tag, "height_adjusted") {
        *piece.state.lock().unwrap() = Some(piece.position[1]);
    }
    if id == "ecp" {
        return Some(Box::new(EndCityPiece(piece, int(tag, "GD"))));
    }
    piece.gen_depth = int(tag, "GD");
    if id == "nefos" {
        Some(Box::new(NetherFossilPiece(piece)))
    } else {
        Some(Box::new(piece))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        registry::BlockRegistry,
        worldgen::{
            generator_structures::read_saved_start,
            structure::{
                StateTransforms, StructureTemplate,
                start::{Piece, StructureDef, StructureKind, StructureStart},
            },
        },
    };
    use horizon_core::{density::TerrainAdjustment, position::ChunkPos};
    use std::collections::{BTreeMap, BTreeSet};

    #[test]
    fn saved_templates_round_trip_and_whole_start_fallback() {
        let (mut registry_tag, _) = crate::nbt::parse_snbt(
            r#"{DataVersion:0,states:[
            {id:"minecraft:air"},{id:"minecraft:cave_air"},{id:"minecraft:void_air"},
            {id:"minecraft:structure_block"},{id:"minecraft:sand"},{id:"minecraft:gravel"},
            {id:"minecraft:suspicious_sand"},{id:"minecraft:suspicious_gravel"},
            {id:"minecraft:gold_block"},{id:"minecraft:lava",properties:{level:"0"}},
            {id:"minecraft:magma_block"},{id:"minecraft:netherrack"}],defaults:{
            "minecraft:air":0,"minecraft:cave_air":1,"minecraft:void_air":2,
            "minecraft:structure_block":3,"minecraft:sand":4,"minecraft:gravel":5,
            "minecraft:suspicious_sand":6,"minecraft:suspicious_gravel":7,
            "minecraft:gold_block":8,"minecraft:lava":9,"minecraft:magma_block":10,
            "minecraft:netherrack":11}}"#,
        )
        .unwrap();
        *registry_tag.get_mut("DataVersion").unwrap() = Tag::Int(crate::DATA_VERSION);
        let registry = Box::leak(Box::new(BlockRegistry::from_nbt(&registry_tag).unwrap()));
        let resources = StructureResources::new(
            registry,
            StateTransforms::default(),
            std::path::Path::new("unused"),
            BTreeMap::new(),
            &BTreeMap::new(),
        );
        let bindings = FeatureBlockBindings::new(registry, BTreeSet::new()).unwrap();
        let def = StructureDef {
            name: "test:templates".into(),
            kind: StructureKind::Unsupported("test".into()),
            biomes: BTreeSet::new(),
            step: 0,
            terrain: TerrainAdjustment::BeardThin,
            enhanced_terrain: None,
        };
        let mut pieces = Vec::new();
        for rotation in Rotation::ALL {
            for variant in 0..6 {
                for (type_name, name) in [
                    ("minecraft:shipwreck", "shipwreck/with_mast"),
                    ("minecraft:orp", "underwater_ruin/warm_1"),
                    (
                        "minecraft:iglu",
                        ["igloo/top", "igloo/middle", "igloo/bottom"][variant % 3],
                    ),
                    ("minecraft:rupo", "ruined_portal/portal_1"),
                    ("minecraft:wmp", "woodland_mansion/entrance"),
                    ("minecraft:nefos", "nether_fossils/fossil_1"),
                    ("minecraft:ecp", "end_city/base_floor"),
                ] {
                    let id = type_name.strip_prefix("minecraft:").unwrap();
                    let name = format!("minecraft:{name}");
                    let mut template = StructureTemplate::empty();
                    template.size = [9, 7, 13];
                    template.location = name.as_str().into();
                    let template = Arc::new(template);
                    resources
                        .templates
                        .loaded
                        .write()
                        .unwrap()
                        .insert(name.clone(), template.clone());
                    let mut pivot = [0; 3];
                    let mut mirror_value = Mirror::None;
                    let mut ignore = vec!["minecraft:structure_block".into()];
                    let mut processors = Arc::new(ProcessorList::default());
                    let on = variant % 2 == 0;
                    let handler = match id {
                        "shipwreck" => {
                            pivot = [4, 0, 15];
                            ignore.push("minecraft:air".into());
                            TemplateHandler::Shipwreck { beached: on }
                        }
                        "iglu" => {
                            pivot = [[3, 5, 5], [1, 3, 1], [3, 6, 7]][variant % 3];
                            TemplateHandler::Igloo
                        }
                        "orp" => {
                            let integrity = [0.9, 0.8, 0.7, 0.5, 0.0, 1.0][variant];
                            processors =
                                ocean_ruin::processors(&resources, &bindings, integrity, on)
                                    .unwrap();
                            ignore.clear();
                            TemplateHandler::OceanRuin {
                                warm: on,
                                large: !on,
                                integrity,
                            }
                        }
                        "rupo" => {
                            pivot = [4, 0, 6];
                            mirror_value =
                                [Mirror::None, Mirror::LeftRight, Mirror::FrontBack][variant % 3];
                            let props = PortalProps {
                                placement: [
                                    Vertical::OnLandSurface,
                                    Vertical::PartlyBuried,
                                    Vertical::OnOceanFloor,
                                    Vertical::InMountain,
                                    Vertical::Underground,
                                    Vertical::InNether,
                                ][variant],
                                mossiness: 0.2,
                                air_pocket: on,
                                blackstone: !on,
                                cold: variant < 3,
                                overgrown: !on,
                                vines: on,
                            };
                            processors =
                                ruined_portal::processors(&resources, &bindings, props).unwrap();
                            if !on {
                                ignore.push("minecraft:air".into());
                            }
                            TemplateHandler::RuinedPortal(props)
                        }
                        "wmp" => {
                            mirror_value =
                                [Mirror::None, Mirror::LeftRight, Mirror::FrontBack][variant % 3];
                            TemplateHandler::Mansion
                        }
                        _ => {
                            if id == "nefos" || !on {
                                ignore.push("minecraft:air".into());
                            }
                            TemplateHandler::Plain
                        }
                    };
                    let mut piece = TemplatePiece::new(
                        type_name,
                        &name,
                        template,
                        [-32, 60, 48],
                        rotation,
                        mirror_value,
                        pivot,
                        processors,
                        id != "iglu",
                        ignore,
                        handler,
                    );
                    if id == "shipwreck" && on {
                        *piece.state.lock().unwrap() = Some(60);
                    }
                    piece.gen_depth = variant as i32 - 3;
                    let piece: Box<dyn LegacyPiece> = match id {
                        "nefos" => Box::new(NetherFossilPiece(piece)),
                        "ecp" => {
                            piece.gen_depth = 0;
                            Box::new(EndCityPiece(piece, -1234567))
                        }
                        _ => Box::new(piece),
                    };
                    pieces.push(Piece::Legacy(piece));
                }
            }
        }
        let chunk = ChunkPos::new(-2, 3);
        let mut original = StructureStart::new(7, &def, chunk, pieces);
        original.references = 3;
        let tag = original.save().unwrap();
        let read_start = |t: &Tag| read_saved_start(7, &def, chunk, t, &resources, &bindings);
        let loaded = read_start(&tag).expect("all seven types decode");
        assert_eq!(loaded.save(), Some(tag.clone()));
        assert_eq!(
            format!("{loaded:?}"),
            format!("{original:?}"),
            "every runtime field"
        );
        let mut changed = tag.clone();
        let Some(Tag::List(children)) = changed.get_mut("Children") else {
            unreachable!()
        };
        *children[0].get_mut("BB").unwrap() = Tag::IntArray(vec![0; 6]);
        *children[0].get_mut("O").unwrap() = Tag::Int(-1);
        *children[0].get_mut("TPX").unwrap() = Tag::Double((-31.5_f64).to_bits());
        *children[0].get_mut("isBeached").unwrap() = Tag::Long(257);
        assert_eq!(
            read_start(&changed).unwrap().save(),
            original.save(),
            "rebuild box and NORTH orientation"
        );
        for (key, value) in [("id", "minecraft:msroom"), ("Rot", "broken")] {
            let mut broken = tag.clone();
            let Some(Tag::List(children)) = broken.get_mut("Children") else {
                unreachable!()
            };
            *children[1].get_mut(key).unwrap() = Tag::String(value.into());
            assert!(
                read_start(&broken).is_none(),
                "one bad child rejects the entire start: {key}"
            );
        }
    }
}
