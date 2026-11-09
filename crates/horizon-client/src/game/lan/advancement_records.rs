use super::*;

impl Game<'_> {
    /// `PlayerAdvancements.save`: the existing network player's advancement file.
    pub(in crate::game) fn save_guest_advancements(
        &self,
        host: &LanHost,
        guest: &horizon_client::lan::guest::Guest,
    ) {
        if !guest.placed || guest.released || guest.fake || self.adv.tree.list.is_empty() {
            return;
        }
        let Some(uuid) = self.records_uuid(host, guest.uuid) else {
            return;
        };
        let path = super::stats::player_json(&self.world_root, "advancements", uuid);
        let json = guest
            .advancements
            .to_json(&self.adv.tree, horizon_world::DATA_VERSION);
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Ok(text) = serde_json::to_string_pretty(&json) {
            let _ = std::fs::write(path, text);
        }
    }
    /// `PlayerAdvancements.save` and `ServerStatsCounter.save` of a network player.
    pub(in crate::game) fn save_guest_records(
        &self,
        host: &LanHost,
        guest: &horizon_client::lan::guest::Guest,
    ) {
        if !guest.placed || guest.released || guest.fake || self.adv.tree.list.is_empty() {
            return;
        }
        let Some(uuid) = self.records_uuid(host, guest.uuid) else {
            return;
        };
        self.save_guest_advancements(host, guest);
        if let Some(play) = host.play.get(&guest.conn) {
            play.stats
                .write(&super::stats::player_json(&self.world_root, "stats", uuid));
        }
    }
}
