use super::*;

fn fixture() -> (BlockRegistry, Biomes) {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let root = std::env::temp_dir().join(format!(
        "horizon-opened-loot-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir(&root).unwrap();
    let maps = root.join("assets/minecraft/textures/colormap");
    fs::create_dir_all(&maps).unwrap();
    // Original one-pixel fixtures: chunk persistence only needs biome names.
    for name in ["grass", "foliage", "dry_foliage"] {
        let file = fs::File::create(maps.join(format!("{name}.png"))).unwrap();
        let mut encoder = png::Encoder::new(file, 1, 1);
        encoder.set_color(png::ColorType::Rgb);
        encoder
            .write_header()
            .unwrap()
            .write_image_data(&[0, 0, 0])
            .unwrap();
    }
    let folder = root.join("data/minecraft/worldgen/biome");
    fs::create_dir_all(&folder).unwrap();
    fs::write(folder.join("plains.json"), "{}").unwrap();
    let biomes = Biomes::load(&root).unwrap();
    fs::remove_dir_all(&root).unwrap();
    let registry = BlockRegistry::from_nbt(&Tag::compound([
        ("DataVersion", Tag::Int(DATA_VERSION)),
        (
            "states",
            Tag::List(vec![
                Tag::compound([("id", string("minecraft:air"))]),
                Tag::compound([("id", string("minecraft:chest"))]),
            ]),
        ),
        (
            "defaults",
            Tag::compound([
                ("minecraft:air", Tag::Int(0)),
                ("minecraft:chest", Tag::Int(1)),
            ]),
        ),
    ]))
    .unwrap();
    (registry, biomes)
}

fn reload_loot_case(
    registry: &BlockRegistry,
    biomes: &Biomes,
    table: &str,
    seed: i64,
    mode: u8,
) -> Chunk {
    let pos = (1, 64, 2);
    let mut slots = vec![None; 27];
    slots[0] = Some(crate::items::ItemStack::new("minecraft:gold_ingot", 2));
    let source = Tag::compound([
        ("id", string("minecraft:chest")),
        ("x", Tag::Int(pos.0)),
        ("y", Tag::Int(pos.1)),
        ("z", Tag::Int(pos.2)),
        ("LootTable", string(table)),
        ("LootTableSeed", Tag::Long(seed)),
        ("CustomName", string("{\"text\":\"Keep me\"}")),
    ]);
    let mut extras = ChunkExtras {
        block_entity_sources: vec![(pos, source)],
        rolled_loot: vec![RolledLoot {
            pos,
            table: table.into(),
            seed: Some(seed),
            items: encode_items(&slots),
        }],
        ..Default::default()
    };
    // Exactly the production transition used when the player opens the chest.
    if mode != 0 {
        consume_container_loot(&mut extras, pos);
        slots.fill(None);
        if mode == 2 {
            slots[0] = Some(crate::items::ItemStack::new("minecraft:gold_ingot", 1));
        }
    }
    let mut chunk = Chunk::empty(0, 0);
    chunk.set(pos.0, pos.1, pos.2, 1);
    chunk
        .block_entities
        .push((pos, crate::containers::BlockEntity::Chest(slots)));
    chunk.saved = Some(Arc::new(extras));
    let encoded = encode(&chunk, registry, biomes);
    let bytes = nbt::write_named(&encoded, Limits::default()).unwrap();
    let (encoded, used) = nbt::read_named(&bytes, Limits::default()).unwrap();
    assert_eq!(used, bytes.len());
    decode(&encoded, registry, biomes).unwrap()
}

#[test]
fn emptied_opened_loot_stays_empty_after_chunk_save_and_reload() {
    let (registry, biomes) = fixture();
    let loaded = reload_loot_case(&registry, &biomes, "minecraft:chests/ruined_portal", 42, 1);
    let source = &loaded.saved.as_ref().unwrap().block_entity_sources[0].1;
    assert!(
        source.get("LootTable").is_none(),
        "opened chest keeps a rerollable loot table: {source:?}"
    );
    assert!(source.get("LootTableSeed").is_none());
    assert_eq!(
        text(source.get("CustomName")).as_deref(),
        Some("{\"text\":\"Keep me\"}")
    );
    let crate::containers::BlockEntity::Chest(slots) = &loaded.block_entities[0].1 else {
        panic!("chest");
    };
    assert!(slots.iter().all(Option::is_none));
}

#[test]
fn consuming_one_table_preserves_unopened_neighbors_and_other_fields() {
    let pos = (1, 64, 2);
    let other = (2, 64, 2);
    let tag = Tag::compound([
        ("LootTable", string("minecraft:chests/ruined_portal")),
        ("LootTableSeed", Tag::Long(42)),
        ("CustomName", string("keep")),
    ]);
    let rolled = |pos| RolledLoot {
        pos,
        table: "minecraft:chests/ruined_portal".into(),
        seed: Some(42),
        items: Tag::List(vec![]),
    };
    let mut extras = ChunkExtras {
        block_entity_sources: vec![(pos, tag.clone()), (other, tag.clone())],
        rolled_loot: vec![rolled(pos), rolled(other)],
        ..Default::default()
    };
    consume_container_loot(&mut extras, pos);
    assert_eq!(extras.rolled_loot, vec![rolled(other)]);
    assert_eq!(extras.block_entity_sources[1].1, tag);
    assert_eq!(
        extras.block_entity_sources[0].1.get("CustomName"),
        Some(&string("keep"))
    );
    let saved = extras.block_entity_sources.clone();
    consume_container_loot(&mut extras, pos);
    assert_eq!(extras.block_entity_sources, saved);
    // No rolled marker means an unopened source is not consumed by accident.
    extras.rolled_loot.clear();
    consume_container_loot(&mut extras, other);
    assert_eq!(extras.block_entity_sources[1].1, tag);
}

#[test]
#[ignore = "run by tools/test_opened_loot_parity.py"]
fn opened_loot_parity_fixture() {
    use std::io::Write;
    let (registry, biomes) = fixture();
    let input = fs::read_to_string(std::env::var("HORIZON_OPENED_LOOT_INPUT").unwrap()).unwrap();
    let mut output =
        fs::File::create(std::env::var("HORIZON_OPENED_LOOT_OUTPUT").unwrap()).unwrap();
    for line in input.lines() {
        let fields: Vec<_> = line.split_whitespace().collect();
        let mode = fields[2].parse().unwrap();
        let chunk = reload_loot_case(
            &registry,
            &biomes,
            fields[0],
            fields[1].parse().unwrap(),
            mode,
        );
        let source = &chunk.saved.as_ref().unwrap().block_entity_sources[0].1;
        let crate::containers::BlockEntity::Chest(slots) = &chunk.block_entities[0].1 else {
            panic!("chest");
        };
        let count = if mode == 0 {
            "unopened".into()
        } else {
            slots
                .iter()
                .flatten()
                .map(|s| s.count)
                .sum::<u32>()
                .to_string()
        };
        writeln!(
            output,
            "{} {} {count}",
            u8::from(source.get("LootTable").is_some()),
            u8::from(source.get("LootTableSeed").is_some())
        )
        .unwrap();
    }
}
