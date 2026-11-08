//! Entity.kill, BlockAttachedEntity.kill, ArmorStand.kill and ItemEntity.dampensVibrations.
use super::{Audio, Game, Options, Progress, Settings, load_resources};
use horizon_client::{
    chunk::Chunk,
    containers::BlockEntity,
    mobs::{Mob, Who, warden},
};
use horizon_core::{geometry::Vec3, position::ChunkPos};
use std::sync::Arc;

fn deaths() -> Vec<(&'static str, [f64; 3], Option<Who>)> {
    super::sculk::TEST_POSTED_EVENTS.with(|events| {
        events
            .borrow_mut()
            .take()
            .unwrap()
            .into_iter()
            .filter(|(event, _, _)| *event == "minecraft:entity_die")
            .collect()
    })
}
fn capture() {
    super::sculk::TEST_POSTED_EVENTS.with(|events| *events.borrow_mut() = Some(Vec::new()));
}

fn with_game(f: impl FnOnce(&mut Game<'_>)) {
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
    game.build_registries();
    game.set_allow_commands(true);
    game.world
        .chunks
        .insert((0, 0), Arc::new(Chunk::empty(0, 0)));
    game.block_ticks
        .add_empty_chunk(ChunkPos::new(0, 0))
        .unwrap();
    game.run_command("forceload add 0 0");
    game.run_command("fill 0 199 0 15 199 15 stone");
    game.run_command("gamerule entity_drops false");
    assert!(!game.ui.rules.bool("entity_drops"));
    f(&mut game);
}

#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES and scratch homes/cwd"]
fn entity_die_commands() {
    with_game(|game| {
        let at = [4.25, 200.0, 4.75];
        let kind = game
            .res
            .mob_types()
            .get("minecraft:warden")
            .unwrap()
            .clone();
        game.mobs
            .push(Mob::new(700, kind, Vec3::new(7.5, 200.0, 4.5), 0.0));
        for (kind, nbt, heard) in [
            ("item", "{Item:{id:\"minecraft:stone\",count:1}}", true),
            (
                "item",
                "{Item:{id:\"minecraft:white_wool\",count:1}}",
                false,
            ),
            ("item", "{Item:{id:\"minecraft:red_wool\",count:1}}", false),
            ("cushion", "{color:\"red\"}", true),
            ("end_crystal", "{Invulnerable:1b}", true),
            ("marker", "{}", true),
            ("armor_stand", "{}", true),
        ] {
            let target = game
                .summon_with_nbt(&format!("minecraft:{kind}"), at, nbt)
                .unwrap()
                .unwrap_or_else(|| panic!("summon did not return {kind} target"));
            assert_eq!(game.target_position(target), Some(at));
            game.run_command("setblock 1 200 4 air strict");
            game.run_command("setblock 1 200 4 sculk_sensor strict");
            game.sculk_prepare();
            game.sculk_sync_index();
            warden::register(&mut game.mobs[0]);
            capture();
            game.run_command(&format!("kill @e[type=minecraft:{kind}]"));
            assert_eq!(game.target_position(target), None, "{kind} removed");
            assert_eq!(deaths(), vec![("minecraft:entity_die", at, None)], "{kind}");
            let Some(BlockEntity::Sculk(sensor)) = game.world.block_entities.get(&(1, 200, 4))
            else {
                panic!("sensor must exist");
            };
            assert_eq!(
                sensor.vibration.selector.current.is_some(),
                heard,
                "{kind} {nbt}"
            );
            if let Some((info, _)) = sensor.vibration.selector.current {
                assert_eq!(info.event, "minecraft:entity_die");
                assert_eq!(info.pos, at);
                assert_eq!(info.source, None);
            }
            let vibration = &game.mobs[0].extra.warden.as_ref().unwrap().vibration;
            assert_eq!(
                vibration.selector.current.is_some(),
                heard && kind != "armor_stand",
                "warden accepts {kind} source"
            );
            assert!(
                game.dropped.is_empty(),
                "/kill must not drop the cushion or crystal"
            );
        }
        game.run_command("summon end_crystal 4.25 200 4.75");
        capture();
        game.run_command("damage @e[type=end_crystal,limit=1] 1 minecraft:explosion");
        assert!(game.crystals.is_empty());
        assert!(
            deaths().is_empty(),
            "EndCrystal.hurtServer removes without Entity.kill"
        );
    });
}

#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES and scratch homes/cwd"]
fn entity_die_explosions() {
    with_game(|game| {
        let at = [4.25, 200.0, 4.75];
        // A real blast must attribute hanging/stand deaths to its causing mob,
        // while vehicles use Entity.kill's self source. No cause stays self-sourced.
        let kind = game
            .res
            .mob_types()
            .get("minecraft:creeper")
            .unwrap()
            .clone();
        game.mobs
            .push(Mob::new(700, kind, Vec3::new(10.0, 200.0, 10.0), 0.0));
        for source in [None, Some(700)] {
            let expected_source = source.map(Who::Mob);
            // The loaded entity exists even on main, where it has no command target.
            game.summon_with_nbt("minecraft:cushion", at, "{}").unwrap();
            assert_eq!(game.cushions.len(), 1);
            let stand_at = [5.25, 200.0, 4.75];
            let stand = game
                .summon_with_nbt("minecraft:armor_stand", stand_at, "{}")
                .unwrap()
                .unwrap();
            let painting = game
                .summon_with_nbt(
                    "minecraft:painting",
                    [4.5, 200.5, 4.5],
                    "{variant:\"minecraft:kebab\",facing:0b}",
                )
                .unwrap()
                .unwrap();
            let frame = game
                .summon_with_nbt("minecraft:item_frame", [4.5, 200.5, 4.5], "{Facing:3b}")
                .unwrap()
                .unwrap();
            let boat = game
                .summon_with_nbt("minecraft:oak_boat", [3.5, 200.0, 4.5], "{}")
                .unwrap()
                .unwrap();
            game.summon_with_nbt(
                "minecraft:item",
                at,
                r#"{Item:{id:"minecraft:stone",count:1}}"#,
            )
            .unwrap();
            game.summon_with_nbt("minecraft:experience_orb", at, "{Value:1s}")
                .unwrap();
            game.combat.explosion_owner = source;
            capture();
            game.server_explode(
                [4.5, 200.5, 4.5],
                2.0,
                super::explosion::BlockInteraction::Keep,
                false,
            )
            .unwrap();
            for target in [stand, painting, frame, boat] {
                assert_eq!(game.target_position(target), None, "{target:?} removed");
            }
            assert!(game.cushions.is_empty());
            assert!(
                game.dropped.is_empty() && game.orbs.is_empty(),
                "blast discards items and orbs: items={:?} orbs={}",
                game.dropped
                    .iter()
                    .map(|i| (&i.stack.item, i.health))
                    .collect::<Vec<_>>(),
                game.orbs.len()
            );
            let events = deaths();
            assert_eq!(events.len(), 5);
            assert!(events.contains(&("minecraft:entity_die", at, expected_source)));
            assert!(events.contains(&("minecraft:entity_die", stand_at, expected_source)));
            assert_eq!(
            events
                .iter()
                .filter(|(_, pos, who)| *pos == [4.5, 200.5, 4.03125] && *who == expected_source)
                .count(),
            2,
            "painting and frame centres: {events:?}"
        );
            assert!(events.contains(&("minecraft:entity_die", [3.5, 200.0, 4.5], None)));
        }
        // Explosion attribution must not leak into subsequent unrelated removals.
        game.combat.explosion_owner = None;
        let target = game
            .summon_with_nbt("minecraft:cushion", at, "{}")
            .unwrap()
            .unwrap();
        capture();
        game.kill_target(target);
        assert_eq!(deaths(), vec![("minecraft:entity_die", at, None)]);
    });
}
