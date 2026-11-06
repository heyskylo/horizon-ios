//! Moving players and vehicles for server mods (docs/native-mods.md, "Moving players and
//! vehicles"): teleports, riding, flight and respawns a port asks for from any server hook,
//! done at once where the player is in reach, else after the players' packets of the server
//! tick (`run_native_moves`).
use super::{Dim, Game, commands::Target, server_travel::LevelTeleport};

/// What a mod asked for a server player who was out of reach (another player's state swapped
/// in, or the hub held by the server): done where `run_native_moves` runs.
pub(super) enum NativeMove {
    Teleport {
        dim: Dim,
        pos: [f64; 3],
        rotation: [f32; 2],
    },
}

/// The level a mod names, of the three a server keeps.
fn native_dim(id: &str) -> Option<Dim> {
    [Dim::Overworld, Dim::Nether, Dim::End]
        .into_iter()
        .find(|d| d.id() == id)
}

/// `/tp`'s world bounds (`Level.isInSpawnableBounds`).
fn teleportable(pos: [f64; 3]) -> bool {
    pos.iter().all(|v| v.is_finite())
        && pos[0].abs() < 30_000_000.0
        && pos[2].abs() < 30_000_000.0
        && (-20_000_000.0..20_000_000.0).contains(&pos[1])
}

// Native mods use these; nothing in Horizon does.
#[allow(dead_code)]
impl Game<'_> {
    /// Whether this game is a server (its thread or a dedicated one), never a client.
    fn native_serves(&self) -> bool {
        self.serving.is_some() || self.options.dedicated.is_some()
    }

    /// Teleports the server player `uuid` to `pos` in `dimension`
    /// (`minecraft:the_nether`), facing `yaw` and `pitch` in degrees, as `/tp` does (Bukkit's
    /// `Player.teleport`): they leave their vehicle, lose their motion and fall, and their
    /// client is told. Callable from any server hook. Within their level it is done at once
    /// when they are the acting player or nobody's state is swapped in (a tick hook); inside
    /// another player's hook, where the other players are out of reach, it is done after the
    /// players' packets of the server tick (the same tick when the hook heard a packet). Into
    /// another level it happens when the tick's level teleports run, as a command's does. True
    /// when it is done or queued; false for a player not on the server or dead (when that can
    /// be seen), a dimension other than the three, a position outside `/tp`'s bounds, an angle
    /// that is not finite, and on a client. API @API:teleport_player@.
    pub(super) fn native_teleport_player(
        &mut self,
        uuid: u128,
        dimension: &str,
        pos: [f64; 3],
        yaw: f32,
        pitch: f32,
    ) -> bool {
        let Some(dim) = native_dim(dimension) else {
            return false;
        };
        if !self.native_serves() || !teleportable(pos) || !yaw.is_finite() || !pitch.is_finite()
        {
            return false;
        }
        let rotation = [yaw, pitch.clamp(-90.0, 90.0)];
        if self.lan_acting_uuid == Some(uuid) {
            let Some(conn) = self.lan_acting else {
                return false;
            };
            if self
                .player
                .as_ref()
                .is_none_or(|p| p.health.health() <= 0.0)
            {
                return false;
            }
            if dim == self.dim {
                self.teleport_to_in_level(Target::Player, pos, Some(rotation), false);
            } else {
                self.level_teleports.push(LevelTeleport {
                    conn,
                    dim,
                    at: pos,
                    rotation: Some(rotation),
                    vehicle: None,
                });
            }
            return true;
        }
        if self.lan_acting.is_some() || self.lan.is_none() {
            self.native_moves
                .push((uuid, NativeMove::Teleport { dim, pos, rotation }));
            return true;
        }
        self.native_teleport_now(uuid, dim, pos, rotation)
    }

    /// The teleport of a server player while nobody's state is swapped in.
    fn native_teleport_now(
        &mut self,
        uuid: u128,
        dim: Dim,
        pos: [f64; 3],
        rotation: [f32; 2],
    ) -> bool {
        let Some((conn, theirs)) = self.lan.as_ref().and_then(|host| {
            host.hub
                .guests
                .iter()
                .find(|g| g.placed && !g.dead && g.uuid == uuid)
                .map(|g| (g.conn, Dim::from_id(&g.dimension)))
        }) else {
            return false;
        };
        if theirs == dim {
            self.teleport_to_in_level(Target::Guest(conn), pos, Some(rotation), false);
        } else {
            self.level_teleports.push(LevelTeleport {
                conn,
                dim,
                at: pos,
                rotation: Some(rotation),
                vehicle: None,
            });
        }
        true
    }

    /// What the mods asked for players out of reach (`NativeMove`), after the players' packets,
    /// with the hub in place and nobody's state swapped in.
    pub(super) fn run_native_moves(&mut self) {
        for (uuid, todo) in std::mem::take(&mut self.native_moves) {
            match todo {
                NativeMove::Teleport { dim, pos, rotation } => {
                    self.native_teleport_now(uuid, dim, pos, rotation);
                }
            }
        }
    }
}
