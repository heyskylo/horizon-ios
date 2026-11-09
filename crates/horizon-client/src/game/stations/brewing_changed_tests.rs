use super::super::native_machines::tests::server_game;
use horizon_client::{brewing::BrewingStand, containers::BlockEntity, items::ItemStack};

/// `BrewingStandBlockEntity.serverTick` sets changed when a brew starts
/// (the fuel is taken and the timer begins); the block state only carries
/// the bottle bits, so nothing else marked this start.
#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn starting_a_brew_marks_the_chunk() {
    let mut game = server_game();
    let state = game
        .pack
        .assets
        .registry
        .default_state("minecraft:brewing_stand")
        .unwrap();
    let pos = (8, 64, 8);
    game.world.set(pos.0, pos.1, pos.2, state).unwrap();
    game.world.block_entities.insert(
        pos,
        BlockEntity::Brewing(Box::new(BrewingStand {
            items: [
                Some(ItemStack::new("minecraft:potion", 1).with_potion("minecraft:water")),
                None,
                None,
                Some(ItemStack::new("minecraft:nether_wart", 1)),
                None,
            ],
            brew_time: 0,
            total_brew_time: 400,
            fuel: 10,
            total_fuel: 20,
            speed: 1.0,
            ingredient: None,
            // The bottle bits the block state already shows: this tick
            // changes only the fuel and the timer.
            last_bottles: Some([true, false, false]),
        })),
    );
    game.world.modified.clear();

    game.tick_brewing_stand(pos).expect("the stand ticks");

    let Some(BlockEntity::Brewing(b)) = game.world.block_entities.get(&pos) else {
        panic!("no brewing stand");
    };
    assert_eq!((b.fuel, b.brew_time), (9, 400), "the brew started");
    assert!(
        game.world.modified.contains(&(0, 0)),
        "a started brew left the chunk unmarked"
    );
}
