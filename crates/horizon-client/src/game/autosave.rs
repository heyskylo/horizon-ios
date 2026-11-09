//! The world's save on a timer (`MinecraftServer.computeNextAutosaveInterval`,
//! `onTickRateChanged`), the players' autosaves, and the world's metadata (level.dat).
use super::*;

impl Game<'_> {
    pub(super) fn save_game(&mut self) -> Result<()> {
        self.save_game_with(true)
    }
    /// `MinecraftServer.computeNextAutosaveInterval`: three hundred seconds
    /// of ticks at the rate the server runs at now - its measured rate while
    /// a `/tick sprint` runs - never fewer than a hundred.
    pub(super) fn next_autosave_interval(&self) -> i64 {
        let rate = if self.clock.sprinting() {
            1.0e9 / (self.clock.average_nanos() + 1) as f32
        } else {
            self.clock.rate()
        };
        autosave_interval(rate)
    }
    /// `MinecraftServer.onTickRateChanged`: a rate that asks for a shorter
    /// wait than the one already running shortens it.
    pub(super) fn on_tick_rate_changed(&mut self) {
        let interval = self.next_autosave_interval();
        if interval < self.ticks_until_autosave {
            self.ticks_until_autosave = interval;
        }
    }

    pub(super) fn finish_player_autosaves(&mut self, wait: bool) -> Result<bool> {
        let saved = self.player_autosaves.completed(wait);
        if !saved.is_empty() {
            let directory = horizon_client::lan::playerdata::directory(&self.world_root);
            for player in saved {
                self.native_player_save_to_file(player.uuid, &player.name, &directory);
            }
        }
        self.player_autosaves.writes_finished()
    }
    pub(super) fn save_game_metadata(&mut self) -> Result<()> {
        self.save_maps();
        self.save_random_sequences();
        self.save_fight();
        self.save_wandering_trader();
        self.save_portal_arrivals();
        self.save_stats();
        self.save_advancements();
        self.save_scoreboard();
        self.save_bossbars();
        self.save_schedule();
        self.save_warden_tracker();
        self.sync_level();
        // Level::write can synchronously replace the integrated owner file.
        let _ = self.finish_player_autosaves(true);
        let stored_owner = self.store_owner();
        write_level_for_role(
            &mut self.level,
            &self.world_root,
            self.options.execution_role,
        )?;
        // PlayerEvent.SaveToFile for the owner, once level.dat is written.
        if let Some((uuid, name)) = stored_owner {
            let directory = horizon_client::lan::playerdata::directory(&self.world_root);
            self.native_player_save_to_file(uuid, &name, &directory);
        }
        // PlayerList.saveAll: every placed player saves with the server, to
        // their own file (`PlayerDataStorage.save`); the owner is also in the
        // file level.dat names, which is the one it loads from
        // (`playerdata::load_owner`).
        if self.options.dedicated.is_some()
            && !self.options.execution_role.is_client()
            && let Some(host) = self.lan.take()
        {
            // The vehicle tree goes into a player's file when they leave
            // (`take_parent_vehicle`); here it stays in the chunk saves.
            let mut host = host;
            for g in host.hub.guests.iter_mut().filter(|g| g.placed) {
                // Read-only fixture input, correlated with the real save receipt.
                // Keep this identical on the untouched/fixed selector builds.
                if std::env::var_os("HORIZON_DEV_ADVANCEMENT_SELECTOR_AUDIT").is_some() {
                    eprintln!(
                        "advancement selector model raw: {}",
                        serde_json::json!({"conn":g.conn,"name":g.name,
                            "uuid":horizon_protocol::crypto::uuid_string(g.uuid),
                            "game_time":self.world.game_time,
                            "progress":g.advancements.to_json(&self.adv.tree, horizon_world::DATA_VERSION)})
                    );
                }
                self.guest_attachments_into(g);
                self.player_autosaves.enqueue(g, &self.world_root);
            }
            self.save_connected_guest_statistics(&host);
            self.lan = Some(host);
        }
        self.save_world_state();
        self.message = Some(("Saved the game".into(), Instant::now()));
        Ok(())
    }
}
