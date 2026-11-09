//! The writers `/tp` and `/rotate` share with a vehicle's riders: a vehicle
//! that is teleported takes its passengers along in the same operation, by the
//! vehicle's offset and the angles it was placed at (`Entity.teleport` ->
//! `teleportSameDimension` -> `calculatePassengerTransition`), and `/rotate`
//! turns an entity without teleporting it (`RotateCommand` ->
//! `Entity.forceSetRotation`).
use super::Game;
use super::commands::Target;
use horizon_core::geometry::Vec3;
use horizon_protocol::packets::play::{self, Clientbound as C};

fn is_horse(species: horizon_client::mobs::Species) -> bool {
    use horizon_client::mobs::Species::*;
    matches!(
        species,
        Horse
            | Donkey
            | Mule
            | SkeletonHorse
            | ZombieHorse
            | Camel
            | CamelHusk
            | Llama
            | TraderLlama
    )
}

/// `Entity.calculatePassengerTransition` with no relative axes: the passenger
/// keeps its offset from the vehicle.
pub(super) fn passenger_destination(old_vehicle: Vec3, new_vehicle: Vec3, passenger: Vec3) -> Vec3 {
    Vec3::new(
        new_vehicle.x + (passenger.x - old_vehicle.x),
        new_vehicle.y + (passenger.y - old_vehicle.y),
        new_vehicle.z + (passenger.z - old_vehicle.z),
    )
}

/// `PositionMoveRotation.calculateAbsolute` then `Entity.setXRot`: the pitch
/// is clamped to -90..90 first (so 360 and +infinity are +90), and NaN is
/// discarded (`None`).
pub(super) fn absolute_pitch(pitch: f32) -> Option<f32> {
    let clamped = pitch.clamp(-90.0, 90.0);
    (!clamped.is_nan()).then(|| (clamped % 360.0).clamp(-90.0, 90.0))
}

/// `Entity.calculatePassengerTransition` for the angles: a rider turns by what
/// the vehicle turned, keeping `passenger.getYRot() - getYRot()`. An absolute
/// angle adds that offset to the vehicle's new one; a relative one (`~`) is a
/// delta the rider adds to its own: the same sum. `after` is the angle before
/// the vehicle's pitch clamp; the rider's pitch is clamped on its own
/// (`Entity.setXRot`), and an axis the command did not set leaves the rider's.
pub(super) fn passenger_rotation(before: [f32; 2], after: [f32; 2], rider: [f32; 2]) -> [f32; 2] {
    let axis = |i: usize| {
        if after[i].is_finite() {
            after[i] + (rider[i] - before[i])
        } else {
            rider[i]
        }
    };
    [axis(0), absolute_pitch(axis(1)).unwrap_or(rider[1])]
}

/// The angles a command turned a vehicle to, with the angles it had before:
/// what `Entity.calculatePassengerTransition` needs for the riders.
#[derive(Clone, Copy)]
pub(super) struct PlacedRotation {
    pub before: [f32; 2],
    pub after: [f32; 2],
}

/// The entity id a vehicle's riders point at, for the targets that carry some.
fn vehicle_id(t: Target) -> Option<u32> {
    match t {
        Target::Boat(id) | Target::Cart(id) | Target::Mob(id) => Some(id),
        _ => None,
    }
}

impl Game<'_> {
    /// `AbstractMinecart.getDismountLocationForPassenger` for a standing mob.
    pub(super) fn cart_dismount_point(&self, vehicle: u32, rider: usize) -> Option<Vec3> {
        use horizon_core::geometry::Aabb;
        let cart = self.carts.iter().find(|c| c.id == vehicle)?;
        let rider = &self.mobs[rider];
        let direction =
            (f64::from(cart.yaw) / 90.0 + 0.5).floor() as i32 + if cart.flipped { 3 } else { 1 };
        let (fx, fz) = [(0, 1), (-1, 0), (0, -1), (1, 0)][(direction & 3) as usize];
        let (rx, rz) = (-fz, fx);
        let offsets = [
            (rx, rz),
            (-rx, -rz),
            (rx - fx, rz - fz),
            (-rx - fx, -rz - fz),
            (fx + rx, fz + rz),
            (fx - rx, fz - rz),
            (-fx, -fz),
            (fx, fz),
        ];
        let [bx, by, bz] =
            [cart.position.x, cart.position.y, cart.position.z].map(|v| v.floor() as i32);
        let half = f64::from(rider.width().min(1.0) / 2.0);
        for dy in [0, 1, -1] {
            for (dx, dz) in offsets {
                let p = [bx + dx, by + dy, bz + dz];
                let shape = self.block_collision_shape(p);
                let below = self.block_collision_shape([p[0], p[1] - 1, p[2]]);
                let floor = horizon_core::geometry::block_floor_height(&shape, || &below);
                if !floor.is_infinite() && floor < 1.0 {
                    let at = Vec3::new(
                        f64::from(p[0]) + 0.5,
                        f64::from(p[1]) + floor,
                        f64::from(p[2]) + 0.5,
                    );
                    let bounds = Aabb::new(
                        Vec3::new(at.x - half, at.y, at.z - half),
                        Vec3::new(at.x + half, at.y + f64::from(rider.height()), at.z + half),
                    );
                    if self.can_dismount_to(bounds) {
                        return Some(at);
                    }
                }
            }
        }
        Some(Vec3::new(
            cart.position.x,
            cart.position.y + super::minecarts::HEIGHT,
            cart.position.z,
        ))
    }

    /// `AbstractHorse.getDismountLocationForPassenger`: try either side
    /// through the mount's height, then fall back to its feet.
    pub(super) fn horse_dismount_point(&self, vehicle: u32, rider: usize) -> Option<Vec3> {
        use horizon_core::geometry::{Aabb, trig};
        let mount = self
            .mobs
            .iter()
            .find(|m| m.id == vehicle && is_horse(m.species()))?;
        let rider = &self.mobs[rider];
        let turn = if rider.main_arm_left() { -90.0 } else { 90.0 };
        let reach =
            (f64::from(mount.width()) + f64::from(rider.width()) + f64::from(1e-5_f32)) / 2.0;
        let limit = mount.bounds().max_position().y + 0.75;
        for turn in [turn, -turn] {
            let radians = f64::from((mount.yaw + turn) * 0.017_453_292);
            let (dx, dz) = (-trig::sin(radians), trig::cos(radians));
            let scale = f64::from(dx.abs().max(dz.abs()));
            let x = mount.position.x + f64::from(dx) * reach / scale;
            let z = mount.position.z + f64::from(dz) * reach / scale;
            let mut y = mount.position.y.floor() as i32;
            loop {
                let p = [x.floor() as i32, y, z.floor() as i32];
                let shape = self.block_collision_shape(p);
                let below = self.block_collision_shape([p[0], p[1] - 1, p[2]]);
                let floor = horizon_core::geometry::block_floor_height(&shape, || &below);
                if f64::from(y) + floor > limit {
                    break;
                }
                if !floor.is_infinite() && floor < 1.0 {
                    let at = Vec3::new(x, f64::from(y) + floor, z);
                    let half = f64::from(rider.width()) / 2.0;
                    let bounds = Aabb::new(
                        Vec3::new(x - half, at.y, z - half),
                        Vec3::new(x + half, at.y + f64::from(rider.height()), z + half),
                    );
                    if self.can_dismount_to(bounds) {
                        return Some(at);
                    }
                }
                y += 1;
                if f64::from(y) >= limit {
                    break;
                }
            }
        }
        Some(mount.position)
    }

    /// `Entity.teleportCrossDimension` ejects the riders before calculating
    /// their transitions. Dismounting also seats their own passengers again.
    fn prepare_vehicle_transition(&mut self, root: u32) {
        let riders: Vec<_> = self
            .mobs
            .iter()
            .filter(|m| m.extra.mount == Some(root))
            .map(|m| m.id)
            .collect();
        for &rider in riders.iter().rev() {
            self.dismount_mob(rider);
            self.snap_mob_passengers(rider);
        }
        for rider in riders {
            self.prepare_vehicle_transition(rider);
            if let Some(m) = self.mobs.iter_mut().find(|m| m.id == rider) {
                // Preserve the tree for serialization after all transitions
                // have used the positions left by ejectPassengers.
                m.extra.mount = Some(root);
                m.extra.passenger = true;
            }
        }
    }

    /// `Entity.teleportPassengers` on a mob dismounted during a transition.
    fn snap_mob_passengers(&mut self, root: u32) {
        use horizon_client::mobs::riders;
        let Some(vehicle) = self.mobs.iter().find(|m| m.id == root) else {
            return;
        };
        let (position, yaw, width, attachment, vehicle_box) = (
            vehicle.position,
            vehicle.yaw,
            vehicle.width(),
            riders::passenger_attachment(vehicle),
            vehicle.bounds(),
        );
        let mut children = Vec::new();
        for m in self.mobs.iter_mut().filter(|m| m.extra.mount == Some(root)) {
            let offset = riders::vehicle_attachment(m, width);
            m.seated(root, offset, vehicle_box);
            m.position = riders::seat(attachment, offset, position, yaw);
            m.previous = m.position;
            children.push(m.id);
        }
        for child in children {
            self.snap_mob_passengers(child);
        }
    }

    /// `Entity.teleportCrossDimension`: carry a command's entire vehicle tree
    /// into the destination level, retaining UUIDs and the passengers' mounts.
    pub(super) fn teleport_vehicle_across(
        &mut self,
        target: Target,
        destination: super::Dim,
        at: [f64; 3],
        rotation: Option<[f32; 2]>,
        turn_riders: bool,
    ) -> horizon_client::Result<bool> {
        let Some(root) = vehicle_id(target) else {
            return Ok(false);
        };
        self.in_target_level(target, |game| {
            let source = game.dim;
            if source == destination {
                return Ok(false);
            }
            // Open the destination before changing or removing any entity.
            game.enter_level(None, destination)?;
            game.enter_level(None, source)?;
            if game
                .mobs
                .iter()
                .any(|m| m.id == root && m.extra.mount.is_some())
            {
                game.dismount_mob(root);
                game.snap_mob_passengers(root);
            }
            game.prepare_vehicle_transition(root);
            game.teleport_to_in_level(target, at, rotation, turn_riders);
            // teleportCrossDimension: the new entity's teleportSetPosition takes
            // the transition's motion, which /tp's teleportTo makes zero. (The
            // ground flag performTeleport then sets lands on the old entity.)
            game.teleport_stops(target);
            let mounts: Vec<_> = game.mobs.iter().map(|m| (m.id, m.extra.mount)).collect();
            let tree = super::persist::vehicle_tree(root, &mounts);
            let tag = game.vehicle_tree_nbt(root).ok_or("missing vehicle tree")?;
            let kind = horizon_client::entity_nbt::kind(&tag).ok_or("missing vehicle type")?;
            let uuid = horizon_client::entity_nbt::uuid(&tag);
            let mut players = Vec::new();
            // A command runs with its source player swapped out of the hub.
            if game.riding.is_some_and(|id| tree.contains(&id))
                && let (Some(conn), Some(player)) = (game.lan_acting, game.player.as_ref())
            {
                players.push((
                    conn,
                    [player.position.x, player.position.y, player.position.z],
                    [game.yaw, game.pitch],
                    game.riding.unwrap(),
                ));
            }
            if let Some(host) = game.lan.as_ref() {
                for guest in &host.hub.guests {
                    if Some(guest.conn) != game.lan_acting
                        && guest.vehicle.is_some_and(|id| tree.contains(&id))
                    {
                        players.push((
                            guest.conn,
                            guest.position,
                            [guest.yaw, guest.pitch],
                            guest.vehicle.unwrap(),
                        ));
                    }
                }
            }
            let players: Vec<_> = players
                .into_iter()
                .map(|(conn, at, rotation, mount)| (conn, at, rotation, game.uuid_for(mount)))
                .collect();
            game.mobs.retain(|m| !tree.contains(&m.id));
            game.boats.retain(|b| !tree.contains(&b.id));
            game.carts.retain(|c| !tree.contains(&c.id));
            for id in tree {
                game.release_vehicle_container(id);
                game.detach_entity(id);
            }
            game.native_leaves_flush();
            game.enter_level(None, destination)?;
            // restoreFrom reloads saved state into fresh entities, rather than
            // retaining navigation paths or goals from the old level.
            game.load_listed(&kind, uuid, game.level.player_uuid, tag)
                .map_err(|_| "could not restore vehicle tree")?;
            // AbstractHorse.addPassenger snaps a reattached passenger to the
            // new horse's getViewYRot(0) / getViewXRot(0). A fresh living
            // entity's previous head yaw is still zero, before its first tick.
            let new_root = uuid
                .and_then(|uuid| game.vehicle_by_uuid(uuid))
                .ok_or("restored vehicle is missing")?;
            let mounts: Vec<_> = game.mobs.iter().map(|m| (m.id, m.extra.mount)).collect();
            let restored = super::persist::vehicle_tree(new_root, &mounts);
            let horses: Vec<_> = game
                .mobs
                .iter()
                .filter(|m| restored.contains(&m.id) && is_horse(m.species()))
                .map(|m| (m.id, m.head_pitch))
                .collect();
            for (horse, pitch) in horses {
                for rider in game
                    .mobs
                    .iter_mut()
                    .filter(|m| m.extra.mount == Some(horse))
                {
                    rider.yaw = 0.0;
                    rider.previous_yaw = 0.0;
                    rider.head_pitch = pitch;
                    rider.previous_head_pitch = pitch;
                }
            }
            for (conn, at, rotation, mount) in players {
                game.level_teleports
                    .push(super::server_travel::LevelTeleport {
                        conn,
                        dim: destination,
                        at,
                        rotation: Some(rotation),
                        vehicle: game.vehicle_by_uuid(mount),
                        cause: None,
                    });
            }
            game.native_joins_flush(false, 0);
            game.enter_level(None, source)?;
            Ok(true)
        })
        .unwrap_or(Ok(false))
    }

    /// `Entity.teleportSameDimension`: after `t` moved from `from` to `to`, its
    /// riders move by the same offset, and to the angles the vehicle was
    /// placed at when it turned (`calculatePassengerTransition`), staying
    /// seated and recursive. The player in place and the hub's guests (a guest
    /// also gets its own position packet, as `ServerPlayer.teleport` sends one
    /// for a passenger).
    pub(super) fn teleport_passengers(
        &mut self,
        t: Target,
        from: [f64; 3],
        to: [f64; 3],
        rotation: Option<PlacedRotation>,
    ) {
        let Some(id) = vehicle_id(t) else {
            return;
        };
        let (old, new) = (
            Vec3::new(from[0], from[1], from[2]),
            Vec3::new(to[0], to[1], to[2]),
        );
        self.move_riders(id, old, new, rotation);
    }
    fn move_riders(
        &mut self,
        vehicle: u32,
        old: Vec3,
        new: Vec3,
        rotation: Option<PlacedRotation>,
    ) {
        let riders: Vec<u32> = self
            .mobs
            .iter()
            .filter(|m| m.extra.mount == Some(vehicle))
            .map(|m| m.id)
            .collect();
        for id in riders {
            if let Some(m) = self.mobs.iter_mut().find(|m| m.id == id) {
                m.position = passenger_destination(old, new, m.position);
                m.previous = m.position;
                // Entity.calculatePassengerTransition turns the rider with the
                // vehicle, then Entity.teleportSetPosition keeps the old angles
                // for the interpolation.
                if let Some(placed) = rotation {
                    let [yaw, pitch] =
                        passenger_rotation(placed.before, placed.after, [m.yaw, m.head_pitch]);
                    m.yaw = yaw;
                    m.body_yaw = yaw;
                    m.head_yaw = yaw;
                    m.head_pitch = pitch;
                    m.previous_yaw = yaw;
                    m.previous_head_pitch = pitch;
                }
            }
            self.move_riders(id, old, new, rotation);
        }
        if self.riding == Some(vehicle)
            && let Some(p) = self.player.as_mut()
        {
            p.position = passenger_destination(old, new, p.position);
            p.teleported = true;
            self.previous_position = p.position;
        }
        if let Some(placed) = rotation
            && self.riding == Some(vehicle)
        {
            let [yaw, pitch] =
                passenger_rotation(placed.before, placed.after, [self.yaw, self.pitch]);
            if yaw.is_finite() {
                self.yaw = yaw;
            }
            if pitch.is_finite() {
                self.pitch = pitch;
            }
        }
        let [yaw, pitch] = self.vehicle_rotation(vehicle);
        if let Some(host) = self.lan.as_mut() {
            for i in 0..host.hub.guests.len() {
                if host.hub.guests[i].vehicle != Some(vehicle) {
                    continue;
                }
                // Entity.sendTeleportTransitionToRidingPlayers: the rider's
                // client moves the vehicle it drives itself, so it hears the
                // vehicle's teleport (else its next MoveVehicle undoes it).
                host.hub.guests[i].send(&C::TeleportEntity(play::TeleportEntity {
                    id: vehicle as i32,
                    position: [new.x, new.y, new.z],
                    delta: [0.0; 3],
                    yaw,
                    pitch,
                    relatives: 0,
                    on_ground: false,
                }));
                if let Some(placed) = rotation {
                    let g = &mut host.hub.guests[i];
                    let [yaw, pitch] =
                        passenger_rotation(placed.before, placed.after, [g.yaw, g.pitch]);
                    g.yaw = yaw;
                    g.head_yaw = yaw;
                    g.pitch = pitch;
                }
                let g = host.hub.guests[i].position;
                let at = passenger_destination(old, new, Vec3::new(g[0], g[1], g[2]));
                if rotation.is_some() {
                    // A rider turned with its vehicle: its position packet
                    // carries the new rotation and keeps its motion.
                    host.hub.teleport_rider(i, [at.x, at.y, at.z]);
                } else {
                    host.hub.teleport_keeping_motion(i, [at.x, at.y, at.z]);
                }
            }
        }
    }

    /// A vehicle's yaw and pitch, for the riders' teleport packet.
    fn vehicle_rotation(&self, id: u32) -> [f32; 2] {
        if let Some(b) = self.boats.iter().find(|b| b.id == id) {
            [b.yaw, b.pitch]
        } else if let Some(c) = self.carts.iter().find(|c| c.id == id) {
            [c.yaw, c.pitch]
        } else if let Some(m) = self.mobs.iter().find(|m| m.id == id) {
            [m.yaw, m.head_pitch]
        } else {
            [0.0, 0.0]
        }
    }

    /// `Entity.forceSetRotation`: yaw, head yaw, pitch and their old values
    /// only. Position, motion, the vehicle and the riders stay.
    pub(super) fn set_rotation(&mut self, t: Target, rotation: [f32; 2]) {
        let _ = self.in_target_level(t, |g| g.set_rotation_in_level(t, rotation));
    }
    fn set_rotation_in_level(&mut self, t: Target, [yaw, pitch]: [f32; 2]) {
        let pitch = absolute_pitch(pitch);
        match t {
            Target::Guest(_) => {
                self.for_player(t, |g| {
                    g.set_rotation_in_level(Target::Player, [yaw, pitch.unwrap_or(f32::NAN)])
                });
            }
            Target::Player => {
                if yaw.is_finite() {
                    self.yaw = yaw;
                }
                if let Some(pitch) = pitch {
                    self.pitch = pitch;
                }
                // A server's player hears the turn alone when its turn ends.
                self.rotation_forced = self.lan_acting.is_some();
            }
            Target::Mob(id) => {
                if let Some(m) = self.mobs.iter_mut().find(|m| m.id == id) {
                    if yaw.is_finite() {
                        m.yaw = yaw;
                        m.body_yaw = yaw;
                        m.head_yaw = yaw;
                    }
                    if let Some(pitch) = pitch {
                        m.head_pitch = pitch;
                    }
                    m.previous_yaw = m.yaw;
                    m.previous_head_pitch = m.head_pitch;
                }
            }
            Target::Other(id) => {
                if let Some(o) = self.others.iter_mut().find(|o| o.id == id) {
                    if yaw.is_finite() {
                        o.yaw = yaw;
                    }
                    if let Some(pitch) = pitch {
                        o.pitch = pitch;
                    }
                }
            }
            Target::Stand(id) if yaw.is_finite() => {
                if let Some(a) = self.armor_stands.iter_mut().find(|a| a.id == id) {
                    a.yaw = yaw;
                }
            }
            Target::Boat(id) if yaw.is_finite() => {
                if let Some(b) = self.boats.iter_mut().find(|b| b.id == id) {
                    b.yaw = yaw;
                    if let Some(pitch) = pitch {
                        b.pitch = pitch;
                    }
                }
            }
            Target::Cart(id) if yaw.is_finite() => {
                if let Some(c) = self.carts.iter_mut().find(|c| c.id == id) {
                    c.yaw = yaw;
                    if let Some(pitch) = pitch {
                        c.pitch = pitch;
                    }
                }
            }
            Target::Fangs(id) if yaw.is_finite() => {
                if let Some(f) = self.fangs.list.iter_mut().find(|f| f.id == id) {
                    f.yaw = yaw;
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn passenger_keeps_its_offset() {
        let at = passenger_destination(
            Vec3::new(0.0, 64.0, 0.0),
            Vec3::new(100.0, 70.0, -20.0),
            Vec3::new(0.0, 64.5, 0.25),
        );
        assert_eq!((at.x, at.y, at.z), (100.0, 70.5, -19.75));
    }
    #[test]
    fn pitch_clamps_before_the_setter() {
        assert_eq!(absolute_pitch(360.0), Some(90.0));
        assert_eq!(absolute_pitch(-360.0), Some(-90.0));
        assert_eq!(absolute_pitch(f32::INFINITY), Some(90.0));
        assert_eq!(absolute_pitch(f32::NEG_INFINITY), Some(-90.0));
        assert_eq!(absolute_pitch(45.0), Some(45.0));
        assert_eq!(absolute_pitch(f32::NAN), None);
    }
    #[test]
    fn an_absolute_turn_keeps_the_riders_angle_to_the_vehicle() {
        // A rider 30 degrees off a vehicle that turns from 0 to 90 ends at
        // 120: it keeps the 30 degrees it had on it.
        assert_eq!(
            passenger_rotation([0.0, 0.0], [90.0, 0.0], [30.0, 0.0]),
            [120.0, 0.0]
        );
        // The pitch keeps its offset the same way, through Entity.setXRot's
        // clamp: 40 - 10 = 30 degrees off a vehicle placed at 70 would be 100.
        assert_eq!(
            passenger_rotation([10.0, 10.0], [0.0, 70.0], [0.0, 40.0]),
            [-10.0, 90.0]
        );
    }
    #[test]
    fn a_rider_turns_from_the_vehicles_unclamped_pitch() {
        // `~ ~100` on a level vehicle: the vehicle stops at 90, a rider 50
        // below it adds the whole 100 (calculatePassengerTransition) and ends at 50.
        assert_eq!(
            passenger_rotation([0.0, 0.0], [0.0, 100.0], [0.0, -50.0]),
            [0.0, 50.0]
        );
    }
    #[test]
    fn an_axis_the_command_did_not_set_leaves_the_riders_own() {
        assert!(passenger_rotation([0.0, 0.0], [f32::NAN, 0.0], [30.0, 0.0])[0] == 30.0);
        assert!(passenger_rotation([0.0, 0.0], [0.0, f32::NAN], [0.0, 40.0])[1] == 40.0);
    }
}
