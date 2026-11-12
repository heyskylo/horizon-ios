//! `FishingHook.getOpenWaterTypeForBlock` tests `!state.isAir()`, and `isAir()` covers
//! cave_air and void_air: a hook over a source under a cave's air is open water, so its
//! catch can be treasure. (A stone block is not air, so it is not open water either.)
use super::{FISHING_TABLE, fishing_loot};
use horizon_world::loot::LootStack;

#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn cave_air_above_a_water_source_is_open_water_and_catches_treasure() {
    let mut game = super::super::native_machines::tests::server_game();
    let (air, cave_air, void_air, water, stone) = {
        let registry = &game.pack.assets.registry;
        let id = |name: &str| registry.default_state(name).unwrap();
        (
            id("minecraft:air"),
            id("minecraft:cave_air"),
            id("minecraft:void_air"),
            id("minecraft:water"),
            id("minecraft:stone"),
        )
    };
    // The three air blocks read alike: ABOVE_WATER (0), never INVALID (2).
    for (name, state) in [("air", air), ("cave_air", cave_air), ("void_air", void_air)] {
        game.world.set(5, 63, 5, state).unwrap();
        assert_eq!(game.open_water_type(5, 63, 5), 0, "{name} is air");
    }
    // calculateOpenWater's 5x4x5 area around the hook's block: a source at -1 and the
    // cave's air at 0..2, as in a mineshaft room filled with water.
    for x in 3..=7 {
        for z in 3..=7 {
            game.world.set(x, 60, z, water).unwrap();
            for y in 61..=63 {
                game.world.set(x, y, z, cave_air).unwrap();
            }
        }
    }
    assert!(
        game.calculate_open_water([5, 61, 5]),
        "cave_air above a source is open water"
    );
    // One block that is not air: the area is not open water.
    game.world.set(5, 62, 5, stone).unwrap();
    assert!(!game.calculate_open_water([5, 61, 5]));
    game.world.set(5, 62, 5, cave_air).unwrap();

    // The flag the hook's retrieval reads: `gameplay/fishing`'s treasure entry is
    // conditioned on `in_open_water`, so the cave's air decides whether treasure comes.
    let loot =
        horizon_client::structure_blocks::loot_data(&game.options.resources, &game.pack_files())
            .expect("the fishing loot table");
    // `gameplay/fishing/treasure`'s items (26.3's table).
    let treasure = [
        "minecraft:name_tag",
        "minecraft:saddle",
        "minecraft:bow",
        "minecraft:fishing_rod",
        "minecraft:book",
        "minecraft:nautilus_shell",
    ];
    let catches = |open_water: bool| -> Vec<LootStack> {
        (0..400)
            .flat_map(|seed| {
                let mut sequences = horizon_client::random_sequences::RandomSequences::default();
                let mut level_random = horizon_core::random::LegacyRandom::new(seed);
                fishing_loot(
                    &loot,
                    FISHING_TABLE,
                    &mut sequences,
                    &mut level_random,
                    seed,
                    0.0,
                    None,
                    open_water,
                )
            })
            .collect()
    };
    let treasure_catches = |items: &[LootStack]| {
        items
            .iter()
            .filter(|item| treasure.contains(&item.item.as_str()))
            .count()
    };
    assert_eq!(
        treasure_catches(&catches(false)),
        0,
        "not open water: no treasure"
    );
    assert!(
        treasure_catches(&catches(true)) > 0,
        "open water can catch treasure"
    );
}
