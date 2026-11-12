//! The structure types whose pieces read their own tags rather than a
//! template: `MineshaftPieces`, `NetherFortressPieces` and
//! `StrongholdPieces`, registered as `StructurePieceType.ContextlessType`s.
use super::{
    BoundingBox,
    fortress::FortressPiece,
    legacy::{LegacyPiece, PieceFrame},
    mineshaft::MineshaftPiece,
    start::{StructureDef, StructureKind},
    stronghold::StrongholdPiece,
};
use crate::{
    datafix::dynamic::{Number, as_number},
    nbt::Tag,
};
use horizon_core::geometry::Direction;
use std::{collections::BTreeSet, sync::Arc};

/// `CompoundTag.getIntOr`: `Mth.floor` of a float or double tag, the low 32
/// bits of the others, 0 for a missing or non-numeric one.
pub(crate) fn int(tag: &Tag, key: &str) -> i32 {
    match tag.get(key).and_then(as_number) {
        Some(Number::Float(v)) => v.floor() as i32,
        Some(Number::Double(v)) => v.floor() as i32,
        Some(n) => n.int_value(),
        None => 0,
    }
}
/// `CompoundTag.getBooleanOr`: any numeric tag, by its low 8 bits.
pub(crate) fn flag(tag: &Tag, key: &str) -> bool {
    int(tag, key) as i8 != 0
}
/// `Direction.from2DDataValue`, which is `Mth.abs(data % 4)` and not a positive
/// modulo: the wrap of a negative `O` picks the same side vanilla picks.
pub(crate) fn direction_2d(value: i32) -> Direction {
    [
        Direction::South,
        Direction::West,
        Direction::North,
        Direction::East,
    ][(value % 4).unsigned_abs() as usize]
}
/// `BoundingBox.CODEC`: six ints, `min` then `max`, with the constructor's swap
/// of inverted bounds.
pub(crate) fn read_box(tag: &Tag) -> Option<BoundingBox> {
    let b = crate::datafix::dynamic::as_int_stream(Some(tag))?;
    let [x0, y0, z0, x1, y1, z1] = <[i32; 6]>::try_from(b.as_slice()).ok()?;
    Some(BoundingBox::from_corners([x0, y0, z0], [x1, y1, z1]))
}
/// `StructurePiece(StructurePieceType, CompoundTag)`'s own fields: the saved
/// box (vanilla requires it, `orElseThrow`), `O` (the orientation's 2D data
/// value, -1 for none) and `GD`.
pub(crate) fn read_frame(tag: &Tag) -> Option<(PieceFrame, i32)> {
    let bounding_box = read_box(tag.get("BB")?)?;
    let o = int(tag, "O");
    let orientation = (o != -1).then(|| direction_2d(o));
    Some((PieceFrame::new(bounding_box, orientation), int(tag, "GD")))
}

/// `StructurePieceType`'s id for these three families, read as their tag
/// constructors do. `None` for another id and for a piece that does not
/// decode, which leaves its start to regeneration.
pub(crate) fn read(id: &str, tag: &Tag, def: &StructureDef) -> Option<Box<dyn LegacyPiece>> {
    let (frame, depth) = read_frame(tag)?;
    match id {
        "msroom" | "mscorridor" | "mscrossing" | "msstairs" => {
            // The #mineshaft_blocking biome tag is not saved. Use the same
            // resolved tag as generation; every piece of a start shares it.
            let blocking = match &def.kind {
                StructureKind::Mineshaft { blocking, .. } => blocking.clone(),
                _ => Arc::new(BTreeSet::new()),
            };
            MineshaftPiece::from_saved(id, tag, frame, depth, blocking)
                .map(|p| Box::new(p) as Box<dyn LegacyPiece>)
        }
        _ if id.starts_with("ne") => FortressPiece::from_saved(id, tag, frame, depth)
            .map(|p| Box::new(p) as Box<dyn LegacyPiece>),
        _ if id.starts_with("sh") => StrongholdPiece::from_saved(id, tag, frame, depth)
            .map(|p| Box::new(p) as Box<dyn LegacyPiece>),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::worldgen::{
        biome::BiomeId,
        generator_structures::read_saved_start,
        structure::start::{Piece, StructureStart},
    };
    use horizon_core::{density::TerrainAdjustment, position::ChunkPos};

    /// The defs the pieces of each family belong to, with the mineshaft's
    /// unsaved #mineshaft_blocking biomes.
    fn defs() -> [StructureDef; 3] {
        let blocking = Arc::new(BTreeSet::from([BiomeId(3), BiomeId(7)]));
        [
            (
                "test:mineshaft",
                StructureKind::Mineshaft {
                    mesa: false,
                    blocking,
                },
            ),
            ("minecraft:fortress", StructureKind::Fortress),
            ("minecraft:stronghold", StructureKind::Stronghold),
        ]
        .map(|(name, kind)| StructureDef {
            name: name.into(),
            kind,
            biomes: BTreeSet::new(),
            step: 0,
            terrain: TerrainAdjustment::BeardThin,
            enhanced_terrain: None,
        })
    }

    #[test]
    fn saved_pieces_round_trip_and_whole_start_fallback() {
        let defs = defs();
        let StructureKind::Mineshaft { blocking, .. } = &defs[0].kind else {
            unreachable!()
        };
        let families = [
            MineshaftPiece::saved_test_pieces(blocking.clone()),
            FortressPiece::saved_test_pieces(),
            StrongholdPiece::saved_test_pieces(),
        ];
        // A fortress start's first piece is built through the `BridgeCrossing`
        // constructor, so vanilla saves it as `nebcr` and reads that id back as
        // a plain bridge crossing (NetherFortressPieces.java 104-106, 1590-1591,
        // 1604-1605): only the kind's name differs afterwards.
        let kinds = |s: String| s.replace("kind: Start,", "kind: BridgeCrossing,");
        let mut pieces = Vec::new();
        let (resources, bindings) = world();
        let chunk = ChunkPos::new(-3, 5);
        for (def, family) in defs.iter().zip(families) {
            for piece in family {
                let tag = Tag::Compound(piece.save().expect("savable"));
                let Some(Tag::String(id)) = tag.get("id") else {
                    unreachable!()
                };
                let id = id.to_string_lossy();
                let id = id.strip_prefix("minecraft:").unwrap_or(&id);
                let original = StructureStart::new(5, def, chunk, vec![Piece::Legacy(piece)]);
                let saved = original.save().unwrap();
                let loaded = read_saved_start(5, def, chunk, &saved, &resources, &bindings)
                    .unwrap_or_else(|| panic!("{id} decodes"));
                let piece = &original.pieces[0];
                assert_eq!(
                    kinds(format!("{:?}", loaded.pieces[0])),
                    kinds(format!("{piece:?}")),
                    "{id}"
                );
                assert_eq!(loaded.save(), Some(saved), "{id}: every saved field");
                pieces.extend(original.pieces);
            }
        }
        // Both woods and each kind of the mineshaft, every fortress and
        // stronghold kind (the fortress Start writes nebcr too).
        assert_eq!(pieces.len(), 8 + 15 + 13);
        // The same pieces as vanilla's saved `Children`, through
        // `StructureStart.loadStaticStart`.
        let def = &defs[0];
        let chunk = ChunkPos::new(-3, 5);
        let (resources, bindings) = world();
        let mut original = StructureStart::new(5, def, chunk, pieces);
        original.references = 2;
        let tag = original.save().unwrap();
        let loaded = read_saved_start(5, def, chunk, &tag, &resources, &bindings)
            .expect("every piece type decodes");
        assert_eq!(loaded.save(), Some(tag.clone()));
        assert_eq!(
            kinds(format!("{loaded:?}")),
            kinds(format!("{original:?}")),
            "every runtime field"
        );
        // One unreadable child takes the whole start back to regeneration,
        // like part 2's template pieces.
        for (index, key, value) in [
            (0, "id", Tag::String("minecraft:msunknown".into())),
            (1, "BB", Tag::IntArray(vec![0; 5])),
            (23, "EntryDoor", Tag::String("BROKEN".into())),
        ] {
            let mut broken = tag.clone();
            let Some(Tag::List(children)) = broken.get_mut("Children") else {
                unreachable!()
            };
            let child = match &mut children[index] {
                Tag::Compound(c) => c,
                _ => unreachable!(),
            };
            child.insert(key.into(), value);
            assert!(
                read_saved_start(5, def, chunk, &broken, &resources, &bindings).is_none(),
                "child {index}: {key}"
            );
        }
    }

    /// Saved NBT expectations from the vanilla tag constructors, including
    /// NumericTag's flooring versus the direction codec's boxed Number cast.
    #[test]
    fn saved_piece_tag_defaults_and_numeric_conversions() {
        let defs = defs();
        let (resources, bindings) = world();
        let chunk = ChunkPos::new(0, 0);
        for (family, id, input, output_id, output) in [
            (
                0,
                "msroom",
                r#"Entrances:"broken""#,
                "msroom",
                "MST:0,Entrances:[]",
            ),
            (
                0,
                "msroom",
                "Entrances:[[I;1,2,3,4,5]]",
                "msroom",
                "MST:0,Entrances:[]",
            ),
            (
                0,
                "mscrossing",
                "MST:99,tf:-0.25f,D:-3.9f",
                "mscrossing",
                "MST:0,tf:1b,D:3b",
            ),
            (
                0,
                "mscorridor",
                "MST:1,hr:256,sc:-0.25f,hps:-257,Num:-1.5d",
                "mscorridor",
                "MST:1,hr:0b,sc:1b,hps:1b,Num:-2",
            ),
            (1, "nebef", "Seed:-1.0E40d", "nebef", "Seed:-2147483648"),
            (1, "nestart", "", "nebcr", ""),
            (
                2,
                "shsd",
                r#"EntryDoor:"OPENING",Source:1b"#,
                "shsd",
                r#"EntryDoor:"OPENING",Source:1b"#,
            ),
            (
                2,
                "shstart",
                r#"EntryDoor:"WOOD_DOOR",Source:0b"#,
                "shstart",
                r#"EntryDoor:"WOOD_DOOR",Source:0b"#,
            ),
        ] {
            let def = &defs[family];
            // Codec.INT_STREAM accepts a numeric list as well as an int array,
            // and BoundingBox normalizes inverted corners.
            let parse = |text: &str| crate::nbt::parse_snbt(text).unwrap().0;
            let child = parse(&format!(
                r#"{{id:"minecraft:{id}",BB:[4,5,6,1,2,3],O:-3,GD:-1.25f,{input}}}"#
            ));
            let expected = parse(&format!(
                r#"{{id:"minecraft:{output_id}",BB:[I;1,2,3,4,5,6],O:3,GD:-2,{output}}}"#
            ));
            let tag = Tag::compound([
                ("id", Tag::String(def.name.as_str().into())),
                ("ChunkX", Tag::Int(0)),
                ("ChunkZ", Tag::Int(0)),
                ("references", Tag::Int(2)),
                ("Children", Tag::List(vec![child])),
            ]);
            let loaded = read_saved_start(5, def, chunk, &tag, &resources, &bindings)
                .unwrap_or_else(|| panic!("{id}: {input}"));
            let saved = loaded.save().unwrap();
            assert_eq!(
                saved.get("Children"),
                Some(&Tag::List(vec![expected])),
                "{id}: {input}"
            );
            assert_eq!(loaded.references, 2);
        }
    }

    /// `read_saved_start` needs resources and bindings even for pieces that
    /// read neither.
    fn world() -> (
        super::super::StructureResources,
        crate::worldgen::block_feature::FeatureBlockBindings<'static>,
    ) {
        let (mut tag, _) = crate::nbt::parse_snbt(
            r#"{DataVersion:0,states:[
            {id:"minecraft:air"},{id:"minecraft:cave_air"},{id:"minecraft:void_air"}],defaults:{
            "minecraft:air":0,"minecraft:cave_air":1,"minecraft:void_air":2}}"#,
        )
        .unwrap();
        *tag.get_mut("DataVersion").unwrap() = Tag::Int(crate::DATA_VERSION);
        let registry: &'static _ = Box::leak(Box::new(
            crate::registry::BlockRegistry::from_nbt(&tag).unwrap(),
        ));
        (
            super::super::StructureResources::new(
                registry,
                super::super::StateTransforms::default(),
                std::path::Path::new("unused"),
                std::collections::BTreeMap::new(),
                &std::collections::BTreeMap::new(),
            ),
            crate::worldgen::block_feature::FeatureBlockBindings::new(registry, BTreeSet::new())
                .unwrap(),
        )
    }
}
