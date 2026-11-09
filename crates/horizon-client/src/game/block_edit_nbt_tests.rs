//! Block entity data in a server mod's bulk block edit, through the real edit on a dedicated server's level.
use super::*;
use crate::game::native_world_edit::{NativeBlockEdit, NativeBlockNbt, NativeBlockRun};
use crate::game::{Audio, Options, Progress, Settings, load_resources};
use horizon_client::chunk::Chunk;

const OVERWORLD: &str = "minecraft:overworld";

fn with_server(test: impl FnOnce(&mut Game<'_>)) {
    let root = std::path::PathBuf::from(std::env::var_os("HORIZON_TEST_RESOURCES").unwrap());
    let settings = Settings::default();
    let loaded = load_resources(
        root.clone(),
        None,
        &root,
        &settings,
        std::time::Instant::now(),
        false,
        true,
        &Progress::new(),
        &mut |_, _| unreachable!(),
    )
    .unwrap();
    let mut options = Options::parse(std::iter::empty()).unwrap().unwrap();
    options.resources = root.clone();
    options.execution_role = crate::game::execution_role::ExecutionRole::IntegratedServer;
    let mut g = Game::new(
        &loaded.resources,
        loaded.pack,
        loaded.gui.unwrap(),
        options,
        settings,
        Audio::silent(&root),
        None,
    )
    .unwrap();
    g.build_registries();
    g.options.dedicated = Some(Vec::new());
    g.world = World::new(g.res.facts().air, 1);
    g.world.chunks.insert((0, 0), Arc::new(Chunk::empty(0, 0)));
    test(&mut g);
}

/// One block at `x 64 z`, palette entry `block`.
fn one(x: i32, z: i32, block: u32) -> NativeBlockRun {
    NativeBlockRun {
        x,
        z,
        y: 64,
        height: 1,
        surface: None,
        block,
        only: None,
    }
}

fn nbt(x: i32, z: i32, nbt: &str) -> NativeBlockNbt {
    NativeBlockNbt {
        x,
        y: 64,
        z,
        nbt: nbt.into(),
    }
}

fn tag(snbt: &str) -> Tag {
    parse_block_entity_nbt(snbt).unwrap()
}

/// The chest at `x 64 z`: each slot's item and count.
fn chest(g: &Game<'_>, x: i32, z: i32) -> Vec<Option<(String, u32)>> {
    let Some(BlockEntity::Chest(items)) = g.world.block_entities.get(&(x, 64, z)) else {
        panic!("no chest at {x} 64 {z}");
    };
    items
        .iter()
        .map(|s| s.as_ref().map(|s| (s.item.to_string(), s.count)))
        .collect()
}

fn chest_and_stone() -> NativeBlockEdit {
    NativeBlockEdit {
        palette: vec!["minecraft:chest".into(), "minecraft:stone".into()],
        runs: vec![one(1, 1, 0), one(3, 1, 1)],
        ..Default::default()
    }
}

#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn a_chest_placed_by_a_bulk_edit_keeps_the_items_given() {
    with_server(|g| {
        let mut edit = chest_and_stone();
        edit.block_entities = vec![
            nbt(
                1,
                1,
                r#"{Items:[{Slot:3b,id:"minecraft:diamond",count:5}]}"#,
            ),
            // stone has no block entity: skipped, the edit still made
            nbt(3, 1, r#"{Items:[{Slot:0b,id:"minecraft:dirt",count:1}]}"#),
        ];
        assert_eq!(g.native_edit_blocks(OVERWORLD, &edit), Ok(2));
        let mut want = vec![None; 27];
        want[3] = Some(("minecraft:diamond".to_string(), 5));
        assert_eq!(chest(g, 1, 1), want);
        assert!(g.world.block_entities.get(&(3, 64, 1)).is_none());

        // NBT that doesn't read refuses the whole edit: the chest it would place is not made
        let mut bad = chest_and_stone();
        bad.runs = vec![one(5, 1, 0)];
        bad.block_entities = vec![nbt(5, 1, "{Items:")];
        assert!(g.native_edit_blocks(OVERWORLD, &bad).is_err());
        assert_eq!(g.world.block(5, 64, 1), g.res.facts().air);
        assert!(g.world.block_entities.get(&(5, 64, 1)).is_none());
    });
}

#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn block_entity_data_of_another_type_or_without_a_block_entity_is_refused() {
    with_server(|g| {
        assert_eq!(g.native_edit_blocks(OVERWORLD, &chest_and_stone()), Ok(2));
        let stone = r#"Items:[{Slot:0b,id:"minecraft:stone",count:1}]"#;
        // a furnace's data on a chest: refused, the chest unchanged
        let furnace = tag(&format!(r#"{{id:"minecraft:furnace",{stone}}}"#));
        assert!(!g.set_block_entity_checked([1, 64, 1], furnace));
        assert_eq!(chest(g, 1, 1), vec![None; 27]);
        // no block entity: stone, and air
        for pos in [[3, 64, 1], [2, 64, 1]] {
            assert!(!g.set_block_entity_checked(pos, tag(&format!("{{{stone}}}"))));
            assert!(
                g.world
                    .block_entities
                    .get(&(pos[0], pos[1], pos[2]))
                    .is_none()
            );
        }
        // the chest's own type, its namespace left out, is loaded
        assert!(g.set_block_entity_checked([1, 64, 1], tag(&format!(r#"{{id:"chest",{stone}}}"#))));
        let mut want = vec![None; 27];
        want[0] = Some(("minecraft:stone".to_string(), 1));
        assert_eq!(chest(g, 1, 1), want);
    });
}
