//! Game event regressions for server-side block actions.
use super::{Audio, Game, Options, Progress, Settings, load_resources};
use horizon_client::{
    chunk::Chunk,
    containers::BlockEntity,
    items::ItemStack,
    mobs::{Mob, Who},
};
use horizon_core::geometry::Vec3;
use std::sync::Arc;
#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn entering_hive_posts_block_change_from_bee() {
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
    game.world
        .chunks
        .insert((0, 0), Arc::new(Chunk::empty(0, 0)));
    let hive = game
        .pack
        .assets
        .registry
        .default_state("minecraft:beehive")
        .unwrap();
    game.world.set(2, 4, 3, hive).unwrap();
    game.world
        .block_entities
        .put((2, 4, 3), BlockEntity::Beehive(Box::default()));
    let kind = game.res.mob_types().get("minecraft:bee").unwrap().clone();
    game.mobs
        .push(Mob::new(42, kind, Vec3::new(2.0, 4.0, 3.0), 0.0));
    super::sculk::TEST_POSTED_EVENTS.with(|events| *events.borrow_mut() = Some(Vec::new()));
    game.bee_enter_hive(42, [2, 4, 3]);
    super::sculk::TEST_POSTED_EVENTS.with(|events| {
        assert!(events.borrow().as_ref().unwrap().contains(&(
            "minecraft:block_change",
            [2.5, 4.5, 3.5],
            Some(Who::Mob(42))
        )));
    });
}
#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn lowering_cauldron_by_bottle_posts_block_change_without_source() {
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
    game.world
        .chunks
        .insert((0, 0), Arc::new(Chunk::empty(0, 0)));
    let water = game
        .pack
        .assets
        .registry
        .default_state("minecraft:water_cauldron")
        .unwrap();
    game.world.set(2, 4, 3, water).unwrap();
    *game.held_mut() = Some(ItemStack::new("minecraft:glass_bottle", 1));
    super::sculk::TEST_POSTED_EVENTS.with(|events| *events.borrow_mut() = Some(Vec::new()));
    assert!(game.use_on_cauldron([2, 4, 3]).unwrap());
    super::sculk::TEST_POSTED_EVENTS.with(|events| {
        assert!(events.borrow().as_ref().unwrap().contains(&(
            "minecraft:block_change",
            [2.5, 4.5, 3.5],
            None
        )));
    });
}

#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn tying_to_fence_and_knot_posts_block_attach_from_player() {
    use horizon_client::mobs::leash::Holder;
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
    let kind = game.res.mob_types().get("minecraft:cow").unwrap().clone();
    let mut cow = Mob::new(42, kind, Vec3::new(2.5, 4.0, 3.5), 0.0);
    cow.extra.leash = Some(Holder::Player);
    cow.extra.leash_player = Some(game.acting_uuid());
    game.mobs.push(cow);
    for (knot_hit, expected) in [(false, [2.5, 4.5, 3.5]), (true, [2.5, 4.375, 3.5])] {
        game.mobs[0].extra.leash = Some(Holder::Player);
        super::sculk::TEST_POSTED_EVENTS.with(|events| *events.borrow_mut() = Some(Vec::new()));
        assert!(game.use_knot([2, 4, 3], knot_hit));
        super::sculk::TEST_POSTED_EVENTS.with(|events| {
            assert!(events.borrow().as_ref().unwrap().contains(&(
                "minecraft:block_attach",
                expected,
                Some(Who::Player)
            )));
        });
    }
}
