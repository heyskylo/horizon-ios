//! Permission nodes for native mods (docs/native-mods.md, "Server hooks for
//! plugin ports: permission nodes"): the one question every mod and the
//! server ask (`native_has_permission`), which a permissions mod answers
//! through `native_permission_check`; without one, the player's operator
//! level decides, as before.
use super::Game;

// Native mods use these; nothing in Horizon calls the helper itself.
#[allow(dead_code)]
impl Game<'_> {
    /// Whether `player` has the permission node `node`: what the mods'
    /// `native_permission_check` answers, or, when none does, whether the
    /// player's operator level is `default_level` or above (Bukkit's
    /// `Permissible.hasPermission` with a default of operators of that
    /// level; 0 is everyone). The level is the one the server holds for a
    /// player who is on or joining; inside a player's hook, with the hub out
    /// of reach, the acting player's. Anyone else, and every player on a
    /// client, counts as level 0. API @API:permission_check@.
    pub(super) fn native_has_permission(
        &self,
        player: u128,
        node: &str,
        default_level: u8,
    ) -> bool {
        self.has_permission_at(player, node, default_level, self.permission_level(player))
    }
}

impl Game<'_> {
    /// `native_has_permission` for a player whose level the caller already
    /// knows.
    pub(super) fn has_permission_at(
        &self,
        player: u128,
        node: &str,
        default_level: u8,
        level: u8,
    ) -> bool {
        let mut result = None;
        self.native_permission_check(player, node, default_level, &mut result);
        result.unwrap_or(level >= default_level)
    }

    /// The operator level of a player who is on or joining (the acting
    /// player's while the hub is out of reach); 0 for anyone else.
    fn permission_level(&self, player: u128) -> u8 {
        if self.lan_acting_uuid == Some(player)
            && let Some(level) = self.lan_acting_permission
        {
            return level;
        }
        self.lan
            .as_ref()
            .and_then(|host| host.hub.guests.iter().find(|g| g.uuid == player))
            .map_or(0, |g| g.permission)
    }
}
