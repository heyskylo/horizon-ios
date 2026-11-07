//! Actual entity removal, commands and chunk-save assembly, with no intervening tick.
use super::*;
use horizon_client::{chunk::Chunk, mobs::Mob};
use horizon_world::nbt::Tag;

#[test]
#[ignore = "requires private prepared resources and fresh Horizon homes"]
fn discarded_mob_leaves_selectors_and_saves_while_frozen() {
    let root = PathBuf::from(std::env::var_os("HORIZON_TEST_RESOURCES").unwrap());
    let settings = Settings::default();
    let loaded = load_resources(
        root.clone(),
        None,
        &root,
        &settings,
        Instant::now(),
        false,
        true,
        &Progress::new(),
        &mut |_, _| unreachable!(),
    )
    .unwrap();
    let mut options = Options::parse(std::iter::empty()).unwrap().unwrap();
    options.resources = root.clone();
    options.execution_role = execution_role::ExecutionRole::IntegratedServer;
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
    game.world = World::new(game.res.facts().air, 1);
    game.world
        .chunks
        .insert((0, 0), Arc::new(Chunk::empty(0, 0)));
    game.player = Some(PlayerMotion::new(game.motion, Vec3::new(0.5, 64.0, 0.5)).unwrap());
    game.build_registries();
    game.set_allow_commands(true);
    assert!(game.run_command("tick freeze"));
    assert!(game.clock.frozen());
    let kind = game.res.mob_types().get("minecraft:blaze").unwrap().clone();
    game.mobs
        .push(Mob::new(42, kind.clone(), Vec3::new(0.5, 64.0, 2.5), 0.0));
    game.mobs
        .push(Mob::new(43, kind, Vec3::new(1.5, 64.0, 2.5), 0.0));
    // The survivor is dying, but not removed. Keep existing selection semantics:
    // generic entity lookup/UUID still finds it; @e keeps its death_time filter.
    game.mobs[1].health = 0.0;
    game.mobs[1].death_time = 1;
    let removed_uuid = game.uuid_for(42);
    let dying_uuid = game.uuid_for(43);
    let selector = |game: &Game<'_>, text: &str| {
        horizon_client::command::selector::parse(
            &mut horizon_client::command::Reader::new(text),
            &game.ui.registries,
            true,
        )
        .unwrap()
    };
    let all = selector(&game, "@e[type=minecraft:blaze]");
    assert_eq!(game.select(&all), [commands::Target::Mob(42)]);
    let before_id = game.level_entity(42).is_some();
    let before_uuid = game.target_with_uuid(removed_uuid).is_some();
    assert!(before_id && before_uuid);
    assert_eq!(
        game.target_with_uuid(dying_uuid),
        Some(commands::Target::Mob(43))
    );
    let keys = [(0, 0)].into();
    let mut saved = HashMap::new();
    game.save_every_entity(&keys, &mut saved);
    assert_eq!(saved.values().map(Vec::len).sum::<usize>(), 1);
    let before_save = saved.values().any(|tags| !tags.is_empty());
    let health = game.mobs[0].health;
    let world_time = game.world.game_time;
    assert!(!game.native_remove_mob("minecraft:the_nether", 42));
    assert!(game.native_remove_mob("minecraft:overworld", 42));
    assert!(!game.native_remove_mob("minecraft:overworld", 42));
    assert_eq!(game.world.game_time, world_time);
    assert_eq!(game.mobs[0].health, health, "discard is not lethal damage");
    assert_eq!(game.mobs[0].death_time, 0);
    assert!(game.dropped.is_empty(), "discard must not roll death loot");
    assert!(game.orbs.is_empty(), "discard must not award death XP");
    let after_enumeration = !game.select(&all).is_empty();
    assert!(!after_enumeration);
    assert_eq!(game.level_entity(42), None);
    assert_eq!(game.target_with_uuid(removed_uuid), None);
    assert_eq!(
        game.target_with_uuid(dying_uuid),
        Some(commands::Target::Mob(43))
    );
    assert_eq!(game.level_entity(43), Some(commands::Target::Mob(43)));
    assert!(game.run_command("execute unless entity @e[type=minecraft:blaze] run data merge storage fixture:discard {gone:1b}"));
    assert_eq!(
        game.score.storage.get("fixture:discard").get("gone"),
        Some(&Tag::Byte(1))
    );
    // UUID-backed commands also bypass the entity-list enumeration.
    let uuid = horizon_client::lan::playerdata::ints_uuid(removed_uuid);
    let uuid = horizon_protocol::crypto::uuid_string(uuid);
    assert!(game.run_command(&format!(
        "execute unless entity {uuid} run data merge storage fixture:discard {{uuid_gone:1b}}"
    )));
    assert_eq!(
        game.score.storage.get("fixture:discard").get("uuid_gone"),
        Some(&Tag::Byte(1))
    );
    saved.clear();
    game.save_every_entity(&keys, &mut saved);
    assert!(saved.values().all(Vec::is_empty));
    let report = serde_json::json!({
        "before_id": before_id, "before_uuid": before_uuid, "before_save": before_save,
        "after_id": game.level_entity(42).is_some(),
        "after_uuid": game.target_with_uuid(removed_uuid).is_some(),
        "after_enumeration": after_enumeration,
        "after_save": saved.values().any(|tags| !tags.is_empty()),
        "removed": game.mobs[0].removed,
    });
    if let Some(path) = std::env::var_os("HORIZON_DISCARD_REPORT") {
        std::fs::write(path, serde_json::to_vec(&report).unwrap()).unwrap();
    }
    // Removing another mob during its death animation also hides UUID lookup
    // immediately. No dead/alive predicate was added to general lookup.
    assert!(game.native_remove_mob("minecraft:overworld", 43));
    assert_eq!(game.target_with_uuid(dying_uuid), None);
    // The old UUID entry can remain in EntityMemory until its normal sweep.
    // It must not shadow a newly joined entity with the same UUID.
    let kind = game.res.mob_types().get("minecraft:blaze").unwrap().clone();
    game.mobs
        .push(Mob::new(44, kind, Vec3::new(0.5, 64.0, 2.5), 0.0));
    game.persist.uuids.insert(44, removed_uuid);
    assert_eq!(
        game.target_with_uuid(removed_uuid),
        Some(commands::Target::Mob(44))
    );
    // Root save suppression alone is insufficient: a live but discarded
    // passenger must not reappear under an otherwise savable vehicle.
    let kind = game.res.mob_types().get("minecraft:blaze").unwrap().clone();
    game.mobs
        .push(Mob::new(45, kind, Vec3::new(0.5, 65.0, 2.5), 0.0));
    game.mobs[3].extra.mount = Some(44);
    saved.clear();
    game.save_every_entity(&keys, &mut saved);
    let root = &saved[&(0, 0)][0];
    assert!(matches!(root.get("Passengers"), Some(Tag::List(list)) if list.len() == 1));
    assert!(game.native_remove_mob("minecraft:overworld", 45));
    saved.clear();
    game.save_every_entity(&keys, &mut saved);
    assert_eq!(saved[&(0, 0)].len(), 1);
    assert!(saved[&(0, 0)][0].get("Passengers").is_none());
}
