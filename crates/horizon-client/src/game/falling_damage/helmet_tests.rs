use crate::game::{Audio, Game, Options, Progress, Settings, load_resources};
use horizon_client::{items::ItemStack, motion::PlayerMotion};
use horizon_core::{geometry::Vec3, random::RandomSource};

#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn falling_impacts_wear_the_helmet_before_normal_armor_and_reduction() {
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
    let scratch = std::env::temp_dir().join(format!("horizon-helmet-{}", std::process::id()));
    std::fs::create_dir_all(&scratch).unwrap();
    game.world_root = scratch.clone();
    game.creative = false;
    // Equipment breaks are server notifications, with no local visual RNG.
    game.lan_acting = Some(1);
    let position = Vec3::new(4.5, 100.0, 4.5);
    let cases: serde_json::Value = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tests/fixtures/helmet_damage.json"
    )))
    .unwrap();
    for (case_index, case) in cases.as_array().unwrap().iter().enumerate() {
        // The `mob_attack` cases are ordinary sources (outside
        // `damages_helmet`): the piece that breaks on the hit must not reduce
        // that same hit, while one that survives still does (#11696).
        for path in 0..2 {
            let index = case_index * 2 + path;
            game.player = Some(PlayerMotion::new(game.motion, position).unwrap());
            game.armor = [None, None, None, None];
            if let Some(item) = case["head"].as_str() {
                let mut stack = ItemStack::new(&format!("minecraft:{item}"), 1);
                stack.damage = case["head_damage"].as_u64().unwrap() as u32;
                if case["unbreaking"].as_u64().unwrap() > 0 {
                    stack.enchant(
                        "minecraft:unbreaking",
                        case["unbreaking"].as_u64().unwrap() as u32,
                    );
                }
                if case["unbreakable"].as_bool().unwrap_or(false) {
                    stack.data_mut().set_other(
                        "minecraft:unbreakable",
                        Some(horizon_world::nbt::Tag::Compound(Default::default())),
                    );
                }
                game.armor[0] = Some(stack);
            }
            if let Some(damage) = case["boots_damage"].as_u64() {
                let mut stack = ItemStack::new("minecraft:iron_boots", 1);
                stack.damage = damage as u32;
                if case["unbreaking"].as_u64().unwrap() > 0 {
                    stack.enchant(
                        "minecraft:unbreaking",
                        case["unbreaking"].as_u64().unwrap() as u32,
                    );
                }
                game.armor[3] = Some(stack);
            }
            game.apply_equipment();
            game.apply_effect_status();
            game.world.random.set_seed(case["seed"].as_i64().unwrap());
            let source = format!("minecraft:{}", case["source"].as_str().unwrap());
            if path == 0 {
                game.falling_hurt_player(position, 8.0, &source);
            } else {
                // This is the authoritative player path used by /damage. A
                // position avoids unrelated random-direction knockback draws.
                game.hurt_player_from(8.0, Some([5.5, 100.0, 4.5]), &source, Some(None));
            }
            // Flush the same equipment events as the server-player tick. This
            // also makes the baseline regression observe completed legacy wear.
            let events = std::mem::take(&mut game.player.as_mut().unwrap().events);
            game.wear_armor(&events);
            let head = game.armor[0].as_ref().map_or(-1, |s| i64::from(s.damage));
            let boots = game.armor[3].as_ref().map_or(-1, |s| i64::from(s.damage));
            let health = game.player.as_ref().unwrap().health.health();
            let next = game.world.random.clone().next_long();
            println!(
                "HELMET {index} {head} {boots} {:08x} {next}",
                health.to_bits()
            );
            if let Some(expected) = case["expected_head"].as_i64() {
                assert_eq!(head, expected, "case={index}");
            }
            if let Some(expected) = case["expected_health"].as_f64() {
                assert_eq!(health, expected as f32, "case={index}");
            }
            let events = std::mem::take(&mut game.player.as_mut().unwrap().events);
            game.wear_armor(&events);
            assert_eq!(
                game.armor[0].as_ref().map_or(-1, |s| i64::from(s.damage)),
                head,
                "equipment must not wear twice when events drain"
            );
            assert_eq!(game.world.random.clone().next_long(), next);
        }
    }
    // LivingEntity.hurtServer wears the helmet even when the cooldown then
    // rejects an equal hit: the second hit has no ordinary armor wear.
    for (initial, expected) in [(0, Some(5)), (160, None)] {
        game.player = Some(PlayerMotion::new(game.motion, position).unwrap());
        let mut helmet = ItemStack::new("minecraft:iron_helmet", 1);
        helmet.damage = initial;
        game.armor = [Some(helmet), None, None, None];
        game.apply_equipment();
        game.apply_effect_status();
        game.falling_hurt_player(position, 8.0, "minecraft:falling_anvil");
        game.falling_hurt_player(position, 8.0, "minecraft:falling_anvil");
        assert_eq!(game.armor[0].as_ref().map(|s| s.damage), expected);
        assert_eq!(
            game.player.as_ref().unwrap().health.health(),
            20.0_f32 - 5.9040003_f32
        );
        if expected.is_none() {
            // The hit that breaks the helmet still gets its 0.75 reduction;
            // only the next hit compares an unprotected 8 against lastHurt=6.
            game.falling_hurt_player(position, 8.0, "minecraft:falling_anvil");
            assert_eq!(game.player.as_ref().unwrap().health.health(), 12.096_f32);
        }
    }
    // Refreshing equipment protection during a hit must preserve absorption
    // that the damage pipeline has not consumed yet.
    game.player = Some(PlayerMotion::new(game.motion, position).unwrap());
    game.armor = [
        Some(ItemStack::new("minecraft:iron_helmet", 1)),
        None,
        None,
        None,
    ];
    game.apply_equipment();
    let player = game.player.as_mut().unwrap();
    player
        .set_modifier_unclamped(
            "minecraft:max_absorption",
            "horizon:helmet_test",
            4.0,
            "add_value",
            true,
        )
        .unwrap();
    player.set_health(4.0, true).unwrap();
    game.falling_hurt_player(position, 8.0, "minecraft:falling_anvil");
    assert_eq!(game.player.as_ref().unwrap().health.health(), 18.096_f32);
    assert_eq!(game.player.as_ref().unwrap().health.absorption(), 0.0);
    std::fs::remove_dir_all(scratch).unwrap();
}
