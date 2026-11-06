//! Packets, plugin channels, resource packs and kicks for server mods (docs/native-mods.md,
//! "Server hooks for plugin ports: packets"): what plugin ports did with raw packets and
//! injections before. The packet hooks themselves run where the hub sends
//! (`horizon_client::lan::conn`), with no `Game`; the helpers here switch them on.
use super::Game;
use std::sync::atomic::Ordering;

// Native mods use these; nothing in Horizon does.
#[allow(dead_code)]
impl Game<'_> {
    /// From now on every play packet the server sends a player reaches
    /// `native_server_packet`, decoded, until the server stops; there is no
    /// switching it off, so one mod can't silence another's. Until a mod calls
    /// this, nothing is decoded or copied. Any thread, any time, a client
    /// included (where nothing is sent). @API:server_packet@.
    pub(super) fn native_watch_server_packets(&self) {
        horizon_client::lan::conn::WATCHED
            .fetch_or(horizon_client::lan::conn::WATCH_ALL, Ordering::Relaxed);
    }
    /// From now on the entity data the server sends each player
    /// (`ClientboundSetEntityDataPacket`) reaches `native_entity_data_to`,
    /// until the server stops; only that packet is decoded for it. As
    /// `native_watch_server_packets`, it cannot be switched off and works
    /// from any thread. @API:entity_data_to@.
    pub(super) fn native_watch_entity_data(&self) {
        horizon_client::lan::conn::WATCHED.fetch_or(
            horizon_client::lan::conn::WATCH_ENTITY_DATA,
            Ordering::Relaxed,
        );
    }
}
