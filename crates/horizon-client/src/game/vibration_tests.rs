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

#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn saddling_a_pig_posts_equip_from_the_pig() {
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
    let kind = game.res.mob_types().get("minecraft:pig").unwrap().clone();
    game.mobs
        .push(Mob::new(42, kind, Vec3::new(2.5, 4.0, 3.5), 0.0));
    *game.held_mut() = Some(ItemStack::new("minecraft:saddle", 1));
    super::sculk::TEST_POSTED_EVENTS.with(|events| *events.borrow_mut() = Some(Vec::new()));
    assert!(game.interact_mount(0, "minecraft:saddle"));
    super::sculk::TEST_POSTED_EVENTS.with(|events| {
        assert!(events.borrow().as_ref().unwrap().contains(&(
            "minecraft:equip",
            [2.5, 4.0, 3.5],
            Some(Who::Mob(42))
        )));
    });
}

/// `CauldronInteractions`: a bucket dipped into a cauldron ends with
/// `level.gameEvent(null, GameEvent.FLUID_PICKUP, pos)` and one poured into
/// it with `FLUID_PLACE`, both after `playSound`; a glass bottle takes a level
/// off first (`lowerFillLevel`'s `BLOCK_CHANGE`, already posted) and then
/// posts `FLUID_PICKUP`, while washing a banner posts only the `BLOCK_CHANGE`.
#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn a_cauldron_use_posts_its_fluid_event_after_the_sound() {
    use horizon_world::registry::BlockStateId;
    type Posted = (&'static str, [f64; 3], Option<Who>);
    fn use_on(game: &mut Game<'_>, cauldron: BlockStateId, held: ItemStack) -> Vec<Posted> {
        game.world.set(2, 4, 3, cauldron).unwrap();
        *game.held_mut() = Some(held);
        super::sculk::TEST_POSTED_EVENTS.with(|events| *events.borrow_mut() = Some(Vec::new()));
        assert!(game.use_on_cauldron([2, 4, 3]).unwrap(), "the use acts");
        super::sculk::TEST_POSTED_EVENTS.with(|events| events.borrow_mut().take().unwrap())
    }
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
    let registry = game.pack.assets.registry;
    let cauldron = |name: &str, level: Option<u8>| {
        registry
            .resolve(&horizon_world::chunk::BlockState {
                name: name.to_owned(),
                properties: level
                    .map(|level| ("level".to_owned(), level.to_string()))
                    .into_iter()
                    .collect(),
            })
            .unwrap()
    };
    let centre = [2.5, 4.5, 3.5];
    let pickup = ("minecraft:fluid_pickup", centre, None);
    let place = ("minecraft:fluid_place", centre, None);
    let change = ("minecraft:block_change", centre, None);

    // WATER.put(BUCKET, ...fillBucket): the full cauldron empties, then
    // `playSound(BUCKET_FILL)` and `gameEvent(FLUID_PICKUP)`.
    let events = use_on(
        &mut game,
        cauldron("minecraft:water_cauldron", Some(3)),
        ItemStack::new("minecraft:bucket", 1),
    );
    assert_eq!(
        game.world.block(2, 4, 3),
        cauldron("minecraft:cauldron", None)
    );
    assert_eq!(
        game.held().map(|s| s.item.to_string()).as_deref(),
        Some("minecraft:water_bucket")
    );
    assert_eq!(events, vec![pickup]);

    // LAVA/POWDER_SNOW's bucket, and `emptyBucket` for every bucket poured in.
    let events = use_on(
        &mut game,
        cauldron("minecraft:cauldron", None),
        ItemStack::new("minecraft:water_bucket", 1),
    );
    assert_eq!(
        game.world.block(2, 4, 3),
        cauldron("minecraft:water_cauldron", Some(3))
    );
    assert_eq!(events, vec![place]);

    // WATER.put(GLASS_BOTTLE): `lowerFillLevel` posts BLOCK_CHANGE, then the
    // sound, then FLUID_PICKUP.
    let events = use_on(
        &mut game,
        cauldron("minecraft:water_cauldron", Some(3)),
        ItemStack::new("minecraft:glass_bottle", 1),
    );
    assert_eq!(
        game.world.block(2, 4, 3),
        cauldron("minecraft:water_cauldron", Some(2))
    );
    assert_eq!(events, vec![change, pickup]);

    // EMPTY.put(POTION, ...): a water potion fills an empty cauldron, then
    // `playSound(BOTTLE_EMPTY)` and `gameEvent(FLUID_PLACE)`.
    let events = use_on(
        &mut game,
        cauldron("minecraft:cauldron", None),
        ItemStack::new("minecraft:potion", 1).with_potion("minecraft:water"),
    );
    assert_eq!(
        game.world.block(2, 4, 3),
        cauldron("minecraft:water_cauldron", Some(1))
    );
    assert_eq!(events, vec![place]);

    // WATER.put(POTION, ...): the level cycles up, then `playSound(BOTTLE_EMPTY)`
    // and `gameEvent(FLUID_PLACE)`.
    let events = use_on(
        &mut game,
        cauldron("minecraft:water_cauldron", Some(2)),
        ItemStack::new("minecraft:potion", 1).with_potion("minecraft:water"),
    );
    assert_eq!(
        game.world.block(2, 4, 3),
        cauldron("minecraft:water_cauldron", Some(3))
    );
    assert_eq!(events, vec![place]);

    // Washing a banner: `bannerInteraction` lowers the level and stops there.
    let mut banner = ItemStack::new("minecraft:white_banner", 1);
    banner
        .data_mut()
        .banner_patterns
        .push(horizon_items::banner_layer::BannerLayer::new(
            "minecraft:stripe_top",
            "minecraft:red",
        ));
    let events = use_on(
        &mut game,
        cauldron("minecraft:water_cauldron", Some(3)),
        banner,
    );
    assert_eq!(
        game.world.block(2, 4, 3),
        cauldron("minecraft:water_cauldron", Some(2))
    );
    assert_eq!(events, vec![change]);
}
