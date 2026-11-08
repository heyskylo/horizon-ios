//! Permission nodes for native mods (docs/native-mods.md, "Server hooks for
//! plugin ports: permission nodes"): the one question every mod and the
//! server ask (`native_has_permission`), which a permissions mod answers
//! through `native_permission_check`; without one, the player's operator
//! level decides, as before.
use super::Game;

// Native mods use these; nothing in Horizon calls the helper itself.
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
    /// Sends a player their command tree again, the mods asked anew
    /// (`native_player_commands_sent`, its v2 and the permission nodes of
    /// the mods' commands), as Bukkit's `Player.updateCommands`: after a
    /// permissions mod changed what they may use. It goes in the next server
    /// tick, so it works from any server hook, the player's own included
    /// (from the commands-sent hooks themselves it would send it every tick).
    /// True when the player is on (or is the acting player); false for
    /// anyone else and on a client. API @API:resend_commands@.
    pub(super) fn native_resend_commands(&mut self, player: u128) -> bool {
        let on = self.lan_acting_uuid == Some(player)
            || self
                .lan
                .as_ref()
                .is_some_and(|host| host.hub.guests.iter().any(|g| g.placed && g.uuid == player));
        if on && !self.native_resend_trees.contains(&player) {
            self.native_resend_trees.push(player);
        }
        on
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

    /// The command trees `native_resend_commands` asked for, sent from the
    /// server's tick with no player's state swapped in.
    pub(super) fn resend_native_trees(&mut self, host: &mut super::lan::LanHost) {
        for player in std::mem::take(&mut self.native_resend_trees) {
            let Some(conn) = host
                .hub
                .guests
                .iter()
                .find(|g| g.placed && g.uuid == player)
                .map(|g| g.conn)
            else {
                continue;
            };
            self.native_filter_commands(host, Some(conn));
            host.hub.send_commands_to(conn);
        }
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
