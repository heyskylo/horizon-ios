use super::{Audio, Game, Options, Progress, Settings, load_resources};
use horizon_client::{chunk::Chunk, motion::PlayerMotion};
use horizon_core::{geometry::Vec3, position::ChunkPos};
use std::sync::Arc;

#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn mining_uses_admitted_stack_tools_and_actual_eye_immersion() {
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
    options.execution_role = super::execution_role::ExecutionRole::IntegratedServer;
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
    let scratch = std::env::temp_dir().join(format!(
        "horizon-stack-mining-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&scratch).unwrap();
    game.world_root = scratch.clone();
    game.player = Some(PlayerMotion::new(&game.motion, Vec3::new(4.5, 62.0, 4.5)).unwrap());
    game.player.as_mut().unwrap().ground.on_ground = true;
    game.creative = false;
    game.build_registries();
    game.set_allow_commands(true);
    assert!(game.run_command("gamemode survival"));
    game.world
        .chunks
        .insert((0, 0), Arc::new(Chunk::empty(0, 0)));
    game.block_ticks
        .add_empty_chunk(ChunkPos::new(0, 0))
        .unwrap();
    let registry = game.pack.assets.registry;
    let stone = registry.default_state("minecraft:stone").unwrap();
    let pos = [5, 63, 4];
    game.world.set(pos[0], pos[1], pos[2], stone).unwrap();
    let tool = "minecraft:tool={rules:[{blocks:\"#minecraft:mineable/pickaxe\",speed:50.0f,correct_for_drops:true}],damage_per_block:3}";
    assert!(game.run_command(&format!("item replace entity @s weapon.mainhand with minecraft:stick[{tool},minecraft:max_stack_size=1,minecraft:max_damage=20]")));
    let expected = 50.0 / 1.5 / 30.0;
    assert_eq!(game.destroy_progress(pos), expected);
    game.break_block(pos).unwrap();
    assert_eq!(
        game.held().unwrap().damage,
        0,
        "custom MAX_DAMAGE without DAMAGE is not damageable"
    );
    assert_eq!(game.stats.get("used", "minecraft:stick"), 1);
    assert!(
        game.dropped
            .iter()
            .any(|item| &*item.stack.item == "minecraft:cobblestone")
    );
    assert!(game.run_command(&format!("item replace entity @s weapon.mainhand with minecraft:diamond_pickaxe[{tool},minecraft:max_damage=20]")));
    game.mine_block_with_held("minecraft:stone", 1.5);
    assert_eq!(game.held().unwrap().damage, 3);
    assert_eq!(game.stats.get("used", "minecraft:diamond_pickaxe"), 1);

    game.world.set(pos[0], pos[1], pos[2], stone).unwrap();
    assert!(game.run_command(
        "item replace entity @s weapon.mainhand with minecraft:diamond_pickaxe[!minecraft:tool]"
    ));
    assert_eq!(game.destroy_progress(pos), 1.0 / 1.5 / 100.0);
    game.mine_block_with_held("minecraft:stone", 1.5);
    assert_eq!(game.held().unwrap().damage, 0);
    assert_eq!(game.stats.get("used", "minecraft:diamond_pickaxe"), 1);

    assert!(game.run_command(&format!(
        "item replace entity @s weapon.mainhand with minecraft:stick[{tool}]"
    )));
    let mut water = registry
        .state(registry.default_state("minecraft:water").unwrap())
        .unwrap()
        .clone();
    water.properties.insert("level".into(), "7".into());
    let shallow = registry.resolve(&water).unwrap();
    game.world.set(4, 63, 4, shallow).unwrap();
    let surface = 63.0 + f64::from(1.0_f32 / 9.0);
    for (eye, factor) in [(surface - 1e-6, 0.2), (surface, 0.2), (surface + 1e-6, 1.0)] {
        let player = game.player.as_mut().unwrap();
        player.position.y = eye - f64::from(player.dimensions.eye_height);
        assert_eq!(
            game.destroy_progress(pos),
            50.0 * factor / 1.5 / 30.0,
            "eye={eye}"
        );
    }
    game.world
        .set(4, 63, 4, registry.default_state("minecraft:water").unwrap())
        .unwrap();
    game.world.set(4, 64, 4, stone).unwrap();
    for (eye, factor) in [(64.0 - 1e-6, 0.2), (64.0, 0.2), (64.0 + 1e-6, 1.0)] {
        let player = game.player.as_mut().unwrap();
        player.position.y = eye - f64::from(player.dimensions.eye_height);
        assert_eq!(
            game.destroy_progress(pos),
            50.0 * factor / 1.5 / 30.0,
            "capped source eye={eye}"
        );
    }
    // The refactored final mutation must keep the caller's explicit break slot.
    assert!(game.run_command("item replace entity @s weapon.mainhand with minecraft:diamond_pickaxe[minecraft:damage=1560]"));
    game.lan_acting = Some(777);
    game.guest_equipment_breaks.clear();
    game.damage_held_in_slot(1, horizon_world::player_inventory::EquipmentSlot::OffHand);
    assert!(game.held().is_none());
    assert_eq!(game.guest_equipment_breaks.last().unwrap().0, 48);
    game.lan_acting = None;
    assert!(game.run_command(&format!("item replace entity @s weapon.mainhand with minecraft:diamond_pickaxe[{tool},minecraft:damage=1560]")));
    game.lan_acting = Some(777);
    game.guest_equipment_breaks.clear();
    game.mine_block_with_held("minecraft:stone", 1.5);
    assert!(game.held().is_none());
    assert_eq!(game.guest_equipment_breaks.last().unwrap().0, 47);
    game.lan_acting = None;
    std::fs::remove_dir_all(scratch).unwrap();
}

/// A survival player on solid ground in an empty chunk, for `f`.
fn with_survival_game(f: impl FnOnce(&mut Game<'_>)) {
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
    options.execution_role = super::execution_role::ExecutionRole::IntegratedServer;
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
    game.player = Some(PlayerMotion::new(&game.motion, Vec3::new(4.5, 62.0, 4.5)).unwrap());
    game.player.as_mut().unwrap().ground.on_ground = true;
    game.creative = false;
    game.build_registries();
    game.set_allow_commands(true);
    assert!(game.run_command("gamemode survival"));
    game.world
        .chunks
        .insert((0, 0), Arc::new(Chunk::empty(0, 0)));
    game.block_ticks
        .add_empty_chunk(ChunkPos::new(0, 0))
        .unwrap();
    f(&mut game);
}

#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn efficiency_applies_to_a_same_item_swap() {
    with_survival_game(|game| {
        let registry = game.pack.assets.registry;
        let pos = [5, 63, 4];
        // Player.getDestroySpeed: the tool's speed, plus Efficiency's
        // mining_efficiency (level squared + 1) when that speed exceeds 1. The
        // plain and the enchanted tool are the same item, so only the tick's
        // detectEquipmentUpdates (the stack differs) hands the player the modifier.
        for (block, hardness, tool, base) in [
            ("minecraft:grass_block", 0.6_f32, "diamond_shovel", 8.0_f32),
            ("minecraft:dirt", 0.5, "diamond_shovel", 8.0),
            ("minecraft:stone", 1.5, "diamond_pickaxe", 8.0),
        ] {
            let state = registry.default_state(block).unwrap();
            game.world.set(pos[0], pos[1], pos[2], state).unwrap();
            assert!(game.run_command(&format!(
                "item replace entity @s weapon.mainhand with minecraft:{tool}"
            )));
            game.tick_hand();
            assert_eq!(
                game.destroy_progress(pos),
                base / hardness / 30.0,
                "{block}"
            );
            assert!(game.run_command(&format!(
                "item replace entity @s weapon.mainhand with minecraft:{tool}[minecraft:enchantments={{\"minecraft:efficiency\":3}}]"
            )));
            game.tick_hand();
            assert_eq!(
                game.destroy_progress(pos),
                (base + 10.0) / hardness / 30.0,
                "{block} with efficiency 3"
            );
        }
    });
}
