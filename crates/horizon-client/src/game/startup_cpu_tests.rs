use super::*;

#[test]
fn a_folder_without_prepared_files_is_named_before_loading() {
    let root = std::env::temp_dir().join(format!("horizon-unprepared-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let missing = missing_resources(&root.join(".local/horizon-client/26.3"))
        .expect("no manifest is reported");
    assert!(missing.contains("horizon-resources.json"), "{missing}");
    assert!(missing.contains("horizon-unprepared-"), "{missing}");
    assert!(missing.contains("launcher"), "{missing}");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("horizon-resources.json"), "{}").unwrap();
    assert_eq!(missing_resources(&root), None);

    // A killed export leaves an empty file behind; the error names it
    // instead of serde's "expected value at line 1 column 1".
    std::fs::write(root.join("gameplay-facts.json"), "").unwrap();
    let damaged = missing_resources(&root).expect("an empty prepared file is reported");
    assert!(damaged.contains("gameplay-facts.json"), "{damaged}");
    assert!(damaged.contains("damaged"), "{damaged}");
    std::fs::write(root.join("gameplay-facts.json"), "{}").unwrap();
    assert_eq!(missing_resources(&root), None);

    std::fs::write(root.join("horizon-resources.json"), " \n").unwrap();
    let damaged = missing_resources(&root).expect("an empty manifest is reported");
    assert!(damaged.contains("horizon-resources.json"), "{damaged}");
    std::fs::remove_dir_all(&root).unwrap();
}

#[test]
fn worker_prepares_owned_models_and_inventory_before_handoff() {
    let root = std::env::temp_dir().join(format!("horizon-startup-cpu-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(
        root.join("entity-models.json"),
        r#"{
        "minecraft:end_crystal#main": {}, "minecraft:ender_dragon#main": {},
        "minecraft:arrow#main": {}, "minecraft:trident#main": {},
        "minecraft:armor_stand#main": {}, "minecraft:cushion#main": {},
        "minecraft:leash_knot#main": {}, "minecraft:wind_charge#main": {},
        "minecraft:shulker_bullet#main": {}, "minecraft:wither_skull#main": {},
        "minecraft:evoker_fangs#main": {}, "minecraft:standing_banner#main": {},
        "minecraft:standing_banner#flag": {}
    }"#,
    )
    .unwrap();
    std::fs::write(
        root.join("creative-tabs.json"),
        r#"[{
        "id":"minecraft:building_blocks", "items":["minecraft:stone"]
    }]"#,
    )
    .unwrap();
    let worker_root = root.clone();
    let prepared = std::thread::spawn(move || {
        GameCpuResources::load(&worker_root, None).map_err(|error| error.to_string())
    })
    .join()
    .unwrap()
    .unwrap();
    std::fs::remove_dir_all(&root).unwrap();
    assert!(prepared.crystal_model.is_some());
    assert!(prepared.cushion_model.is_some());
    let models = &prepared.entity_models;
    assert!(models.leash_knot.is_some() && models.wind_charge.is_some());
    assert!(models.shulker_bullet.is_some() && models.wither_skull.is_some());
    assert!(models.evoker_fangs.is_some() && models.banner.is_some());
    assert!(prepared.dragon_model.is_some());
    assert!(prepared.arrow_model.is_some());
    assert!(prepared.trident_model.is_some());
    assert!(prepared.armor_stand_assets.main.is_some());
    assert_eq!(
        prepared.creative_screen.tabs[0].items[0].item.as_ref(),
        "minecraft:stone"
    );
    // No documents from the old generation remain available to reload.
    assert!(GameCpuResources::load(&root, None).is_err());
}
