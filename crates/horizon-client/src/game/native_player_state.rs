//! A player's state for server mods (docs/native-mods.md, "Server hooks for
//! plugin ports: player state"): what a mod that keeps players in step
//! across servers (a HuskSync port) reads from a player and writes back.
//! Only the server's own players (the hub's guests, as
//! `native_server_players` lists them). Inside a player's turn (their hook,
//! their command) only that player is reached, outside one (`native_tick`,
//! `native_server_tick`, `native_menu_clicked`) everyone on.
use super::Game;
use horizon_client::lan::playerdata;
use horizon_world::nbt::Tag;

/// Where a player's state is while a helper reads it.
#[derive(Clone, Copy)]
enum Place {
    /// Swapped in: the acting player of the turn.
    Acting,
    /// Their record in the hub, at this index.
    Guest(usize),
}

// Native mods use these; nothing in Horizon does.
#[allow(dead_code)]
impl Game<'_> {
    /// The player `uuid` as a helper finds them: the acting player inside a
    /// turn (nobody else then), a placed guest outside one.
    fn native_place(&self, uuid: u128) -> Option<Place> {
        if self.lan_acting.is_some() {
            return (self.lan_acting_uuid == Some(uuid) && self.player.is_some())
                .then_some(Place::Acting);
        }
        let host = self.lan.as_ref()?;
        host.hub
            .guests
            .iter()
            .position(|g| g.uuid == uuid && g.placed && !g.released)
            .map(Place::Guest)
    }

    /// The player `uuid`'s compound as the server would save it now
    /// (`players/data/<uuid>.dat`, vanilla's `ServerPlayer.saveWithoutId`):
    /// inventory, health, food, experience, effects, attributes, position,
    /// and every key Horizon keeps without modelling it. What a mod stores to
    /// give the player back elsewhere. None inside a player's turn (their
    /// live state is swapped out of their record: call from
    /// `native_server_tick`), and for a player not on. API
    /// @API:player_compound@.
    pub(super) fn native_player_compound(&self, uuid: u128) -> Option<Tag> {
        match self.native_place(uuid)? {
            Place::Acting => None,
            Place::Guest(i) => Some(playerdata::player_tag(&self.lan.as_ref()?.hub.guests[i])),
        }
    }
}
