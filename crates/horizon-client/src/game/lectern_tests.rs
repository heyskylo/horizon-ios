//! Lectern placement and the `block_change` game event of a book going on or
//! off (`LecternBlock.getStateForPlacement`, `LecternBlock.placeBook` and
//! `LecternBlockEntity.onBookItemRemove`).
use super::super::native_machines::tests::server_game;
use super::Game;
use crate::game::{Hit, sculk::TEST_POSTED_EVENTS};
use horizon_client::{
    containers::{BlockEntity, Lectern},
    items::ItemStack,
    mobs::Who,
};

/// A lectern at `pos`: the state's `has_book`, and the entity's book.
fn lectern(game: &mut Game<'_>, pos: [i32; 3], has_book: bool, book: Option<ItemStack>) {
    let registry = game.pack.assets.registry;
    let default = registry.default_state("minecraft:lectern").unwrap();
    let mut state = registry.state(default).unwrap().clone();
    state
        .properties
        .insert("has_book".into(), has_book.to_string());
    let state = registry.id(&state).unwrap();
    game.world.set(pos[0], pos[1], pos[2], state).unwrap();
    game.world.block_entities.insert(
        (pos[0], pos[1], pos[2]),
        BlockEntity::Lectern(Lectern { book, page: 0 }),
    );
}

/// The lectern item `/give` hands out with a written book in its
/// `block_entity_data`.
fn lectern_item() -> ItemStack {
    let text = r#"{"minecraft:block_entity_data":{id:"minecraft:lectern",Book:{id:"minecraft:written_book",count:1,components:{"minecraft:written_book_content":{title:"t",author:"a",pages:["p1","p2"]}}}}}"#;
    let components = horizon_world::nbt::parse_snbt(text).expect("snbt").0;
    let (damage, extra) = horizon_client::item_data::decode_components(Some(&components));
    ItemStack {
        damage,
        extra,
        ..ItemStack::new("minecraft:lectern", 1)
    }
}

fn click(pos: [i32; 3]) -> Hit {
    Hit {
        pos,
        face: [0, 1, 0],
        point: [f64::from(pos[0]) + 0.5, f64::from(pos[1]), 0.5],
    }
}

/// The `has_book` of the lectern state at `pos`, and what a comparator beside
/// it reads (`LecternBlock.getAnalogOutputSignal`).
fn state_at(game: &Game<'_>, pos: [i32; 3]) -> (String, i32) {
    let registry = game.pack.assets.registry;
    let state = game.world.block(pos[0], pos[1], pos[2]);
    let has_book = registry
        .state(state)
        .unwrap()
        .properties
        .get("has_book")
        .cloned()
        .unwrap_or_default();
    let at = horizon_core::position::BlockPos::new(pos[0], pos[1], pos[2]);
    let comparator = super::lectern_signal(&game.world, registry, state, at).expect("a lectern");
    (has_book, comparator)
}

/// `LecternBlock.getStateForPlacement`: the state's `has_book` comes from the
/// item's `block_entity_data.Book` for a placer who may use game-master blocks
/// (`minecraft:lectern` is in `BlockEntityTypes.OP_ONLY_CUSTOM_DATA`, so the
/// block entity only takes the book for one too), and not for anyone else.
#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn a_placed_lectern_takes_has_book_from_its_item() {
    let mut game = server_game();
    game.set_mode_id(1);
    game.ui.registries.permission = 2;
    *game.held_mut() = Some(lectern_item());
    assert!(game.use_hand(Some(&click([8, 64, 8]))).unwrap());
    assert!(
        matches!(game.world.block_entities.get(&(8, 64, 8)), Some(BlockEntity::Lectern(l)) if l.book.is_some())
    );
    // A two-page book on page 0: `floor(0 / 1 * 14) + 1`.
    assert_eq!(state_at(&game, [8, 64, 8]), ("true".into(), 1));

    // The same item from a placer who may not use game-master blocks gains
    // neither the state nor the block entity book, so the comparator reads 0.
    game.ui.registries.permission = 0;
    *game.held_mut() = Some(lectern_item());
    assert!(game.use_hand(Some(&click([10, 64, 8]))).unwrap());
    assert!(
        matches!(game.world.block_entities.get(&(10, 64, 8)), Some(BlockEntity::Lectern(l)) if l.book.is_none())
    );
    assert_eq!(state_at(&game, [10, 64, 8]), ("false".into(), 0));
}

fn record_events(record: bool) {
    TEST_POSTED_EVENTS.with(|e| *e.borrow_mut() = record.then(Vec::new));
}

/// The `block_change` game events posted since `record_events(true)`, with
/// their position and source entity.
fn block_changes() -> Vec<(&'static str, [f64; 3], Option<Who>)> {
    TEST_POSTED_EVENTS.with(|e| {
        let events = e.borrow();
        let changes = events.as_ref().unwrap().iter();
        changes
            .filter(|(e, _, _)| *e == "minecraft:block_change")
            .copied()
            .collect()
    })
}

/// `LecternBlock.placeBook` -> `resetBookState` with the player as the source
/// entity, and `LecternBlockEntity.onBookItemRemove` -> `resetBookState(null,
/// ...)`: one `block_change` at the lectern each way, and `has_book` follows.
#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn a_book_going_on_or_off_posts_one_block_change() {
    let mut game = server_game();
    let key = (8, 64, 8);
    lectern(&mut game, [8, 64, 8], false, None);
    *game.held_mut() = Some(ItemStack::new("minecraft:written_book", 1));
    record_events(true);
    assert!(game.use_hand(Some(&click([8, 64, 8]))).unwrap());
    assert_eq!(
        block_changes(),
        [(
            "minecraft:block_change",
            [8.5, 64.5, 8.5],
            Some(Who::Player)
        )]
    );
    assert_eq!(state_at(&game, [8, 64, 8]).0, "true");

    // Taking it back off through the lectern's menu.
    record_events(true);
    let mut menu = game.lectern_menu(key).expect("the lectern's menu");
    menu.set_slot(&game.menu_env(), 0, None);
    game.lectern_menu_done(key, &menu);
    assert_eq!(
        block_changes(),
        [("minecraft:block_change", [8.5, 64.5, 8.5], None)]
    );
    assert_eq!(state_at(&game, [8, 64, 8]).0, "false");
    record_events(false);
}
