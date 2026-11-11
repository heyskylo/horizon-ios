//! `ChestBlock.isChestBlockedAt` (ChestBlock.java:318-319) is a redstone
//! conductor above the chest *or* a cat sitting in the block above it
//! (`isCatSittingOnChest`, :327-338). `getAnalogOutputSignal` (:346-348)
//! reads `getContainer(.., false)`, whose `combine(.., false)` (:279) uses
//! that predicate, so a sitting cat leaves the comparator at 0. A hopper's
//! `getContainer(.., true)` ignores the block, and the menu path keeps
//! closing as before.
use super::super::native_machines::tests::server_game;
use super::*;
use horizon_client::chunk::Chunk;
use horizon_core::geometry::Vec3;
use std::sync::Arc;

const CHEST: Pos = (0, 64, 0);
const COMPARATOR: Pos = (1, 64, 0);
const WIRE: Pos = (2, 64, 0);

/// A full chest at `CHEST`, a comparator east of it reading west into it,
/// and the wire that carries the comparator's output, on a floor in a
/// ticking chunk.
fn game() -> Game<'static> {
    let mut game = server_game();
    game.build_registries();
    game.set_allow_commands(true);
    for x in -1..=1 {
        for z in -1..=1 {
            let mut chunk = Chunk::empty(x, z);
            chunk.lit = true;
            game.world.chunks.insert((x, z), Arc::new(chunk));
            game.block_ticks
                .add_empty_chunk(BlockPos::new(x * 16, 0, z * 16).chunk())
                .unwrap();
        }
    }
    game.player = Some(
        horizon_client::motion::PlayerMotion::new(game.motion, Vec3::new(8.5, 64.0, 8.5)).unwrap(),
    );
    for command in [
        "forceload add 0 0",
        "fill 0 63 0 15 63 15 stone",
        "setblock 0 64 0 minecraft:chest{Items:[{Slot:0b,id:\"minecraft:stone\",count:64}]}",
        "setblock 1 64 0 minecraft:comparator[facing=west]",
        "setblock 2 64 0 minecraft:redstone_wire",
    ] {
        assert!(game.run_command(command), "{command}");
    }
    game.update_chunk_tickets();
    assert!(game.tickets.levels().block_ticking(0, 0));
    game
}

/// A block change beside the comparator, so it reads its input again, then
/// the ticks that settle the comparator and the wire.
fn poke(game: &mut Game<'_>, block: &str) {
    assert!(game.run_command(&format!(
        "setblock {} {} {} {block}",
        COMPARATOR.0,
        COMPARATOR.1,
        COMPARATOR.2 + 1
    )));
    for _ in 0..4 {
        game.world.game_time += 1;
        game.update_chunk_tickets();
        game.tick_blocks().unwrap();
    }
}

fn summon_cat(game: &mut Game<'_>, sitting: bool) {
    assert!(game.run_command(&format!(
        "summon cat 0.5 65 0.5 {{Sitting:{}b,Tame:1b}}",
        i8::from(sitting)
    )));
}

fn wire_power(game: &Game<'_>) -> i32 {
    let state = game.world.block(WIRE.0, WIRE.1, WIRE.2);
    let info = game
        .pack
        .assets
        .registry
        .state(state)
        .expect("the wire is a known state");
    assert_eq!(info.name, "minecraft:redstone_wire");
    info.properties["power"]
        .parse()
        .expect("a wire has a power property")
}

#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn a_sitting_cat_silences_the_comparator_reading_a_chest() {
    let mut game = game();
    poke(&mut game, "glass");
    assert_eq!(wire_power(&game), 1, "a full chest reads 1");

    summon_cat(&mut game, true);
    poke(&mut game, "stone");
    assert_eq!(wire_power(&game), 0, "a sitting cat blocks the chest");

    // Standing the cat up gives the signal back.
    assert!(game.run_command("data merge entity @e[type=minecraft:cat,limit=1] {Sitting:0b}"));
    poke(&mut game, "dirt");
    assert_eq!(wire_power(&game), 1, "a standing cat blocks nothing");
}

#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn a_sitting_cat_leaves_the_hopper_and_the_menu_alone() {
    let mut game = game();
    summon_cat(&mut game, true);
    assert!(
        game.containers().view(CHEST, false).is_some(),
        "a hopper's getContainer(.., true) ignores the cat"
    );
    assert!(
        game.containers().view(CHEST, true).is_none(),
        "the comparator's getContainer(.., false) combines to nothing"
    );
    assert!(
        game.chest_menu_blocked(CHEST, "minecraft:chest"),
        "a chest with a cat on it opens no menu"
    );
}

#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn a_conductor_above_a_chest_still_silences_the_comparator() {
    let mut game = game();
    poke(&mut game, "glass");
    assert_eq!(wire_power(&game), 1);
    assert!(game.run_command("setblock 0 65 0 stone"));
    poke(&mut game, "stone");
    assert_eq!(wire_power(&game), 0, "a conductor above blocks the chest");
}
