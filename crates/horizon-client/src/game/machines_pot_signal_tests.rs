//! `DecoratedPotBlock.getAnalogOutputSignal` (`getRedstoneSignalFromBlockEntity`
//! → `getRedstoneSignalFromContainer`): a pot's one slot lives in the chunk's
//! saved block entity, so a comparator beside it has to read that tag.
use super::super::{native_machines::tests::server_game, spill};
use super::*;
use horizon_client::save;
use horizon_world::level::LevelAccess;

/// `getRedstoneSignalFromBlockEntity` for the block at `pos`, the path a
/// comparator's `getAnalogOutputSignal` takes.
fn signal(game: &mut Game<'_>, pos: Pos) -> i32 {
    let state = game.world.block(pos.0, pos.1, pos.2);
    game.with_level(|_, level, _| {
        level.block_entity_analog_signal(BlockPos::new(pos.0, pos.1, pos.2), state, Direction::West)
    })
}

fn saved_count(game: &Game<'_>, pos: Pos) -> u32 {
    let tag = spill::saved_entity(&game.world, [pos.0, pos.1, pos.2]).expect("a saved pot");
    save::item_from_nbt(tag.get("item").expect("the pot's item"))
        .expect("a decodable pot item")
        .count
}

#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES and scratch homes/cwd"]
fn a_pot_feeds_a_comparator_from_its_saved_slot_like_a_chest() {
    let mut game = server_game();
    game.build_registries();
    game.set_allow_commands(true);
    assert!(game.run_command("fill 0 199 0 15 199 15 stone"));
    let pot = (8, 200, 8);
    let chest = (8, 200, 10);
    assert!(game.run_command("setblock 8 200 8 minecraft:decorated_pot"));
    // Vanilla's signal for one slot (`Mth.lerpDiscrete(count / 64, 0, 15)`); a
    // chest at the same fill fraction in all 27 slots reads the same.
    for (count, expected) in [(64, 15), (32, 8), (1, 1)] {
        assert!(game.run_command(&format!(
            "data merge block 8 200 8 {{item:{{id:\"minecraft:stone\",count:{count}}}}}"
        )));
        let items: Vec<String> = (0..27)
            .map(|slot| format!("{{Slot:{slot}b,id:\"minecraft:stone\",count:{count}}}"))
            .collect();
        assert!(game.run_command(&format!(
            "setblock 8 200 10 minecraft:chest{{Items:[{}]}}",
            items.join(",")
        )));
        assert_eq!(signal(&mut game, pot), expected, "a pot holding {count}");
        assert_eq!(
            signal(&mut game, chest),
            expected,
            "a chest holding {count} in every slot"
        );
    }
    assert!(game.run_command("data remove block 8 200 8 item"));
    assert!(game.run_command("data merge block 8 200 10 {Items:[]}"));
    assert_eq!(signal(&mut game, pot), 0, "an empty pot");
    assert_eq!(signal(&mut game, chest), 0, "an empty chest");
}

#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES and scratch homes/cwd"]
fn a_hopper_inserted_item_updates_the_comparator_beside_the_pot() {
    let mut game = server_game();
    game.build_registries();
    game.set_allow_commands(true);
    assert!(game.run_command("fill 0 199 0 15 199 15 stone"));
    let hopper = (7, 200, 8);
    let pot = (8, 200, 8);
    let comparator = BlockPos::new(9, 200, 8);
    assert!(game.run_command(
        "setblock 7 200 8 minecraft:hopper[facing=east,enabled=true]{Items:[{Slot:0b,id:\"minecraft:stone\",count:1}],TransferCooldown:0}"
    ));
    assert!(game.run_command(
        "setblock 8 200 8 minecraft:decorated_pot{item:{id:\"minecraft:stone\",count:63}}"
    ));
    assert!(game.run_command("setblock 9 200 8 minecraft:comparator[facing=west]"));
    let output = |game: &Game<'_>| {
        game.block_ticks
            .comparator_outputs
            .get(&comparator)
            .copied()
            .unwrap_or(0)
    };
    // `ComparatorBlock.tick` on the read side of the comparator.
    let tick = |game: &mut Game<'_>| {
        game.with_level(|b, level, ticks| b.scheduled_block_tick(level, ticks, comparator));
    };
    tick(&mut game);
    assert_eq!(output(&game), 14, "63 items: lerpDiscrete(63/64, 0, 15)");
    for _ in 0..8 {
        game.world.game_time += 1;
        game.tick_hopper(hopper);
    }
    assert_eq!(saved_count(&game, pot), 64, "the hopper filled the pot");
    tick(&mut game);
    assert_eq!(output(&game), 15, "the comparator re-read the saved pot");
}
