//! Campfire food placement's gate and criterion: `CampfireBlock.useItemOn`
//! reached through `ServerPlayerGameMode.useItemOn`, which skips the block
//! while sneaking with something in hand and then triggers
//! ITEM_USED_ON_BLOCK for the use it consumed.
use super::*;
use crate::game::{Hit, native_machines::tests::server_game};
use horizon_client::{
    advancements::{Advancement, Criterion, Rewards, Tree},
    held::Source,
    items::ItemStack,
    motion::PlayerMotion,
};
use horizon_core::geometry::Vec3;
use horizon_world::lazy_json::LazyJson;
use std::sync::Arc;
use winit::keyboard::KeyCode;

/// A server game with a campfire at `pos` (its block entity too) and beef in
/// the main hand, the click forced onto that block.
fn campfire_game(pos: [i32; 3]) -> Game<'static> {
    let mut game = server_game();
    // A local player, not a dedicated server's silent world: criteria and
    // statistics apply to them.
    game.options.dedicated = None;
    let state = game
        .pack
        .assets
        .registry
        .default_state("minecraft:campfire")
        .unwrap();
    game.world.set(pos[0], pos[1], pos[2], state).unwrap();
    game.world.block_entities.put(
        (pos[0], pos[1], pos[2]),
        BlockEntity::Campfire(Box::default()),
    );
    let mut player = PlayerMotion::new(
        &game.motion,
        Vec3::new(
            f64::from(pos[0]) + 0.5,
            f64::from(pos[1]),
            f64::from(pos[2]) + 2.5,
        ),
    )
    .unwrap();
    // `Player.canEat`: hungry, so the beef's own use can start.
    player.health.food.food = 6;
    game.player = Some(player);
    game.creative = false;
    *game.held_mut() = Some(ItemStack::new("minecraft:beef", 4));
    game.forced_hit = Some(Hit {
        pos,
        face: [0, 1, 0],
        point: [
            f64::from(pos[0]) + 0.5,
            f64::from(pos[1]) + 1.0,
            f64::from(pos[2]) + 0.5,
        ],
    });
    game
}

/// The campfire's four cooking slots.
fn slots(game: &Game<'_>, pos: [i32; 3]) -> usize {
    match game.world.block_entities.get(&(pos[0], pos[1], pos[2])) {
        Some(BlockEntity::Campfire(c)) => c.items.iter().filter(|i| i.is_some()).count(),
        _ => panic!("no campfire block entity at {pos:?}"),
    }
}

/// `ServerPlayerGameMode.useItemOn`: with something in hand, sneaking skips
/// `state.useItemOn` entirely (`suppressUsingBlock`), so the beef is neither
/// placed nor consumed; the item's own use, eating, starts instead. Without
/// the sneak the campfire takes the beef, and the use it consumed triggers
/// ITEM_USED_ON_BLOCK with the stack before the use.
#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn a_sneak_keeps_the_food_and_placing_it_fires_item_used_on_block() {
    const POS: [i32; 3] = [2, 4, 3];
    let mut game = campfire_game(POS);
    // The tree a data pack with one `item_used_on_block` criterion on this
    // campfire would be indexed into.
    let mut tree = Tree::default();
    tree.list.push(Advancement {
        id: "horizon:test/campfire".into(),
        parent: None,
        display: None,
        criteria: vec![Criterion {
            name: "food".into(),
            trigger: "minecraft:item_used_on_block".into(),
            conditions: LazyJson::from(serde_json::json!({
                "location": {
                    "type": "minecraft:location_check",
                    "predicate": {"block": {"blocks": "minecraft:campfire"}}
                }
            })),
        }],
        requirements: vec![vec!["food".into()]],
        rewards: Rewards::default(),
    });
    tree.by_id.insert("horizon:test/campfire".into(), 0);
    tree.by_trigger
        .insert("minecraft:item_used_on_block".into(), vec![(0, 0)]);
    game.adv.tree = Arc::new(tree);
    let hit = game.forced_hit.unwrap();
    game.keys.press(KeyCode::ShiftLeft, Source::Keyboard);
    assert!(game.use_hand(Some(&hit)).unwrap());
    assert_eq!(slots(&game, POS), 0, "the sneak put no beef on the fire");
    assert_eq!(game.held().unwrap().count, 4, "the beef stayed in hand");
    assert!(game.using.is_some(), "the beef is being eaten");
    assert!(
        !game.adv.progress.has("horizon:test/campfire", "food"),
        "the skipped block use triggers nothing"
    );
    // Stop eating, let go of the sneak: the same click now feeds the fire.
    game.using = None;
    game.keys.release(&KeyCode::ShiftLeft, Source::Keyboard);
    assert!(game.use_hand(Some(&hit)).unwrap());
    assert_eq!(slots(&game, POS), 1, "the beef went on the fire");
    assert_eq!(game.held().unwrap().count, 3, "one beef was used");
    assert!(game.using.is_none(), "the click was consumed");
    assert!(
        game.adv.progress.has("horizon:test/campfire", "food"),
        "the placed food fired item_used_on_block"
    );
}
