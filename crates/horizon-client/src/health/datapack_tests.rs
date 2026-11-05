use super::*;

#[test]
#[ignore = "requires prepared vanilla resources"]
fn datapack_damage_new_world_drops_a_removed_resource_root_definition() {
    use horizon_world::loot::PackOverlay;
    let prepared = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../.local/horizon-client")
        .join(horizon_core::VANILLA_VERSION);
    let root = std::env::temp_dir().join(format!("horizon-damage-root-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(root.join("data/demo/damage_type")).unwrap();
    for file in [
        "damage-catalog.json",
        "damage-tags.json",
        "player-float-ids.json",
    ] {
        fs::copy(prepared.join(file), root.join(file)).unwrap();
    }
    let definition = root.join("data/demo/damage_type/installed.json");
    fs::write(
        &definition,
        r#"{"message_id":"installed","scaling":"never","exhaustion":0.25}"#,
    )
    .unwrap();
    let cached_resources = HealthResources::load(&root).unwrap();
    let first_world = cached_resources
        .with_packs(&root, &PackOverlay::NONE, false)
        .unwrap();
    let custom = first_world.damage.id("demo:installed").unwrap();
    let vanilla = first_world.damage.id("minecraft:fall").unwrap();
    fs::remove_file(definition).unwrap();
    let reloaded = first_world
        .with_packs(&root, &PackOverlay::NONE, true)
        .unwrap();
    assert_eq!(reloaded.damage.id("demo:installed"), Some(custom));
    let next_world = cached_resources
        .with_packs(&root, &PackOverlay::NONE, false)
        .unwrap();
    assert!(next_world.damage.id("demo:installed").is_none());
    assert_eq!(next_world.damage.id("minecraft:fall"), Some(vanilla));
    assert_eq!(first_world.damage.id("demo:installed"), Some(custom));
    fs::remove_dir_all(root).unwrap();
}

#[test]
#[ignore = "requires prepared vanilla resources"]
fn datapack_damage_initial_snapshot_uses_the_prepared_worlds_features() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../.local/horizon-client")
        .join(horizon_core::VANILLA_VERSION);
    let world =
        std::env::temp_dir().join(format!("horizon-damage-features-{}", std::process::id()));
    let pack = world.join("datapacks/feature_damage");
    let _ = fs::remove_dir_all(&world);
    fs::create_dir_all(pack.join("data/demo/damage_type")).unwrap();
    fs::write(
        pack.join("pack.mcmeta"),
        r#"{"features":{"enabled":["minecraft:trade_rebalance"]}}"#,
    )
    .unwrap();
    fs::write(
        pack.join("data/demo/damage_type/heat.json"),
        r#"{"message_id":"heat","scaling":"never","exhaustion":0.1}"#,
    )
    .unwrap();
    fs::write(
        world.join("datapacks.json"),
        r#"{"enabled":["file/feature_damage"],"disabled":[]}"#,
    )
    .unwrap();
    let packs = crate::datapack::discover_with_features(&world, &root);
    let snapshot = |features: &[String]| {
        let enabled = crate::datapack::enabled_ids(&world, &packs, features);
        let selected: Vec<_> = enabled
            .iter()
            .filter_map(|id| packs.iter().find(|p| &p.id == id))
            .collect();
        let store = crate::datapack::DataStore::load(&selected);
        let files: Vec<_> = store.files.into_iter().collect();
        horizon_world::damage_source::Catalog::load(
            &root,
            &horizon_world::loot::PackOverlay { files: &files },
        )
        .unwrap()
    };
    let stale = snapshot(&["minecraft:vanilla".into()]);
    assert!(stale.id("demo:heat").is_none());
    let prepared = snapshot(&[
        "minecraft:vanilla".into(),
        "minecraft:trade_rebalance".into(),
    ]);
    assert!(prepared.id("demo:heat").is_some());
    fs::remove_dir_all(world).unwrap();
}

/// Ordinary player hurt, using a custom source and a frozen-registry tag reload.
#[test]
#[ignore = "requires prepared vanilla resources"]
fn datapack_damage_uses_custom_identity_armor_fire_and_reloaded_tags() {
    use horizon_world::loot::PackOverlay;
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../.local/horizon-client")
        .join(horizon_core::VANILLA_VERSION);
    let assets = crate::assets::Assets::load(&root).unwrap();
    let resources = crate::motion::MotionResources::load(&root, &assets).unwrap();
    let files = vec![
        ("damage_type/demo:heated_air".into(), vec![br#"{"message_id":"heated_air","scaling":"never","exhaustion":0.25,"effects":"burning"}"#.to_vec()]),
        ("tags/minecraft:damage_type/is_fire".into(), vec![br#"{"values":["demo:heated_air"]}"#.to_vec()]),
        ("tags/minecraft:damage_type/bypasses_armor".into(), vec![br#"{"values":["demo:heated_air"]}"#.to_vec()]),
    ];
    let open = resources
        .health
        .with_packs(&root, &PackOverlay { files: &files }, false)
        .unwrap();
    let id = open.damage.id("demo:heated_air").unwrap();
    let player = || {
        let mut p = PlayerMotion::new(&resources, horizon_core::geometry::Vec3::ZERO).unwrap();
        p.entity_id = 9701;
        let key = p.attributes.key("minecraft:armor").unwrap();
        let armor = p.attributes.get_instance(Some(key)).unwrap().unwrap();
        p.attributes
            .edit(&armor, |a, o| a.set_base_value(20.0, o))
            .unwrap();
        p
    };
    let rules = HealthRules::default();
    let mut armored = player();
    assert!(armored.hurt(&open, rules, id, 4.0).unwrap());
    assert_eq!(armored.health.health(), 16.0);
    assert_eq!(armored.health.food.exhaustion, 0.25);
    assert_eq!(armored.hurt_log.last().unwrap().source, "demo:heated_air");
    let mut resistant = player();
    resistant.status.fire_resistance = true;
    assert!(!resistant.hurt(&open, rules, id, 4.0).unwrap());
    assert_eq!(resistant.health.health(), 20.0);
    let mut item_rules = crate::enchant::Enchantments::default();
    item_rules.bind_damage_catalog(&open.damage);
    assert!(item_rules.damage_tags["minecraft:is_fire"].contains("demo:heated_air"));
    // Disable the pack: the registered type remains, its added tags do not.
    let reload = open.with_packs(&root, &PackOverlay::NONE, true).unwrap();
    assert_eq!(reload.damage.id("demo:heated_air"), Some(id));
    let mut after = player();
    after.status.fire_resistance = true;
    assert!(after.hurt(&reload, rules, id, 4.0).unwrap());
    assert!(
        after.health.health() > 16.0 && after.health.health() < 20.0,
        "the same source now encounters armor and ignores fire resistance"
    );
    item_rules.bind_damage_catalog(&reload.damage);
    assert!(!item_rules.damage_tags["minecraft:is_fire"].contains("demo:heated_air"));
    assert!(
        open.damage.in_tag(id, "minecraft:is_fire"),
        "another snapshot remains unchanged"
    );
}
