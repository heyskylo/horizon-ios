//! One player's view distance for native server mods (docs/native-mods.md,
//! "Server hooks for plugin ports: world"): a mod that tunes distances by
//! load or by player lowers a single player's below the server's.
use super::Game;

// Native mods use these; nothing in Horizon does.
impl Game<'_> {
    /// Limit the view distance of the player `uuid` to `distance` chunks (2
    /// to 32), or with None give it the server's again. The player's
    /// distance is the server's view distance, what its client asked for and
    /// this limit, whichever is smallest (`ChunkMap.getPlayerViewDistance`
    /// under the limit): only that player is told its radius
    /// (`SetChunkCacheRadius`), and it is sent and tracks chunks and entities
    /// within it from its next view update. The server still loads its own
    /// view distance round every player. The limit lasts until the player
    /// leaves; a later server change (`view-distance`) keeps it. False off a
    /// server, for a player not online, and inside a guest's packet hook,
    /// where the hub is taken out (call it from a tick hook). API
    /// @API:player_view_distance@.
    pub(super) fn native_set_player_view_distance(
        &mut self,
        uuid: u128,
        distance: Option<i32>,
    ) -> bool {
        if !self.native_authoritative() {
            return false;
        }
        self.lan
            .as_mut()
            .is_some_and(|host| host.hub.set_guest_view_limit(uuid, distance))
    }

    /// The view distance the player `uuid` has now: the radius of chunks it
    /// is sent (`native_set_player_view_distance`). None off a server, for a
    /// player not online, and inside a guest's packet hook. API
    /// @API:player_view_distance@.
    pub(super) fn native_player_view_distance(&self, uuid: u128) -> Option<i32> {
        if !self.native_authoritative() {
            return None;
        }
        let host = self.lan.as_ref()?;
        host.hub
            .guests
            .iter()
            .find(|g| g.uuid == uuid)
            .map(|g| g.view_distance)
    }
}
