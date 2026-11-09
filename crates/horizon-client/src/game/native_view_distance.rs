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

    /// Mark the player `uuid` as a fake player (a mod's bot with no client), or not: it then keeps
    /// only a small area loaded and ticking around it (its view distance plus one, at most the
    /// server's plus one), its packets are cut to the few a bot needs, and it does not count for
    /// natural mob spawning. The mark lasts until the player leaves. False off a server, for a
    /// player not online, and inside a guest's packet hook, where the hub is taken out (call it
    /// from a tick hook). API @API:set_player_fake@.
    pub(super) fn native_set_player_fake(&mut self, uuid: u128, fake: bool) -> bool {
        if !self.native_authoritative() {
            return false;
        }
        self.lan
            .as_mut()
            .is_some_and(|host| host.hub.set_guest_fake(uuid, fake))
    }

    /// Say that a player named `name` (any case) will join as a fake player (a mod's bot with no
    /// client), or not any more. The player is then fake from its first packet on, so its join
    /// sends and builds nothing a client-less player cannot read; `native_set_player_fake` marks
    /// a player that is already online. False off a server. API @API:expect_fake_player@.
    pub(super) fn native_expect_fake_player(&mut self, name: &str, fake: bool) -> bool {
        if !self.native_authoritative() {
            return false;
        }
        self.lan.as_mut().is_some_and(|host| {
            host.hub.expect_fake_player(name, fake);
            true
        })
    }

    /// Whether the player `uuid` is a fake player (a mod's bot with no client): marked by
    /// `native_set_player_fake` or announced by `native_expect_fake_player`. False off a server and for
    /// a player not online. Cheap: meant for join hooks that get only a uuid. API
    /// @API:is_player_fake@.
    pub(super) fn native_is_player_fake(&self, uuid: u128) -> bool {
        self.lan
            .as_ref()
            .is_some_and(|host| host.hub.guests.iter().any(|g| g.uuid == uuid && g.fake))
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
