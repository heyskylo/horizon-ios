use horizon_world::{
    compression::Compression,
    nbt::{self, Limits, NamedTag, Tag},
    registry::{BlockRegistry, BlockStateId},
    worldgen::structure::{StateTransforms, StructureResources, template::StructureTemplate},
};

#[test]
fn templates_read_numeric_fields_like_vanilla() {
    let state = |name: &str| Tag::compound([("id", Tag::String(name.into()))]);
    let registry = BlockRegistry::from_nbt(&Tag::compound([
        ("DataVersion", Tag::Int(horizon_core::version::DATA_VERSION)),
        (
            "states",
            Tag::List(vec![state("minecraft:air"), state("minecraft:stone")]),
        ),
        (
            "defaults",
            Tag::compound([
                ("minecraft:air", Tag::Int(0)),
                ("minecraft:stone", Tag::Int(1)),
            ]),
        ),
    ]))
    .unwrap();
    let registry = Box::leak(Box::new(registry));
    let transforms = StateTransforms::from_json(
        r#"{"rotate":[[0,0,0],[1,1,1]],"mirror":[[0,0],[1,1]],"full_block":[false,true],"loot_containers":[],"block_entities":{}}"#,
        2,
    ).unwrap();
    let pos = Tag::List(vec![
        Tag::Float((-1.2f32).to_bits()),
        Tag::Long(4_294_967_297),
        Tag::Double((-0.5f64).to_bits()),
    ]);
    let blocks = [
        Tag::Byte(1),
        Tag::Short(1),
        Tag::Int(1),
        Tag::Long(4_294_967_297),
        Tag::Float(1.9f32.to_bits()),
        Tag::Double(1.9f64.to_bits()),
    ]
    .into_iter()
    .map(|index| Tag::compound([("pos", pos.clone()), ("state", index)]))
    .collect();
    let tag = Tag::compound([
        (
            "size",
            Tag::List(vec![
                Tag::Short(4),
                Tag::Long(6),
                Tag::Float(8.9f32.to_bits()),
            ]),
        ),
        (
            "palette",
            Tag::List(vec![state("minecraft:air"), state("minecraft:stone")]),
        ),
        ("blocks", Tag::List(blocks)),
        (
            "entities",
            Tag::List(vec![Tag::compound([
                (
                    "pos",
                    Tag::List(vec![
                        Tag::Float(1.25f32.to_bits()),
                        Tag::Int(2),
                        Tag::Long(3),
                    ]),
                ),
                (
                    "blockPos",
                    Tag::List(vec![Tag::Byte(1), Tag::Short(2), Tag::Long(3)]),
                ),
                (
                    "nbt",
                    Tag::compound([("id", Tag::String("minecraft:pig".into()))]),
                ),
            ])]),
        ),
    ]);
    let raw = nbt::write_named(
        &NamedTag {
            name: "".into(),
            tag: tag.clone(),
        },
        Limits::templates(),
    )
    .unwrap();
    let compressed = Compression::Gzip.encode(&raw).unwrap();
    let tree = StructureTemplate::from_nbt(&tag, registry, &transforms);
    let resources = StructureResources::new(
        registry,
        transforms,
        std::path::Path::new("/nonexistent"),
        Default::default(),
        &Default::default(),
    )
    .with_pack_templates([("test:numeric".into(), compressed.into())].into());
    let loaded = resources.template("test:numeric").unwrap();
    // StructureTemplate.load/loadPalette use NumericTag accessors, not exact tag types.
    for template in [&tree, loaded.as_ref()] {
        assert_eq!(template.size, [4, 6, 8]);
        let blocks = template.palettes()[0].blocks();
        assert_eq!(blocks.len(), 6);
        for block in blocks {
            assert_eq!(block.pos, [-2, 1, -1]);
            assert_eq!(block.state, BlockStateId(1));
        }
        assert_eq!(template.entities.len(), 1);
        assert_eq!(template.entities[0].pos, [1.25, 2.0, 3.0]);
        assert_eq!(template.entities[0].block_pos, [1, 2, 3]);
    }
}
