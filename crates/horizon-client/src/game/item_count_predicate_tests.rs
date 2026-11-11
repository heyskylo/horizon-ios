use super::super::native_machines::tests::server_game;
use super::*;
use horizon_core::position::BlockPos;

#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES and scratch homes/cwd"]
fn count_equality_uses_live_stack_bounds_and_command_flow() {
    let mut game = server_game();
    game.build_registries();
    game.set_allow_commands(true);
    game.block_ticks
        .add_empty_chunk(BlockPos::new(0, 0, 0).chunk())
        .unwrap();
    assert!(game.run_command("forceload add 0 0 15 15"));
    assert!(game.world.loaded(4, 4), "actual loaded world chunk");
    assert!(game.run_command("setblock 4 199 4 stone"));
    assert!(game.run_command("setblock 4 200 4 chest{Items:[{Slot:0b,id:\"minecraft:diamond\",count:1},{Slot:1b,id:\"minecraft:diamond\",count:3},{Slot:2b,id:\"minecraft:diamond\",count:4},{Slot:3b,id:\"minecraft:dirt\",count:3},{Slot:5b,id:\"minecraft:stone\",count:2,components:{\"minecraft:custom_data\":{h194:1}}}]}"));
    assert!(game.run_command("scoreboard objectives add h194 dummy"));
    let clock = game.world.game_time;
    let counts = |g: &Game<'_>| {
        let horizon_client::containers::BlockEntity::Chest(slots) =
            g.world.block_entities.get(&(4, 200, 4)).unwrap()
        else {
            panic!("actual chest model missing")
        };
        assert!(slots[4].is_none(), "actual empty slot");
        [
            slots[0].as_ref().unwrap().count,
            slots[1].as_ref().unwrap().count,
            slots[2].as_ref().unwrap().count,
        ]
    };
    assert_eq!(counts(&game), [1, 3, 4]);
    let mut failed = Vec::new();
    for (slot, count) in [1, 3, 4].into_iter().enumerate() {
        assert!(
            game.run_command(&format!(
                "execute if items block 4 200 4 container.{slot} diamond"
            )),
            "plain admitted command path"
        );
        let stack = ItemStack::new("minecraft:diamond", count);
        assert!(
            horizon_client::item_data::encode_components(&stack)
                .is_none_or(|t| t.get("minecraft:count").is_none()),
            "count is not a component patch"
        );
        for (term, expected) in [
            ("count=3", count == 3),
            ("count=4", count == 4),
            ("!count=3", count != 3),
            ("count={min:2}", count >= 2),
            ("count~3", count == 3),
            ("minecraft:count={min:2,max:3}", (2..=3).contains(&count)),
        ] {
            let predicate = ItemInput {
                id: "minecraft:diamond".into(),
                tag: false,
                any: false,
                components: Some(format!("[{term}]")),
            };
            let actual = game.stack_matches(&predicate, &stack);
            if actual != expected {
                failed.push(format!(
                    "direct count{count} {term}: {actual}, expected {expected}"
                ));
            }
            assert_eq!(stack.count, count, "read-only evaluator");
            assert!(game.run_command("scoreboard players set #r h194 91"));
            assert!(game.run_command("scoreboard players set #s h194 17"));
            let accepted = game.run_command(&format!("execute store result score #r h194 store success score #s h194 if items block 4 200 4 container.{slot} diamond[{term}]"));
            let score = |name| game.score.board.score(name, "h194").unwrap().value;
            let got = (accepted, score("#r"), score("#s"));
            let want = if expected {
                (true, count as i32, 1)
            } else {
                (false, 0, 0)
            };
            if got != want {
                failed.push(format!(
                    "command slot{slot} {term}: {got:?}, expected {want:?}"
                ));
            }
        }
    }
    assert!(game.run_command("execute unless items block 4 200 4 container.3 diamond[count=3]"));
    assert!(game.run_command("execute unless items block 4 200 4 container.4 diamond[count=3]"));
    assert!(
        game.run_command("execute if items block 4 200 4 container.5 stone[custom_data={h194:1}]")
    );
    assert_eq!(counts(&game), [1, 3, 4], "no inventory mutation");
    assert_eq!(game.world.game_time, clock, "zero actor ticks");
    assert!(
        failed.is_empty(),
        "actual count predicate mismatches: {failed:?}"
    );
}
