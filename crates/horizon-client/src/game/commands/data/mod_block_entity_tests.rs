//! Actual command execution with prepared resources. No native mod bundle or
//! process-wide trait hooks are installed; this is a generic content fixture.
use super::*;
use crate::game::{Audio, Options, Progress, Settings, load_resources};
use horizon_client::{
    chunk::Chunk,
    containers::{ModBlockEntity, ModRevision},
    mod_content::{ContentMod, build},
    world::World,
};
use horizon_mods::content::{Block, Content};
use std::{path::PathBuf, sync::Arc};

const KEY: (i32, i32, i32) = (2, 64, 2);
const POS: [i32; 3] = [2, 64, 2];
const BLOCK: &str = "fixture:machine";
const ENTITY: &str = "fixture:machine_entity";

fn entity<'a>(game: &'a Game<'_>) -> &'a ModBlockEntity {
    let Some(BlockEntity::Mod(entity)) = game.world.block_entities.get(&KEY) else {
        panic!("fixture mod block entity missing");
    };
    entity
}
fn changed(game: &Game<'_>, before: ModRevision) {
    let entity = entity(game);
    assert_ne!(
        entity.revision(),
        before,
        "saved data invalidates content caches"
    );
    assert_eq!(
        entity.data,
        [71, -3, 907],
        "runtime menu data survives loading"
    );
    assert!(game.world.modified.contains(&(0, 0)), "chunk must be dirty");
    assert!(
        game.world.block_log.as_ref().unwrap().contains(&KEY),
        "normal update must be queued"
    );
}
fn run(game: &mut Game<'_>, command: &str) -> ModRevision {
    let before = entity(game).revision();
    game.world.modified.clear();
    game.world.block_log.as_mut().unwrap().clear();
    assert!(game.run_command(command), "{command}");
    before
}

#[test]
#[ignore = "requires private prepared 26.3 resources in HORIZON_TEST_RESOURCES"]
fn actual_data_commands_preserve_same_mod_entity_runtime_state() {
    let base = PathBuf::from(std::env::var_os("HORIZON_TEST_RESOURCES").unwrap());
    let root =
        PathBuf::from(std::env::var_os("HORIZON_HOME").unwrap()).join("data-command-resources");
    let mut content = Content::declared("fixture");
    content
        .declare(
            &[Block {
                id: BLOCK.into(),
                template: "minecraft:stone".into(),
                ..Default::default()
            }],
            &[],
        )
        .unwrap();
    build(
        &base,
        &[ContentMod {
            id: "fixture".into(),
            content,
        }],
        &root,
    )
    .unwrap();
    let settings = Settings::default();
    let loaded = load_resources(
        root.clone(),
        None,
        &base,
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
    let mut game = Game::new(
        &loaded.resources,
        loaded.pack,
        loaded.gui.unwrap(),
        options,
        settings,
        Audio::silent(&root),
        None,
    )
    .unwrap();
    game.world = World::new(game.res.facts().air, 1);
    game.world
        .chunks
        .insert((0, 0), Arc::new(Chunk::empty(0, 0)));
    let state = game.pack.assets.registry.default_state(BLOCK).unwrap();
    game.world.set(POS[0], POS[1], POS[2], state).unwrap();
    let initial = parse_snbt(
        r#"{id:"fixture:machine_entity",x:2,y:64,z:2,fuel:4,Opaque:{typed:[L;7L,-9L]},Items:[{Slot:0b,id:"minecraft:coal",count:2}]}"#,
    ).unwrap().0;
    let (_, initial) = save::decode_block_entity(&initial).unwrap();
    game.world.block_entities.insert(KEY, initial);
    let Some(BlockEntity::Mod(initial)) = game.world.block_entities.get_mut(&KEY) else {
        panic!()
    };
    initial.data = vec![71, -3, 907];
    let transient_allocation = initial.data.as_ptr();
    game.world.block_log = Some(Vec::new());
    game.build_registries();
    game.set_allow_commands(true);

    let before = run(
        &mut game,
        r#"data merge block 2 64 2 {fuel:41,Opaque:{added:9s},Items:[{Slot:1b,id:"minecraft:diamond",count:3,components:{"minecraft:custom_data":{stamp:8b}}}]}"#,
    );
    changed(&game, before);
    let loaded = entity(&game);
    assert_eq!(loaded.id, ENTITY);
    assert_eq!(loaded.data.as_ptr(), transient_allocation);
    assert_eq!(loaded.nbt.get(&"fuel".into()), Some(&Tag::Int(41)));
    assert_eq!(
        loaded.nbt.get(&"Opaque".into()),
        Some(&Tag::compound([
            ("typed", Tag::LongArray(vec![7, -9])),
            ("added", Tag::Short(9)),
        ]))
    );
    assert_eq!(loaded.items.len(), 2);
    assert!(
        loaded.items[0].is_none(),
        "saved inventory replaces the old slots"
    );
    let stack = loaded.items[1].as_ref().unwrap();
    assert_eq!(stack.item.as_ref(), "minecraft:diamond");
    assert_eq!(stack.count, 3);
    assert_eq!(
        stack.data().unwrap().other("minecraft:custom_data"),
        Some(&Tag::compound([("stamp", Tag::Byte(8))]))
    );

    let before = run(&mut game, "data remove block 2 64 2 Opaque.typed");
    changed(&game, before);
    assert_eq!(
        entity(&game).nbt.get(&"Opaque".into()),
        Some(&Tag::compound([("added", Tag::Short(9))]))
    );
    assert_eq!(entity(&game).items[1].as_ref().unwrap().count, 3);
    let before = run(&mut game, "data remove block 2 64 2 Items");
    changed(&game, before);
    assert!(
        entity(&game).items.is_empty(),
        "removed inventory must not resurrect"
    );
    assert!(!entity(&game).nbt.contains_key(&"Items".into()));
    let before = run(&mut game, "data remove block 2 64 2 Opaque");
    changed(&game, before);
    assert!(!entity(&game).nbt.contains_key(&"Opaque".into()));

    // Unknown, non-inventory Items belongs to the mod's opaque saved data.
    let before = run(&mut game, "data merge block 2 64 2 {Items:{opaque:5b}}");
    changed(&game, before);
    assert!(entity(&game).items.is_empty());
    assert_eq!(
        entity(&game).nbt.get(&"Items".into()),
        Some(&Tag::compound([("opaque", Tag::Byte(5))]))
    );
    let before = run(&mut game, "data remove block 2 64 2 Items");
    changed(&game, before);
    assert!(!entity(&game).nbt.contains_key(&"Items".into()));

    // Keep the pre-existing type-changing behavior: this patch adds no new
    // block/type validation policy to /data, and a new type has fresh state.
    let before = run(
        &mut game,
        r#"data merge block 2 64 2 {id:"fixture:replacement",fuel:12}"#,
    );
    assert_ne!(entity(&game).revision(), before);
    assert!(entity(&game).data.is_empty());
    assert_eq!(entity(&game).id, "fixture:replacement");
    assert_eq!(entity(&game).nbt.get(&"fuel".into()), Some(&Tag::Int(12)));
    assert_eq!(game.world.block(POS[0], POS[1], POS[2]), state);

    // Persistence/chunk loading still constructs a fresh live entity; neither
    // the identity nor runtime menu data is encoded in the normal saved tag.
    let saved = game.data_of(&DataRef::Block(POS)).unwrap();
    let before = entity(&game).revision();
    let (_, decoded) = save::decode_block_entity(&saved).unwrap();
    game.world.block_entities.load_chunk(vec![(KEY, decoded)]);
    assert_ne!(entity(&game).revision(), before);
    assert!(entity(&game).data.is_empty());
    assert_eq!(game.data_of(&DataRef::Block(POS)).unwrap(), saved);
}
