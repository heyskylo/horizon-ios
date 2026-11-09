//! What a server mod reads about a player (docs/native-mods.md, "Server hooks for plugin ports:
//! Reading players"): their client, statistics and advancements in vanilla's JSON form, what they
//! hold and wear, and the server's player limit, folder and log. A player is the acting one
//! (their state swapped in) or one the hub holds; with the hub out of reach, inside another
//! player's hook, only the acting player is found.
use super::{Game, lan::LanHost};
use horizon_client::{
    items::ItemStack,
    lan::guest::Guest,
    mod_api::{EquipmentSlot, NativeClientInfo},
};
use std::path::PathBuf;

// Native mods use these; nothing in Horizon calls them.
impl Game<'_> {
    /// What the player's client told the server (brand, `ClientInformation`) and what the server
    /// sent it, as Paper's `getClientBrandName`, `locale` and `getClientViewDistance` read it.
    /// None for a player who is not on, while the hub is out of reach, and on a client.
    /// API @API:player_client@.
    pub(super) fn native_player_client(&self, uuid: u128) -> Option<NativeClientInfo> {
        let (_, g) = self.hub_guest(uuid)?;
        Some(NativeClientInfo {
            brand: g.brand.clone(),
            language: g.info.language.clone(),
            view_distance: g.info.view_distance,
            chat_visibility: g.info.chat_visibility,
            main_hand: g.info.main_hand,
            particle_status: g.info.particle_status,
            sent_chunks: g.sent_chunks.len(),
            wire_bytes: g.sender.wire_bytes(),
        })
    }

    /// The player's statistics as vanilla saves them (`players/stats/<uuid>.json`:
    /// `{"stats": {"minecraft:custom": {"minecraft:jump": 3}}, "DataVersion": ..}`), as
    /// Bukkit's `getStatistic` reads them. None for a player who is not on and on a client.
    /// API @API:player_records@.
    pub(super) fn native_player_stats(&self, uuid: u128) -> Option<serde_json::Value> {
        if self.acting(uuid) {
            return Some(self.stats.to_json());
        }
        let (host, g) = self.hub_guest(uuid)?;
        Some(host.play.get(&g.conn)?.stats.to_json())
    }

    /// One statistic of the player: its type and key (`minecraft:custom`,
    /// `minecraft:jump`; `custom`, `jump` mean the same), 0 when never counted. None for a
    /// player who is not on and on a client. API @API:player_records@.
    pub(super) fn native_player_stat(&self, uuid: u128, stat_type: &str, key: &str) -> Option<i32> {
        let stats = if self.acting(uuid) {
            &self.stats
        } else {
            let (host, g) = self.hub_guest(uuid)?;
            &host.play.get(&g.conn)?.stats
        };
        let (ty, key) = (
            horizon_client::advancements::full(stat_type),
            horizon_client::advancements::full(key),
        );
        Some(
            stats
                .values
                .get(&*ty)
                .and_then(|m| m.get(&*key))
                .copied()
                .unwrap_or(0),
        )
    }

    /// Replaces the player's statistics with those of `stats`, in the form
    /// `native_player_stats` gives (HuskSync's restore of a saved player). They are saved
    /// with the player and all go to the client at its next request, as after a join;
    /// scoreboard objectives that count a statistic are not told. False for a player who is
    /// not on and on a client. API @API:player_records@.
    pub(super) fn native_set_player_stats(
        &mut self,
        uuid: u128,
        stats: &serde_json::Value,
    ) -> bool {
        let values = super::stats::Stats::from_json(stats).values;
        let target = if self.acting(uuid) {
            &mut self.stats
        } else {
            let Some(host) = self.lan.as_mut() else {
                return false;
            };
            let Some(conn) = host
                .hub
                .guests
                .iter()
                .find(|g| g.uuid == uuid)
                .map(|g| g.conn)
            else {
                return false;
            };
            let Some(play) = host.play.get_mut(&conn) else {
                return false;
            };
            &mut play.stats
        };
        target.values = values;
        target.mark_all_dirty();
        true
    }

    /// The player's advancement progress as vanilla saves it (`players/advancements/<uuid>
    /// .json`: each advancement's obtained criteria with their dates and `done`), as Bukkit's
    /// `getAdvancementProgress` reads it. None for a player who is not on and on a client.
    /// API @API:player_records@.
    pub(super) fn native_player_advancements(&self, uuid: u128) -> Option<serde_json::Value> {
        let progress = if self.acting(uuid) {
            &self.adv.progress
        } else {
            &self.hub_guest(uuid)?.1.advancements
        };
        Some(progress.to_json(&self.adv.tree, horizon_world::DATA_VERSION))
    }

    /// Replaces the player's advancement progress with `advancements`, in the form
    /// `native_player_advancements` gives; advancements this server lacks are dropped, as
    /// when the file loads (`PlayerAdvancements.applyFrom`). The client is sent every
    /// advancement's progress again with the player's next tick; one newly done shows its toast,
    /// as a grant would, but gives no rewards. False for a player who is not on and on a client.
    /// API @API:player_records@.
    pub(super) fn native_set_player_advancements(
        &mut self,
        uuid: u128,
        advancements: &serde_json::Value,
    ) -> bool {
        let tree = self.advancement_tree();
        let mut progress = horizon_client::advancements::Progress::from_json(advancements);
        progress
            .criteria
            .retain(|id, _| tree.by_id.contains_key(id));
        let all = 0..tree.list.len();
        if self.acting(uuid) {
            self.adv.progress = progress;
            self.adv.changed.extend(all);
            return true;
        }
        let Some(g) = self
            .lan
            .as_mut()
            .and_then(|host| host.hub.guests.iter_mut().find(|g| g.uuid == uuid))
        else {
            return false;
        };
        g.advancements = progress;
        g.advancements_loaded = true;
        g.advancements_changed.extend(all);
        true
    }

    /// The server's player limit (`max-players`, as `/list` shows it), as Bukkit's
    /// `Server.getMaxPlayers`. None while the hub is out of reach and on a client.
    /// API @API:server_files@.
    pub(super) fn native_max_players(&self) -> Option<u32> {
        let host = self.lan.as_ref()?;
        Some(host.hub.lan.shared.max_players().min(u32::MAX as usize) as u32)
    }

    /// The dedicated server's folder (where `server.properties` is), absolute. None
    /// elsewhere. API @API:server_files@.
    pub(super) fn native_server_dir(&self) -> Option<PathBuf> {
        self.options.dedicated.as_ref()?;
        std::env::current_dir().ok()
    }

    /// The log file this run writes (`logs/latest.log` of the server's or the game's folder),
    /// absolute; None when it logs to the terminal only. API @API:server_files@.
    pub(super) fn native_log_file(&self) -> Option<PathBuf> {
        horizon_client::logging::latest_path()
    }

    /// The stack in the player's main hand, components and all (Bukkit's
    /// `getEquipment().getItemInMainHand()`). None for an empty hand, a player who is not on
    /// and on a client. API @API:player_equipment@.
    pub(super) fn native_held_stack(&self, uuid: u128) -> Option<ItemStack> {
        self.native_player_equipment(uuid, EquipmentSlot::MainHand)
    }

    /// The stack in one of the player's equipment slots (`LivingEntity.getItemBySlot`), from
    /// any server hook, the acting player's or another's. None for an empty slot, for `Body`
    /// and `Saddle`, for a player who is not on and on a client. API @API:player_equipment@.
    pub(super) fn native_player_equipment(
        &self,
        uuid: u128,
        slot: EquipmentSlot,
    ) -> Option<ItemStack> {
        if self.acting(uuid) {
            return self.native_equipment(slot);
        }
        let (_, g) = self.hub_guest(uuid)?;
        match slot {
            EquipmentSlot::MainHand => g.inventory.get(g.selected).cloned().flatten(),
            EquipmentSlot::OffHand => g.offhand.clone(),
            EquipmentSlot::Head => g.armor[0].clone(),
            EquipmentSlot::Chest => g.armor[1].clone(),
            EquipmentSlot::Legs => g.armor[2].clone(),
            EquipmentSlot::Feet => g.armor[3].clone(),
            EquipmentSlot::Body | EquipmentSlot::Saddle => None,
        }
        .filter(|stack| stack.count > 0)
    }
}

impl Game<'_> {
    /// The player whose state is swapped in now.
    fn acting(&self, uuid: u128) -> bool {
        self.lan_acting_uuid == Some(uuid) && self.player.is_some()
    }

    /// A player the hub holds, with the hub, while it is in reach.
    fn hub_guest(&self, uuid: u128) -> Option<(&LanHost, &Guest)> {
        let host = self.lan.as_ref()?;
        let g = host.hub.guests.iter().find(|g| g.uuid == uuid)?;
        Some((host, g))
    }
}
