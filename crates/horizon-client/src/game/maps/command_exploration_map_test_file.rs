#[cfg(test)]
mod command_exploration_map_tests {
    use horizon_world::nbt::Tag;
    use std::sync::Arc;

    #[test]
    #[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
    fn actual_item_modify_materializes_selected_exploration_map() {
        let mut game = crate::game::native_machines::tests::server_game();
        game.player = Some(
            horizon_client::motion::PlayerMotion::new(
                game.motion,
                horizon_core::geometry::Vec3::new(4.5, 200.0, 4.5),
            )
            .unwrap(),
        );
        assert_eq!(
            game.player_source().entity,
            Some(crate::game::commands::Target::Player)
        );
        game.build_registries();
        game.set_allow_commands(true);
        // A world of its own under the temporary folder, removed at the end.
        game.world_root = std::env::temp_dir().join(format!(
            "horizon-command-exploration-map-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&game.world_root);
        let root = std::path::PathBuf::from(std::env::var_os("HORIZON_TEST_RESOURCES").unwrap());
        let terrain = horizon_client::vanillagen::VanillaTerrain::new(
            &root.join("worldgen"),
            game.pack.assets.registry,
            &game.res.core.biomes,
            42,
        )
        .unwrap();
        game.end.set_locator(Some(Arc::new(terrain)));
        assert!(game.run_command("difficulty normal"));
        assert!(game.run_command("forceload add 0 0"));
        // Independent normal-world destination control, not the command result.
        let names = game.structure_tag("minecraft:on_ocean_monument_maps");
        assert!(!names.is_empty());
        let target = game
            .end
            .locator()
            .unwrap()
            .locate_for_map(&names, [0, 64, 0], 50, false)
            .unwrap();
        assert_eq!([target.1[0], target.1[2]], [784, -176]);
        assert_eq!(game.world.game_time, 0);
        assert!(game.run_command(
            "item replace entity @s hotbar.0 with map[custom_data={map_command_sentinel:2109}]"
        ));
        assert!(game.run_command("item replace entity @s hotbar.1 with map 2"));
        let original = game.inventory[0].clone().unwrap();
        assert_eq!(&*original.item, "minecraft:map");
        assert_eq!(original.count, 1);
        assert_eq!(game.selected, 0);
        assert!(
            game.run_command(
                "item modify entity @s hotbar.1 {type:\"minecraft:set_count\",count:2}"
            )
        );
        assert_eq!(game.inventory[0].as_ref(), Some(&original));
        assert_eq!(game.inventory[1].as_ref().unwrap().count, 2);
        assert!(game.maps.data.is_empty());
        assert!(game.maps.last_id.is_none());
        assert!(
            !game.run_command(
                "item modify entity @s hotbar.2 {type:\"minecraft:set_count\",count:2}"
            )
        );
        assert!(game.inventory[2].is_none());
        assert!(game.maps.data.is_empty());
        assert!(game.run_command("execute positioned 0.0 64.0 0.0 run item modify entity @s hotbar.0 {type:\"minecraft:exploration_map\",destination:\"#minecraft:on_ocean_monument_maps\",decoration:\"minecraft:monument\",zoom:2,skip_existing_chunks:false}"));
        let actual = game.inventory[0].as_ref().unwrap();
        assert_eq!(&*actual.item, "minecraft:map");
        assert_eq!(actual.count, 1);
        assert_eq!(
            actual.data().unwrap().other("minecraft:custom_data"),
            original.data().unwrap().other("minecraft:custom_data")
        );
        let item_id = actual.data().and_then(|d| d.map_id);
        let decoration = actual
            .data()
            .and_then(|d| d.other("minecraft:map_decorations"))
            .cloned();
        game.save_maps();
        let saved =
            horizon_client::save::read_dat(&game.world_root.join("data/minecraft/maps/0.dat"));
        let counter = horizon_client::save::read_dat(
            &game.world_root.join("data/minecraft/maps/last_id.dat"),
        );
        let headers = saved.as_ref().and_then(|t| t.get("data")).map(|d| {
            (
                d.get("xCenter").cloned(),
                d.get("zCenter").cloned(),
                d.get("scale").cloned(),
            )
        });
        assert_eq!(game.world.game_time, 0);
        assert_eq!(
            (
                item_id,
                game.maps.data.contains_key(&0),
                game.maps.last_id,
                headers,
                counter.and_then(|t| t.get("data").and_then(|d| d.get("map")).cloned()),
                decoration.is_some()
            ),
            (
                Some(0),
                true,
                Some(0),
                Some((
                    Some(Tag::Int(704)),
                    Some(Tag::Int(-320)),
                    Some(Tag::Byte(2))
                )),
                Some(Tag::Int(0)),
                true
            ),
            "actual registered item command must allocate, decorate, model and save its selected stack's map"
        );
    }
}

// Added after the qualified baseline: bounded source-backed preservation controls.
#[cfg(test)]
mod added_preservation_tests {
    use horizon_client::{containers::BlockEntity, items::ItemStack, structure_blocks};
    use horizon_world::loot::{ExplorationMap, LootStack};
    use serde_json::json;

    fn game(label: &str) -> crate::game::Game<'static> {
        let mut game = crate::game::native_machines::tests::server_game();
        game.player = Some(
            horizon_client::motion::PlayerMotion::new(
                game.motion,
                horizon_core::geometry::Vec3::new(4.5, 200.0, 4.5),
            )
            .unwrap(),
        );
        game.world_root =
            std::path::PathBuf::from(std::env::var_os("HORIZON_HOME").unwrap()).join(label);
        let pack = game.world_root.join("datapacks/mapcmd");
        let tag = pack.join("data/mapcmd/tags/worldgen/structure/empty.json");
        std::fs::create_dir_all(tag.parent().unwrap()).unwrap();
        std::fs::write(
            pack.join("pack.mcmeta"),
            r#"{"pack":{"description":"Map command controls"}}"#,
        )
        .unwrap();
        std::fs::write(tag, r#"{"values":[]}"#).unwrap();
        game.build_registries();
        game.set_allow_commands(true);
        game.reload_datapacks(false);
        assert!(game.structure_tag("mapcmd:empty").is_empty());
        game
    }

    fn pending() -> ItemStack {
        structure_blocks::loot_item_with_components(&LootStack {
            item: "minecraft:map".into(),
            count: 1,
            damage: 0,
            enchantments: Vec::new(),
            potion: None,
            map: true,
            exploration: Some(Box::new(ExplorationMap::from_json(&json!({
                "destination": "#mapcmd:empty", "skip_existing_chunks": false
            })))),
            ominous_amplifier: None,
            stew_effects: Vec::new(),
            names: Vec::new(),
            components: Vec::new(),
        })
    }

    #[test]
    #[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
    fn nonempty_command_keeps_no_destination_but_pending_loot_filters_it() {
        let mut game = game("map-command-no-destination");
        assert!(
            game.run_command(
                "item replace entity @s hotbar.0 with map[custom_data={sentinel:5}] 3"
            )
        );
        let original = game.inventory[0].clone().unwrap();
        assert!(game.run_command("execute positioned -0.25 64.0 -0.25 run item modify entity @s hotbar.0 {type:\"minecraft:exploration_map\",destination:\"#mapcmd:empty\",skip_existing_chunks:false}"));
        assert_eq!(game.inventory[0].as_ref(), Some(&original));
        let mut no_request = original.clone();
        assert!(game.resolve_pending_map(&mut no_request, [0.; 3]));
        assert_eq!(no_request, original);
        let mut request = pending();
        assert!(structure_blocks::has_pending_map(&request));
        assert!(!game.resolve_pending_map(&mut request, [0.; 3]));
        assert!(!structure_blocks::has_pending_map(&request));
        assert_eq!(request.count, 1);
        assert!(game.resolve_pending_map(&mut request, [0.; 3]));
        game.world.block_entities.insert(
            (1, 200, 1),
            BlockEntity::Chest(vec![Some(pending()), Some(original.clone())]),
        );
        game.resolve_container_maps((1, 200, 1), [0.; 3]);
        let Some(BlockEntity::Chest(slots)) = game.world.block_entities.get(&(1, 200, 1)) else {
            panic!("actual container missing")
        };
        assert!(
            slots[0].is_none(),
            "existing pending-loot caller filters a failed search"
        );
        assert_eq!(slots[1].as_ref(), Some(&original));
        assert!(game.maps.data.is_empty());
        assert!(game.maps.last_id.is_none());
        assert_eq!(game.world.game_time, 0);
    }

    #[test]
    #[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
    fn loaded_reference_list_sequence_filter_order_preserves_counts_and_empty() {
        let mut game = game("map-command-reference-order");
        let path = game
            .world_root
            .join("datapacks/mapcmd/data/mapcmd/item_modifier");
        std::fs::create_dir_all(&path).unwrap();
        let first = json!({"type":"minecraft:set_count","count":2});
        let second = json!({"type":"minecraft:set_count","count":1,"add":true});
        let ordered = json!([
            "mapcmd:first",
            {"type":"minecraft:sequence","functions":[
                {"type":"minecraft:exploration_map","destination":"#mapcmd:empty","skip_existing_chunks":false},
                "mapcmd:second",
                {"type":"minecraft:filtered","item_filter":{"items":"minecraft:map","count":3},
                 "on_pass":{"type":"minecraft:set_count","count":5},"on_fail":{"type":"minecraft:discard"}}
            ]}
        ]);
        for (name, body) in [
            ("first", &first),
            ("second", &second),
            ("ordered", &ordered),
        ] {
            std::fs::write(
                path.join(format!("{name}.json")),
                serde_json::to_vec(body).unwrap(),
            )
            .unwrap();
        }
        game.reload_datapacks(false);
        assert_eq!(
            game.fns.store.item_modifiers.get("mapcmd:ordered"),
            Some(&ordered)
        );
        assert!(
            game.run_command("item replace entity @s hotbar.0 with map[custom_data={sentinel:8}]")
        );
        let custom = game.inventory[0]
            .as_ref()
            .unwrap()
            .data()
            .unwrap()
            .other("minecraft:custom_data")
            .cloned();
        assert!(game.run_command("item modify entity @s hotbar.0 mapcmd:ordered"));
        let item = game.inventory[0].as_ref().unwrap();
        assert_eq!(
            item.count, 5,
            "named references, list, sequence and filter execute in place"
        );
        assert_eq!(
            item.data().unwrap().other("minecraft:custom_data"),
            custom.as_ref()
        );
        assert_eq!(
            game.fns.store.item_modifiers.get("mapcmd:ordered"),
            Some(&ordered)
        );
        assert!(!game.run_command("item modify entity @s hotbar.2 mapcmd:ordered"));
        assert!(game.inventory[2].is_none());
        assert!(game.run_command("item modify entity @s hotbar.0 [{type:\"minecraft:discard\"},{type:\"minecraft:exploration_map\",destination:\"#mapcmd:empty\"}]"));
        assert!(game.inventory[0].is_none());
        assert!(game.maps.data.is_empty());
        assert!(game.maps.last_id.is_none());
        assert_eq!(game.world.game_time, 0);
        let _ = std::fs::remove_dir_all(&game.world_root);
    }
}
