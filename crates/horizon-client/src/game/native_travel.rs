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
    Flight {
        allow: bool,
        flying: bool,
    },
    Respawn,
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
        if !self.native_serves() || !teleportable(pos) || !yaw.is_finite() || !pitch.is_finite() {
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
                    cause: Some("unknown"),
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

    /// Teleports the vehicle `id` of the level in place, `dimension` (a boat, a minecart, or a
    /// mob a rider sits on: a horse, a pig), to `pos` with its riders, as `/tp` does a vehicle
    /// (Bukkit's `Vehicle.teleport`, vanilla's `Entity.teleportSameDimension`): they stay
    /// seated and move by the same offset, and a server player riding it is told, as their
    /// client drives the vehicle itself. A mob that rides something itself leaves it. Only
    /// where nobody's state is swapped in (a tick hook, a vehicle's hook nobody rides; inside
    /// `native_vehicle_moved_v2` a mod changes `to` instead): false inside a player's hook, for
    /// no such vehicle here, another dimension, a position outside `/tp`'s bounds, and on a
    /// client. API @API:teleport_vehicle@.
    pub(super) fn native_teleport_vehicle(
        &mut self,
        dimension: &str,
        id: u32,
        pos: [f64; 3],
    ) -> bool {
        if !self.native_serves()
            || dimension != self.dim.id()
            || !teleportable(pos)
            || self.lan_acting.is_some()
            || self.lan.is_none()
        {
            return false;
        }
        let Some(vehicle) = self.native_vehicle_target(id) else {
            return false;
        };
        self.teleport_to_in_level(vehicle, pos, None, false);
        true
    }

    /// The boat, minecart or living mob `id` of the level in place.
    pub(super) fn native_vehicle_target(&self, id: u32) -> Option<Target> {
        if self.boats.iter().any(|b| b.id == id) {
            Some(Target::Boat(id))
        } else if self.carts.iter().any(|c| c.id == id) {
            Some(Target::Cart(id))
        } else if self
            .mobs
            .iter()
            .any(|m| m.id == id && !m.removed && m.alive())
        {
            Some(Target::Mob(id))
        } else {
            None
        }
    }

    /// Seats the entity `entity` (a server player or a mob) on the vehicle `vehicle` (a boat, a
    /// minecart or a mob), as `/ride <entity> mount <vehicle>` does (Bukkit's
    /// `Entity.addPassenger`), with its checks: neither rides already, no loop, a free seat,
    /// minecarts carry players only, and `native_entity_mount` may refuse. Both in the level in
    /// place. Inside a player's hook only the acting player and mobs are reached. True when it
    /// rides now; false otherwise and on a client. API @API:mount@.
    pub(super) fn native_mount(&mut self, entity: u32, vehicle: u32) -> bool {
        if !self.native_serves() {
            return false;
        }
        let (Some(rider), Some(vehicle)) = (
            self.native_rider(entity),
            self.native_vehicle_target(vehicle),
        ) else {
            return false;
        };
        match rider {
            Target::Guest(_) => self
                .for_player(rider, |g| g.ride_mount(Target::Player, vehicle).is_ok())
                .unwrap_or(false),
            _ => self.ride_mount(rider, vehicle).is_ok(),
        }
    }

    /// Takes the entity `entity` (a server player or a mob) off what it rides, as `/ride
    /// <entity> dismount` does (Bukkit's `Entity.leaveVehicle`): it lands beside the vehicle,
    /// and `native_entity_mount` may refuse. In the level in place; inside a player's hook only
    /// the acting player and mobs are reached. True when it got off; false when it rode
    /// nothing, was not reached, and on a client. API @API:mount@.
    pub(super) fn native_dismount(&mut self, entity: u32) -> bool {
        if !self.native_serves() {
            return false;
        }
        match self.native_rider(entity) {
            Some(rider @ (Target::Player | Target::Guest(_))) => self
                .for_player(rider, |g| {
                    if g.riding.is_none() {
                        return false;
                    }
                    g.stop_riding();
                    g.riding.is_none()
                })
                .unwrap_or(false),
            Some(Target::Mob(id)) => {
                let rides = |g: &Self| g.mobs.iter().any(|m| m.id == id && m.extra.mount.is_some());
                if !rides(self) {
                    return false;
                }
                self.dismount_mob(id);
                !rides(self)
            }
            _ => false,
        }
    }

    /// Takes every rider off the vehicle `vehicle` (a boat, a minecart or a mob), as
    /// `native_dismount` does each (Bukkit's `Entity.eject`): the server players and mobs on it.
    /// In the level in place; inside a player's hook only the acting player and mobs are
    /// reached. True when one got off; false otherwise and on a client. API @API:mount@.
    pub(super) fn native_eject_passengers(&mut self, vehicle: u32) -> bool {
        if !self.native_serves() || self.native_vehicle_target(vehicle).is_none() {
            return false;
        }
        let mut riders: Vec<u32> = self
            .mobs
            .iter()
            .filter(|m| m.extra.mount == Some(vehicle))
            .map(|m| m.id)
            .collect();
        if self.lan_acting.is_some() && self.riding == Some(vehicle) {
            riders.push(self.native_acting_entity());
        }
        if self.lan_acting.is_none()
            && let Some(host) = &self.lan
        {
            riders.extend(
                host.hub
                    .guests
                    .iter()
                    .filter(|g| g.placed && g.vehicle == Some(vehicle))
                    .map(|g| g.entity_id as u32),
            );
        }
        let mut any = false;
        for rider in riders {
            any |= self.native_dismount(rider);
        }
        any
    }

    /// The rider an entity id names in the level in place: the acting player, a server player
    /// out of their turn (only while nobody's is swapped in), or a living mob.
    fn native_rider(&self, entity: u32) -> Option<Target> {
        if self.lan_acting.is_some() && self.native_acting_entity() == entity {
            return Some(Target::Player);
        }
        if self.lan_acting.is_none()
            && let Some(host) = &self.lan
            && let Some(g) = host.hub.guests.iter().find(|g| {
                g.placed && !g.dead && g.entity_id as u32 == entity && g.dimension == self.dim.id()
            })
        {
            return Some(Target::Guest(g.conn));
        }
        self.mobs
            .iter()
            .any(|m| m.id == entity && !m.removed && m.alive())
            .then_some(Target::Mob(entity))
    }

    /// Lets the server player `uuid` fly in any game mode (`allow`) or not, and sets whether
    /// they fly now (`flying`), as Bukkit's `Player.setAllowFlight` and `setFlying`
    /// (`Abilities.mayfly` and `flying`); their client is told. A player who may not fly does
    /// not fly, and creative and spectator players fly whatever `allow` is. Kept until their
    /// game mode changes, they respawn or they leave (vanilla's `GameType.updatePlayerAbilities`
    /// sets it for the mode then); not saved. Where nobody's state is swapped in it is done at
    /// once, else after the players' packets of the server tick. True when done or queued;
    /// false for a player not on the server (when that can be seen) and on a client. API
    /// @API:set_flight@.
    pub(super) fn native_set_flight(&mut self, uuid: u128, allow: bool, flying: bool) -> bool {
        if !self.native_serves() {
            return false;
        }
        if self.lan_acting.is_some() || self.lan.is_none() {
            self.native_moves
                .push((uuid, NativeMove::Flight { allow, flying }));
            return true;
        }
        self.native_flight_now(uuid, allow, flying)
    }

    fn native_flight_now(&mut self, uuid: u128, allow: bool, flying: bool) -> bool {
        let Some(host) = self.lan.as_mut() else {
            return false;
        };
        let Some(conn) = host
            .hub
            .guests
            .iter()
            .find(|g| g.placed && g.uuid == uuid)
            .map(|g| g.conn)
        else {
            return false;
        };
        host.hub.set_flight(conn, allow, flying);
        true
    }

    /// Respawns the dead server player `uuid` as their client's Respawn click does (Spigot's
    /// `player.spigot().respawn()`, `ClientCommand` `PERFORM_RESPAWN`): at their bed, anchor or
    /// the world spawn, with `native_player_respawned` and the respawn hooks, without waiting
    /// for the click. Done after the players' packets of the server tick (the same tick from a
    /// packet's hook or the tick's start, else the next). True when queued; false for a player
    /// seen not on the server or alive (inside a player's hook nobody else can be seen: it is
    /// queued, and does nothing for a living player), and on a client. API
    /// @API:respawn_player@.
    pub(super) fn native_respawn_player(&mut self, uuid: u128) -> bool {
        if !self.native_serves() {
            return false;
        }
        if self.lan_acting_uuid == Some(uuid)
            && self
                .player
                .as_ref()
                .is_some_and(|p| p.health.health() > 0.0)
        {
            return false;
        }
        if self.lan_acting.is_none()
            && let Some(host) = &self.lan
            && !host
                .hub
                .guests
                .iter()
                .any(|g| g.placed && g.dead && g.uuid == uuid)
        {
            return false;
        }
        self.native_moves.push((uuid, NativeMove::Respawn));
        true
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
                cause: Some("unknown"),
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
                NativeMove::Flight { allow, flying } => {
                    self.native_flight_now(uuid, allow, flying);
                }
                NativeMove::Respawn => {
                    let Some(mut host) = self.lan.take() else {
                        continue;
                    };
                    let dead = host
                        .hub
                        .guests
                        .iter()
                        .find(|g| g.placed && g.dead && g.uuid == uuid)
                        .map(|g| g.conn);
                    if let Some(conn) = dead {
                        self.guest_respawn(&mut host, conn);
                    }
                    self.lan = Some(host);
                }
            }
        }
    }
}
