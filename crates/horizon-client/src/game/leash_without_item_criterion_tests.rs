use super::*;
use crate::game::native_machines::tests::server_game;
use horizon_client::motion::PlayerMotion;
use horizon_core::{geometry::Vec3, position::BlockPos};

#[test]
#[ignore = "needs prepared resources"]
fn actual_holder_detach_does_not_reaward_the_held_lead_criterion() {
    let mut game = server_game();
    game.options.dedicated = None;
    game.player = Some(PlayerMotion::new(game.motion, Vec3::new(4.5, 100.0, 4.5)).unwrap());
    game.level.player_uuid = Some([413, 0, 0, 1]);
    game.level.health = 20.0;
    game.creative = false;
    // A world of its own under the temporary folder, never the cwd.
    game.world_root = std::env::temp_dir().join(format!(
        "horizon-leash-criterion-world-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&game.world_root);
    game.build_registries();
    game.set_allow_commands(true);
    assert!(game.run_command("reload"));
    const ADV: &str = "minecraft:husbandry/leash_all_frog_variants";
    const CRITERION: &str = "minecraft:temperate";
    let advancement = game
        .adv
        .tree
        .get(ADV)
        .expect("actual built-in advancement loaded");
    assert!(advancement.criteria.iter().any(|c| c.name == CRITERION));
    game.block_ticks
        .add_empty_chunk(BlockPos::new(0, 0, 0).chunk())
        .unwrap();
    assert!(game.run_command("forceload add 0 0"));
    assert!(game.run_command("fill 0 99 0 15 99 15 stone"));
    assert!(game.world.loaded(4, 6));
    assert_eq!(
        &*game.res.facts().get(game.world.block(4, 99, 6)).name,
        "minecraft:stone"
    );
    assert!(game.run_command("execute if entity @s[type=player,nbt={UUID:[I;413,0,0,1],Health:20f,playerGameType:0,Pos:[4.5d,100d,4.5d]}]"));
    assert!(game.run_command(
        "summon frog 4.5 100 6.5 {UUID:[I;9,9,9,9],NoAI:1b,variant:\"minecraft:temperate\"}"
    ));
    assert!(game.run_command("execute if entity @e[type=frog,nbt={UUID:[I;9,9,9,9],Health:10f,NoAI:1b,variant:\"minecraft:temperate\",Pos:[4.5d,100d,6.5d]}]"));
    let id = game.mob_by_uuid([9, 9, 9, 9]).unwrap();
    let index = game.mobs.iter().position(|m| m.id == id).unwrap();
    assert!(game.run_command("item replace entity @s hotbar.0 with lead 4"));
    assert!(!game.adv.progress.has(ADV, CRITERION));
    // Actual modeled frog and player are authoritative; no replayed source tag.
    game.persist.raw.remove(&id);
    let clock = game.world.game_time;
    let target = || {
        Some(InteractTarget {
            id: id as i32,
            location: [0.0; 3],
        })
    };
    assert!(game.interact_mob(target()).unwrap());
    assert_eq!(
        game.mobs[index].extra.leash,
        Some(horizon_client::mobs::leash::Holder::Player)
    );
    assert_eq!(game.mobs[index].extra.leash_player, Some([413, 0, 0, 1]));
    assert_eq!(game.held().unwrap().count, 3);
    assert!(
        game.adv.progress.has(ADV, CRITERION),
        "positive attach reaches the actual player criterion"
    );
    assert!(game.run_command(&format!("advancement revoke @s only {ADV} {CRITERION}")));
    assert!(!game.adv.progress.has(ADV, CRITERION));
    assert!(game.interact_mob(target()).unwrap());
    assert!(game.mobs[index].extra.leash.is_none());
    assert_eq!(game.held().unwrap().item.as_ref(), "minecraft:lead");
    assert_eq!(game.held().unwrap().count, 3);
    assert_eq!(game.level.player_uuid, Some([413, 0, 0, 1]));
    assert_eq!(game.world.game_time, clock);
    assert!(!game.persist.raw.contains_key(&id));
    assert!(
        !game.adv.progress.has(ADV, CRITERION),
        "holder detach is a without-item success, not a second held-lead use"
    );
    let _ = std::fs::remove_dir_all(&game.world_root);
}
