use super::*;
use crate::game::native_machines::tests::server_game;
use horizon_client::motion::PlayerMotion;
use horizon_core::{geometry::Vec3, position::BlockPos};

/// Added after baseline: an item-unconstrained interaction still dispatches
/// on detach. This prevents suppressing the whole trigger to pass the lead test.
#[test]
#[ignore = "needs prepared resources"]
fn holder_detach_still_dispatches_the_actual_interaction_criterion() {
    let mut game = server_game();
    game.options.dedicated = None;
    game.player = Some(PlayerMotion::new(&game.motion, Vec3::new(4.5, 100.0, 4.5)).unwrap());
    game.level.player_uuid = Some([413, 0, 0, 2]);
    game.level.health = 20.0;
    game.creative = false;
    // A world of its own under the temporary folder, never the cwd.
    game.world_root = std::env::temp_dir().join(format!(
        "horizon-leash-context-control-world-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&game.world_root);
    let pack = game.world_root.join("datapacks/leash_context");
    let dir = pack.join("data/leash_context/advancement");
    std::fs::create_dir_all(&dir).unwrap();
    let f = horizon_core::version::DATA_PACK_FORMAT;
    std::fs::write(
        pack.join("pack.mcmeta"),
        serde_json::json!({"pack": {
            "description":"Postbaseline actual interaction context control", "pack_format":f.0,
            "min_format":[f.0,f.1],"max_format":[f.0,f.1]}})
        .to_string(),
    )
    .unwrap();
    std::fs::write(
        dir.join("interaction.json"),
        serde_json::json!({"criteria": {
            "used":{"trigger":"minecraft:player_interacted_with_entity"}}})
        .to_string(),
    )
    .unwrap();
    game.build_registries();
    game.set_allow_commands(true);
    assert!(game.run_command("reload"));
    assert!(game.adv.tree.get("leash_context:interaction").is_some());
    game.block_ticks
        .add_empty_chunk(BlockPos::new(0, 0, 0).chunk())
        .unwrap();
    assert!(game.run_command("forceload add 0 0"));
    assert!(game.run_command("fill 0 99 0 15 99 15 stone"));
    assert!(game.world.loaded(4, 6));
    assert!(game.run_command(
        "summon frog 4.5 100 6.5 {UUID:[I;9,9,9,8],NoAI:1b,variant:\"minecraft:temperate\"}"
    ));
    let id = game.mob_by_uuid([9, 9, 9, 8]).unwrap();
    let index = game.mobs.iter().position(|m| m.id == id).unwrap();
    assert!(game.run_command("item replace entity @s hotbar.0 with lead 4"));
    game.persist.raw.remove(&id);
    let target = || {
        Some(InteractTarget {
            id: id as i32,
            location: [0.0; 3],
        })
    };
    let clock = game.world.game_time;
    assert!(game.interact_mob(target()).unwrap());
    assert!(game.adv.progress.has("leash_context:interaction", "used"));
    assert!(game.adv.progress.has(
        "minecraft:husbandry/leash_all_frog_variants",
        "minecraft:temperate"
    ));
    assert_eq!(game.mobs[index].extra.leash_player, Some([413, 0, 0, 2]));
    assert!(game.run_command("advancement revoke @s only leash_context:interaction used"));
    assert!(game.run_command(
        "advancement revoke @s only minecraft:husbandry/leash_all_frog_variants minecraft:temperate"
    ));
    assert!(game.interact_mob(target()).unwrap());
    assert!(
        game.adv.progress.has("leash_context:interaction", "used"),
        "without-item success still dispatches the actual trigger"
    );
    assert!(!game.adv.progress.has(
        "minecraft:husbandry/leash_all_frog_variants",
        "minecraft:temperate"
    ));
    assert!(game.mobs[index].extra.leash.is_none());
    assert_eq!(game.held().unwrap().count, 3);
    assert_eq!(game.level.player_uuid, Some([413, 0, 0, 2]));
    assert_eq!(game.world.game_time, clock);
    let _ = std::fs::remove_dir_all(&game.world_root);
}
