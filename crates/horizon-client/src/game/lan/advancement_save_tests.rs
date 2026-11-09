#[cfg(test)]
mod connected_advancement_save_tests {
    use super::super::Game;
    use std::fs;
    fn command(game: &mut Game<'_>, text: &str) -> Vec<horizon_client::text::Text> {
        game.chat_capture = Some(Vec::new());
        let dispatcher = horizon_client::command::vanilla::build_for(false, true);
        game.ui.registries.permission = 4;
        let parsed = dispatcher.parse(text, &game.ui.registries);
        let (ctx, _) = parsed
            .executable()
            .expect("actual registered dedicated command");
        let (root, ctx) = (parsed.context.clone(), ctx.clone());
        assert_eq!(
            game.run_chain(&root, &ctx).unwrap(),
            if text == "reload" { 0 } else { 1 },
            "{text}"
        );
        game.chat_capture.take().unwrap()
    }
    #[test]
    #[ignore = "needs prepared resources and scratch homes"]
    fn save_commands_write_the_connected_nonowners_current_advancements() {
        use super::super::{execution_role::ExecutionRole, stats};
        use std::sync::{Arc, Mutex};
        let mut game = super::super::native_machines::tests::server_game();
        game.options.execution_role = ExecutionRole::DedicatedServer;
        let dir = std::env::temp_dir().join(format!(
            "horizon-connected-advancement-save-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        game.world_root = dir.clone();
        game.save = Arc::new(horizon_client::save::SaveDir::new(
            dir.clone(),
            horizon_client::save::LevelShape::OVERWORLD,
        ));
        use horizon_client::server::user_lists::{BanList, IpBanList, OpList, WhiteList};
        game.server_lists = Some(Arc::new(Mutex::new(super::super::dedicated::Lists {
            ops: OpList::new(dir.join("ops.json")),
            white: WhiteList::new(dir.join("whitelist.json")),
            bans: BanList::new(dir.join("banned-players.json")),
            ip_bans: IpBanList::new(dir.join("banned-ips.json")),
            use_whitelist: false,
            enforce_whitelist: false,
            op_level: 4,
            player_idle_timeout: 0,
            stop: false,
            autosave: true,
            whitelist_bypass: Vec::new(),
        })));
        game.set_allow_commands(true);
        let _rx = super::world_spawn_sync_tests::host_for(&mut game);
        let host = game.lan.as_mut().unwrap();
        host.hub.guests[0].name = "Bob".into();
        host.owner = None;
        host.hub.guests[0].dimension = "minecraft:overworld".into();
        assert!(host.hub.guests[0].placed);
        assert!(!host.hub.guests[0].released);
        assert_eq!(host.hub.guests[0].uuid, 1);

        assert!(host.owner.is_none());
        assert_ne!(game.level.player_uuid, Some([0, 0, 0, 1]));
        let start_clock = game.world.game_time;
        let reload = command(&mut game, "reload");
        assert!(
            reload
                .iter()
                .any(|m| m.translation_key() == Some("commands.reload.success"))
        );
        for id in ["minecraft:story/root", "minecraft:story/mine_stone"] {
            assert!(
                game.adv.tree.find_id(id).is_some(),
                "actual loaded tree {id}"
            );
        }
        assert!(
            !game.lan.as_ref().unwrap().hub.guests[0]
                .advancements
                .has("minecraft:story/root", "crafting_table")
        );
        assert!(
            !game.lan.as_ref().unwrap().hub.guests[0]
                .advancements
                .has("minecraft:story/mine_stone", "get_stone")
        );
        command(&mut game, "advancement grant Bob only minecraft:story/root");
        let path = stats::player_json(&dir, "advancements", [0, 0, 0, 1]);
        let host = game.lan.as_ref().unwrap();
        assert!(
            host.hub.guests[0]
                .advancements
                .has("minecraft:story/root", "crafting_table")
        );
        // Seed a real prior record with the existing record writer. This is not
        // the connected save under test; its result must not substitute for it.
        game.save_guest_records(host, &host.hub.guests[0]);
        let prior: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(prior["minecraft:story/root"]["done"], true);
        assert!(prior.get("minecraft:story/mine_stone").is_none());
        command(
            &mut game,
            "advancement grant Bob only minecraft:story/mine_stone",
        );
        let mut failures = Vec::new();
        for (phase, save, root_expected) in [
            ("save", "save-all", true),
            ("flush", "save-all flush", false),
            ("repeat", "save-all flush", false),
        ] {
            if phase == "flush" {
                command(
                    &mut game,
                    "advancement revoke Bob only minecraft:story/root",
                );
            }
            let host = game.lan.as_ref().unwrap();
            assert!(host.owner.is_none());
            assert_eq!(host.hub.guests.len(), 1);
            let guest = &host.hub.guests[0];
            assert_eq!(guest.uuid, 1);
            assert!(guest.placed && !guest.released && !guest.fake);
            assert_eq!(
                guest
                    .advancements
                    .has("minecraft:story/root", "crafting_table"),
                root_expected
            );
            assert!(
                guest
                    .advancements
                    .has("minecraft:story/mine_stone", "get_stone")
            );
            let messages = command(&mut game, save);
            assert!(
                messages
                    .iter()
                    .any(|m| m.translation_key() == Some("commands.save.success")),
                "actual save-success receipt"
            );
            assert_eq!(game.lan.as_ref().unwrap().hub.guests[0].uuid, 1);
            assert_eq!(game.world.game_time, start_clock);
            let raw = fs::read_to_string(&path).unwrap();
            println!("phase={phase} connected_uuid=1 command={save} file={raw}");
            let saved: serde_json::Value = serde_json::from_str(&raw).unwrap();
            let root_saved = saved["minecraft:story/root"]["done"] == true;
            let stone_saved = saved["minecraft:story/mine_stone"]["done"] == true;
            if (root_saved, stone_saved) != (root_expected, true) {
                failures.push((phase, root_saved, stone_saved, root_expected));
            }
        }
        fs::remove_dir_all(&dir).unwrap();
        assert!(
            failures.is_empty(),
            "successful connected saves kept stale advancement progress: {failures:?}"
        );
    }
}

#[cfg(test)]
mod connected_advancement_preservation_tests {
    use super::super::Game;
    use std::fs;
    fn command(game: &mut Game<'_>, text: &str) -> Vec<horizon_client::text::Text> {
        game.chat_capture = Some(Vec::new());
        let dispatcher = horizon_client::command::vanilla::build_for(false, true);
        game.ui.registries.permission = 4;
        let parsed = dispatcher.parse(text, &game.ui.registries);
        let (ctx, _) = parsed
            .executable()
            .expect("actual registered dedicated command");
        let (root, ctx) = (parsed.context.clone(), ctx.clone());
        assert_eq!(
            game.run_chain(&root, &ctx).unwrap(),
            if text == "reload" { 0 } else { 1 },
            "{text}"
        );
        game.chat_capture.take().unwrap()
    }
    // Added after baseline: actual command/model preservation, not new Java breadth.
    #[test]
    #[ignore = "needs prepared resources and scratch homes"]
    fn empty_progress_missing_play_and_disconnect_wrapper_stay_distinct() {
        use super::super::{execution_role::ExecutionRole, stats};
        use std::sync::{Arc, Mutex};
        let mut game = super::super::native_machines::tests::server_game();
        game.options.execution_role = ExecutionRole::DedicatedServer;
        let dir = std::env::temp_dir().join(format!(
            "horizon-advancement-preservation-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        game.world_root = dir.clone();
        game.save = Arc::new(horizon_client::save::SaveDir::new(
            dir.clone(),
            horizon_client::save::LevelShape::OVERWORLD,
        ));
        use horizon_client::server::user_lists::{BanList, IpBanList, OpList, WhiteList};
        game.server_lists = Some(Arc::new(Mutex::new(super::super::dedicated::Lists {
            ops: OpList::new(dir.join("ops.json")),
            white: WhiteList::new(dir.join("whitelist.json")),
            bans: BanList::new(dir.join("banned-players.json")),
            ip_bans: IpBanList::new(dir.join("banned-ips.json")),
            use_whitelist: false,
            enforce_whitelist: false,
            op_level: 4,
            player_idle_timeout: 0,
            stop: false,
            autosave: true,
            whitelist_bypass: Vec::new(),
        })));
        game.set_allow_commands(true);
        let _rx = super::world_spawn_sync_tests::host_for(&mut game);
        let host = game.lan.as_mut().unwrap();
        host.hub.guests[0].name = "Bob".into();
        host.owner = None;
        host.hub.guests[0].dimension = "minecraft:overworld".into();
        assert!(host.hub.guests[0].placed);
        assert!(!host.hub.guests[0].released);
        assert_eq!(host.hub.guests[0].uuid, 1);

        assert!(host.owner.is_none());
        assert!(!host.hub.guests[0].advancements_loaded);
        let reload = command(&mut game, "reload");
        assert!(
            reload
                .iter()
                .any(|m| m.translation_key() == Some("commands.reload.success"))
        );
        assert!(game.adv.tree.find_id("minecraft:story/root").is_some());
        let path = stats::player_json(&dir, "advancements", [0, 0, 0, 1]);
        let stats_path = stats::player_json(&dir, "stats", [0, 0, 0, 1]);
        let read = || -> serde_json::Value {
            serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap()
        };
        for (action, save, empty) in [
            (
                "advancement grant Bob only minecraft:story/root",
                "save-all",
                false,
            ),
            ("advancement revoke Bob everything", "save-all flush", true),
        ] {
            command(&mut game, action);
            let g = &game.lan.as_ref().unwrap().hub.guests[0];
            assert!(
                !g.advancements_loaded,
                "real new-player state stays unloaded"
            );
            assert_eq!(
                g.advancements.criteria.values().all(|m| m.is_empty()),
                empty
            );
            let messages = command(&mut game, save);
            assert!(
                messages
                    .iter()
                    .any(|m| m.translation_key() == Some("commands.save.success"))
            );
            let saved = read();
            if empty {
                assert_eq!(
                    saved.as_object().unwrap().len(),
                    1,
                    "only DataVersion, no stale progress"
                );
            } else {
                assert_eq!(saved["minecraft:story/root"]["done"], true);
            }
            assert!(!game.lan.as_ref().unwrap().hub.guests[0].advancements_loaded);
        }
        // A valid loaded-empty value is saved too, independently of missing play.
        game.lan.as_mut().unwrap().hub.guests[0].advancements_loaded = true;
        command(&mut game, "save-all");
        assert_eq!(read().as_object().unwrap().len(), 1);
        command(&mut game, "advancement grant Bob only minecraft:story/root");
        let before = fs::read(&path).unwrap();
        let play = game.lan.as_mut().unwrap().play.remove(&1).unwrap();
        command(&mut game, "save-all flush");
        assert_eq!(
            fs::read(&path).unwrap(),
            before,
            "missing play is not invented or serialized"
        );
        assert!(!game.lan.as_ref().unwrap().play.contains_key(&1));
        game.lan.as_mut().unwrap().play.insert(1, play);
        for excluded in ["unplaced", "released", "fake"] {
            let g = &mut game.lan.as_mut().unwrap().hub.guests[0];
            match excluded {
                "unplaced" => g.placed = false,
                "released" => g.released = true,
                _ => g.fake = true,
            }
            command(&mut game, "save-all");
            assert_eq!(
                fs::read(&path).unwrap(),
                before,
                "{excluded}: connected advancement unchanged"
            );
            let g = &mut game.lan.as_mut().unwrap().hub.guests[0];
            g.placed = true;
            g.released = false;
            g.fake = false;
        }
        let mut host = game.lan.take().unwrap();
        host.play
            .get_mut(&1)
            .unwrap()
            .stats
            .add("used", "minecraft:stick", 2);
        // The old disconnect wrapper still writes advancement THEN statistics.
        game.save_guest_records(&host, &host.hub.guests[0]);
        assert_eq!(read()["minecraft:story/root"]["done"], true);
        let stat: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&stats_path).unwrap()).unwrap();
        assert_eq!(stat["stats"]["minecraft:used"]["minecraft:stick"], 2);
        assert_eq!(
            host.hub.guests[0]
                .advancements
                .to_json(&game.adv.tree, horizon_world::DATA_VERSION),
            read()
        );
        game.lan = Some(host);
        fs::remove_dir_all(&dir).unwrap();
    }
}
