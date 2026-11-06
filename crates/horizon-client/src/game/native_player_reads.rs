//! What a server mod reads about a player (docs/native-mods.md, "Server hooks for plugin ports:
//! Reading players"): their client, statistics and advancements in vanilla's JSON form, what they
//! hold and wear, and the server's player limit, folder and log. A player is the acting one
//! (their state swapped in) or one the hub holds; with the hub out of reach, inside another
//! player's hook, only the acting player is found.
use super::{Game, lan::LanHost};
use horizon_client::{
    lan::guest::Guest,
    mod_api::NativeClientInfo,
};

// Native mods use these; nothing in Horizon calls them.
#[allow(dead_code)]
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
}

impl Game<'_> {
    /// A player the hub holds, with the hub, while it is in reach.
    fn hub_guest(&self, uuid: u128) -> Option<(&LanHost, &Guest)> {
        let host = self.lan.as_ref()?;
        let g = host.hub.guests.iter().find(|g| g.uuid == uuid)?;
        Some((host, g))
    }
}
