#[cfg(test)]
mod typed_state_range_read_tests {
    use super::*;
    #[test]
    #[ignore = "needs prepared resources, actual pack and scratch homes/cwd"]
    fn loaded_pack_state_ranges_match_typed_controls() {
        let mut game = crate::game::native_machines::tests::server_game();
        let source =
            std::path::PathBuf::from(std::env::var_os("HORIZON_TYPED_RANGE_PACK").unwrap());
        game.world_root = std::env::current_dir().unwrap().join("typed-range-world");
        let target = game.world_root.join("datapacks/typed_ranges");
        let preds = target.join("data/ranges/predicate");
        std::fs::create_dir_all(&preds).unwrap();
        std::fs::copy(source.join("pack.mcmeta"), target.join("pack.mcmeta")).unwrap();
        for entry in std::fs::read_dir(source.join("data/ranges/predicate")).unwrap() {
            let path = entry.unwrap().path();
            std::fs::copy(&path, preds.join(path.file_name().unwrap())).unwrap();
        }
        game.build_registries();
        game.set_allow_commands(true);
        assert!(game.run_command("reload"));
        assert!(game.fns.enabled.contains(&"file/typed_ranges".to_owned()));
        assert!(
            game.fns.store.errors.is_empty(),
            "{:?}",
            game.fns.store.errors
        );
        assert_eq!(
            game.fns
                .store
                .predicates
                .keys()
                .filter(|id| id.starts_with("ranges:"))
                .count(),
            13
        );
        for entry in std::fs::read_dir(source.join("data/ranges/predicate")).unwrap() {
            let path = entry.unwrap().path();
            let id = format!("ranges:{}", path.file_stem().unwrap().to_str().unwrap());
            let expected: serde_json::Value =
                serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
            assert_eq!(game.fns.store.predicates.get(&id), Some(&expected), "{id}");
        }
        // The helper's empty resident column has not passed server loading.
        // Use actual streaming and forced-ticket paths, never a lit/status override.
        game.world.chunks.remove(&(0, 0));
        // Game::new pauses its placeholder streamer until world opening.
        game.streaming.paused = false;
        assert!(game.run_command("forceload add 0 0 15 15"));
        game.update_chunk_tickets();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(120);
        while !game.streaming.is_lit(0, 0) || !game.world.chunks.contains_key(&(0, 0)) {
            let _ = game.streaming.update(&mut game.world);
            game.spawn_loaded_entities();
            assert!(
                std::time::Instant::now() < deadline,
                "server column did not finish loading"
            );
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        game.spawn_loaded_entities();
        game.update_chunk_tickets();
        assert!(game.run_command("execute if loaded 0 200 0"));
        assert!(game.run_command("scoreboard objectives add ranges dummy"));
        assert!(game.run_command(
            "setblock 0 200 0 minecraft:chest[facing=north,type=single,waterlogged=false]"
        ));
        assert!(game.run_command("item replace block 0 200 0 container.0 with minecraft:stone 3"));
        let chest = game.block_entity_nbt([0, 200, 0]).unwrap();
        let (expected_chest, _) = horizon_world::nbt::parse_snbt(r#"{components:{},x:0,y:200,z:0,id:"minecraft:chest",Items:[{count:3,Slot:0b,id:"minecraft:stone"}]}"#).unwrap();
        assert_eq!(chest, expected_chest);
        let clock = game.world.game_time;
        let mut failures = Vec::new();
        let mut callback = |game: &mut Game<'_>, x: i32, name: &str, expected: bool| {
            for (sign, expected) in [("if", expected), ("unless", !expected)] {
                for h in ["#r", "#s"] {
                    assert!(game.run_command(&format!("scoreboard players set {h} ranges -99")));
                }
                game.run_command(&format!("execute store result score #r ranges store success score #s ranges positioned {x} 200 0 {sign} predicate ranges:{name}"));
                for h in ["#r", "#s"] {
                    let actual = game.score.board.score(h, "ranges").unwrap().value;
                    if actual != i32::from(expected) {
                        failures.push(format!(
                            "x={x} {name}/{sign}/{h}: expected {}, got {actual}",
                            i32::from(expected)
                        ));
                    }
                }
                assert_eq!(game.block_entity_nbt([0, 200, 0]).as_ref(), Some(&chest));
                assert_eq!(game.world.game_time, clock);
            }
        };
        callback(&mut game, 0, "always", true);
        callback(&mut game, 0, "never", false);
        for sign in ["if", "unless"] {
            for h in ["#r", "#s"] {
                assert!(game.run_command(&format!("scoreboard players set {h} ranges -99")));
            }
            assert!(!game.run_command(&format!("execute store result score #r ranges store success score #s ranges positioned 0 200 0 {sign} predicate ranges:missing")));
            for h in ["#r", "#s"] {
                assert_eq!(game.score.board.score(h, "ranges").unwrap().value, -99);
            }
        }
        for (i, facing) in ["north", "south", "west", "east"].iter().enumerate() {
            let x = 2 + 2 * i as i32;
            let wet = i % 2 == 1;
            assert!(game.run_command(&format!(
                "setblock {x} 200 0 minecraft:chest[facing={facing},type=single,waterlogged={wet}]"
            )));
            assert!(game.run_command(&format!("execute if loaded {x} 200 0")));
            assert!(game.run_command(&format!("execute if block {x} 200 0 minecraft:chest[facing={facing},type=single,waterlogged={wet}]")));
            for (name, want) in [
                ("enum", matches!(*facing, "south" | "west")),
                ("enum_max", *facing != "east"),
                ("exact", *facing == "west"),
                ("invalid", false),
                ("inverted", false),
                ("boolean", !wet),
            ] {
                callback(&mut game, x, name, want);
            }
        }
        assert!(game.run_command("setblock 10 199 0 minecraft:farmland"));
        for age in [0, 2, 5, 7] {
            assert!(game.run_command(&format!("setblock 10 200 0 minecraft:wheat[age={age}]")));
            assert!(game.run_command("execute if loaded 10 200 0"));
            assert!(game.run_command(&format!(
                "execute if block 10 200 0 minecraft:wheat[age={age}]"
            )));
            assert!(game.run_command("execute if block 10 199 0 minecraft:farmland"));
            for (name, want) in [
                ("age", (2..=6).contains(&age)),
                ("age_padded", (2..=6).contains(&age)),
                ("age_invalid", false),
                ("age_inverted", false),
                ("age_exact", age == 5),
            ] {
                callback(&mut game, 10, name, want);
            }
        }
        assert_eq!(game.block_entity_nbt([0, 200, 0]).as_ref(), Some(&chest));
        assert_eq!(game.world.game_time, clock);
        assert!(
            failures.is_empty(),
            "typed state range failures:\n{}",
            failures.join("\n")
        );
    }

    #[test]
    #[ignore = "needs prepared resources, actual pack and scratch homes/cwd"]
    fn loaded_pack_direction_ranges_keep_full_and_filtered_domains() {
        let mut game = crate::game::native_machines::tests::server_game();
        let source =
            std::path::PathBuf::from(std::env::var_os("HORIZON_TYPED_RANGE_PACK").unwrap());
        game.world_root = std::env::current_dir()
            .unwrap()
            .join("typed-direction-world");
        let target = game.world_root.join("datapacks/typed_ranges");
        let preds = target.join("data/ranges/predicate");
        std::fs::create_dir_all(&preds).unwrap();
        std::fs::copy(source.join("pack.mcmeta"), target.join("pack.mcmeta")).unwrap();
        for entry in std::fs::read_dir(source.join("data/ranges/predicate")).unwrap() {
            let path = entry.unwrap().path();
            std::fs::copy(&path, preds.join(path.file_name().unwrap())).unwrap();
        }
        game.build_registries();
        game.set_allow_commands(true);
        assert!(game.run_command("reload"));
        assert!(game.fns.enabled.contains(&"file/typed_ranges".to_owned()));
        assert!(
            game.fns.store.errors.is_empty(),
            "{:?}",
            game.fns.store.errors
        );
        assert_eq!(
            game.fns
                .store
                .predicates
                .keys()
                .filter(|id| id.starts_with("ranges:"))
                .count(),
            13
        );
        for entry in std::fs::read_dir(source.join("data/ranges/predicate")).unwrap() {
            let path = entry.unwrap().path();
            let id = format!("ranges:{}", path.file_stem().unwrap().to_str().unwrap());
            let expected: serde_json::Value =
                serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
            assert_eq!(game.fns.store.predicates.get(&id), Some(&expected), "{id}");
        }
        // The helper's empty resident column has not passed server loading.
        // Use actual streaming and forced-ticket paths, never a lit/status override.
        game.world.chunks.remove(&(0, 0));
        // Game::new pauses its placeholder streamer until world opening.
        game.streaming.paused = false;
        assert!(game.run_command("forceload add 0 0 15 15"));
        game.update_chunk_tickets();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(120);
        while !game.streaming.is_lit(0, 0) || !game.world.chunks.contains_key(&(0, 0)) {
            let _ = game.streaming.update(&mut game.world);
            game.spawn_loaded_entities();
            assert!(
                std::time::Instant::now() < deadline,
                "server column did not finish loading"
            );
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        game.spawn_loaded_entities();
        game.update_chunk_tickets();
        assert!(game.run_command("execute if loaded 0 200 0"));

        let cases = [
            (
                "direction_span",
                "facing",
                serde_json::json!({"min":"north","max":"west"}),
            ),
            (
                "direction_reversed",
                "facing",
                serde_json::json!({"min":"up","max":"down"}),
            ),
            (
                "vertical_up",
                "vertical_direction",
                serde_json::json!({"min":"up"}),
            ),
            (
                "vertical_invalid",
                "vertical_direction",
                serde_json::json!({"min":"north"}),
            ),
        ];
        for (name, prop, bounds) in &cases {
            std::fs::write(preds.join(format!("{name}.json")), serde_json::json!({"type":"minecraft:location_check","predicate":{"block":{"state":{prop.to_string():bounds}}}}).to_string()).unwrap();
        }
        assert!(game.run_command("reload"));
        assert_eq!(
            game.fns
                .store
                .predicates
                .keys()
                .filter(|id| id.starts_with("ranges:"))
                .count(),
            17
        );
        assert!(game.run_command("scoreboard objectives add ranges dummy"));
        assert!(game.run_command("setblock 0 200 0 minecraft:chest"));
        assert!(game.run_command("item replace block 0 200 0 container.0 with minecraft:stone 3"));
        let chest = game.block_entity_nbt([0, 200, 0]).unwrap();
        let clock = game.world.game_time;
        let callback = |game: &mut Game<'_>, x: i32, name: &str, expected: bool| {
            for (sign, want) in [("if", expected), ("unless", !expected)] {
                for h in ["#r", "#s"] {
                    assert!(game.run_command(&format!("scoreboard players set {h} ranges -99")));
                }
                game.run_command(&format!("execute store result score #r ranges store success score #s ranges positioned {x} 200 0 {sign} predicate ranges:{name}"));
                for h in ["#r", "#s"] {
                    assert_eq!(
                        game.score.board.score(h, "ranges").unwrap().value,
                        i32::from(want),
                        "{name}/{sign}/{h}"
                    );
                }
                assert_eq!(game.block_entity_nbt([0, 200, 0]).as_ref(), Some(&chest));
                assert_eq!(game.world.game_time, clock);
            }
        };
        for facing in ["down", "up", "north", "south", "west", "east"] {
            assert!(game.run_command(&format!(
                "setblock 2 200 0 minecraft:barrel[facing={facing}]"
            )));
            assert!(game.run_command(&format!(
                "execute if block 2 200 0 minecraft:barrel[facing={facing}]"
            )));
            assert!(game.run_command("execute if loaded 2 200 0"));
            callback(
                &mut game,
                2,
                "direction_span",
                matches!(facing, "north" | "south" | "west"),
            );
            callback(&mut game, 2, "direction_reversed", false);
        }
        for (x, block) in [(4, "pointed_dripstone"), (6, "sulfur_spike")] {
            for vertical in ["up", "down"] {
                assert!(game.run_command(&format!(
                    "setblock {x} 200 0 minecraft:{block}[vertical_direction={vertical}]"
                )));
                assert!(game.run_command(&format!(
                    "execute if block {x} 200 0 minecraft:{block}[vertical_direction={vertical}]"
                )));
                assert!(game.run_command(&format!("execute if loaded {x} 200 0")));
                callback(&mut game, x, "vertical_up", vertical == "up");
                callback(&mut game, x, "vertical_invalid", false);
            }
        }
    }
}
